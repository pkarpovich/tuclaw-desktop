use std::collections::{BTreeMap, HashMap};

use futures::channel::mpsc::TryRecvError;
use futures::executor::block_on;
use tuclaw_core::v3::{
    Applied, Client, ClientMessageId, Connection, Frame, InputAccepted, Message, MessageId,
    MessageKind, MockTransport, Pace, Post, Run, RunId, RunState, Scenario, Seq, StepKind,
    SurfaceId, TextDelta,
};

struct Session {
    mock: MockTransport,
    client: Client,
    connection: Connection,
    focus: Vec<SurfaceId>,
    last_seq: Seq,
    messages: BTreeMap<MessageId, Message>,
    runs: BTreeMap<RunId, Run>,
    answered: BTreeMap<RunId, Run>,
    queued: Vec<Run>,
    early_deltas: HashMap<RunId, Vec<TextDelta>>,
    refetches: usize,
}

impl Session {
    fn connect(mock: &MockTransport, focus: Vec<SurfaceId>) -> Session {
        let client = Client::mock(mock);
        let mut connection = block_on(client.connect(None)).expect("the mock connects");
        let Ok(Frame::Hello(hello)) = connection.frames.try_recv() else {
            panic!("the first frame is hello");
        };
        connection.focus(focus.clone()).expect("the socket is open");
        Session {
            mock: mock.clone(),
            client,
            connection,
            focus,
            last_seq: hello.head,
            messages: BTreeMap::new(),
            runs: BTreeMap::new(),
            answered: BTreeMap::new(),
            queued: Vec::new(),
            early_deltas: HashMap::new(),
            refetches: 0,
        }
    }

    fn start(mock: &MockTransport, focus: Vec<SurfaceId>) -> Session {
        let mut session = Session::connect(mock, focus);
        session.fetch();
        session.drain();
        session
    }

    fn fetch(&mut self) {
        block_on(self.client.surfaces()).expect("surfaces");
        block_on(self.client.agents()).expect("agents");
        for surface in self.focus.clone() {
            let page = block_on(self.client.messages(surface, 50)).expect("page");
            for message in page.messages {
                self.messages.insert(message.id, message);
            }
        }
    }

    fn drain(&mut self) -> bool {
        self.mock.pump_control();
        loop {
            match self.connection.frames.try_recv() {
                Ok(frame) => self.apply(frame),
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Closed) => return false,
            }
        }
    }

    fn play(&mut self) {
        self.mock.play_all();
        if !self.drain() {
            self.reconnect();
        }
    }

    fn step(&mut self, frames: usize) {
        for _ in 0..frames {
            self.mock.step();
        }
        self.drain();
    }

    fn reconnect(&mut self) {
        self.connection = block_on(self.client.connect(Some(self.last_seq))).expect("reconnects");
        self.connection
            .focus(self.focus.clone())
            .expect("the socket is open");
        self.drain();
    }

    fn post(&mut self, surface: SurfaceId, text: &str) -> ClientMessageId {
        let client_message_id = ClientMessageId::random();
        let post = Post {
            text: text.into(),
            addressed_agent_id: None,
            client_message_id: client_message_id.clone(),
        };
        block_on(self.client.post(surface, &post)).expect("posted");
        client_message_id
    }

    fn apply(&mut self, frame: Frame) {
        if let Some(seq) = frame.seq()
            && seq > self.last_seq
        {
            self.last_seq = seq;
        }
        match &frame {
            Frame::Hello(hello) => self.last_seq = hello.head,
            Frame::Gap(_) => {
                self.refetches += 1;
                self.messages.clear();
                self.runs.clear();
                self.fetch();
            }
            Frame::InputAccepted(accepted) => self.accept(*accepted),
            Frame::RunStarted(event) => {
                let mut claimed = None;
                for (index, queued) in self.queued.iter_mut().enumerate() {
                    if queued.start(event) {
                        claimed = Some(index);
                        break;
                    }
                }
                let run = match claimed {
                    Some(index) => self.queued.remove(index),
                    None => match self.runs.remove(&event.run_id) {
                        Some(mut known) => {
                            known.apply(&frame);
                            known
                        }
                        None => Run::from_started(event),
                    },
                };
                let mut run = run;
                for delta in self.early_deltas.remove(&event.run_id).unwrap_or_default() {
                    run.apply(&Frame::TextDelta(delta));
                }
                self.runs.insert(event.run_id.clone(), run);
            }
            Frame::RunSnapshot(snapshot) => match self.runs.get_mut(&snapshot.run_id) {
                Some(run) => {
                    run.apply(&frame);
                }
                None => {
                    self.runs
                        .insert(snapshot.run_id.clone(), Run::from_snapshot(snapshot));
                }
            },
            Frame::TextDelta(delta) => {
                if self.apply_to_run(&frame) == Applied::NotMine {
                    self.early_deltas
                        .entry(delta.run_id.clone())
                        .or_default()
                        .push(delta.clone());
                }
            }
            Frame::TaskFired(_) => {}
            Frame::SurfaceRead(_) => {}
            Frame::MessageCreated(created) => {
                let message = created.message.clone();
                if let Some(run_id) = &message.run_id
                    && let Some(run) = self.runs.remove(run_id)
                {
                    self.answered.insert(run_id.clone(), run);
                }
                self.messages.insert(message.id, message);
            }
            Frame::StepText(_) => {
                self.apply_to_run(&frame);
            }
            Frame::ToolStarted(_) => {
                self.apply_to_run(&frame);
            }
            Frame::ToolFinished(_) => {
                self.apply_to_run(&frame);
            }
            Frame::Task(_) => {
                self.apply_to_run(&frame);
            }
            Frame::Status(_) => {
                self.apply_to_run(&frame);
            }
            Frame::RunReset(_) => {
                self.apply_to_run(&frame);
            }
            Frame::RunFinished(_) => {
                self.apply_to_run(&frame);
            }
            Frame::Unknown(_) => {}
        }
    }

    fn accept(&mut self, accepted: InputAccepted) {
        for run in self.runs.values() {
            if run.input_ids.contains(&accepted.input_id) {
                return;
            }
        }
        self.queued.push(Run::queued(accepted));
    }

    fn apply_to_run(&mut self, frame: &Frame) -> Applied {
        let Some(run_id) = frame.run_id() else {
            return Applied::NotMine;
        };
        if let Some(run) = self.runs.get_mut(run_id) {
            return run.apply(frame);
        }
        if let Some(run) = self.answered.get_mut(run_id) {
            return run.apply(frame);
        }
        Applied::NotMine
    }

    fn run(&self, run_id: &RunId) -> &Run {
        let Some(run) = self.runs.get(run_id).or_else(|| self.answered.get(run_id)) else {
            panic!("the session knows run {run_id:?}");
        };
        run
    }

    fn only_run(&self) -> RunId {
        let mut ids = Vec::new();
        for id in self.runs.keys() {
            ids.push(id.clone());
        }
        for id in self.answered.keys() {
            ids.push(id.clone());
        }
        let parked = RunId("0b9d2c4e-5a61-4f7e-8c3d-1e2f3a4b5c6d".into());
        let mut mine = Vec::new();
        for id in ids {
            if id != parked {
                mine.push(id);
            }
        }
        assert_eq!(mine.len(), 1, "exactly one run besides the parked one");
        mine.remove(0)
    }

    fn answers_for(&self, run_id: &RunId) -> usize {
        let mut count = 0;
        for message in self.messages.values() {
            if message.run_id.as_ref() == Some(run_id) {
                count += 1;
            }
        }
        count
    }
}

