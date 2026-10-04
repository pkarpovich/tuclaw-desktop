use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use futures::StreamExt;
use futures::channel::mpsc::UnboundedSender;
use gpui::{AsyncApp, Context, EventEmitter, Task, WeakEntity};
use time::OffsetDateTime;
use tuclaw_core::model::{
    Agent, AgentId, Author, Channel, ChannelId, Message, MessageId, Picture, RecordingId, Span,
    Voice,
};
use tuclaw_core::v3::{
    self, Applied, Backoff, ClientFrame, ClientMessageId, Frame, InputAccepted, Post, Run, RunId,
    RunState, Seq, TextDelta, VoicePost,
};

use crate::agent_settings::{
    self, Field, FieldError, Joinable, Saving, Settings, Target, Toast, TopicRow, Undo,
};
use crate::audio::{self, Pcm, PeakCache, Speaker, Waveform};
use crate::link::{self, Source};
use crate::people::{self, Gallery, Me, People};
use crate::picture::{self, Upload};
use crate::pictures::{self, Remote, Shelf, Viewed};
use crate::recorder::{self, NoRecorder, Recorder, Take};
use crate::runlog::{self, Disclosure, RunLog};

const PAGE: u32 = 50;
const TICK: Duration = Duration::from_millis(200);
const TOAST_LIFETIME: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Conversation,
    Agents,
    Automations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarVisibility {
    Shown,
    Hidden,
}

