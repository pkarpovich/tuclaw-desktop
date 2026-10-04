use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use anyhow::{Result, bail};
use futures::StreamExt;
use futures::channel::mpsc::UnboundedSender;
use gpui::{AsyncApp, Context, EventEmitter, Task, WeakEntity};
use time::OffsetDateTime;
use tuclaw_core::model::{
    Agent, Author, Channel, ChannelId, Message, MessageId, RecordingId, Span, Voice,
};
use tuclaw_core::v3::{
    self, Applied, Backoff, ClientFrame, ClientMessageId, Frame, InputAccepted, Post, Run, RunId,
    RunState, Seq, TextDelta,
};

use crate::audio::{self, Pcm, PeakCache, Speaker, Waveform};
use crate::link::{self, Source};
use crate::runlog::{self, Disclosure, RunLog};

const PAGE: u32 = 50;
const TICK: Duration = Duration::from_millis(200);

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
    FoldToggled,
    OlderLoaded,
    SendFailed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum History {
    Unknown,
    More,
    Loading,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Player {
    Stopped,
    Loading,
    Playing { position: Duration, total: Duration },
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Filling {
    Idle,
    Running,
}

struct Playback {
    message: MessageId,
    player: Player,
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
    history: History,
    runs: BTreeMap<RunId, Run>,
    queued: Vec<Run>,
    early: HashMap<RunId, Vec<TextDelta>>,
    pending: Vec<Pending>,
    next_local: i64,
    last_seq: Option<Seq>,
    control: Option<UnboundedSender<ClientFrame>>,
    view: View,
    sidebar: SidebarVisibility,
    expanded: HashSet<MessageId>,
    toggled: HashSet<Disclosure>,
    run_logs: HashMap<String, RunLog>,
    speaker: Box<dyn Speaker>,
    playback: Option<Playback>,
    _playback: Option<Task<()>>,
    waveforms: HashMap<RecordingId, Waveform>,
    unreadable: HashSet<RecordingId>,
    peak_cache: Option<PeakCache>,
    filling: Filling,
    _waveforms: Option<Task<()>>,
    _link: Option<Task<()>>,
}

impl EventEmitter<StateEvent> for AppState {}

impl AppState {
    pub fn new(client: v3::Client, source: Source, speaker: Box<dyn Speaker>) -> AppState {
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
            history: History::Unknown,
            runs: BTreeMap::new(),
            queued: Vec::new(),
            early: HashMap::new(),
            pending: Vec::new(),
            next_local: 0,
            last_seq: None,
            control: None,
            view: View::Conversation,
            sidebar: SidebarVisibility::Shown,
            expanded: HashSet::new(),
            toggled: HashSet::new(),
            run_logs: HashMap::new(),
            speaker,
            playback: None,
            _playback: None,
            waveforms: HashMap::new(),
            unreadable: HashSet::new(),
            peak_cache: None,
            filling: Filling::Idle,
            _waveforms: None,
            _link: None,
        }
    }

    pub fn with_peak_cache(mut self, cache: PeakCache) -> AppState {
        self.peak_cache = Some(cache);
        self
    }

    pub fn waveform(&self, recording: RecordingId) -> Option<Waveform> {
        self.waveforms.get(&recording).copied()
    }

    fn fill_waveforms(&mut self, cx: &mut Context<Self>) {
        match self.filling {
            Filling::Running => return,
            Filling::Idle => {}
        }
        let Some(first) = self.missing_waveform() else {
            return;
        };
        self.filling = Filling::Running;
        let client = self.client.clone();
        let cache = self.peak_cache.clone();
        self._waveforms = Some(cx.spawn(async move |this, cx| {
            let mut next = Some(first);
            while let Some(voice) = next {
                let recording = voice.recording;
                let peaks = waveform_for(&client, cache.clone(), voice, cx).await;
                let Ok(following) =
                    this.update(cx, |state, cx| state.waveform_ready(recording, peaks, cx))
                else {
                    return;
                };
                next = following;
            }
        }));
    }

    fn missing_waveform(&self) -> Option<Voice> {
        for message in &self.messages {
            let Some(voice) = &message.voice else {
                continue;
            };
            if self.waveforms.contains_key(&voice.recording)
                || self.unreadable.contains(&voice.recording)
            {
                continue;
            }
            return Some(voice.clone());
        }
        None
    }

    fn waveform_ready(
        &mut self,
        recording: RecordingId,
        waveform: Option<Waveform>,
        cx: &mut Context<Self>,
    ) -> Option<Voice> {
        match waveform {
            Some(waveform) => {
                self.waveforms.insert(recording, waveform);
            }
            None => {
                self.unreadable.insert(recording);
            }
        }
        cx.notify();
        let next = self.missing_waveform();
        if next.is_none() {
            self.filling = Filling::Idle;
        }
        next
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

    pub fn is_expanded(&self, message: MessageId) -> bool {
        self.expanded.contains(&message)
    }

    pub fn toggle_thinking(&mut self, message: MessageId, cx: &mut Context<Self>) {
        if !self.expanded.remove(&message) {
            self.expanded.insert(message);
        }
        cx.emit(StateEvent::FoldToggled);
        cx.notify();
    }

    pub fn player(&self, message: MessageId) -> Player {
        match &self.playback {
            Some(playback) if playback.message == message => playback.player.clone(),
            Some(_) => Player::Stopped,
            None => Player::Stopped,
        }
    }

    pub fn toggle_voice(&mut self, message: MessageId, cx: &mut Context<Self>) {
        match self.player(message) {
            Player::Loading => return,
            Player::Playing {
                position: _,
                total: _,
            } => {
                self.halt();
                cx.notify();
                return;
            }
            Player::Stopped => {}
            Player::Failed(_) => {}
        }
        let Some(Voice {
            recording,
            mime,
            duration: _,
        }) = self.voice_of(message)
        else {
            return;
        };
        self.halt();
        self.playback = Some(Playback {
            message,
            player: Player::Loading,
        });
        let request = self.client.attachment(link::recording_id(recording));
        self._playback = Some(cx.spawn(async move |this, cx| {
            let pcm = match request.await {
                Ok(bytes) => {
                    cx.background_executor()
                        .spawn(async move { audio::decode(&mime, bytes) })
                        .await
                }
                Err(error) => Err(error.to_string()),
            };
            let Ok(true) = this.update(cx, |state, cx| state.decoded(message, pcm, cx)) else {
                return;
            };
            loop {
                cx.background_executor().timer(TICK).await;
                let Ok(true) = this.update(cx, |state, cx| state.tick(message, cx)) else {
                    return;
                };
            }
        }));
        cx.notify();
    }

    fn voice_of(&self, message: MessageId) -> Option<Voice> {
        for candidate in &self.messages {
            if candidate.id == message {
                return candidate.voice.clone();
            }
        }
        None
    }

    fn halt(&mut self) {
        self.speaker.stop();
        self.playback = None;
        self._playback = None;
    }

    fn decoded(
        &mut self,
        message: MessageId,
        pcm: Result<Pcm, String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(playback) = &mut self.playback else {
            return false;
        };
        if playback.message != message || playback.player != Player::Loading {
            return false;
        }
        let started = match pcm {
            Ok(pcm) => {
                let total = pcm.duration();
                self.speaker.start(pcm).map(|()| total)
            }
            Err(reason) => Err(reason),
        };
        let playing = match started {
            Ok(total) => {
                playback.player = Player::Playing {
                    position: Duration::ZERO,
                    total,
                };
                true
            }
            Err(reason) => {
                playback.player = Player::Failed(reason);
                false
            }
        };
        cx.notify();
        playing
    }

    fn tick(&mut self, message: MessageId, cx: &mut Context<Self>) -> bool {
        let Some(playback) = &mut self.playback else {
            return false;
        };
        if playback.message != message {
            return false;
        }
        if self.speaker.finished() {
            self.speaker.stop();
            self.playback = None;
            cx.notify();
            return false;
        }
        if let Player::Playing { position, total: _ } = &mut playback.player {
            *position = self.speaker.position();
        }
        cx.notify();
        true
    }

    pub fn is_open(&self, disclosure: Disclosure, by_default: bool) -> bool {
        by_default != self.toggled.contains(&disclosure)
    }

    pub fn run_log(&self, run: &str) -> Option<&RunLog> {
        self.run_logs.get(run)
    }

    pub fn toggle(&mut self, disclosure: Disclosure, cx: &mut Context<Self>) {
        if !self.toggled.remove(&disclosure) {
            self.toggled.insert(disclosure);
        }
        if let Disclosure::Log(message) = disclosure {
            self.ensure_log(message, cx);
        }
        cx.emit(StateEvent::FoldToggled);
        cx.notify();
    }

    fn ensure_log(&mut self, message: MessageId, cx: &mut Context<Self>) {
        let mut found = None;
        for candidate in &self.messages {
            if candidate.id == message {
                found = candidate.run.clone();
            }
        }
        let Some(run) = found else {
            return;
        };
        if self.run_logs.contains_key(&run.id) {
            return;
        }
        self.run_logs.insert(run.id.clone(), RunLog::Loading);
        let request = self.client.run(&link::run_id(&run));
        let id = run.id;
        cx.spawn(async move |this, cx| {
            let detail = request.await;
            this.update(cx, |state, cx| {
                let log = match detail {
                    Ok(detail) => RunLog::Loaded(Box::new(detail)),
                    Err(_error) => RunLog::Failed,
                };
                state.run_logs.insert(id, log);
                cx.emit(StateEvent::FoldToggled);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn open_failed_logs(&mut self, cx: &mut Context<Self>) {
        let mut failed = Vec::new();
        for message in &self.messages {
            if let Some(run) = &message.run
                && runlog::opens_by_default(run)
                && !self.run_logs.contains_key(&run.id)
            {
                failed.push(message.id);
            }
        }
        for message in failed {
            self.ensure_log(message, cx);
        }
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
            self.halt();
            self.messages.clear();
            self.history = History::Unknown;
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
            voice: None,
            run: None,
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

    pub fn history(&self) -> History {
        self.history
    }

    pub fn load_older(&mut self, cx: &mut Context<Self>) {
        match self.history {
            History::More => {}
            History::Unknown => return,
            History::Loading => return,
            History::Complete => return,
        }
        let Some(channel) = self.selected else {
            return;
        };
        let mut oldest = None;
        for message in &self.messages {
            let MessageId(raw) = message.id;
            if raw > 0 {
                oldest = Some(oldest.map_or(raw, |current: i64| current.min(raw)));
            }
        }
        let Some(oldest) = oldest else {
            return;
        };
        self.history = History::Loading;
        let request =
            self.client
                .messages_before(link::surface_id(channel), v3::MessageId(oldest), PAGE);
        cx.spawn(async move |this, cx| {
            let page = request.await;
            this.update(cx, |state, cx| state.older_loaded(channel, page, cx))
                .ok();
        })
        .detach();
    }

    fn older_loaded(
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
            Err(_error) => {
                self.history = History::More;
                return;
            }
        };
        let mut older = Vec::new();
        for message in &page.messages {
            let mapped = link::message(message);
            let mut known = false;
            for candidate in &self.messages {
                if candidate.id == mapped.id {
                    known = true;
                }
            }
            if !known {
                older.push(mapped);
            }
        }
        self.history = if page.has_more {
            History::More
        } else {
            History::Complete
        };
        if older.is_empty() {
            return;
        }
        older.append(&mut self.messages);
        self.messages = older;
        self.fill_waveforms(cx);
        self.open_failed_logs(cx);
        cx.emit(StateEvent::OlderLoaded);
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
        self.history = if page.has_more {
            History::More
        } else {
            History::Complete
        };
        self.fill_waveforms(cx);
        self.open_failed_logs(cx);
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
            self.fill_waveforms(cx);
            self.open_failed_logs(cx);
        }
        cx.emit(StateEvent::MessageAppended);
        cx.notify();
    }
}

async fn waveform_for(
    client: &v3::Client,
    cache: Option<PeakCache>,
    voice: Voice,
    cx: &mut AsyncApp,
) -> Option<Waveform> {
    let Voice {
        recording,
        mime,
        duration: _,
    } = voice;
    let RecordingId(raw) = recording;
    let executor = cx.background_executor().clone();
    if let Some(cache) = cache.clone()
        && let Some(waveform) = executor.spawn(async move { cache.read(raw) }).await
    {
        return Some(waveform);
    }
    let bytes = client
        .attachment(link::recording_id(recording))
        .await
        .ok()?;
    let waveform = executor
        .spawn(async move { audio::decode(&mime, bytes).map(|pcm| audio::waveform(&pcm)) })
        .await
        .ok()?;
    if let Some(cache) = cache {
        executor
            .spawn(async move { cache.write(raw, &waveform) })
            .await
            .ok();
    }
    Some(waveform)
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

    use std::time::Duration;

    use gpui::Entity;
    use tuclaw_core::model::MessageId;
    use tuclaw_core::v3::MockTransport;

    use super::{AppState, Link, Player, Segment, SidebarVisibility, TICK, View};
    use crate::testing::{FakeSpeaker, channel_named, loaded, mocked, play, speaking};

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

    fn on_magnet_feed(
        cx: &mut TestAppContext,
    ) -> (MockTransport, Entity<AppState>, FakeSpeaker, MessageId) {
        let (mock, state, speaker) = speaking(cx, Scenario::default());
        let magnet = channel_named(&state, cx, "Magnet Feed");
        state.update(cx, |state, cx| state.select(magnet, cx));
        cx.run_until_parked();
        let spoken = state.read_with(cx, |state, _cx| {
            let mut spoken = None;
            for message in state.messages() {
                if message.voice.is_some() {
                    spoken = Some(message.id);
                }
            }
            spoken.expect("the mock world carries a voice message on Magnet Feed")
        });
        (mock, state, speaker, spoken)
    }

    fn player(state: &Entity<AppState>, cx: &mut TestAppContext, message: MessageId) -> Player {
        state.read_with(cx, |state, _cx| state.player(message))
    }

    #[gpui::test]
    fn a_voice_message_plays_its_recording_and_stops_on_a_second_press(cx: &mut TestAppContext) {
        let (_mock, state, speaker, spoken) = on_magnet_feed(cx);
        assert_eq!(player(&state, cx, spoken), Player::Stopped);
        state.update(cx, |state, cx| state.toggle_voice(spoken, cx));
        assert_eq!(player(&state, cx, spoken), Player::Loading);
        cx.run_until_parked();
        let Player::Playing { position, total } = player(&state, cx, spoken) else {
            panic!("the decoded recording plays");
        };
        assert_eq!(position, Duration::ZERO);
        assert!((total.as_secs_f64() - 3.0).abs() < 0.05, "{total:?}");
        assert_eq!(speaker.0.borrow().started.len(), 1);
        speaker.0.borrow_mut().position = Duration::from_secs(1);
        cx.executor().advance_clock(TICK);
        cx.run_until_parked();
        let Player::Playing { position, total: _ } = player(&state, cx, spoken) else {
            panic!("still playing");
        };
        assert_eq!(position, Duration::from_secs(1));
        state.update(cx, |state, cx| state.toggle_voice(spoken, cx));
        assert_eq!(player(&state, cx, spoken), Player::Stopped);
        assert!(!speaker.0.borrow().playing);
    }

    #[gpui::test]
    fn a_finished_recording_returns_to_stopped(cx: &mut TestAppContext) {
        let (_mock, state, speaker, spoken) = on_magnet_feed(cx);
        state.update(cx, |state, cx| state.toggle_voice(spoken, cx));
        cx.run_until_parked();
        speaker.0.borrow_mut().playing = false;
        cx.executor().advance_clock(TICK);
        cx.run_until_parked();
        assert_eq!(player(&state, cx, spoken), Player::Stopped);
    }

    #[gpui::test]
    fn a_failed_fetch_or_a_missing_output_shows_why(cx: &mut TestAppContext) {
        let (mock, state, speaker, spoken) = on_magnet_feed(cx);
        mock.fail_next_call();
        state.update(cx, |state, cx| state.toggle_voice(spoken, cx));
        cx.run_until_parked();
        let Player::Failed(_) = player(&state, cx, spoken) else {
            panic!("an unavailable daemon fails the playback");
        };
        speaker.0.borrow_mut().refuse = Some("no audio output".into());
        state.update(cx, |state, cx| state.toggle_voice(spoken, cx));
        cx.run_until_parked();
        assert_eq!(
            player(&state, cx, spoken),
            Player::Failed("no audio output".into())
        );
        speaker.0.borrow_mut().refuse = None;
        state.update(cx, |state, cx| state.toggle_voice(spoken, cx));
        cx.run_until_parked();
        let Player::Playing {
            position: _,
            total: _,
        } = player(&state, cx, spoken)
        else {
            panic!("a retry after a failure plays");
        };
    }

    #[gpui::test]
    fn switching_channels_stops_the_recording(cx: &mut TestAppContext) {
        let (_mock, state, speaker, spoken) = on_magnet_feed(cx);
        state.update(cx, |state, cx| state.toggle_voice(spoken, cx));
        cx.run_until_parked();
        assert!(speaker.0.borrow().playing);
        let general = channel_named(&state, cx, "General");
        state.update(cx, |state, cx| state.select(general, cx));
        assert!(!speaker.0.borrow().playing);
        assert_eq!(player(&state, cx, spoken), Player::Stopped);
    }

    #[gpui::test]
    fn a_message_without_a_recording_plays_nothing(cx: &mut TestAppContext) {
        let (_mock, state, speaker) = speaking(cx, Scenario::default());
        let silent = state.read_with(cx, |state, _cx| state.messages()[0].id);
        state.update(cx, |state, cx| state.toggle_voice(silent, cx));
        cx.run_until_parked();
        assert_eq!(player(&state, cx, silent), Player::Stopped);
        assert!(speaker.0.borrow().started.is_empty());
    }

    fn voice_recording(
        state: &Entity<AppState>,
        cx: &mut TestAppContext,
    ) -> tuclaw_core::model::RecordingId {
        state.read_with(cx, |state, _cx| {
            let mut found = None;
            for message in state.messages() {
                if let Some(voice) = &message.voice {
                    found = Some(voice.recording);
                }
            }
            found.expect("a voice message is loaded")
        })
    }

    #[gpui::test]
    fn a_loaded_voice_message_gets_its_measured_waveform(cx: &mut TestAppContext) {
        let (_mock, state, _speaker, _spoken) = on_magnet_feed(cx);
        let recording = voice_recording(&state, cx);
        let waveform = state.read_with(cx, |state, _cx| state.waveform(recording));
        let Some(crate::audio::Waveform {
            peaks: crate::audio::Peaks(levels),
            duration,
        }) = waveform
        else {
            panic!("the waveform is computed from the fetched recording");
        };
        assert!(
            (duration.as_secs_f64() - 3.0).abs() < 0.05,
            "the duration is measured from the decoded recording: {duration:?}"
        );
        let mut loudest = 0;
        for level in levels {
            loudest = loudest.max(level);
        }
        assert!(loudest > 25, "the tone is audible in the peaks: {levels:?}");
    }

    #[gpui::test]
    fn a_cached_waveform_is_read_instead_of_fetched(cx: &mut TestAppContext) {
        let directory =
            std::env::temp_dir().join(format!("tuclaw-state-peaks-{}", std::process::id()));
        std::fs::remove_dir_all(&directory).ok();
        let cache = crate::audio::PeakCache::new(directory.clone());
        let (_mock, state, _speaker) =
            crate::testing::speaking_with(cx, Scenario::default(), Some(cache.clone()));
        let magnet = channel_named(&state, cx, "Magnet Feed");
        state.update(cx, |state, cx| state.select(magnet, cx));
        cx.run_until_parked();
        let recording = voice_recording(&state, cx);
        let tuclaw_core::model::RecordingId(raw) = recording;
        let stored = cache
            .read(raw)
            .expect("the computed waveform is cached on disk");
        let mut levels = [7u8; crate::audio::PEAKS];
        levels[0] = 200;
        let planted = crate::audio::Waveform {
            peaks: crate::audio::Peaks(levels),
            duration: std::time::Duration::from_secs(42),
        };
        cache
            .write(raw, &planted)
            .expect("the cache is overwritten");
        assert_ne!(stored, planted);
        let (_mock, again, _speaker) =
            crate::testing::speaking_with(cx, Scenario::default(), Some(cache));
        let magnet = channel_named(&again, cx, "Magnet Feed");
        again.update(cx, |state, cx| state.select(magnet, cx));
        cx.run_until_parked();
        let waveform = again.read_with(cx, |state, _cx| state.waveform(recording));
        assert_eq!(waveform, Some(planted));
        std::fs::remove_dir_all(directory).ok();
    }

    fn ids(state: &Entity<AppState>, cx: &mut TestAppContext) -> (usize, i64) {
        state.read_with(cx, |state, _cx| {
            let tuclaw_core::model::MessageId(first) = state.messages()[0].id;
            (state.messages().len(), first)
        })
    }

    #[gpui::test]
    fn older_pages_load_until_the_history_is_complete(cx: &mut TestAppContext) {
        let (_mock, state) = crate::testing::seeded(cx, crate::testing::long_world(120));
        assert_eq!(ids(&state, cx), (50, 71));
        assert_eq!(
            state.read_with(cx, |state, _cx| state.history()),
            super::History::More
        );
        state.update(cx, |state, cx| state.load_older(cx));
        assert_eq!(
            state.read_with(cx, |state, _cx| state.history()),
            super::History::Loading
        );
        cx.run_until_parked();
        assert_eq!(ids(&state, cx), (100, 21));
        state.update(cx, |state, cx| state.load_older(cx));
        cx.run_until_parked();
        assert_eq!(ids(&state, cx), (120, 1));
        assert_eq!(
            state.read_with(cx, |state, _cx| state.history()),
            super::History::Complete
        );
        state.update(cx, |state, cx| state.load_older(cx));
        cx.run_until_parked();
        assert_eq!(ids(&state, cx), (120, 1));
    }

    fn run_world() -> tuclaw_core::v3::Seed {
        let mut world = crate::testing::long_world(0);
        let ok: tuclaw_core::v3::RunDetail =
            serde_json::from_str(include_str!("../../core/testdata/v3/run.json")).expect("run");
        let mut failed = ok.clone();
        failed.run.id = tuclaw_core::v3::RunId("failed-run".into());
        failed.run.status = tuclaw_core::v3::RunStatus::Error;
        let answer = |id: i64, run: &str, status: &str| {
            serde_json::from_value(serde_json::json!({
                "id": id, "surface_id": 1, "kind": "answer",
                "author": {"kind": "agent", "agent_id": 1},
                "text": "Готово.", "run_id": run, "created_at": "2026-10-03T15:26:13Z",
                "run_summary": {"status": status, "step_count": 2, "tool_count": 1, "duration_ms": 13029}
            }))
            .expect("message")
        };
        world.messages = vec![
            answer(1, "6763eb02-7f3e-4c4d-9b1a-2f0c5d8e9a11", "ok"),
            answer(2, "failed-run", "error"),
        ];
        world.runs = vec![ok, failed];
        world
    }

    #[gpui::test]
    fn a_run_log_loads_when_opened_and_a_failed_one_is_open_at_once(cx: &mut TestAppContext) {
        use crate::runlog::{Disclosure, RunLog};
        let (_mock, state) = crate::testing::seeded(cx, run_world());
        let ok = "6763eb02-7f3e-4c4d-9b1a-2f0c5d8e9a11";
        state.read_with(cx, |state, _cx| {
            assert!(state.run_log(ok).is_none(), "nothing loads until opened");
            let Some(RunLog::Loaded(_)) = state.run_log("failed-run") else {
                panic!("a failed run's log loads with the page");
            };
            assert!(state.is_open(Disclosure::Log(MessageId(2)), true));
            assert!(!state.is_open(Disclosure::Log(MessageId(1)), false));
        });
        state.update(cx, |state, cx| {
            state.toggle(Disclosure::Log(MessageId(1)), cx)
        });
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.run_log(ok), Some(&RunLog::Loading));
            assert!(state.is_open(Disclosure::Log(MessageId(1)), false));
        });
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            let Some(RunLog::Loaded(detail)) = state.run_log(ok) else {
                panic!("the opened log loads");
            };
            assert_eq!(detail.steps.len(), 2);
        });
        state.update(cx, |state, cx| {
            state.toggle(Disclosure::Log(MessageId(2)), cx)
        });
        state.read_with(cx, |state, _cx| {
            assert!(!state.is_open(Disclosure::Log(MessageId(2)), true));
        });
    }
}
