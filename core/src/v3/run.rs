//! The run reducer: frames of one run folded into a [`Run`].
//!
//! The text model: [`Run::segment`] is the text block in progress only. `text.delta` appends to it,
//! `step.text` records the finished block as a text step and clears it, `run.reset` clears it and
//! the text steps and keeps the tool steps. A `run.snapshot` replaces the segment and the steps
//! wholesale and seeds [`Run::last_seq`] from its `as_of_seq`, so the persisted frames that follow
//! with a seq at or below it are replays and change nothing.
//!
//! Ordering facts a caller has to handle, because the daemon does not order them:
//!
//! - a `text.delta` can arrive before its run's `run.started`; keep it aside until the run exists;
//! - an `input.accepted` can arrive after the `run.started` that claimed its input; ignore it when
//!   the input already belongs to a run;
//! - a `message.created` carrying a `run_id` arrives before that run's `run.finished` for user and
//!   a2a runs, and may arrive after it for scheduled runs;
//! - an interrupted run never gets an answer message; its streamed text stays as it was.

use serde_json::Value;
use time::{Duration, OffsetDateTime};

use super::dto::{
    AgentId, InputId, RowKind, RunId, RunStatus, RunSummary, Seq, StepRow, SurfaceId, ToolUseId,
};
use super::frames::{
    Frame, InputAccepted, RunEvent, RunSnapshot, RunStarted, TaskUpdate, ToolFinished,
};

/// Where a run is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    /// The daemon accepted the input; no `run.started` yet.
    Queued,
    /// Live.
    Running,
    /// An interrupt was accepted; `run.finished` has not arrived.
    Stopping,
    /// Finished successfully.
    Ok,
    /// Finished with an error.
    Error,
    /// Stopped by an interrupt.
    Interrupted,
}

impl RunState {
    /// Returns whether the run has finished.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuclaw_core::v3::RunState;
    ///
    /// assert!(RunState::Interrupted.is_finished());
    /// assert!(!RunState::Stopping.is_finished());
    /// ```
    pub fn is_finished(self) -> bool {
        match self {
            RunState::Queued => false,
            RunState::Running => false,
            RunState::Stopping => false,
            RunState::Ok => true,
            RunState::Error => true,
            RunState::Interrupted => true,
        }
    }
}

/// The state of a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    /// Started, not finished.
    Running,
    /// Finished successfully.
    Ok,
    /// Finished with an error.
    Error,
}

/// What one step of a run is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    /// A finished text segment.
    Text {
        /// The segment.
        text: String,
    },
    /// A tool call.
    Tool {
        /// The tool call's identifier.
        tool_use_id: ToolUseId,
        /// The tool; empty when only the finish was seen.
        name: String,
        /// The tool's JSON input; `Null` when only the finish was seen.
        input: Value,
        /// The result or error text, clamped to 2 KB by the agent.
        output: Option<String>,
        /// Its state.
        status: ToolStatus,
        /// When it finished.
        finished_at: Option<OffsetDateTime>,
    },
    /// A background task update.
    Task {
        /// The task.
        task_id: String,
        /// Its type.
        task_type: String,
        /// Its state.
        state: String,
        /// What it does.
        description: Option<String>,
        /// What it reported.
        summary: Option<String>,
    },
    /// A status line.
    Status {
        /// The status.
        status: String,
        /// Its detail.
        detail: String,
    },
    /// A step row of a kind this build does not know.
    Other {
        /// The row's name.
        name: Option<String>,
        /// The row's output.
        output: Option<String>,
    },
}

/// One step of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// When the step started, when known.
    pub started_at: Option<OffsetDateTime>,
    /// What the step is.
    pub kind: StepKind,
}

