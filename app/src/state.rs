use std::collections::{BTreeMap, HashMap};

use anyhow::{Result, bail};
use futures::StreamExt;
use futures::channel::mpsc::UnboundedSender;
use gpui::{AsyncApp, Context, EventEmitter, Task, WeakEntity};
use time::OffsetDateTime;
use tuclaw_core::model::{Agent, Author, Channel, ChannelId, Message, MessageId, Span};
use tuclaw_core::v3::{
    self, Applied, Backoff, ClientFrame, ClientMessageId, Frame, InputAccepted, Post, Run, RunId,
    RunState, Seq, TextDelta,
};

use crate::link::{self, Source};

const PAGE: u32 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Conversation,
    Agents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarVisibility {
    Shown,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    Channel,
    Agents,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Connecting,
    Live,
    Reconnecting,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateEvent {
    SelectionChanged,
    MessagesLoaded,
    MessageAppended,
    RunsChanged,
    SendFailed(String),
}

struct Pending {
    client_message_id: ClientMessageId,
    local: MessageId,
}

pub struct AppState {
    client: v3::Client,
    source: Source,
    link: Link,
    surfaces: Vec<v3::Surface>,
    directory: Vec<v3::Agent>,
    agents: Vec<Agent>,
    channels: Vec<Channel>,
    selected: Option<ChannelId>,
    messages: Vec<Message>,
    runs: BTreeMap<RunId, Run>,
    queued: Vec<Run>,
    early: HashMap<RunId, Vec<TextDelta>>,
    pending: Vec<Pending>,
    next_local: i64,
    last_seq: Option<Seq>,
    control: Option<UnboundedSender<ClientFrame>>,
    view: View,
    sidebar: SidebarVisibility,
    _link: Option<Task<()>>,
}

impl EventEmitter<StateEvent> for AppState {}

impl AppState {
    pub fn new(client: v3::Client, source: Source) -> AppState {
        AppState {
            client,
            source,
            link: Link::Connecting,
            surfaces: Vec::new(),
            directory: Vec::new(),
            agents: Vec::new(),
            channels: Vec::new(),
            selected: None,
            messages: Vec::new(),
            runs: BTreeMap::new(),
            queued: Vec::new(),
            early: HashMap::new(),
            pending: Vec::new(),
            next_local: 0,
            last_seq: None,
            control: None,
            view: View::Conversation,
            sidebar: SidebarVisibility::Shown,
            _link: None,
        }
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        self._link = Some(cx.spawn(async move |this, cx| run_link(this, client, cx).await));
    }

    pub fn agents(&self) -> &[Agent] {
        &self.agents
    }

    pub fn channels(&self) -> &[Channel] {
        &self.channels
    }

    pub fn selected(&self) -> Option<ChannelId> {
        self.selected
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn view(&self) -> View {
        self.view
    }

    pub fn sidebar(&self) -> SidebarVisibility {
        self.sidebar
    }

    pub fn link(&self) -> &Link {
        &self.link
    }

    pub fn source(&self) -> &Source {
        &self.source
    }

    pub fn live_runs(&self) -> Vec<&Run> {
        let Some(selected) = self.selected else {
            return Vec::new();
        };
        let surface = link::surface_id(selected);
        let mut runs = Vec::new();
        for run in self.runs.values() {
            if run.surface_id == Some(surface) {
                runs.push(run);
            }
        }
        for run in &self.queued {
            if run.surface_id == Some(surface) {
                runs.push(run);
            }
        }
        runs
    }

    pub fn wired_agents(&self, channel: ChannelId) -> usize {
        let surface = link::surface_id(channel);
        let mut count = 0;
        for candidate in &self.surfaces {
            if candidate.id == surface {
                count = candidate.agents.len();
            }
        }
        count
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar = match self.sidebar {
            SidebarVisibility::Shown => SidebarVisibility::Hidden,
            SidebarVisibility::Hidden => SidebarVisibility::Shown,
        };
        cx.notify();
    }

    pub fn select(&mut self, channel: ChannelId, cx: &mut Context<Self>) {
        let mut known = false;
        for candidate in &self.channels {
            if candidate.id == channel {
                known = true;
            }
        }
        if !known {
            return;
        }
        self.view = View::Conversation;
        if self.selected != Some(channel) {
            self.selected = Some(channel);
            self.messages.clear();
            self.pending.clear();
            self.forget_finished_runs();
            self.send_focus();
            self.load_page(channel, cx);
        }
        cx.emit(StateEvent::SelectionChanged);
        cx.notify();
    }

    pub fn active_segment(&self) -> Segment {
        match self.view {
            View::Agents => Segment::Agents,
            View::Conversation => Segment::Channel,
        }
    }

    pub fn activate_segment(&mut self, segment: Segment, cx: &mut Context<Self>) {
        match segment {
            Segment::Agents => self.view = View::Agents,
            Segment::Channel => self.view = View::Conversation,
        }
        cx.notify();
    }

    pub fn send(&mut self, body: String, cx: &mut Context<Self>) -> Result<()> {
        let text = body.trim().to_string();
        if text.is_empty() {
            return Ok(());
        }
        let Some(channel) = self.selected else {
            bail!("no channel is selected");
        };
        let client_message_id = ClientMessageId::random();
        self.next_local -= 1;
        let local = MessageId(self.next_local);
        self.messages.push(Message {
            id: local,
            author: Author::User,
            body: vec![Span::Text(text.clone())],
            sent_at: OffsetDateTime::now_utc(),
        });
        self.pending.push(Pending {
            client_message_id: client_message_id.clone(),
            local,
        });
        let post = Post {
            text: text.clone(),
            addressed_agent_id: self.addressed(channel, &text),
            client_message_id: client_message_id.clone(),
        };
        let request = self.client.post(link::surface_id(channel), &post);
        cx.spawn(async move |this, cx| {
            let posted = request.await;
            this.update(cx, |state, cx| {
                state.posted(client_message_id, posted, body, cx)
            })
            .ok();
        })
        .detach();
        cx.emit(StateEvent::MessageAppended);
        cx.notify();
        Ok(())
    }

    pub fn interrupt(&mut self, run: &RunId, cx: &mut Context<Self>) {
        let Some(live) = self.runs.get_mut(run) else {
            return;
        };
        live.mark_stopping();
        let request = self.client.interrupt(run);
        cx.spawn(async move |_this, _cx| {
            request.await.ok();
        })
        .detach();
        cx.emit(StateEvent::RunsChanged);
        cx.notify();
    }

    fn addressed(&self, channel: ChannelId, text: &str) -> Option<v3::AgentId> {
        let ident = text.strip_prefix('@')?;
        let mut name = String::new();
        for letter in ident.chars() {
            if letter.is_alphanumeric() || letter == '_' {
                name.push(letter);
            } else {
                break;
            }
        }
        let surface = link::surface_id(channel);
        let mut found = None;
        for candidate in &self.surfaces {
            if candidate.id != surface {
                continue;
            }
            for wiring in &candidate.agents {
                for agent in &self.directory {
                    if agent.id == wiring.agent_id && agent.ident == name {
                        found = Some(agent.id);
                    }
                }
            }
        }
        found
    }

    fn posted(
        &mut self,
        client_message_id: ClientMessageId,
        posted: Result<v3::Posted, v3::ApiError>,
        body: String,
        cx: &mut Context<Self>,
    ) {
        let mut index = None;
        for (position, pending) in self.pending.iter().enumerate() {
            if pending.client_message_id == client_message_id {
                index = Some(position);
            }
        }
        let Some(index) = index else {
            return;
        };
        let local = self.pending[index].local;
        match posted {
            Ok(posted) => {
                let v3::MessageId(real) = posted.message_id;
                let real = MessageId(real);
                let mut seen = false;
                for message in &self.messages {
                    if message.id == real {
                        seen = true;
                    }
                }
                if seen {
                    self.messages.retain(|message| message.id != local);
                } else {
                    for message in &mut self.messages {
                        if message.id == local {
                            message.id = real;
                        }
                    }
                }
                self.pending.remove(index);
            }
            Err(_error) => {
                self.pending.remove(index);
                self.messages.retain(|message| message.id != local);
                cx.emit(StateEvent::SendFailed(body));
            }
        }
        cx.notify();
    }

    fn load_page(&self, channel: ChannelId, cx: &mut Context<Self>) {
        let request = self.client.messages(link::surface_id(channel), PAGE);
        cx.spawn(async move |this, cx| {
            let page = request.await;
            this.update(cx, |state, cx| state.page_loaded(channel, page, cx))
                .ok();
        })
        .detach();
    }

    fn page_loaded(
        &mut self,
        channel: ChannelId,
        page: Result<v3::MessagesPage, v3::ApiError>,
        cx: &mut Context<Self>,
    ) {
        if self.selected != Some(channel) {
            return;
        }
        let page = match page {
            Ok(page) => page,
            Err(error) => {
                self.link = Link::Failed(error.to_string());
                cx.notify();
                return;
            }
        };
        let mut messages = Vec::new();
        for message in &page.messages {
            messages.push(link::message(message));
        }
        for message in &self.messages {
            let mut fetched = false;
            for candidate in &messages {
                if candidate.id == message.id {
                    fetched = true;
                }
            }
            if !fetched {
                messages.push(message.clone());
            }
        }
        self.messages = messages;
        cx.emit(StateEvent::MessagesLoaded);
        cx.notify();
    }

    fn directory_loaded(
        &mut self,
        surfaces: Vec<v3::Surface>,
        agents: Vec<v3::Agent>,
        cx: &mut Context<Self>,
    ) {
        let mut channels = Vec::new();
        for surface in &surfaces {
            channels.push(link::channel(surface));
        }
        self.channels = channels;
        self.surfaces = surfaces;
        self.directory = agents;
        self.refresh_agents();
        let selected = match self.selected {
            Some(selected) => Some(selected),
            None => self.channels.first().map(|channel| channel.id),
        };
        if let Some(selected) = selected {
            self.selected = Some(selected);
            self.send_focus();
            self.load_page(selected, cx);
        }
        cx.emit(StateEvent::SelectionChanged);
        cx.notify();
    }

    fn connected(
        &mut self,
        head: Seq,
        control: UnboundedSender<ClientFrame>,
        fresh: bool,
        cx: &mut Context<Self>,
    ) {
        if fresh {
            self.last_seq = Some(head);
        }
        self.control = Some(control);
        self.link = Link::Live;
        self.send_focus();
        cx.notify();
    }

    fn disconnected(&mut self, cx: &mut Context<Self>) -> Option<Seq> {
        self.control = None;
        self.link = Link::Reconnecting;
        cx.notify();
        self.last_seq
    }

    fn failed(&mut self, reason: String, cx: &mut Context<Self>) {
        self.link = Link::Failed(reason);
        cx.notify();
    }

    fn send_focus(&self) {
        let (Some(control), Some(selected)) = (&self.control, self.selected) else {
            return;
        };
        let focus = ClientFrame::Focus {
            surface_ids: vec![link::surface_id(selected)],
        };
        control.unbounded_send(focus).ok();
    }

    fn forget_finished_runs(&mut self) {
        let mut live = BTreeMap::new();
        for (id, run) in std::mem::take(&mut self.runs) {
            if !run.state.is_finished() {
                live.insert(id, run);
            }
        }
        self.runs = live;
    }

    fn refresh_agents(&mut self) {
        let mut agents = Vec::new();
        for agent in &self.directory {
            let mut busy_on = None;
            for run in self.runs.values() {
                if run.agent_id != agent.id || run.state.is_finished() {
                    continue;
                }
                for surface in &self.surfaces {
                    if Some(surface.id) == run.surface_id {
                        busy_on = Some(surface.name.as_str());
                    }
                }
            }
            agents.push(link::agent(agent, busy_on));
        }
        self.agents = agents;
    }

    fn apply(&mut self, frame: Frame, cx: &mut Context<Self>) {
        if let Some(seq) = frame.seq()
            && self.last_seq.is_none_or(|last| seq > last)
        {
            self.last_seq = Some(seq);
        }
        let changed = match &frame {
            Frame::Hello(hello) => {
                self.last_seq = Some(hello.head);
                false
            }
            Frame::Gap(_) => {
                self.runs.clear();
                self.queued.clear();
                true
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
                let mut run = match claimed {
                    Some(index) => self.queued.remove(index),
                    None => match self.runs.remove(&event.run_id) {
                        Some(mut known) => {
                            known.apply(&frame);
                            known
                        }
                        None => Run::from_started(event),
                    },
                };
                for delta in self.early.remove(&event.run_id).unwrap_or_default() {
                    run.apply(&Frame::TextDelta(delta));
                }
                self.runs.insert(event.run_id.clone(), run);
                true
            }
            Frame::RunSnapshot(snapshot) => {
                match self.runs.get_mut(&snapshot.run_id) {
                    Some(run) => {
                        run.apply(&frame);
                    }
                    None => {
                        self.runs
                            .insert(snapshot.run_id.clone(), Run::from_snapshot(snapshot));
                    }
                }
                true
            }
            Frame::TextDelta(delta) => match self.runs.get_mut(&delta.run_id) {
                Some(run) => run.apply(&frame) == Applied::Changed,
                None => {
                    self.early
                        .entry(delta.run_id.clone())
                        .or_default()
                        .push(delta.clone());
                    false
                }
            },
            Frame::MessageCreated(created) => {
                self.message_created(&created.message, cx);
                false
            }
            Frame::StepText(_) => self.apply_to_run(&frame),
            Frame::ToolStarted(_) => self.apply_to_run(&frame),
            Frame::ToolFinished(_) => self.apply_to_run(&frame),
            Frame::Task(_) => self.apply_to_run(&frame),
            Frame::Status(_) => self.apply_to_run(&frame),
            Frame::RunReset(_) => self.apply_to_run(&frame),
            Frame::RunFinished(_) => self.apply_to_run(&frame),
            Frame::Unknown(_) => false,
        };
        if changed {
            self.refresh_agents();
            cx.emit(StateEvent::RunsChanged);
            cx.notify();
        }
    }

    fn accept(&mut self, accepted: InputAccepted) -> bool {
        for run in self.runs.values() {
            if run.input_ids.contains(&accepted.input_id) {
                return false;
            }
        }
        self.queued.push(Run::queued(accepted));
        true
    }

    fn apply_to_run(&mut self, frame: &Frame) -> bool {
        let Some(run_id) = frame.run_id() else {
            return false;
        };
        let Some(run) = self.runs.get_mut(run_id) else {
            return false;
        };
        run.apply(frame) == Applied::Changed
    }

    fn message_created(&mut self, message: &v3::Message, cx: &mut Context<Self>) {
        if let Some(run_id) = &message.run_id
            && message.kind != v3::MessageKind::User
            && let Some(run) = self.runs.get(run_id)
            && run.state != RunState::Interrupted
        {
            self.runs.remove(run_id);
            self.refresh_agents();
            cx.emit(StateEvent::RunsChanged);
        }
        let Some(selected) = self.selected else {
            return;
        };
        if message.surface_id != link::surface_id(selected) {
            return;
        }
        let mapped = link::message(message);
        let mut pending = None;
        if let Some(id) = &message.client_message_id {
            for (index, candidate) in self.pending.iter().enumerate() {
                if &candidate.client_message_id == id {
                    pending = Some(index);
                }
            }
        }
        if let Some(index) = pending {
            let local = self.pending.remove(index).local;
            self.messages.retain(|candidate| candidate.id != local);
        }
        let mut seen = false;
        for candidate in &self.messages {
            if candidate.id == mapped.id {
                seen = true;
            }
        }
        if !seen {
            self.messages.push(mapped);
        }
        cx.emit(StateEvent::MessageAppended);
        cx.notify();
    }
}

fn jitter() -> f64 {
    f64::from(OffsetDateTime::now_utc().nanosecond() % 1000) / 1000.0
}

async fn fetch_directory(
    this: &WeakEntity<AppState>,
    client: &v3::Client,
    cx: &mut AsyncApp,
) -> bool {
    let surfaces = client.surfaces().await;
    let agents = client.agents().await;
    let loaded = match (surfaces, agents) {
        (Ok(surfaces), Ok(agents)) => {
            this.update(cx, |state, cx| state.directory_loaded(surfaces, agents, cx))
        }
        (Err(error), Ok(_)) => this.update(cx, |state, cx| state.failed(error.to_string(), cx)),
        (Ok(_), Err(error)) => this.update(cx, |state, cx| state.failed(error.to_string(), cx)),
        (Err(error), Err(_)) => this.update(cx, |state, cx| state.failed(error.to_string(), cx)),
    };
    loaded.is_ok()
}

fn is_gap(frame: &Frame) -> bool {
    match frame {
        Frame::Gap(_) => true,
        Frame::Hello(_) => false,
        Frame::RunSnapshot(_) => false,
        Frame::RunStarted(_) => false,
        Frame::StepText(_) => false,
        Frame::ToolStarted(_) => false,
        Frame::ToolFinished(_) => false,
        Frame::Task(_) => false,
        Frame::Status(_) => false,
        Frame::RunReset(_) => false,
        Frame::RunFinished(_) => false,
        Frame::MessageCreated(_) => false,
        Frame::TextDelta(_) => false,
        Frame::InputAccepted(_) => false,
        Frame::Unknown(_) => false,
    }
}

async fn run_link(this: WeakEntity<AppState>, client: v3::Client, cx: &mut AsyncApp) {
    let mut backoff = Backoff::default();
    let mut since: Option<Seq> = None;
    loop {
        let connection = client.connect(since).await;
        let mut connection = match connection {
            Ok(connection) => connection,
            Err(error) => {
                let Ok(()) = this.update(cx, |state, cx| state.failed(error.to_string(), cx))
                else {
                    return;
                };
                cx.background_executor()
                    .timer(backoff.next_delay(jitter()))
                    .await;
                continue;
            }
        };
        let Some(Frame::Hello(hello)) = connection.frames.next().await else {
            cx.background_executor()
                .timer(backoff.next_delay(jitter()))
                .await;
            continue;
        };
        backoff.reset();
        let fresh = since.is_none();
        let control = connection.control.clone();
        let Ok(()) = this.update(cx, |state, cx| {
            state.connected(hello.head, control, fresh, cx)
        }) else {
            return;
        };
        if fresh && !fetch_directory(&this, &client, cx).await {
            return;
        }
        while let Some(frame) = connection.frames.next().await {
            let gap = is_gap(&frame);
            let Ok(()) = this.update(cx, |state, cx| state.apply(frame, cx)) else {
                return;
            };
            if gap && !fetch_directory(&this, &client, cx).await {
                return;
            }
        }
        let Ok(last) = this.update(cx, |state, cx| state.disconnected(cx)) else {
            return;
        };
        since = last;
        cx.background_executor()
            .timer(backoff.next_delay(jitter()))
            .await;
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;
    use tuclaw_core::model::{AgentId, AgentStatus, ChannelId};
    use tuclaw_core::v3::{AgentId as WireAgent, Scenario};

    use super::{Link, Segment, SidebarVisibility, View};
    use crate::testing::{channel_named, loaded, mocked, play};

    #[gpui::test]
    fn the_fresh_start_loads_the_directory_and_the_first_surface(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.link(), &Link::Live);
            assert_eq!(state.channels().len(), 3);
            assert_eq!(state.agents().len(), 4);
            assert_eq!(state.selected(), Some(ChannelId(1)));
            assert_eq!(state.messages().len(), 30);
        });
    }

    #[gpui::test]
    fn the_live_snapshot_marks_its_agent_busy(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        state.read_with(cx, |state, _cx| {
            let mut magnet = None;
            for agent in state.agents() {
                if agent.id == AgentId(3) {
                    magnet = Some(agent.status.clone());
                }
            }
            assert_eq!(magnet, Some(AgentStatus::Busy("in #Magnet Feed".into())));
        });
    }

    #[gpui::test]
    fn a_finished_run_frees_its_agent(cx: &mut TestAppContext) {
        let (mock, state) = loaded(cx);
        state.update(cx, |state, cx| {
            state.send("Привет".to_string(), cx).expect("queued")
        });
        cx.run_until_parked();
        mock.pump_control();
        for _ in 0..3 {
            mock.step();
        }
        cx.run_until_parked();
        let busy = |state: &super::AppState| {
            let mut busy = false;
            for agent in state.agents() {
                if agent.id == AgentId(1) && agent.status != AgentStatus::Idle {
                    busy = true;
                }
            }
            busy
        };
        state.read_with(cx, |state, _cx| assert!(busy(state)));
        play(&mock, cx);
        state.read_with(cx, |state, _cx| assert!(!busy(state)));
    }

    #[gpui::test]
    fn selecting_an_unknown_channel_changes_nothing(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        state.update(cx, |state, cx| state.select(ChannelId(99), cx));
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.selected(), Some(ChannelId(1)))
        });
    }

    #[gpui::test]
    fn selecting_returns_from_the_agents_view(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        let home = channel_named(&state, cx, "Smart Home");
        state.update(cx, |state, cx| {
            state.activate_segment(Segment::Agents, cx);
            state.select(home, cx);
        });
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.view(), View::Conversation);
            assert_eq!(state.active_segment(), Segment::Channel);
        });
    }

    #[gpui::test]
    fn the_segments_switch_the_view(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        state.update(cx, |state, cx| state.activate_segment(Segment::Agents, cx));
        state.read_with(cx, |state, _cx| assert_eq!(state.view(), View::Agents));
        state.update(cx, |state, cx| state.activate_segment(Segment::Channel, cx));
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.view(), View::Conversation)
        });
    }

    #[gpui::test]
    fn a_leading_mention_addresses_a_wired_agent_only(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        state.read_with(cx, |state, _cx| {
            let general = ChannelId(1);
            assert_eq!(
                state.addressed(general, "@magnet_feed что нового?"),
                Some(WireAgent(3))
            );
            assert_eq!(state.addressed(general, "@scout найди"), None);
            assert_eq!(state.addressed(general, "привет @magnet_feed"), None);
        });
    }

    #[gpui::test]
    fn a_blank_body_sends_nothing(cx: &mut TestAppContext) {
        let (mock, state) = loaded(cx);
        state.update(cx, |state, cx| {
            state.send("   ".to_string(), cx).expect("ok")
        });
        state.read_with(cx, |state, _cx| assert_eq!(state.messages().len(), 30));
        assert_eq!(mock.pending(), 0);
    }

    #[gpui::test]
    fn a_dropped_socket_reconnects_and_replays(cx: &mut TestAppContext) {
        let (mock, state) = loaded(cx);
        state.update(cx, |state, cx| {
            state.send("Привет".to_string(), cx).expect("queued")
        });
        cx.run_until_parked();
        mock.pump_control();
        for _ in 0..4 {
            mock.step();
        }
        cx.run_until_parked();
        mock.disconnect_all();
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.link(), &Link::Reconnecting)
        });
        mock.play_all();
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.link(), &Link::Live);
            assert_eq!(state.messages().len(), 32);
        });
    }

    #[gpui::test]
    fn a_gap_refetches_the_directory(cx: &mut TestAppContext) {
        let (mock, state) = mocked(
            cx,
            Scenario {
                gap_once: true,
                unavailable_once: false,
            },
        );
        mock.disconnect_all();
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.link(), &Link::Live);
            assert_eq!(state.channels().len(), 3);
            assert_eq!(state.messages().len(), 30);
        });
    }

    #[gpui::test]
    fn the_sidebar_starts_shown_and_toggles(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.sidebar(), SidebarVisibility::Shown)
        });
        state.update(cx, |state, cx| state.toggle_sidebar(cx));
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.sidebar(), SidebarVisibility::Hidden)
        });
    }
}