struct Shape {
    texts: usize,
    tools: usize,
    tasks: usize,
}

fn shape(run: &Run) -> Shape {
    let mut shape = Shape {
        texts: 0,
        tools: 0,
        tasks: 0,
    };
    for step in &run.steps {
        match &step.kind {
            StepKind::Text { .. } => shape.texts += 1,
            StepKind::Tool { .. } => shape.tools += 1,
            StepKind::Task { .. } => shape.tasks += 1,
            StepKind::Status { .. } => {}
            StepKind::Other { .. } => {}
        }
    }
    shape
}

fn stepped() -> MockTransport {
    MockTransport::new(Scenario::default(), Pace::Stepped)
}

#[test]
fn a_frame_emitted_during_the_fetch_is_applied_exactly_once() {
    let mock = stepped();
    let mut session = Session::connect(&mock, vec![SurfaceId(1)]);
    mock.telegram_tick();
    mock.play_all();
    session.fetch();
    let fetched = session.messages.len();
    session.drain();
    assert_eq!(session.messages.len(), fetched);
    let run = session.only_run();
    let Shape {
        texts,
        tools,
        tasks,
    } = shape(session.run(&run));
    assert_eq!((texts, tools, tasks), (2, 1, 1));
    assert_eq!(session.answers_for(&run), 1);
    assert_eq!(session.last_seq, mock.head());
}

#[test]
fn a_post_ends_as_one_finished_run_with_its_answer() {
    let mock = stepped();
    let mut session = Session::start(&mock, vec![SurfaceId(1)]);
    let client_message_id = session.post(SurfaceId(1), "Лисички появились, что приготовить?");
    session.play();
    let run_id = session.only_run();
    let run = session.run(&run_id);
    let Shape {
        texts,
        tools,
        tasks,
    } = shape(run);
    assert_eq!((texts, tools, tasks), (2, 1, 1));
    assert_eq!(run.segment, "");
    assert_eq!(run.state, RunState::Ok);
    assert_eq!(session.answers_for(&run_id), 1);
    assert!(session.queued.is_empty());
    let mut echoed = 0;
    for message in session.messages.values() {
        if message.client_message_id.as_ref() == Some(&client_message_id) {
            echoed += 1;
        }
    }
    assert_eq!(echoed, 1);
}