impl Step {
    /// Maps a generic step row of `GET /runs/{id}` or `run.snapshot` by its kind.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuclaw_core::v3::{RowKind, Step, StepKind, StepRow};
    /// use time::macros::datetime;
    ///
    /// let row = StepRow {
    ///     seq: 4,
    ///     kind: RowKind::Status,
    ///     tool_use_id: None,
    ///     name: Some("compacting".into()),
    ///     input: None,
    ///     output: Some("context 91%".into()),
    ///     status: None,
    ///     started_at: datetime!(2026-10-03 15:26:05 UTC),
    ///     finished_at: None,
    /// };
    /// let step = Step::from_row(&row);
    /// assert_eq!(
    ///     step.kind,
    ///     StepKind::Status { status: "compacting".into(), detail: "context 91%".into() }
    /// );
    /// ```
    pub fn from_row(row: &StepRow) -> Step {
        let StepRow {
            seq: _,
            kind,
            tool_use_id,
            name,
            input,
            output,
            status,
            started_at,
            finished_at,
        } = row;
        let kind = match kind {
            RowKind::Text => StepKind::Text {
                text: output.clone().unwrap_or_default(),
            },
            RowKind::Tool => StepKind::Tool {
                tool_use_id: tool_use_id
                    .clone()
                    .unwrap_or_else(|| ToolUseId(String::new())),
                name: name.clone().unwrap_or_default(),
                input: input.clone().unwrap_or(Value::Null),
                output: output.clone(),
                status: tool_status(status.as_deref(), finished_at.is_some()),
                finished_at: *finished_at,
            },
            RowKind::Task => task_from_row(name, status, output),
            RowKind::Status => StepKind::Status {
                status: name.clone().unwrap_or_default(),
                detail: output.clone().unwrap_or_default(),
            },
            RowKind::Unknown => StepKind::Other {
                name: name.clone(),
                output: output.clone(),
            },
        };
        Step {
            started_at: Some(*started_at),
            kind,
        }
    }

    fn is_text(&self) -> bool {
        match &self.kind {
            StepKind::Text { .. } => true,
            StepKind::Tool { .. } => false,
            StepKind::Task { .. } => false,
            StepKind::Status { .. } => false,
            StepKind::Other { .. } => false,
        }
    }

    fn is_tool(&self) -> bool {
        match &self.kind {
            StepKind::Text { .. } => false,
            StepKind::Tool { .. } => true,
            StepKind::Task { .. } => false,
            StepKind::Status { .. } => false,
            StepKind::Other { .. } => false,
        }
    }
}

fn tool_status(status: Option<&str>, finished: bool) -> ToolStatus {
    match status {
        Some("ok") => ToolStatus::Ok,
        Some("error") => ToolStatus::Error,
        Some("running") => ToolStatus::Running,
        Some(_) | None => {
            if finished {
                ToolStatus::Ok
            } else {
                ToolStatus::Running
            }
        }
    }
}

fn task_from_row(
    name: &Option<String>,
    status: &Option<String>,
    output: &Option<String>,
) -> StepKind {
    let task_type = name.clone().unwrap_or_default();
    let state = status.clone().unwrap_or_default();
    let Some(body) = output else {
        return StepKind::Task {
            task_id: String::new(),
            task_type,
            state,
            description: None,
            summary: None,
        };
    };
    let Ok(update) = serde_json::from_str::<TaskUpdate>(body) else {
        return StepKind::Task {
            task_id: String::new(),
            task_type,
            state,
            description: None,
            summary: Some(body.clone()),
        };
    };
    let TaskUpdate {
        task_id,
        task_type: _,
        state: _,
        description,
        summary,
    } = update;
    StepKind::Task {
        task_id,
        task_type,
        state,
        description,
        summary,
    }
}

/// What applying a frame did to a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// The run changed.
    Changed,
    /// The frame is this run's but changed nothing: a replay, or a frame with no effect.
    Unchanged,
    /// The frame belongs to another run or to none.
    NotMine,
}

/// One run as a client renders it, built by folding its frames.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::{decode, Applied, Frame, Run, RunState};
///
/// let started = r#"{"v": 1, "seq": 1, "type": "run.started", "run_id": "r1",
///     "payload": {"agent_id": 1, "input_ids": [42]}}"#;
/// let Frame::RunStarted(event) = decode(started).unwrap() else { unreachable!() };
/// let mut run = Run::from_started(&event);
///
/// let delta = r#"{"v": 1, "type": "text.delta", "run_id": "r1", "payload": {"text": "Hi"}}"#;
/// assert_eq!(run.apply(&decode(delta).unwrap()), Applied::Changed);
/// assert_eq!(run.segment, "Hi");
/// assert_eq!(run.state, RunState::Running);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The run; `None` while queued.
    pub id: Option<RunId>,
    /// The agent running it.
    pub agent_id: AgentId,
    /// The surface it is stamped with.
    pub surface_id: Option<SurfaceId>,
    /// Where it is in its life.
    pub state: RunState,
    /// When it started.
    pub started_at: Option<OffsetDateTime>,
    /// When it finished.
    pub finished_at: Option<OffsetDateTime>,
    /// The text block in progress.
    pub segment: String,
    /// The finished steps, in order.
    pub steps: Vec<Step>,
    /// The error it finished with.
    pub error: Option<String>,
    /// Why it finished.
    pub terminal_reason: Option<String>,
    /// The inputs it claimed, or the one it waits to claim while queued.
    pub input_ids: Vec<InputId>,
    /// The newest persisted seq applied.
    pub last_seq: Option<Seq>,
}