const RECORDING_TICK: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recording {
    Idle,
    Live { since: Instant, channel: ChannelId },
    Sending,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    Channel,
    Agents,
    Automations,
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
    PicturesLoaded,
    PictureOpened,
    SendFailed(String),
    Mention(String),
    TasksLoaded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Tools,
    Thoughts,
    Errors,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inspector {
    pub message: MessageId,
    pub filter: Filter,
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
    inspector: Option<Inspector>,
    settings: Option<Settings>,
    saving: Saving,
    field_error: Option<FieldError>,
    toast: Option<Toast>,
    next_toast: u64,
    fires: Vec<v3::FireMark>,
    tasks: Vec<v3::Task>,
    selected_task: Option<v3::TaskId>,
    task_runs: HashMap<v3::TaskId, Vec<v3::TaskRun>>,
    run_logs: HashMap<String, RunLog>,
    speaker: Box<dyn Speaker>,
    recorder: Box<dyn Recorder>,
    pictures: Shelf,
    viewer: Option<Viewed>,
    cursors: HashMap<ChannelId, MessageId>,
    divider: Option<MessageId>,
    window_active: bool,
    recording: Recording,
    playback: Option<Playback>,
    _playback: Option<Task<()>>,
    waveforms: HashMap<RecordingId, Waveform>,
    unreadable: HashSet<RecordingId>,
    peak_cache: Option<PeakCache>,
    filling: Filling,
    _waveforms: Option<Task<()>>,
    me: Me,
    gallery: Gallery,
    requested: HashSet<Picture>,
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
            inspector: None,
            settings: None,
            saving: Saving::Idle,
            field_error: None,
            toast: None,
            next_toast: 0,
            fires: Vec::new(),
            tasks: Vec::new(),
            selected_task: None,
            task_runs: HashMap::new(),
            run_logs: HashMap::new(),
            speaker,
            recorder: Box::new(NoRecorder),
            pictures: Shelf::new(),
            viewer: None,
            cursors: HashMap::new(),
            divider: None,
            window_active: true,
            recording: Recording::Idle,
            playback: None,
            _playback: None,
            waveforms: HashMap::new(),
            unreadable: HashSet::new(),
            peak_cache: None,
            filling: Filling::Idle,
            _waveforms: None,
            me: Me::default(),
            gallery: Gallery::new(),
            requested: HashSet::new(),
            _link: None,
        }
    }

    pub fn with_recorder(mut self, recorder: Box<dyn Recorder>) -> AppState {
        self.recorder = recorder;
        self
    }

    pub fn set_recorder(&mut self, recorder: Box<dyn Recorder>) {
        self.recorder = recorder;
    }

    pub fn divider(&self) -> Option<MessageId> {
        self.divider
    }

    fn divider_for(&self, channel: ChannelId) -> Option<MessageId> {
        let mut unread = false;
        for candidate in &self.channels {
            if candidate.id == channel && candidate.unread > 0 {
                unread = true;
            }
        }
        if !unread {
            return None;
        }
        Some(self.cursors.get(&channel).copied().unwrap_or(MessageId(0)))
    }

    fn count_unread(&mut self, message: &v3::Message, cx: &mut Context<Self>) {
        let counted = match message.author.kind {
            v3::AuthorKind::User => false,
            v3::AuthorKind::Agent => true,
            v3::AuthorKind::System => true,
            v3::AuthorKind::Unknown => true,
        };
        if !counted {
            return;
        }
        let channel = link::channel_id(message.surface_id);
        let id = link::message_id(message.id);
        if self
            .cursors
            .get(&channel)
            .is_some_and(|cursor| id <= *cursor)
        {
            return;
        }
        for candidate in &mut self.channels {
            if candidate.id == channel {
                candidate.unread += 1;
            }
        }
        cx.notify();
    }

    fn surface_read(&mut self, read: &v3::SurfaceRead, cx: &mut Context<Self>) {
        let v3::SurfaceRead {
            seq: _,
            surface_id,
            last_read_message_id,
            unread,
        } = read;
        self.apply_read(
            link::channel_id(*surface_id),
            v3::ReadAnswer {
                last_read_message_id: *last_read_message_id,
                unread: *unread,
            },
        );
        cx.notify();
    }

    fn apply_read(&mut self, channel: ChannelId, answer: v3::ReadAnswer) {
        let v3::ReadAnswer {
            last_read_message_id,
            unread,
        } = answer;
        let cursor = link::message_id(last_read_message_id);
        let moved = match self.cursors.get(&channel) {
            Some(known) => cursor >= *known,
            None => true,
        };
        if !moved {
            return;
        }
        self.cursors.insert(channel, cursor);
        for candidate in &mut self.channels {
            if candidate.id == channel {
                candidate.unread = unread as usize;
            }
        }
    }

    pub fn set_window_active(&mut self, active: bool, cx: &mut Context<Self>) {
        self.window_active = active;
        if active {
            self.read_to_newest(cx);
        }
    }

    pub fn read_to_newest(&mut self, cx: &mut Context<Self>) {
        if !self.window_active {
            return;
        }
        let Some(channel) = self.selected else {
            return;
        };
        let mut newest = None;
        for message in &self.messages {
            let MessageId(raw) = message.id;
            if raw > 0 && newest.is_none_or(|known: MessageId| message.id > known) {
                newest = Some(message.id);
            }
        }
        let Some(newest) = newest else {
            return;
        };
        if self
            .cursors
            .get(&channel)
            .is_some_and(|cursor| *cursor >= newest)
        {
            return;
        }
        self.cursors.insert(channel, newest);
        for candidate in &mut self.channels {
            if candidate.id == channel {
                candidate.unread = 0;
            }
        }
        let request = self
            .client
            .mark_read(link::surface_id(channel), link::v3_message_id(newest));
        cx.spawn(async move |this, cx| {
            let Ok(answer) = request.await else {
                return;
            };
            this.update(cx, |state, cx| {
                state.apply_read(channel, answer);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub fn viewer(&self) -> Option<&Viewed> {
        self.viewer.as_ref()
    }

    pub fn view_picture(&mut self, viewed: Viewed, cx: &mut Context<Self>) {
        self.viewer = Some(viewed);
        cx.emit(StateEvent::PictureOpened);
        cx.notify();
    }

    pub fn close_picture(&mut self, cx: &mut Context<Self>) {
        if self.viewer.take().is_some() {
            cx.notify();
        }
    }

    pub fn pictures(&self) -> &Shelf {
        &self.pictures
    }

    fn fill_message_pictures(&mut self, cx: &mut Context<Self>) {
        for url in pictures::wanted(&self.messages, &self.pictures) {
            self.pictures.insert(url.clone(), Remote::Loading);
            let request = self.client.public_picture(&url);
            cx.spawn(async move |this, cx| {
                let remote = match request.await {
                    Ok(bytes) => match pictures::decode(bytes) {
                        Some(shown) => Remote::Ready(shown),
                        None => Remote::Failed,
                    },
                    Err(_) => Remote::Failed,
                };
                this.update(cx, |state, cx| {
                    state.pictures.insert(url, remote);
                    cx.emit(StateEvent::PicturesLoaded);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    pub fn recording(&self) -> &Recording {
        &self.recording
    }

    pub fn start_recording(&mut self, cx: &mut Context<Self>) {
        match self.recording {
            Recording::Idle => {}
            Recording::Failed(_) => {}
            Recording::Live {
                since: _,
                channel: _,
            } => return,
            Recording::Sending => return,
        }
        let Some(channel) = self.selected else {
            return;
        };
        if let Err(reason) = self.recorder.start() {
            self.recording = Recording::Failed(reason);
            cx.notify();
            return;
        }
        let since = cx.background_executor().now();
        self.recording = Recording::Live { since, channel };
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(RECORDING_TICK).await;
                let Ok(going) = this.update(cx, |state, cx| state.recording_tick(since, cx)) else {
                    return;
                };
                if !going {
                    return;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn recording_tick(&mut self, started: Instant, cx: &mut Context<Self>) -> bool {
        let Recording::Live { since, channel: _ } = self.recording else {
            return false;
        };
        if since != started {
            return false;
        }
        if cx.background_executor().now() - since >= recorder::LIMIT {
            self.finish_recording(cx);
            return false;
        }
        cx.notify();
        true
    }

    pub fn finish_recording(&mut self, cx: &mut Context<Self>) {
        let Recording::Live { since: _, channel } = self.recording else {
            return;
        };
        let Take { kind, bytes } = match self.recorder.finish() {
            Ok(take) => take,
            Err(reason) => {
                self.recording = Recording::Failed(reason);
                cx.notify();
                return;
            }
        };
        self.recording = Recording::Sending;
        let request = self.client.post_voice(
            link::surface_id(channel),
            VoicePost {
                kind,
                bytes,
                addressed_agent_id: None,
                client_message_id: ClientMessageId::random(),
            },
        );
        cx.spawn(async move |this, cx| {
            let posted = request.await;
            this.update(cx, |state, cx| {
                state.recording = match posted {
                    Ok(_) => Recording::Idle,
                    Err(error) => Recording::Failed(voice_failure(&error)),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub fn cancel_recording(&mut self, cx: &mut Context<Self>) {
        match self.recording {
            Recording::Live {
                since: _,
                channel: _,
            } => self.recorder.cancel(),
            Recording::Failed(_) => {}
            Recording::Idle => return,
            Recording::Sending => return,
        }
        self.recording = Recording::Idle;
        cx.notify();
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

    pub fn people(&self) -> People<'_> {
        People {
            agents: &self.agents,
            directory: &self.directory,
            me: &self.me,
            gallery: &self.gallery,
        }
    }

    fn me_loaded(&mut self, me: v3::Me, cx: &mut Context<Self>) {
        let v3::Me {
            name,
            description,
            avatar_url,
        } = me;
        self.me = Me {
            name,
            description,
            picture: link::picture(avatar_url.as_ref()),
        };
        self.fill_pictures(cx);
        cx.notify();
    }

    fn fill_pictures(&mut self, cx: &mut Context<Self>) {
        let mut wanted = Vec::new();
        for agent in &self.agents {
            if let Some(picture) = &agent.picture {
                wanted.push(picture.clone());
            }
        }
        if let Some(picture) = &self.me.picture {
            wanted.push(picture.clone());
        }
        for picture in wanted {
            if !self.requested.insert(picture.clone()) {
                continue;
            }
            let request = self.client.avatar(&link::avatar_url(&picture));
            cx.spawn(async move |this, cx| {
                let image = match request.await {
                    Ok(bytes) => people::decode(bytes),
                    Err(_) => None,
                };
                let Some(image) = image else {
                    return;
                };
                this.update(cx, |state, cx| {
                    state.gallery.insert(picture, image);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
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

    pub fn working(&self, channel: ChannelId) -> Vec<String> {
        let surface = link::surface_id(channel);
        let mut names = Vec::new();
        for run in self.runs.values() {
            if run.surface_id != Some(surface) || run.state.is_finished() {
                continue;
            }
            for agent in &self.directory {
                if agent.id == run.agent_id && !names.contains(&agent.name) {
                    names.push(agent.name.clone());
                }
            }
        }
        names
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

    pub fn inspector(&self) -> Option<Inspector> {
        self.inspector
    }

    pub fn close_inspector(&mut self, cx: &mut Context<Self>) {
        self.inspector = None;
        self.settings = None;
        cx.notify();
    }

    pub fn settings(&self) -> Option<Settings> {
        self.settings
    }

    pub fn saving(&self) -> &Saving {
        &self.saving
    }

    pub fn field_error(&self) -> Option<&FieldError> {
        self.field_error.as_ref()
    }

    pub fn toast(&self) -> Option<&Toast> {
        self.toast.as_ref()
    }

    pub fn directory_agent(&self, agent: AgentId) -> Option<&v3::Agent> {
        let wanted = link::v3_agent_id(agent);
        let mut found = None;
        for candidate in &self.directory {
            if candidate.id == wanted {
                found = Some(candidate);
                break;
            }
        }
        found
    }

    pub fn agent_topics(&self, agent: AgentId) -> Vec<TopicRow> {
        agent_settings::topics_of(&self.surfaces, &self.directory, agent)
    }

    pub fn joinable_topics(&self, agent: AgentId) -> Vec<Joinable> {
        agent_settings::joinable(&self.surfaces, &self.directory, agent)
    }

    pub fn surface_name(&self, surface: v3::SurfaceId) -> Option<String> {
        let mut found = None;
        for candidate in &self.surfaces {
            if candidate.id == surface {
                found = Some(candidate.name.clone());
            }
        }
        found
    }

    pub fn open_settings(&mut self, agent: AgentId, cx: &mut Context<Self>) {
        let back = match self.settings.take() {
            Some(Settings { target: _, back }) => back,
            None => self.inspector.take(),
        };
        self.settings = Some(Settings {
            target: Target::Agent(agent),
            back,
        });
        self.saving = Saving::Idle;
        self.field_error = None;
        self.toast = None;
        cx.notify();
    }

    pub fn fires(&self) -> &[v3::FireMark] {
        &self.fires
    }

    pub fn tasks(&self) -> &[v3::Task] {
        &self.tasks
    }

    pub fn task(&self, id: &v3::TaskId) -> Option<&v3::Task> {
        let mut found = None;
        for task in &self.tasks {
            if task.id == *id {
                found = Some(task);
            }
        }
        found
    }

    pub fn task_runs(&self, id: &v3::TaskId) -> Option<&[v3::TaskRun]> {
        self.task_runs.get(id).map(Vec::as_slice)
    }

    fn merge_fires(&mut self, marks: Vec<v3::FireMark>) {
        for mark in marks {
            let mut known = false;
            for stored in &self.fires {
                if stored.task_id == mark.task_id && stored.at == mark.at {
                    known = true;
                }
            }
            if !known {
                self.fires.push(mark);
            }
        }
        self.fires.sort_by_key(|mark| mark.at);
    }

    fn task_fired(&mut self, fired: &v3::TaskFired, cx: &mut Context<Self>) {
        let v3::TaskFired {
            seq: _,
            surface_id,
            mark,
        } = fired;
        for task in &mut self.tasks {
            if task.id == mark.task_id {
                task.last_run_at = mark.at;
                task.last_outcome = Some(mark.outcome);
            }
        }
        self.task_runs.remove(&mark.task_id);
        let shown = match (self.selected, surface_id) {
            (Some(selected), Some(surface)) => link::surface_id(selected) == *surface,
            (Some(_), None) => false,
            (None, _) => false,
        };
        if !shown {
            cx.notify();
            return;
        }
        self.merge_fires(vec![mark.clone()]);
        cx.emit(StateEvent::MessageAppended);
        cx.notify();
    }

    pub fn selected_task(&self) -> Option<&v3::TaskId> {
        self.selected_task.as_ref()
    }

    pub fn open_task(&mut self, id: v3::TaskId, cx: &mut Context<Self>) {
        self.view = View::Automations;
        self.selected_task = Some(id.clone());
        self.load_task_runs(id, cx);
        cx.notify();
    }

    fn tasks_loaded(&mut self, tasks: Vec<v3::Task>, cx: &mut Context<Self>) {
        self.tasks = tasks;
        cx.emit(StateEvent::TasksLoaded);
        cx.notify();
    }

    pub fn load_tasks(&mut self, cx: &mut Context<Self>) {
        let request = self.client.tasks(v3::TaskScope::Recent);
        cx.spawn(async move |this, cx| {
            let Ok(tasks) = request.await else {
                return;
            };
            this.update(cx, |state, cx| state.tasks_loaded(tasks, cx))
                .ok();
        })
        .detach();
    }

    pub fn load_task_runs(&mut self, id: v3::TaskId, cx: &mut Context<Self>) {
        let request = self.client.task_runs(&id);
        cx.spawn(async move |this, cx| {
            let Ok(runs) = request.await else {
                return;
            };
            this.update(cx, |state, cx| {
                state.task_runs.insert(id, runs);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn set_task_paused(&mut self, id: v3::TaskId, pause: v3::Pause, cx: &mut Context<Self>) {
        let request = self.client.set_task_paused(&id, pause);
        cx.spawn(async move |this, cx| {
            let answer = request.await;
            this.update(cx, |state, cx| match answer {
                Ok(updated) => {
                    for task in &mut state.tasks {
                        if task.id == updated.id {
                            *task = updated.clone();
                        }
                    }
                    cx.notify();
                }
                Err(_) => state.load_tasks(cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn cancel_task(&mut self, id: v3::TaskId, cx: &mut Context<Self>) {
        let request = self.client.cancel_task(&id);
        cx.spawn(async move |this, cx| {
            let answer = request.await;
            this.update(cx, |state, cx| {
                if answer.is_ok() {
                    for task in &mut state.tasks {
                        if task.id == id {
                            task.status = v3::TaskStatus::Cancelled;
                        }
                    }
                }
                state.load_tasks(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn open_profile(&mut self, cx: &mut Context<Self>) {
        let back = match self.settings.take() {
            Some(Settings { target: _, back }) => back,
            None => self.inspector.take(),
        };
        self.settings = Some(Settings {
            target: Target::Me,
            back,
        });
        self.saving = Saving::Idle;
        self.field_error = None;
        self.toast = None;
        cx.notify();
    }

    pub fn save_my_name(&mut self, name: String, cx: &mut Context<Self>) {
        let name = name.trim().to_string();
        if name == self.me.name {
            return;
        }
        let patch = v3::MePatch {
            name: Some(name),
            description: None,
        };
        self.update_me(patch, Field::Name, cx);
    }

    pub fn save_my_description(&mut self, text: String, cx: &mut Context<Self>) {
        if text == self.me.description {
            return;
        }
        let patch = v3::MePatch {
            name: None,
            description: Some(text),
        };
        self.update_me(patch, Field::Description, cx);
    }

    fn update_me(&mut self, patch: v3::MePatch, field: Field, cx: &mut Context<Self>) {
        self.saving = Saving::Saving;
        if self
            .field_error
            .as_ref()
            .is_some_and(|error| error.field == field)
        {
            self.field_error = None;
        }
        cx.notify();
        let request = self.client.update_me(&patch);
        cx.spawn(async move |this, cx| {
            let answer = request.await;
            this.update(cx, |state, cx| match answer {
                Ok(me) => {
                    state.me_loaded(me, cx);
                    state.saving = Saving::Saved;
                    cx.notify();
                }
                Err(error) => {
                    state.saving = Saving::Failed(error.to_string());
                    state.field_error = Some(FieldError {
                        field,
                        message: field_message(&error),
                    });
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn upload_my_avatar(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        let Upload { kind, bytes } = match picture::prepare(bytes) {
            Ok(upload) => upload,
            Err(reason) => {
                self.saving = Saving::Failed(reason);
                cx.notify();
                return;
            }
        };
        let request = self.client.set_avatar(v3::AvatarOwner::Me, kind, bytes);
        self.after_my_avatar_write(async move { request.await.map(|_url| ()) }, cx);
    }

    pub fn clear_my_avatar(&mut self, cx: &mut Context<Self>) {
        let request = self.client.clear_avatar(v3::AvatarOwner::Me);
        self.after_my_avatar_write(request, cx);
    }

    fn after_my_avatar_write(
        &mut self,
        write: impl std::future::Future<Output = Result<(), v3::ApiError>> + 'static,
        cx: &mut Context<Self>,
    ) {
        self.saving = Saving::Saving;
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let written = write.await;
            let me = match written {
                Ok(()) => client.me().await,
                Err(error) => Err(error),
            };
            this.update(cx, |state, cx| match me {
                Ok(me) => {
                    state.me_loaded(me, cx);
                    state.saving = Saving::Saved;
                    cx.notify();
                }
                Err(error) => {
                    state.saving = Saving::Failed(error.to_string());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn back_to_run(&mut self, cx: &mut Context<Self>) {
        let Some(Settings { target: _, back }) = self.settings.take() else {
            return;
        };
        self.inspector = back;
        cx.notify();
    }

    pub fn save_description(&mut self, agent: AgentId, text: String, cx: &mut Context<Self>) {
        let unchanged = match self.directory_agent(agent) {
            Some(known) => known.description == text,
            None => return,
        };
        if unchanged {
            return;
        }
        let patch = v3::AgentPatch {
            description: Some(text),
            model: v3::ModelChange::Keep,
        };
        self.update_agent(agent, patch, Field::Description, cx);
    }

    pub fn save_model(&mut self, agent: AgentId, spec: String, cx: &mut Context<Self>) {
        let spec = spec.trim().to_string();
        let model = if spec.is_empty() {
            v3::ModelChange::Default
        } else {
            v3::ModelChange::Set(spec.clone())
        };
        let unchanged = match (self.directory_agent(agent), &model) {
            (Some(known), v3::ModelChange::Set(_)) => known.model == spec,
            (Some(_), v3::ModelChange::Default) => false,
            (Some(_), v3::ModelChange::Keep) => true,
            (None, v3::ModelChange::Set(_) | v3::ModelChange::Default | v3::ModelChange::Keep) => {
                return;
            }
        };
        if unchanged {
            return;
        }
        let patch = v3::AgentPatch {
            description: None,
            model,
        };
        self.update_agent(agent, patch, Field::Model, cx);
    }

    fn update_agent(
        &mut self,
        agent: AgentId,
        patch: v3::AgentPatch,
        field: Field,
        cx: &mut Context<Self>,
    ) {
        self.saving = Saving::Saving;
        if self
            .field_error
            .as_ref()
            .is_some_and(|error| error.field == field)
        {
            self.field_error = None;
        }
        cx.notify();
        let request = self.client.update_agent(link::v3_agent_id(agent), &patch);
        cx.spawn(async move |this, cx| {
            let answer = request.await;
            this.update(cx, |state, cx| match answer {
                Ok(updated) => {
                    state.store_agent(updated);
                    state.saving = Saving::Saved;
                    cx.notify();
                }
                Err(error) => {
                    state.saving = Saving::Failed(error.to_string());
                    state.field_error = Some(FieldError {
                        field,
                        message: field_message(&error),
                    });
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn store_agent(&mut self, updated: v3::Agent) {
        for agent in &mut self.directory {
            if agent.id == updated.id {
                *agent = updated.clone();
            }
        }
        self.refresh_agents();
    }

    fn store_surface(&mut self, updated: v3::Surface) {
        for surface in &mut self.surfaces {
            if surface.id == updated.id {
                *surface = updated.clone();
            }
        }
    }

    pub fn toggle_hears(&mut self, agent: AgentId, surface: v3::SurfaceId, cx: &mut Context<Self>) {
        let Some(row) = self.topic_row(agent, surface) else {
            return;
        };
        let change = v3::WiringChange {
            role: row.role,
            listens: !row.listens,
        };
        self.wire(agent, surface, change, None, cx);
    }

    pub fn make_lead(&mut self, agent: AgentId, surface: v3::SurfaceId, cx: &mut Context<Self>) {
        let Some(row) = self.topic_row(agent, surface) else {
            return;
        };
        let demoted = self.lead_of(surface);
        let name = row.name.clone();
        let undo = Undo::Restore {
            surface,
            agent,
            change: v3::WiringChange {
                role: row.role,
                listens: row.listens,
            },
            demoted,
        };
        let who = self.agent_name(agent);
        let toast = (format!("{who} is now Lead in #{name}"), undo);
        let change = v3::WiringChange {
            role: v3::Role::Lead,
            listens: row.listens,
        };
        self.wire(agent, surface, change, Some(toast), cx);
    }

    pub fn join_topic(&mut self, agent: AgentId, option: &Joinable, cx: &mut Context<Self>) {
        let change = v3::WiringChange {
            role: agent_settings::joining_role(option),
            listens: false,
        };
        self.wire(agent, option.surface, change, None, cx);
    }

    pub fn leave_topic(&mut self, agent: AgentId, surface: v3::SurfaceId, cx: &mut Context<Self>) {
        let Some(row) = self.topic_row(agent, surface) else {
            return;
        };
        self.saving = Saving::Saving;
        cx.notify();
        let request = self.client.remove_wiring(surface, link::v3_agent_id(agent));
        let undo = Undo::Rewire {
            surface,
            agent,
            change: v3::WiringChange {
                role: row.role,
                listens: row.listens,
            },
        };
        let text = format!("Removed from #{}", row.name);
        cx.spawn(async move |this, cx| {
            let answer = request.await;
            this.update(cx, |state, cx| match answer {
                Ok(()) => {
                    state.unwire_locally(agent, surface);
                    state.saving = Saving::Saved;
                    state.show_toast(text, undo, cx);
                    cx.notify();
                }
                Err(error) => {
                    state.saving = Saving::Failed(error.to_string());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(Toast {
            id: _,
            text: _,
            undo,
        }) = self.toast.take()
        else {
            return;
        };
        match undo {
            Undo::Rewire {
                surface,
                agent,
                change,
            } => self.wire(agent, surface, change, None, cx),
            Undo::Restore {
                surface,
                agent,
                change,
                demoted,
            } => self.restore_lead(surface, agent, change, demoted, cx),
        }
        cx.notify();
    }

    fn restore_lead(
        &mut self,
        surface: v3::SurfaceId,
        agent: AgentId,
        change: v3::WiringChange,
        demoted: Option<AgentId>,
        cx: &mut Context<Self>,
    ) {
        let Some(previous) = demoted else {
            self.wire(agent, surface, change, None, cx);
            return;
        };
        let back = v3::WiringChange {
            role: v3::Role::Lead,
            listens: self
                .topic_row(previous, surface)
                .is_some_and(|row| row.listens),
        };
        self.saving = Saving::Saving;
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let promoted = client
                .set_wiring(surface, link::v3_agent_id(previous), back)
                .await;
            let answer = match promoted {
                Ok(_) => {
                    client
                        .set_wiring(surface, link::v3_agent_id(agent), change)
                        .await
                }
                Err(error) => Err(error),
            };
            this.update(cx, |state, cx| match answer {
                Ok(updated) => {
                    state.store_surface(updated);
                    state.saving = Saving::Saved;
                    cx.notify();
                }
                Err(error) => {
                    state.saving = Saving::Failed(error.to_string());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn wire(
        &mut self,
        agent: AgentId,
        surface: v3::SurfaceId,
        change: v3::WiringChange,
        toast: Option<(String, Undo)>,
        cx: &mut Context<Self>,
    ) {
        self.saving = Saving::Saving;
        cx.notify();
        let request = self
            .client
            .set_wiring(surface, link::v3_agent_id(agent), change);
        cx.spawn(async move |this, cx| {
            let answer = request.await;
            this.update(cx, |state, cx| match answer {
                Ok(updated) => {
                    state.store_surface(updated);
                    state.saving = Saving::Saved;
                    if let Some((text, undo)) = toast {
                        state.show_toast(text, undo, cx);
                    }
                    cx.notify();
                }
                Err(error) => {
                    state.saving = Saving::Failed(error.to_string());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn mention(&mut self, agent: AgentId, cx: &mut Context<Self>) {
        let Some(known) = self.directory_agent(agent) else {
            return;
        };
        let ident = known.ident.clone();
        self.view = View::Conversation;
        cx.emit(StateEvent::Mention(format!("@{ident} ")));
        cx.notify();
    }

    pub fn view_run(&mut self, agent: AgentId, cx: &mut Context<Self>) {
        let Some(live) = self
            .directory_agent(agent)
            .and_then(|known| known.live_run.clone())
        else {
            return;
        };
        self.settings = None;
        let v3::SurfaceId(raw) = live.surface_id;
        self.select(ChannelId(raw), cx);
    }

    pub fn upload_avatar(&mut self, agent: AgentId, bytes: Vec<u8>, cx: &mut Context<Self>) {
        let Upload { kind, bytes } = match picture::prepare(bytes) {
            Ok(upload) => upload,
            Err(reason) => {
                self.saving = Saving::Failed(reason);
                cx.notify();
                return;
            }
        };
        let request = self.client.set_avatar(
            v3::AvatarOwner::Agent(link::v3_agent_id(agent)),
            kind,
            bytes,
        );
        self.after_avatar_write(async move { request.await.map(|_url| ()) }, cx);
    }

    pub fn clear_avatar(&mut self, agent: AgentId, cx: &mut Context<Self>) {
        let request = self
            .client
            .clear_avatar(v3::AvatarOwner::Agent(link::v3_agent_id(agent)));
        self.after_avatar_write(request, cx);
    }

    fn after_avatar_write(
        &mut self,
        write: impl std::future::Future<Output = Result<(), v3::ApiError>> + 'static,
        cx: &mut Context<Self>,
    ) {
        self.saving = Saving::Saving;
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let written = write.await;
            let agents = match written {
                Ok(()) => client.agents().await,
                Err(error) => Err(error),
            };
            this.update(cx, |state, cx| match agents {
                Ok(agents) => {
                    state.directory = agents;
                    state.refresh_agents();
                    state.fill_pictures(cx);
                    state.saving = Saving::Saved;
                    cx.notify();
                }
                Err(error) => {
                    state.saving = Saving::Failed(error.to_string());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn unwire_locally(&mut self, agent: AgentId, surface: v3::SurfaceId) {
        let wanted = link::v3_agent_id(agent);
        for candidate in &mut self.surfaces {
            if candidate.id != surface {
                continue;
            }
            let mut kept = Vec::new();
            for wiring in std::mem::take(&mut candidate.agents) {
                if wiring.agent_id != wanted {
                    kept.push(wiring);
                }
            }
            candidate.agents = kept;
        }
    }

    fn show_toast(&mut self, text: String, undo: Undo, cx: &mut Context<Self>) {
        self.next_toast += 1;
        let id = self.next_toast;
        self.toast = Some(Toast { id, text, undo });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_LIFETIME).await;
            this.update(cx, |state, cx| {
                if state.toast.as_ref().is_some_and(|toast| toast.id == id) {
                    state.toast = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn topic_row(&self, agent: AgentId, surface: v3::SurfaceId) -> Option<TopicRow> {
        let mut found = None;
        for row in self.agent_topics(agent) {
            if row.surface == surface {
                found = Some(row);
            }
        }
        found
    }

    fn lead_of(&self, surface: v3::SurfaceId) -> Option<AgentId> {
        let mut found = None;
        for candidate in &self.surfaces {
            if candidate.id == surface {
                found = candidate.lead_agent_id.map(link::agent_id);
            }
        }
        found
    }

    fn agent_name(&self, agent: AgentId) -> String {
        match self.directory_agent(agent) {
            Some(known) => known.name.clone(),
            None => "the agent".into(),
        }
    }

    pub fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        if let Some(inspector) = &mut self.inspector {
            inspector.filter = filter;
        }
        cx.notify();
    }

    pub fn toggle(&mut self, disclosure: Disclosure, cx: &mut Context<Self>) {
        if let Disclosure::Inspect(message) = disclosure {
            self.settings = None;
            self.inspector = Some(Inspector {
                message,
                filter: Filter::All,
            });
            self.ensure_log(message, cx);
            cx.notify();
            return;
        }
        if !self.toggled.remove(&disclosure) {
            self.toggled.insert(disclosure.clone());
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
            self.divider = self.divider_for(channel);
            self.selected = Some(channel);
            self.halt();
            self.inspector = None;
            self.fires.clear();
            if let Some(settings) = &mut self.settings {
                settings.back = None;
            }
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
            View::Automations => Segment::Automations,
        }
    }

    pub fn activate_segment(&mut self, segment: Segment, cx: &mut Context<Self>) {
        match segment {
            Segment::Agents => self.view = View::Agents,
            Segment::Channel => self.view = View::Conversation,
            Segment::Automations => {
                self.view = View::Automations;
                self.load_tasks(cx);
            }
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
        self.merge_fires(page.automations);
        if older.is_empty() {
            return;
        }
        older.append(&mut self.messages);
        self.messages = older;
        self.fill_waveforms(cx);
        self.fill_message_pictures(cx);
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
        self.merge_fires(page.automations);
        self.history = if page.has_more {
            History::More
        } else {
            History::Complete
        };
        self.fill_waveforms(cx);
        self.fill_message_pictures(cx);
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
            if let Some(cursor) = surface.last_read_message_id {
                self.cursors
                    .insert(link::channel_id(surface.id), link::message_id(cursor));
            }
        }
        self.channels = channels;
        self.surfaces = surfaces;
        self.directory = agents;
        self.refresh_agents();
        self.fill_pictures(cx);
        let selected = match self.selected {
            Some(selected) => Some(selected),
            None => self.channels.first().map(|channel| channel.id),
        };
        if let Some(selected) = selected {
            if self.selected != Some(selected) {
                self.divider = self.divider_for(selected);
            }
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
            Frame::TaskFired(fired) => {
                self.task_fired(fired, cx);
                false
            }
            Frame::SurfaceRead(read) => {
                self.surface_read(read, cx);
                false
            }
            Frame::StepText(_) => self.apply_to_run(&frame),
            Frame::ToolStarted(_) => self.apply_to_run(&frame),
            Frame::ToolFinished(_) => self.apply_to_run(&frame),
            Frame::Task(_) => self.apply_to_run(&frame),
            Frame::Status(_) => self.apply_to_run(&frame),
            Frame::RunReset(_) => self.apply_to_run(&frame),
            Frame::RunFinished(_) => {
                let changed = self.apply_to_run(&frame);
                self.close_quiet_run(&frame) || changed
            }
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

    fn close_quiet_run(&mut self, frame: &Frame) -> bool {
        let Some(run_id) = frame.run_id() else {
            return false;
        };
        let Some(run) = self.runs.get(run_id) else {
            return false;
        };
        if run.state != RunState::Ok {
            return false;
        }
        self.runs.remove(run_id);
        true
    }

    fn message_created(&mut self, message: &v3::Message, cx: &mut Context<Self>) {
        if let Some(run_id) = &message.run_id
            && ends_its_run(message.kind)
            && let Some(run) = self.runs.get(run_id)
            && run.state != RunState::Interrupted
        {
            self.runs.remove(run_id);
            self.refresh_agents();
            cx.emit(StateEvent::RunsChanged);
        }
        let shown = self.selected == Some(link::channel_id(message.surface_id));
        if !(shown && self.window_active) {
            self.count_unread(message, cx);
        }
        if !shown {
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
            self.fill_message_pictures(cx);
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

fn ends_its_run(kind: v3::MessageKind) -> bool {
    match kind {
        v3::MessageKind::Answer => true,
        v3::MessageKind::Notice => true,
        v3::MessageKind::Post => false,
        v3::MessageKind::A2a => false,
        v3::MessageKind::User => false,
        v3::MessageKind::Unknown => false,
    }
}

fn voice_failure(error: &v3::ApiError) -> String {
    match error {
        v3::ApiError::Invalid(message) => message.clone(),
        v3::ApiError::Refused { code: _, message } => message.clone(),
        v3::ApiError::NotFound => "the channel is gone; not sent".into(),
        v3::ApiError::Unauthorized => "the daemon rejected the token".into(),
        v3::ApiError::Conflict => "the recording was already sent elsewhere".into(),
        v3::ApiError::Unavailable => "the daemon is unavailable; not sent".into(),
        v3::ApiError::Transport(_) => "the daemon did not answer; not sent".into(),
        v3::ApiError::Decode(_) => "the daemon answered something unexpected".into(),
    }
}

fn field_message(error: &v3::ApiError) -> String {
    match error {
        v3::ApiError::Invalid(message) => message.clone(),
        v3::ApiError::Refused { code: _, message } => message.clone(),
        v3::ApiError::NotFound => "the agent is gone".into(),
        v3::ApiError::Unauthorized => "the daemon rejected the token".into(),
        v3::ApiError::Conflict => "the change conflicts with the current state".into(),
        v3::ApiError::Unavailable => "the daemon is unavailable; not saved".into(),
        v3::ApiError::Transport(_) => "the daemon did not answer; not saved".into(),
        v3::ApiError::Decode(_) => "the daemon answered something unexpected".into(),
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
    if let Ok(me) = client.me().await {
        this.update(cx, |state, cx| state.me_loaded(me, cx)).ok();
    }
    if let Ok(tasks) = client.tasks(v3::TaskScope::Recent).await {
        this.update(cx, |state, cx| state.tasks_loaded(tasks, cx))
            .ok();
    }
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
        Frame::TaskFired(_) => false,
        Frame::SurfaceRead(_) => false,
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
    use tuclaw_core::v3::{self, AgentId as WireAgent, Scenario};

    use std::time::Duration;

    use gpui::Entity;
    use tuclaw_core::model::MessageId;
    use tuclaw_core::v3::MockTransport;

    use super::{
        AppState, Filter, Inspector, Link, Player, Segment, SidebarVisibility, TICK, View,
    };
    use crate::testing::{FakeSpeaker, channel_named, loaded, mocked, play, speaking};

    #[gpui::test]
    fn settings_take_the_run_slot_and_give_it_back(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        let inspector = Inspector {
            message: MessageId(7),
            filter: Filter::Tools,
        };
        state.update(cx, |state, cx| {
            state.inspector = Some(inspector);
            state.open_settings(AgentId(3), cx);
        });
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.inspector(), None);
            assert_eq!(
                state.settings(),
                Some(crate::agent_settings::Settings {
                    target: crate::agent_settings::Target::Agent(AgentId(3)),
                    back: Some(inspector),
                })
            );
        });
        state.update(cx, |state, cx| state.open_settings(AgentId(1), cx));
        state.read_with(cx, |state, _cx| {
            assert_eq!(
                state.settings().and_then(|settings| settings.back),
                Some(inspector)
            );
        });
        state.update(cx, |state, cx| state.back_to_run(cx));
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.settings(), None);
            assert_eq!(state.inspector(), Some(inspector));
        });
    }

    #[gpui::test]
    fn make_lead_demotes_the_old_lead_and_undo_restores_both(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        let general = v3::SurfaceId(1);
        state.update(cx, |state, cx| state.make_lead(AgentId(3), general, cx));
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            let roles = roles_in(state, general);
            assert_eq!(roles, vec![(1, v3::Role::Mention), (3, v3::Role::Lead)]);
            let Some(toast) = state.toast() else {
                panic!("make lead offers an undo");
            };
            assert!(toast.text.contains("Lead in #General"));
        });
        state.update(cx, |state, cx| state.undo(cx));
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            assert_eq!(
                roles_in(state, general),
                vec![(1, v3::Role::Lead), (3, v3::Role::Mention)]
            );
            assert_eq!(state.toast(), None);
        });
    }

    #[gpui::test]
    fn leaving_a_topic_can_be_undone(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        let general = v3::SurfaceId(1);
        state.update(cx, |state, cx| state.leave_topic(AgentId(3), general, cx));
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            assert_eq!(roles_in(state, general), vec![(1, v3::Role::Lead)]);
            assert!(state.toast().is_some());
        });
        state.update(cx, |state, cx| state.undo(cx));
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            assert_eq!(
                roles_in(state, general),
                vec![(1, v3::Role::Lead), (3, v3::Role::Mention)]
            );
        });
    }

    #[gpui::test]
    fn a_rejected_model_is_reported_on_its_field(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        state.update(cx, |state, cx| {
            state.save_model(AgentId(3), "opus medium".into(), cx)
        });
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            let Some(error) = state.field_error() else {
                panic!("the model is refused");
            };
            assert_eq!(error.field, crate::agent_settings::Field::Model);
            assert_eq!(
                state
                    .directory_agent(AgentId(3))
                    .map(|agent| agent.model.as_str()),
                Some("sonnet")
            );
        });
        state.update(cx, |state, cx| {
            state.save_model(AgentId(3), "opus[1m]:high".into(), cx);
            state.save_description(AgentId(3), "Pulls releases".into(), cx);
        });
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            assert_eq!(state.field_error(), None);
            assert_eq!(state.saving(), &crate::agent_settings::Saving::Saved);
            let Some(agent) = state.directory_agent(AgentId(3)) else {
                panic!("agent 3 is known");
            };
            assert_eq!(agent.model, "opus[1m]:high");
            assert_eq!(agent.description, "Pulls releases");
        });
    }

    fn roles_in(state: &AppState, surface: v3::SurfaceId) -> Vec<(i64, v3::Role)> {
        let mut roles = Vec::new();
        for candidate in &state.surfaces {
            if candidate.id != surface {
                continue;
            }
            for wiring in &candidate.agents {
                let v3::AgentId(id) = wiring.agent_id;
                roles.push((id, wiring.role));
            }
        }
        roles.sort_by_key(|(id, _role)| *id);
        roles
    }

    #[gpui::test]
    fn the_start_loads_me_and_every_avatar(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            let people = state.people();
            assert_eq!(people.me.name, "You");
            assert!(people.picture(people.me.picture.as_ref()).is_some());
            let mut drawn = Vec::new();
            for agent in people.agents {
                drawn.push(people.picture(agent.picture.as_ref()).is_some());
            }
            assert_eq!(drawn, vec![true, false, false, false]);
        });
    }

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

    #[gpui::test]
    fn a_post_during_a_run_keeps_the_live_run(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        let magnet = channel_named(&state, cx, "Magnet Feed");
        state.update(cx, |state, cx| state.select(magnet, cx));
        cx.run_until_parked();
        let run = state.read_with(cx, |state, _cx| {
            let live = state.live_runs();
            let run = live.first().expect("Magnet Feed has a live run");
            run.id.clone().expect("the live run has an id")
        });
        let tuclaw_core::v3::RunId(raw) = run.clone();
        let created = |id: i64, kind: &str| {
            let frame = serde_json::json!({
                "v": 1, "seq": 900 + id, "type": "message.created", "surface_id": 2, "run_id": raw,
                "at": "2026-10-04T11:15:00Z",
                "payload": {"message": {
                    "id": id, "surface_id": 2, "kind": kind,
                    "author": {"kind": "agent", "agent_id": 3},
                    "text": "Читаю вопросы, это займёт несколько минут.", "run_id": raw,
                    "created_at": "2026-10-04T11:15:00Z"
                }}
            });
            tuclaw_core::v3::decode(&frame.to_string()).expect("a frame")
        };
        state.update(cx, |state, cx| state.apply(created(9298, "post"), cx));
        state.read_with(cx, |state, _cx| {
            assert_eq!(
                state.live_runs().len(),
                1,
                "a send_message post leaves the run live"
            );
            assert!(
                state
                    .messages()
                    .iter()
                    .any(|message| message.id == MessageId(9298))
            );
        });
        state.update(cx, |state, cx| state.apply(created(9299, "a2a"), cx));
        state.read_with(cx, |state, _cx| assert_eq!(state.live_runs().len(), 1));
        state.update(cx, |state, cx| state.apply(created(9300, "answer"), cx));
        state.read_with(cx, |state, _cx| {
            assert!(
                state.live_runs().is_empty(),
                "the answer replaces the live run"
            );
        });
    }

    #[gpui::test]
    fn a_run_that_finishes_without_an_answer_closes_its_card(cx: &mut TestAppContext) {
        let (_mock, state) = loaded(cx);
        let magnet = channel_named(&state, cx, "Magnet Feed");
        state.update(cx, |state, cx| state.select(magnet, cx));
        cx.run_until_parked();
        let tuclaw_core::v3::RunId(raw) = state.read_with(cx, |state, _cx| {
            let live = state.live_runs();
            live.first()
                .and_then(|run| run.id.clone())
                .expect("Magnet Feed has a live run")
        });
        let frame = serde_json::json!({
            "v": 1, "seq": 1000000, "type": "run.finished", "surface_id": 2, "run_id": raw,
            "at": "2026-10-04T11:25:00Z",
            "payload": {"is_error": false, "terminal_reason": "success"}
        });
        let frame = tuclaw_core::v3::decode(&frame.to_string()).expect("a frame");
        state.update(cx, |state, cx| state.apply(frame, cx));
        state.read_with(cx, |state, _cx| {
            assert!(
                state.live_runs().is_empty(),
                "a silent finish leaves nothing to show"
            );
        });
    }

    #[gpui::test]
    fn the_inspector_opens_on_a_run_filters_and_closes(cx: &mut TestAppContext) {
        use crate::runlog::{Disclosure, RunLog};
        let (_mock, state) = crate::testing::seeded(cx, run_world());
        state.update(cx, |state, cx| {
            state.toggle(Disclosure::Inspect(MessageId(1)), cx)
        });
        cx.run_until_parked();
        state.read_with(cx, |state, _cx| {
            assert_eq!(
                state.inspector(),
                Some(super::Inspector {
                    message: MessageId(1),
                    filter: super::Filter::All
                })
            );
            let Some(RunLog::Loaded(_)) = state.run_log("6763eb02-7f3e-4c4d-9b1a-2f0c5d8e9a11")
            else {
                panic!("opening the inspector loads the run");
            };
        });
        state.update(cx, |state, cx| state.set_filter(super::Filter::Errors, cx));
        state.read_with(cx, |state, _cx| {
            assert_eq!(
                state.inspector().map(|inspector| inspector.filter),
                Some(super::Filter::Errors)
            );
        });
        state.update(cx, |state, cx| state.close_inspector(cx));
        state.read_with(cx, |state, _cx| assert_eq!(state.inspector(), None));
    }
}