#[test]
fn a_delta_before_its_run_started_ends_up_in_the_segment() {
    let mock = stepped();
    let mut session = Session::start(&mock, vec![SurfaceId(1)]);
    session.post(SurfaceId(1), "Лисички?");
    for _ in 0..4 {
        mock.step();
    }
    let mut frames = Vec::new();
    while let Ok(frame) = session.connection.frames.try_recv() {
        frames.push(frame);
    }
    let mut started = None;
    let mut delta = None;
    let mut rest = Vec::new();
    for frame in frames {
        if let Frame::RunStarted(_) = frame {
            started = Some(frame);
        } else if let Frame::TextDelta(_) = frame {
            delta = Some(frame);
        } else {
            rest.push(frame);
        }
    }
    for frame in rest {
        session.apply(frame);
    }
    session.apply(delta.expect("a delta was played"));
    session.apply(started.expect("the run started"));
    let run_id = session.only_run();
    assert_eq!(session.run(&run_id).segment, "Посмотрю, ");
}

#[test]
fn an_interrupted_run_keeps_its_text_and_gets_no_answer() {
    let mock = stepped();
    let mut session = Session::start(&mock, vec![SurfaceId(1)]);
    session.post(SurfaceId(1), "Лисички?");
    session.step(4);
    let run_id = session.only_run();
    session.runs.get_mut(&run_id).expect("live").mark_stopping();
    block_on(session.client.interrupt(&run_id)).expect("interrupted");
    session.play();
    let run = session.run(&run_id);
    assert_eq!(run.state, RunState::Interrupted);
    assert_eq!(run.segment, "Посмотрю, ");
    assert_eq!(session.answers_for(&run_id), 0);
    assert!(session.runs.contains_key(&run_id));
}

#[test]
fn a_reconnect_with_the_last_seq_applies_nothing_twice() {
    let mock = stepped();
    let mut session = Session::start(&mock, vec![SurfaceId(1)]);
    session.post(SurfaceId(1), "Лисички?");
    session.step(6);
    mock.disconnect_all();
    session.play();
    let run_id = session.only_run();
    let run = session.run(&run_id);
    let Shape {
        texts,
        tools,
        tasks,
    } = shape(run);
    assert_eq!((texts, tools, tasks), (2, 1, 1));
    assert_eq!(run.state, RunState::Ok);
    assert_eq!(session.answers_for(&run_id), 1);
    assert_eq!(session.last_seq, mock.head());
    assert_eq!(session.refetches, 0);
}

#[test]
fn a_gap_refetches_and_continues_from_the_head() {
    let mock = MockTransport::new(
        Scenario {
            gap_once: true,
            unavailable_once: false,
        },
        Pace::Stepped,
    );
    let mut session = Session::start(&mock, vec![SurfaceId(1)]);
    session.post(SurfaceId(1), "Лисички?");
    session.step(6);
    mock.disconnect_all();
    mock.play_all();
    session.drain();
    session.reconnect();
    assert_eq!(session.refetches, 1);
    assert_eq!(session.last_seq, mock.head());
    let mut answers = 0;
    for message in session.messages.values() {
        if message.kind == MessageKind::Answer && message.text.starts_with("## Лисички") {
            answers += 1;
        }
    }
    assert_eq!(answers, 1);
}

#[test]
fn a_focused_live_run_arrives_as_a_snapshot_without_duplicate_steps() {
    let mock = stepped();
    let mut session = Session::start(&mock, vec![SurfaceId(1)]);
    let parked = RunId("0b9d2c4e-5a61-4f7e-8c3d-1e2f3a4b5c6d".into());
    assert_eq!(session.run(&parked).steps.len(), 3);
    session
        .connection
        .focus(vec![SurfaceId(1), SurfaceId(2)])
        .expect("open");
    session.drain();
    session.connection.focus(vec![SurfaceId(1)]).expect("open");
    session.drain();
    session
        .connection
        .focus(vec![SurfaceId(1), SurfaceId(2)])
        .expect("open");
    session.drain();
    let run = session.run(&parked);
    assert_eq!(run.steps.len(), 3);
    assert_eq!(run.segment, "Нашёл три новых релиза, ");
    assert_eq!(run.state, RunState::Running);
}

#[test]
fn a2a_keeps_two_runs_live_on_one_surface() {
    let mock = stepped();
    let mut session = Session::start(&mock, vec![SurfaceId(1)]);
    mock.play_a2a();
    session.step(8);
    let mut live_on_general = 0;
    for run in session.runs.values() {
        if run.surface_id == Some(SurfaceId(1)) && run.state == RunState::Running {
            live_on_general += 1;
        }
    }
    assert_eq!(live_on_general, 2);
    session.play();
    for run in session.answered.values() {
        assert_eq!(run.state, RunState::Ok);
    }
    assert_eq!(session.answered.len(), 2);
}