impl Run {
    /// Creates the placeholder of an accepted input, shown until its `run.started`.
    pub fn queued(accepted: InputAccepted) -> Run {
        let InputAccepted {
            input_id,
            surface_id,
            agent_id,
        } = accepted;
        Run {
            id: None,
            agent_id,
            surface_id: Some(surface_id),
            state: RunState::Queued,
            started_at: None,
            finished_at: None,
            segment: String::new(),
            steps: Vec::new(),
            error: None,
            terminal_reason: None,
            input_ids: vec![input_id],
            last_seq: None,
        }
    }

    /// Creates a running run from its `run.started`.
    pub fn from_started(event: &RunEvent<RunStarted>) -> Run {
        let mut run = Run {
            id: None,
            agent_id: event.body.agent_id,
            surface_id: event.surface_id,
            state: RunState::Queued,
            started_at: None,
            finished_at: None,
            segment: String::new(),
            steps: Vec::new(),
            error: None,
            terminal_reason: None,
            input_ids: Vec::new(),
            last_seq: None,
        };
        run.begin(event);
        run
    }

    /// Creates a running run from a `run.snapshot`.
    pub fn from_snapshot(snapshot: &RunSnapshot) -> Run {
        let mut run = Run {
            id: Some(snapshot.run_id.clone()),
            agent_id: snapshot.agent_id,
            surface_id: Some(snapshot.surface_id),
            state: RunState::Running,
            started_at: Some(snapshot.started_at),
            finished_at: None,
            segment: String::new(),
            steps: Vec::new(),
            error: None,
            terminal_reason: None,
            input_ids: Vec::new(),
            last_seq: None,
        };
        run.restore(snapshot);
        run
    }

    /// Turns a queued run into the running run whose `run.started` claimed its input.
    ///
    /// Returns whether the event claimed it; an event naming none of its inputs changes nothing.
    pub fn start(&mut self, event: &RunEvent<RunStarted>) -> bool {
        if self.id.is_some() {
            return false;
        }
        let mut claimed = false;
        for input_id in &self.input_ids {
            if event.body.input_ids.contains(input_id) {
                claimed = true;
            }
        }
        if !claimed {
            return false;
        }
        self.begin(event);
        true
    }

    /// Marks a live run as stopping after the daemon accepted an interrupt.
    ///
    /// Its `run.finished` overwrites the state.
    pub fn mark_stopping(&mut self) {
        if self.state == RunState::Running {
            self.state = RunState::Stopping;
        }
    }

    /// Folds one frame into the run.
    pub fn apply(&mut self, frame: &Frame) -> Applied {
        let Some(run_id) = frame.run_id() else {
            return Applied::NotMine;
        };
        if self.id.as_ref() != Some(run_id) {
            return Applied::NotMine;
        }
        if let (Some(seq), Some(last_seq)) = (frame.seq(), self.last_seq)
            && seq <= last_seq
        {
            return Applied::Unchanged;
        }
        let applied = match frame {
            Frame::Hello(_) => Applied::NotMine,
            Frame::Gap(_) => Applied::NotMine,
            Frame::InputAccepted(_) => Applied::NotMine,
            Frame::Unknown(_) => Applied::NotMine,
            Frame::MessageCreated(_) => Applied::Unchanged,
            Frame::TaskFired(_) => Applied::NotMine,
            Frame::RunSnapshot(snapshot) => self.apply_snapshot(snapshot),
            Frame::RunStarted(event) => {
                self.begin(event);
                Applied::Changed
            }
            Frame::StepText(event) => {
                self.segment.clear();
                self.steps.push(Step {
                    started_at: event.at,
                    kind: StepKind::Text {
                        text: event.body.text.clone(),
                    },
                });
                Applied::Changed
            }
            Frame::ToolStarted(event) => {
                self.steps.push(Step {
                    started_at: event.at,
                    kind: StepKind::Tool {
                        tool_use_id: event.body.tool_use_id.clone(),
                        name: event.body.name.clone(),
                        input: event.body.input.clone(),
                        output: None,
                        status: ToolStatus::Running,
                        finished_at: None,
                    },
                });
                Applied::Changed
            }
            Frame::ToolFinished(event) => {
                self.finish_tool(event);
                Applied::Changed
            }
            Frame::Task(event) => {
                let TaskUpdate {
                    task_id,
                    task_type,
                    state,
                    description,
                    summary,
                } = event.body.clone();
                self.steps.push(Step {
                    started_at: event.at,
                    kind: StepKind::Task {
                        task_id,
                        task_type,
                        state,
                        description,
                        summary,
                    },
                });
                Applied::Changed
            }
            Frame::Status(event) => {
                self.steps.push(Step {
                    started_at: event.at,
                    kind: StepKind::Status {
                        status: event.body.status.clone(),
                        detail: event.body.detail.clone(),
                    },
                });
                Applied::Changed
            }
            Frame::RunReset(_) => {
                self.segment.clear();
                let mut kept = Vec::new();
                for step in self.steps.drain(..) {
                    if !step.is_text() {
                        kept.push(step);
                    }
                }
                self.steps = kept;
                Applied::Changed
            }
            Frame::RunFinished(event) => {
                self.finished_at = event.at;
                self.error = event.body.error.clone();
                self.terminal_reason = Some(event.body.terminal_reason.clone());
                self.state = if event.body.terminal_reason == "interrupted" {
                    RunState::Interrupted
                } else if event.body.is_error {
                    RunState::Error
                } else {
                    RunState::Ok
                };
                Applied::Changed
            }
            Frame::TextDelta(delta) => {
                if self.state.is_finished() {
                    return Applied::Unchanged;
                }
                self.segment.push_str(&delta.text);
                Applied::Changed
            }
        };
        if applied == Applied::Changed
            && let Some(seq) = frame.seq()
        {
            self.last_seq = Some(seq);
        }
        applied
    }

    /// Returns the counts a message carries about this run.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuclaw_core::v3::{AgentId, InputAccepted, InputId, Run, RunStatus, SurfaceId};
    ///
    /// let run = Run::queued(InputAccepted {
    ///     input_id: InputId(42),
    ///     surface_id: SurfaceId(1),
    ///     agent_id: AgentId(1),
    /// });
    /// let summary = run.summary();
    /// assert_eq!(summary.status, RunStatus::Running);
    /// assert_eq!(summary.step_count, 0);
    /// ```
    pub fn summary(&self) -> RunSummary {
        let mut tool_count = 0;
        for step in &self.steps {
            if step.is_tool() {
                tool_count += 1;
            }
        }
        let duration_ms =
            if let (Some(started_at), Some(finished_at)) = (self.started_at, self.finished_at) {
                u64::try_from((finished_at - started_at).whole_milliseconds()).unwrap_or(0)
            } else {
                0
            };
        RunSummary {
            status: self.status(),
            step_count: u32::try_from(self.steps.len()).unwrap_or(u32::MAX),
            tool_count,
            duration_ms,
        }
    }

    /// Returns how long the run has taken: until it finished, or until `now` while live.
    ///
    /// `None` before it started.
    pub fn elapsed(&self, now: OffsetDateTime) -> Option<Duration> {
        let started_at = self.started_at?;
        let until = self.finished_at.unwrap_or(now);
        Some(until - started_at)
    }

    fn status(&self) -> RunStatus {
        match self.state {
            RunState::Queued => RunStatus::Running,
            RunState::Running => RunStatus::Running,
            RunState::Stopping => RunStatus::Running,
            RunState::Ok => RunStatus::Ok,
            RunState::Error => RunStatus::Error,
            RunState::Interrupted => RunStatus::Interrupted,
        }
    }

    fn begin(&mut self, event: &RunEvent<RunStarted>) {
        self.id = Some(event.run_id.clone());
        self.agent_id = event.body.agent_id;
        if event.surface_id.is_some() {
            self.surface_id = event.surface_id;
        }
        self.input_ids = event.body.input_ids.clone();
        if self.started_at.is_none() {
            self.started_at = event.at;
        }
        if self.state == RunState::Queued {
            self.state = RunState::Running;
        }
        self.last_seq = Some(event.seq);
    }

    fn apply_snapshot(&mut self, snapshot: &RunSnapshot) -> Applied {
        if self.state.is_finished() {
            return Applied::Unchanged;
        }
        if let Some(last_seq) = self.last_seq
            && last_seq > snapshot.as_of_seq
        {
            return Applied::Unchanged;
        }
        self.restore(snapshot);
        Applied::Changed
    }

    fn restore(&mut self, snapshot: &RunSnapshot) {
        let mut steps = Vec::new();
        for row in &snapshot.steps {
            steps.push(Step::from_row(row));
        }
        self.steps = steps;
        self.segment = snapshot.text.clone();
        self.agent_id = snapshot.agent_id;
        self.surface_id = Some(snapshot.surface_id);
        self.started_at = Some(snapshot.started_at);
        if self.state == RunState::Queued {
            self.state = RunState::Running;
        }
        self.last_seq = Some(snapshot.as_of_seq);
    }

    fn finish_tool(&mut self, event: &RunEvent<ToolFinished>) {
        let body = &event.body;
        let output = if body.is_error
            && let Some(error) = &body.error
        {
            error.clone()
        } else {
            body.summary.clone()
        };
        let status = if body.is_error {
            ToolStatus::Error
        } else {
            ToolStatus::Ok
        };
        for step in self.steps.iter_mut().rev() {
            let StepKind::Tool {
                tool_use_id,
                name: _,
                input: _,
                output: step_output,
                status: step_status,
                finished_at,
            } = &mut step.kind
            else {
                continue;
            };
            if *tool_use_id != body.tool_use_id {
                continue;
            }
            *step_output = Some(output);
            *step_status = status;
            *finished_at = event.at;
            return;
        }
        self.steps.push(Step {
            started_at: event.at,
            kind: StepKind::Tool {
                tool_use_id: body.tool_use_id.clone(),
                name: String::new(),
                input: Value::Null,
                output: Some(output),
                status,
                finished_at: event.at,
            },
        });
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use time::macros::datetime;

    use super::*;
    use crate::v3::frames::decode;
    use crate::v3::golden::frame;

    const RUN_ID: &str = "6763eb02-7f3e-4c4d-9b1a-2f0c5d8e9a11";

    fn event(seq: i64, type_name: &str, payload: Value) -> Frame {
        let line = json!({
            "v": 1, "seq": seq, "type": type_name, "surface_id": 1, "run_id": RUN_ID,
            "at": "2026-10-03T15:26:10Z", "payload": payload
        });
        decode(&line.to_string()).unwrap()
    }

    fn delta(text: &str) -> Frame {
        let line = json!({"v": 1, "type": "text.delta", "surface_id": 1, "run_id": RUN_ID, "payload": {"text": text}});
        decode(&line.to_string()).unwrap()
    }

    fn started() -> Run {
        let Frame::RunStarted(event) = frame("run_started") else {
            panic!("expected run.started");
        };
        Run::from_started(&event)
    }

    fn snapshot() -> RunSnapshot {
        let Frame::RunSnapshot(snapshot) = frame("run_snapshot") else {
            panic!("expected run.snapshot");
        };
        snapshot
    }

    fn texts(run: &Run) -> Vec<String> {
        let mut texts = Vec::new();
        for step in &run.steps {
            if let StepKind::Text { text } = &step.kind {
                texts.push(text.clone());
            }
        }
        texts
    }

    fn tool_count(run: &Run) -> usize {
        let mut count = 0;
        for step in &run.steps {
            if step.is_tool() {
                count += 1;
            }
        }
        count
    }

    #[test]
    fn from_started_is_running_with_its_inputs() {
        let run = started();
        assert_eq!(run.id, Some(RunId(RUN_ID.into())));
        assert_eq!(run.state, RunState::Running);
        assert_eq!(run.input_ids, vec![InputId(42)]);
        assert_eq!(run.surface_id, Some(SurfaceId(1)));
        assert_eq!(run.started_at, Some(datetime!(2026-10-03 15:26:00 UTC)));
        assert_eq!(run.last_seq, Some(Seq(1202)));
    }

    #[test]
    fn every_fixture_frame_applies_to_a_running_run() {
        let cases = [
            ("step_text", Applied::Changed),
            ("step_tool_started", Applied::Changed),
            ("step_tool_finished", Applied::Changed),
            ("step_task", Applied::Changed),
            ("step_status", Applied::Changed),
            ("run_reset", Applied::Changed),
            ("step_tool_started_truncated", Applied::Changed),
            ("message_created", Applied::Unchanged),
            ("text_delta", Applied::Changed),
            ("run_finished", Applied::Changed),
            ("hello", Applied::NotMine),
            ("gap", Applied::NotMine),
            ("input_accepted", Applied::NotMine),
            ("unknown", Applied::NotMine),
            ("event_frame", Applied::NotMine),
        ];
        let mut run = started();
        for (name, expected) in cases {
            assert_eq!(run.apply(&frame(name)), expected, "{name}");
        }
        assert_eq!(run.state, RunState::Ok);
        assert_eq!(run.last_seq, Some(Seq(1211)));
        assert_eq!(texts(&run), Vec::<String>::new());
        assert_eq!(tool_count(&run), 2);
    }

    #[test]
    fn every_fixture_frame_on_a_finished_run_is_a_replay_or_ignored() {
        let mut run = started();
        run.apply(&frame("run_finished"));
        for name in [
            "run_started",
            "step_text",
            "step_tool_started",
            "step_tool_finished",
            "step_task",
            "step_status",
            "run_reset",
            "run_finished",
            "message_created",
            "text_delta",
            "run_snapshot",
        ] {
            assert_eq!(run.apply(&frame(name)), Applied::Unchanged, "{name}");
        }
        assert_eq!(run.state, RunState::Ok);
    }

    #[test]
    fn deltas_then_step_text_leave_one_text_step_and_an_empty_segment() {
        let mut run = started();
        assert_eq!(run.apply(&delta("Посмотрю, ")), Applied::Changed);
        assert_eq!(run.apply(&delta("что есть в заметках.")), Applied::Changed);
        assert_eq!(run.segment, "Посмотрю, что есть в заметках.");
        assert_eq!(run.apply(&frame("step_text")), Applied::Changed);
        assert_eq!(run.segment, "");
        assert_eq!(
            texts(&run),
            vec!["Посмотрю, что есть в заметках.".to_string()]
        );
    }

    #[test]
    fn a_replayed_seq_is_unchanged() {
        let mut run = started();
        assert_eq!(run.apply(&frame("step_text")), Applied::Changed);
        assert_eq!(run.apply(&frame("step_text")), Applied::Unchanged);
        assert_eq!(run.apply(&frame("run_started")), Applied::Unchanged);
        assert_eq!(texts(&run).len(), 1);
    }

    #[test]
    fn frames_of_another_run_are_not_mine() {
        let mut run = started();
        let other = json!({"v": 1, "seq": 1300, "type": "step.text", "run_id": "other", "payload": {"text": "x"}});
        assert_eq!(
            run.apply(&decode(&other.to_string()).unwrap()),
            Applied::NotMine
        );
        let other =
            json!({"v": 1, "type": "text.delta", "run_id": "other", "payload": {"text": "x"}});
        assert_eq!(
            run.apply(&decode(&other.to_string()).unwrap()),
            Applied::NotMine
        );
        assert!(run.steps.is_empty());
        assert_eq!(run.segment, "");
    }

    #[test]
    fn after_a_snapshot_frames_at_or_below_as_of_seq_are_replays() {
        let mut run = Run::from_snapshot(&snapshot());
        assert_eq!(run.last_seq, Some(Seq(1207)));
        assert_eq!(run.steps.len(), 4);
        assert_eq!(run.segment, "Сливки лучше брать");
        for name in [
            "run_started",
            "step_text",
            "step_tool_started",
            "step_tool_finished",
            "step_task",
            "step_status",
        ] {
            assert_eq!(run.apply(&frame(name)), Applied::Unchanged, "{name}");
        }
        assert_eq!(run.steps.len(), 4);
        assert_eq!(
            run.apply(&event(
                1208,
                "step.text",
                json!({"text": "Сливки лучше брать 20%."})
            )),
            Applied::Changed
        );
        assert_eq!(run.steps.len(), 5);
        assert_eq!(run.segment, "");
    }

    #[test]
    fn a_snapshot_replaces_segment_and_steps_wholesale() {
        let mut run = started();
        run.apply(&event(1203, "step.text", json!({"text": "draft"})));
        run.apply(&delta("half a sente"));
        assert_eq!(run.apply(&frame("run_snapshot")), Applied::Changed);
        assert_eq!(run.steps.len(), 4);
        assert_eq!(
            texts(&run),
            vec!["Посмотрю, что есть в заметках.".to_string()]
        );
        assert_eq!(run.segment, "Сливки лучше брать");
        assert_eq!(run.last_seq, Some(Seq(1207)));
        assert_eq!(run.state, RunState::Running);
    }

    #[test]
    fn a_stale_snapshot_is_unchanged() {
        let mut run = started();
        run.apply(&event(1250, "step.text", json!({"text": "newer"})));
        assert_eq!(run.apply(&frame("run_snapshot")), Applied::Unchanged);
        assert_eq!(texts(&run), vec!["newer".to_string()]);
    }

    #[test]
    fn a_snapshot_keeps_stopping() {
        let mut run = started();
        run.mark_stopping();
        assert_eq!(run.apply(&frame("run_snapshot")), Applied::Changed);
        assert_eq!(run.state, RunState::Stopping);
    }

    #[test]
    fn run_reset_drops_text_and_keeps_tools() {
        let mut run = started();
        run.apply(&frame("step_text"));
        run.apply(&frame("step_tool_started"));
        run.apply(&frame("step_tool_finished"));
        run.apply(&frame("step_task"));
        run.apply(&delta("leaked <invoke"));
        assert_eq!(run.apply(&frame("run_reset")), Applied::Changed);
        assert_eq!(run.segment, "");
        assert!(texts(&run).is_empty());
        assert_eq!(run.steps.len(), 2);
        assert_eq!(tool_count(&run), 1);
    }

    #[test]
    fn tool_finished_completes_its_started_step() {
        let mut run = started();
        run.apply(&frame("step_tool_started"));
        run.apply(&frame("step_tool_finished"));
        let StepKind::Tool {
            name,
            output,
            status,
            finished_at,
            input,
            ..
        } = &run.steps[0].kind
        else {
            panic!("expected a tool step");
        };
        assert_eq!(name, "Bash");
        assert_eq!(input["command"], json!("rg -i лисич ~/notes"));
        assert_eq!(output.as_deref(), Some("recipes.md: лисички со сливками"));
        assert_eq!(*status, ToolStatus::Ok);
        assert_eq!(*finished_at, Some(datetime!(2026-10-03 15:26:04 UTC)));
        assert_eq!(run.steps.len(), 1);
    }

    #[test]
    fn a_failed_tool_shows_its_error() {
        let mut run = started();
        run.apply(&event(
            1203,
            "step.tool_started",
            json!({"tool_use_id": "t1", "name": "Bash", "input": {}}),
        ));
        run.apply(&event(
            1204,
            "step.tool_finished",
            json!({"tool_use_id": "t1", "is_error": true, "error": "exit 1", "summary": "partial"}),
        ));
        let StepKind::Tool { output, status, .. } = &run.steps[0].kind else {
            panic!("expected a tool step");
        };
        assert_eq!(output.as_deref(), Some("exit 1"));
        assert_eq!(*status, ToolStatus::Error);
    }

    #[test]
    fn a_finish_for_an_unknown_tool_pushes_a_finished_step() {
        let mut run = started();
        assert_eq!(run.apply(&frame("step_tool_finished")), Applied::Changed);
        let StepKind::Tool {
            name,
            input,
            status,
            output,
            ..
        } = &run.steps[0].kind
        else {
            panic!("expected a tool step");
        };
        assert_eq!(name, "");
        assert_eq!(*input, Value::Null);
        assert_eq!(*status, ToolStatus::Ok);
        assert_eq!(output.as_deref(), Some("recipes.md: лисички со сливками"));
    }

    #[test]
    fn run_finished_maps_the_terminal_state() {
        let cases = [
            (
                json!({"is_error": false, "terminal_reason": "success"}),
                RunState::Ok,
                None,
            ),
            (
                json!({"is_error": false, "terminal_reason": "interrupted"}),
                RunState::Interrupted,
                None,
            ),
            (
                json!({"is_error": true, "terminal_reason": "interrupted"}),
                RunState::Interrupted,
                None,
            ),
            (
                json!({"is_error": true, "error": "boom", "terminal_reason": "agent_crashed"}),
                RunState::Error,
                Some("boom"),
            ),
        ];
        for (payload, expected, error) in cases {
            let mut run = started();
            run.mark_stopping();
            assert_eq!(run.state, RunState::Stopping);
            run.apply(&event(1300, "run.finished", payload.clone()));
            assert_eq!(run.state, expected, "{payload}");
            assert_eq!(run.error.as_deref(), error, "{payload}");
            assert_eq!(run.finished_at, Some(datetime!(2026-10-03 15:26:10 UTC)));
        }
    }

    #[test]
    fn mark_stopping_only_affects_a_live_run() {
        let mut queued = Run::queued(InputAccepted {
            input_id: InputId(42),
            surface_id: SurfaceId(1),
            agent_id: AgentId(1),
        });
        queued.mark_stopping();
        assert_eq!(queued.state, RunState::Queued);
        let mut finished = started();
        finished.apply(&frame("run_finished"));
        finished.mark_stopping();
        assert_eq!(finished.state, RunState::Ok);
    }

    #[test]
    fn a_delta_after_the_finish_is_ignored() {
        let mut run = started();
        run.apply(&frame("run_finished_interrupted"));
        assert_eq!(run.state, RunState::Interrupted);
        assert_eq!(run.apply(&delta("late")), Applied::Unchanged);
        assert_eq!(run.segment, "");
    }

    #[test]
    fn an_interrupted_run_keeps_its_streamed_text() {
        let mut run = started();
        run.apply(&delta("Сливки "));
        run.apply(&frame("run_finished_interrupted"));
        assert_eq!(run.segment, "Сливки ");
        assert_eq!(run.state, RunState::Interrupted);
    }

    #[test]
    fn a_queued_run_is_started_by_the_run_that_claims_its_input() {
        let Frame::InputAccepted(accepted) = frame("input_accepted") else {
            panic!("expected input.accepted");
        };
        let mut run = Run::queued(accepted);
        assert_eq!(run.state, RunState::Queued);
        assert_eq!(run.id, None);
        assert_eq!(run.apply(&frame("step_text")), Applied::NotMine);

        let other = json!({"v": 1, "seq": 5, "type": "run.started", "run_id": "other", "payload": {"agent_id": 1, "input_ids": [7]}});
        let Frame::RunStarted(other) = decode(&other.to_string()).unwrap() else {
            panic!("expected run.started");
        };
        assert!(!run.start(&other));
        assert_eq!(run.state, RunState::Queued);

        let Frame::RunStarted(claiming) = frame("run_started") else {
            panic!("expected run.started");
        };
        assert!(run.start(&claiming));
        assert_eq!(run.state, RunState::Running);
        assert_eq!(run.id, Some(RunId(RUN_ID.into())));
        assert!(!run.start(&claiming));
        assert_eq!(run.apply(&frame("step_text")), Applied::Changed);
    }

    #[test]
    fn summary_counts_steps_tools_and_duration() {
        let mut run = started();
        for name in [
            "step_text",
            "step_tool_started",
            "step_tool_finished",
            "step_task",
            "step_status",
        ] {
            run.apply(&frame(name));
        }
        assert_eq!(
            run.summary(),
            RunSummary {
                status: RunStatus::Running,
                step_count: 4,
                tool_count: 1,
                duration_ms: 0,
            }
        );
        run.apply(&frame("run_finished"));
        assert_eq!(
            run.summary(),
            RunSummary {
                status: RunStatus::Ok,
                step_count: 4,
                tool_count: 1,
                duration_ms: 13_000,
            }
        );
    }

    #[test]
    fn elapsed_runs_to_now_while_live_and_to_the_finish_after() {
        let mut run = started();
        let now = datetime!(2026-10-03 15:26:05 UTC);
        assert_eq!(run.elapsed(now), Some(Duration::seconds(5)));
        run.apply(&frame("run_finished"));
        assert_eq!(run.elapsed(now), Some(Duration::seconds(13)));
        let Frame::InputAccepted(accepted) = frame("input_accepted") else {
            panic!("expected input.accepted");
        };
        assert_eq!(Run::queued(accepted).elapsed(now), None);
    }

    #[test]
    fn rows_map_by_kind() {
        let snapshot = snapshot();
        let mut kinds = Vec::new();
        for row in &snapshot.steps {
            kinds.push(Step::from_row(row).kind);
        }
        assert_eq!(
            kinds[0],
            StepKind::Text {
                text: "Посмотрю, что есть в заметках.".into()
            }
        );
        let StepKind::Tool {
            status,
            finished_at,
            ..
        } = &kinds[1]
        else {
            panic!("expected a tool step");
        };
        assert_eq!(*status, ToolStatus::Ok);
        assert!(finished_at.is_some());
        assert_eq!(
            kinds[2],
            StepKind::Task {
                task_id: "t1".into(),
                task_type: "local_agent".into(),
                state: "running".into(),
                description: Some("search the recipe site".into()),
                summary: None,
            }
        );
        assert_eq!(
            kinds[3],
            StepKind::Status {
                status: "compacting".into(),
                detail: "context 91%".into(),
            }
        );
    }

    #[test]
    fn odd_rows_still_map() {
        let row = |value: Value| -> StepRow { serde_json::from_value(value).unwrap() };
        let task = row(
            json!({"seq": 1, "kind": "task", "name": "bash", "status": "done", "output": "not json", "started_at": "2026-10-03T15:26:00Z"}),
        );
        assert_eq!(
            Step::from_row(&task).kind,
            StepKind::Task {
                task_id: String::new(),
                task_type: "bash".into(),
                state: "done".into(),
                description: None,
                summary: Some("not json".into()),
            }
        );
        let unknown = row(
            json!({"seq": 2, "kind": "thinking", "name": "x", "output": "y", "started_at": "2026-10-03T15:26:00Z"}),
        );
        assert_eq!(
            Step::from_row(&unknown).kind,
            StepKind::Other {
                name: Some("x".into()),
                output: Some("y".into()),
            }
        );
        let cases = [
            (json!("running"), false, ToolStatus::Running),
            (json!("error"), true, ToolStatus::Error),
            (Value::Null, false, ToolStatus::Running),
            (Value::Null, true, ToolStatus::Ok),
            (json!("weird"), true, ToolStatus::Ok),
        ];
        for (status, finished, expected) in cases {
            let mut value = json!({"seq": 3, "kind": "tool", "tool_use_id": "t", "name": "Read", "status": status, "started_at": "2026-10-03T15:26:00Z"});
            if finished {
                value["finished_at"] = json!("2026-10-03T15:26:01Z");
            }
            let StepKind::Tool { status: mapped, .. } = Step::from_row(&row(value)).kind else {
                panic!("expected a tool step");
            };
            assert_eq!(mapped, expected);
        }
    }
}
