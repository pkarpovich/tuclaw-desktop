//! An in-process daemon speaking the v3 contract, for development and tests.
//!
//! [`MockTransport`] answers REST from a scripted world and streams scripted frames. Every frame
//! is built as the JSON line the daemon would send and goes through [`decode`], so the mock drives
//! the same decoder as the socket. With [`Pace::Stepped`] nothing plays until the caller's
//! [`MockTransport::step`] or [`MockTransport::play_all`], so tests stay on their own thread; with
//! [`Pace::Realtime`] a task on the `core` runtime plays the queue with real delays.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use futures::FutureExt;
use futures::channel::mpsc::{self, TryRecvError, UnboundedReceiver, UnboundedSender};
use futures::future::{BoxFuture, ready};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;
use time::macros::datetime;
use time::{Duration as TimeDuration, OffsetDateTime};
use uuid::Uuid;

use super::client::AvatarOwner;
use super::dto::{
    Agent, AgentId, AgentRun, AgentState, Attachment, AttachmentId, AttachmentKind, Author,
    AuthorKind, AvatarSet, AvatarUrl, Binding, Channel, ClientMessageId, ContextUsage, FireMark,
    Group, GroupId, ImageKind, InputId, Me, MePatch, Message, MessageId, MessageKind, MessagesPage,
    Mirror, Outcome, Placement, Post, Posted, ReadAnswer, Role, RowKind, RunDetail, RunId, RunRow,
    RunStatus, RunSummary, Schedule, ScheduleKind, Seq, StepRow, Surface, SurfaceId, SurfaceKind,
    SurfaceRun, Task, TaskId, TaskRun, TaskStatus, ToolUseId, Usage, Wiring, WiringChange,
};
use super::frames::{
    AuthMode, Capabilities, ClientFrame, Frame, Gap, Hello, InputAccepted, RunFinished,
    RunSnapshot, RunStarted, StepText, TaskUpdate, ToolFinished, ToolStarted, decode,
};
use super::runtime::handle;
use super::transport::{ApiError, Body, Connection, Method, PublicUrl, Request, Transport};

const RING: usize = 1000;
const DELTA_DELAY: Duration = Duration::from_millis(50);
const STEP_DELAY: Duration = Duration::from_millis(150);
const IDLE_DELAY: Duration = Duration::from_millis(50);

/// A world the mock starts from: the contract's REST bodies, as a daemon would answer them.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::Seed;
///
/// let seed: Seed = serde_json::from_str(r#"{"surfaces": [], "agents": []}"#).unwrap();
/// assert!(seed.messages.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Seed {
    /// The answer to `GET /surfaces`.
    pub surfaces: Vec<Surface>,
    /// The answer to `GET /agents`.
    pub agents: Vec<Agent>,
    /// Every message of every surface, oldest first.
    #[serde(default)]
    pub messages: Vec<Message>,
    /// The runs `GET /runs/{id}` answers, finished ones included.
    #[serde(default)]
    pub runs: Vec<RunDetail>,
    /// Where the bytes of each attachment live, for `GET /attachments/{id}`.
    #[serde(default)]
    pub media: Vec<SeedMedia>,
    /// The answer to `GET /me`; "You" with no avatar when absent.
    #[serde(default)]
    pub me: Option<Me>,
    /// The answer to `GET /tasks`.
    #[serde(default)]
    pub tasks: Vec<Task>,
}

/// A local file serving one attachment of a [`Seed`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeedMedia {
    /// The attachment.
    pub id: AttachmentId,
    /// The file holding its bytes.
    pub path: PathBuf,
}

const TONE: &[u8] = include_bytes!("../../testdata/v3/media/tone.ogg");
const AGENT_AVATAR: &[u8] = include_bytes!("../../testdata/v3/media/avatar_agent.png");
const MY_AVATAR: &[u8] = include_bytes!("../../testdata/v3/media/avatar_me.png");
const AVATAR_LIMIT: usize = 2 * 1024 * 1024;
const DEFAULT_NAME: &str = "You";
const DEFAULT_MODEL: &str = "opus[1m]";
const DESCRIPTION_LIMIT: usize = 140;
const PROFILE_LIMIT: usize = 280;

#[derive(Debug, Clone)]
struct Avatar {
    bytes: Vec<u8>,
    version: String,
}

impl Avatar {
    fn new(bytes: Vec<u8>) -> Avatar {
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        Avatar {
            version: format!("{:016x}", hasher.finish()),
            bytes,
        }
    }

    fn url(&self, owner: AvatarOwner) -> AvatarUrl {
        let path = match owner {
            AvatarOwner::Agent(AgentId(agent)) => format!("/api/v3/agents/{agent}/avatar"),
            AvatarOwner::Me => "/api/v3/me/avatar".into(),
        };
        AvatarUrl(format!("{path}?v={}", self.version))
    }
}

#[derive(Debug, Clone)]
enum Media {
    Embedded(&'static [u8]),
    Owned(Vec<u8>),
    File(PathBuf),
}

/// How the mock plays its queued frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    /// A task on the `core` runtime plays the queue with real delays (`text.delta` every 50 ms).
    Realtime,
    /// Nothing plays until [`MockTransport::step`] or [`MockTransport::play_all`].
    Stepped,
}

/// Knobs for the failure paths a client has to handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Scenario {
    /// The next connection with `since` gets a `gap` whatever its `since`.
    pub gap_once: bool,
    /// The next REST call answers `503 unavailable`.
    pub unavailable_once: bool,
}

#[derive(Debug, Clone)]
struct MockRun {
    id: RunId,
    agent: AgentId,
    surface: SurfaceId,
    origin: String,
    kind: String,
    status: RunStatus,
    terminal_reason: Option<String>,
    started_at: OffsetDateTime,
    finished_at: Option<OffsetDateTime>,
    steps: Vec<StepRow>,
    segment: String,
    last_seq: Seq,
}

#[derive(Debug, Clone)]
struct Begin {
    id: RunId,
    agent: AgentId,
    surface: SurfaceId,
    inputs: Vec<InputId>,
    origin: String,
}

#[derive(Debug, Clone)]
enum Script {
    Accepted(InputAccepted),
    Created(Message),
    Started(Begin),
    Delta(RunId, String),
    Text(RunId, String),
    ToolStart(RunId, ToolStarted),
    ToolFinish(RunId, ToolFinished),
    Task(RunId, TaskUpdate),
    Finished(RunId, RunFinished),
}

impl Script {
    fn run(&self) -> Option<&RunId> {
        match self {
            Script::Accepted(_) => None,
            Script::Created(message) => message.run_id.as_ref(),
            Script::Started(begin) => Some(&begin.id),
            Script::Delta(run, _) => Some(run),
            Script::Text(run, _) => Some(run),
            Script::ToolStart(run, _) => Some(run),
            Script::ToolFinish(run, _) => Some(run),
            Script::Task(run, _) => Some(run),
            Script::Finished(run, _) => Some(run),
        }
    }

    fn delay(&self) -> Duration {
        match self {
            Script::Delta(_, _) => DELTA_DELAY,
            Script::Accepted(_) => STEP_DELAY,
            Script::Created(_) => STEP_DELAY,
            Script::Started(_) => STEP_DELAY,
            Script::Text(_, _) => STEP_DELAY,
            Script::ToolStart(_, _) => STEP_DELAY,
            Script::ToolFinish(_, _) => STEP_DELAY,
            Script::Task(_, _) => STEP_DELAY,
            Script::Finished(_, _) => STEP_DELAY,
        }
    }
}

struct Subscriber {
    frames: UnboundedSender<Frame>,
    control: UnboundedReceiver<ClientFrame>,
    focus: BTreeSet<SurfaceId>,
}

enum Reach {
    Everyone,
    Focused(SurfaceId),
}

struct World {
    scenario: Scenario,
    now: OffsetDateTime,
    head: i64,
    ring: VecDeque<(i64, String)>,
    surfaces: Vec<Surface>,
    agents: Vec<Agent>,
    messages: Vec<Message>,
    runs: Vec<MockRun>,
    queue: VecDeque<Script>,
    subscribers: Vec<Subscriber>,
    posted: HashMap<ClientMessageId, (SurfaceId, Posted)>,
    media: HashMap<AttachmentId, Media>,
    public: HashMap<String, Vec<u8>>,
    cursors: HashMap<SurfaceId, MessageId>,
    groups: Vec<Group>,
    my_name: String,
    my_description: String,
    tasks: Vec<Task>,
    task_runs: HashMap<TaskId, Vec<TaskRun>>,
    fires: Vec<(SurfaceId, FireMark)>,
    avatars: HashMap<AvatarOwner, Avatar>,
    next_message: i64,
    next_input: i64,
}

/// The in-process daemon: a scripted world behind the [`Transport`] seam.
///
/// `Clone` shares the world, so a test keeps one handle to drive the script while a
/// [`Client`](super::Client) holds another.
///
/// # Examples
///
/// ```
/// use futures::executor::block_on;
/// use tuclaw_core::v3::{Client, MockTransport, Pace, Scenario};
///
/// let mock = MockTransport::new(Scenario::default(), Pace::Stepped);
/// let client = Client::mock(&mock);
/// let surfaces = block_on(client.surfaces()).unwrap();
/// assert_eq!(surfaces.len(), 3);
/// ```
#[derive(Clone)]
pub struct MockTransport {
    world: Arc<Mutex<World>>,
}

impl MockTransport {
    /// Creates the mock world and, with [`Pace::Realtime`], starts playing it.
    pub fn new(scenario: Scenario, pace: Pace) -> MockTransport {
        MockTransport::start(World::new(scenario), pace)
    }

    /// Creates a mock world from a [`Seed`] instead of the built-in one, e.g. a snapshot of a real
    /// daemon's data; posts still play the canned run.
    pub fn seeded(seed: Seed, scenario: Scenario, pace: Pace) -> MockTransport {
        MockTransport::start(World::from_seed(seed, scenario), pace)
    }

    fn start(world: World, pace: Pace) -> MockTransport {
        let world = Arc::new(Mutex::new(world));
        match pace {
            Pace::Realtime => {
                let weak = Arc::downgrade(&world);
                handle().spawn(play_realtime(weak));
            }
            Pace::Stepped => {}
        }
        MockTransport { world }
    }

    /// Plays the next queued frame; returns whether there was one.
    pub fn step(&self) -> bool {
        let mut world = self.lock();
        world.drain_control();
        let Some(script) = world.queue.pop_front() else {
            return false;
        };
        world.emit(script);
        true
    }

    /// Plays every queued frame, including ones queued while playing; returns how many.
    pub fn play_all(&self) -> usize {
        let mut played = 0;
        while self.step() {
            played += 1;
        }
        played
    }

    /// Returns how many frames are queued.
    pub fn pending(&self) -> usize {
        self.lock().queue.len()
    }

    /// Returns the newest seq in the mock's event log.
    pub fn head(&self) -> Seq {
        Seq(self.lock().head)
    }

    /// Fires an automation now: records the attempt and sends `task.fired` on its surface.
    pub fn fire_task(&self, id: &TaskId, outcome: Outcome) {
        self.lock().fire(id, outcome);
    }

    /// Serves `bytes` at a public `url`, as a picture host an agent links to would.
    pub fn serve_public(&self, url: &str, bytes: Vec<u8>) {
        self.lock().public.insert(url.to_string(), bytes);
    }

    /// Applies the client frames sent so far (a `focus` sends snapshots right away).
    pub fn pump_control(&self) {
        self.lock().drain_control();
    }

    /// Queues a message an agent posts on a surface outside any run, e.g. a picture it linked.
    pub fn agent_posts(&self, surface: SurfaceId, agent: AgentId, text: &str) {
        let mut world = self.lock();
        let message = Message {
            id: world.next_message(),
            surface_id: surface,
            kind: MessageKind::Post,
            author: Author {
                kind: AuthorKind::Agent,
                agent_id: Some(agent),
            },
            addressed_agent_id: None,
            reply_to_message_id: None,
            text: text.to_string(),
            run_id: None,
            origin: "user".into(),
            channel: None,
            client_message_id: None,
            created_at: world.now,
            run_summary: None,
            attachments: Vec::new(),
        };
        world.queue.push_back(Script::Created(message));
    }

    /// Queues a Telegram user message in General and the lead's run answering it.
    pub fn telegram_tick(&self) {
        let mut world = self.lock();
        let surface = SurfaceId(1);
        let agent = AgentId(1);
        let message = world.user_message(
            surface,
            "Что по погоде на выходные?",
            Channel::Telegram,
            None,
        );
        let input = world.next_input();
        world.queue.push_back(Script::Created(message));
        let run = run_script(Begin {
            id: fresh_run(),
            agent,
            surface,
            inputs: vec![input],
            origin: "user".into(),
        });
        world.queue.extend(run);
    }

    /// Queues two runs on General at once: the lead asks `@magnet_feed`, which answers while the
    /// lead's run continues.
    pub fn play_a2a(&self) {
        let mut world = self.lock();
        let surface = SurfaceId(1);
        let lead = run_script(Begin {
            id: fresh_run(),
            agent: AgentId(1),
            surface,
            inputs: vec![world.next_input()],
            origin: "user".into(),
        });
        let target = run_script(Begin {
            id: fresh_run(),
            agent: AgentId(3),
            surface,
            inputs: vec![world.next_input()],
            origin: "a2a".into(),
        });
        let mut lead = VecDeque::from(lead);
        let mut target = VecDeque::from(target);
        loop {
            let first = lead.pop_front();
            let second = target.pop_front();
            if first.is_none() && second.is_none() {
                break;
            }
            if let Some(script) = first {
                world.queue.push_back(script);
            }
            if let Some(script) = second {
                world.queue.push_back(script);
            }
        }
    }

    /// Makes the next REST call answer `503 unavailable`, as [`Scenario::unavailable_once`] does at
    /// start.
    pub fn fail_next_call(&self) {
        self.lock().scenario.unavailable_once = true;
    }

    /// Closes every open event socket, as a dropped network would.
    pub fn disconnect_all(&self) {
        self.lock().subscribers.clear();
    }

    fn lock(&self) -> MutexGuard<'_, World> {
        lock(&self.world)
    }
}

fn lock(world: &Mutex<World>) -> MutexGuard<'_, World> {
    match world.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

async fn play_realtime(world: Weak<Mutex<World>>) {
    loop {
        let Some(shared) = world.upgrade() else {
            return;
        };
        let delay = {
            let mut world = lock(&shared);
            world.drain_control();
            match world.queue.front() {
                Some(script) => script.delay(),
                None => IDLE_DELAY,
            }
        };
        drop(shared);
        tokio::time::sleep(delay).await;
        let Some(shared) = world.upgrade() else {
            return;
        };
        let mut world = lock(&shared);
        let Some(script) = world.queue.pop_front() else {
            continue;
        };
        world.emit(script);
    }
}

impl Transport for MockTransport {
    fn get(&self, path: &str) -> BoxFuture<'static, Result<Value, ApiError>> {
        let answer = self.lock().get(path);
        ready(answer).boxed()
    }

    fn post(&self, path: &str, body: Option<Value>) -> BoxFuture<'static, Result<Value, ApiError>> {
        let answer = self.lock().post(path, body);
        ready(answer).boxed()
    }

    fn fetch(&self, path: &str) -> BoxFuture<'static, Result<Vec<u8>, ApiError>> {
        let answer = self.lock().fetch(path);
        ready(answer).boxed()
    }

    fn fetch_public(&self, url: &PublicUrl) -> BoxFuture<'static, Result<Vec<u8>, ApiError>> {
        let answer = match self.lock().public.get(url.as_str()) {
            Some(bytes) => Ok(bytes.clone()),
            None => Err(ApiError::NotFound),
        };
        ready(answer).boxed()
    }

    fn send(&self, request: Request) -> BoxFuture<'static, Result<Value, ApiError>> {
        let answer = self.lock().send(request);
        ready(answer).boxed()
    }

    fn connect(&self, since: Option<Seq>) -> BoxFuture<'static, Result<Connection, ApiError>> {
        let connection = self.lock().connect(since);
        ready(Ok(connection)).boxed()
    }
}

const VOICE_LIMIT: usize = 20 * 1024 * 1024;
const NAME_LIMIT: usize = 64;
const VOICE_ATTACHMENTS: i64 = 100;
const VOICE_TRANSCRIPT: &str = "Что нового за сегодня?";

fn fresh_run() -> RunId {
    RunId(Uuid::new_v4().to_string())
}

fn answer_text() -> String {
    [
        "## Лисички со сливками",
        "",
        "1. Обжарить лук до прозрачности.",
        "2. Добавить лисички, 10 минут на среднем огне.",
        "3. Влить сливки и томить ещё 5 минут.",
        "",
        "```",
        "лисички 400 г",
        "сливки  200 мл",
        "```",
        "",
        "| Шаг | Минуты |",
        "|-----|--------|",
        "| лук | 3 |",
        "| грибы | 10 |",
        "| сливки | 5 |",
    ]
    .join("\n")
}

fn run_script(begin: Begin) -> Vec<Script> {
    let run = begin.id.clone();
    let tool = ToolUseId(format!("toolu_{}", &run.0[..8]));
    let mut summary = String::from("recipes.md:12: лисички со сливками\n");
    while summary.len() < 2048 {
        summary.push_str("notes/2026-09.md: грибной сезон, рынок по субботам\n");
    }
    let mut cut = 2048;
    while !summary.is_char_boundary(cut) {
        cut -= 1;
    }
    summary.truncate(cut);
    let answer = answer_text();
    let mut script = vec![
        Script::Started(begin),
        Script::Delta(run.clone(), "Посмотрю, ".into()),
        Script::Delta(run.clone(), "что есть в заметках.".into()),
        Script::Text(run.clone(), "Посмотрю, что есть в заметках.".into()),
        Script::ToolStart(
            run.clone(),
            ToolStarted {
                tool_use_id: tool.clone(),
                name: "Bash".into(),
                input: json!({
                    "command": "rg -i 'лисич' ~/notes \\\n  --glob '*.md' \\\n  --max-count 5",
                    "description": "Search the notes for chanterelles",
                }),
                parent_tool_use_id: None,
            },
        ),
        Script::ToolFinish(
            run.clone(),
            ToolFinished {
                tool_use_id: tool,
                is_error: false,
                error: None,
                summary,
            },
        ),
        Script::Task(
            run.clone(),
            TaskUpdate {
                task_id: format!("task-{}", &run.0[..8]),
                task_type: "local_agent".into(),
                state: "running".into(),
                description: Some("check the market's opening hours".into()),
                summary: None,
            },
        ),
    ];
    let mut piece = String::new();
    for word in answer.split_inclusive(' ') {
        piece.push_str(word);
        if piece.len() > 40 {
            script.push(Script::Delta(run.clone(), std::mem::take(&mut piece)));
        }
    }
    if !piece.is_empty() {
        script.push(Script::Delta(run.clone(), piece));
    }
    script.push(Script::Text(run.clone(), answer));
    script.push(Script::Finished(
        run,
        RunFinished {
            is_error: false,
            error: None,
            terminal_reason: "success".into(),
            usage: Some(Usage {
                input_tokens: 12,
                output_tokens: 412,
                cache_read_tokens: 321_004,
                cache_creation_tokens: 2950,
                num_turns: 2,
                duration_api_ms: 11_840,
            }),
            context_usage: Some(ContextUsage {
                total_tokens: 323_968,
                max_tokens: 1_000_000,
                percentage: 32.4,
                model: "claude-opus-5-5[1m]".into(),
            }),
        },
    ));
    script
}

impl World {
    fn new(scenario: Scenario) -> World {
        let base = datetime!(2026-10-03 09:00 UTC);
        let mut world = World {
            scenario,
            now: base + TimeDuration::hours(7),
            head: 1200,
            ring: VecDeque::new(),
            surfaces: seed_surfaces(),
            agents: seed_agents(),
            messages: Vec::new(),
            runs: Vec::new(),
            queue: VecDeque::new(),
            subscribers: Vec::new(),
            posted: HashMap::new(),
            media: HashMap::new(),
            public: HashMap::new(),
            cursors: HashMap::new(),
            groups: Vec::new(),
            my_name: DEFAULT_NAME.into(),
            my_description: String::new(),
            tasks: Vec::new(),
            task_runs: HashMap::new(),
            fires: Vec::new(),
            avatars: HashMap::from([
                (
                    AvatarOwner::Agent(AgentId(1)),
                    Avatar::new(AGENT_AVATAR.to_vec()),
                ),
                (AvatarOwner::Me, Avatar::new(MY_AVATAR.to_vec())),
            ]),
            next_message: 9000,
            next_input: 40,
        };
        world.seed_messages(base);
        world.seed_tasks(base);
        world.seed_live_run();
        world.seed_voice(base);
        world.read_everything();
        world
    }

    fn read_everything(&mut self) {
        for message in &self.messages {
            let newest = self
                .cursors
                .get(&message.surface_id)
                .is_none_or(|cursor| message.id > *cursor);
            if newest {
                self.cursors.insert(message.surface_id, message.id);
            }
        }
    }

    fn from_seed(seed: Seed, scenario: Scenario) -> World {
        let Seed {
            surfaces,
            agents,
            messages,
            runs,
            media: files,
            me,
            tasks,
        } = seed;
        let (my_name, my_description) = match me {
            Some(Me {
                name,
                description,
                avatar_url: _,
            }) => (name, description),
            None => (DEFAULT_NAME.into(), String::new()),
        };
        let mut media = HashMap::new();
        for SeedMedia { id, path } in files {
            media.insert(id, Media::File(path));
        }
        let mut next_message = 0;
        let mut now = datetime!(2026-10-03 00:00 UTC);
        for message in &messages {
            let MessageId(id) = message.id;
            next_message = next_message.max(id);
            if message.created_at > now {
                now = message.created_at;
            }
        }
        let mut mock_runs = Vec::new();
        for RunDetail { run, steps } in runs {
            let RunRow {
                id,
                agent_id,
                surface_id,
                origin,
                kind,
                status,
                terminal_reason,
                error: _,
                started_at,
                finished_at,
                usage: _,
                context: _,
            } = run;
            let Some(surface) = surface_id else {
                continue;
            };
            let finished_at = match (status, finished_at) {
                (RunStatus::Running, _) => None,
                (RunStatus::Ok, at) => Some(at.unwrap_or(started_at)),
                (RunStatus::Error, at) => Some(at.unwrap_or(started_at)),
                (RunStatus::Interrupted, at) => Some(at.unwrap_or(started_at)),
                (RunStatus::Unknown, at) => Some(at.unwrap_or(started_at)),
            };
            mock_runs.push(MockRun {
                id,
                agent: agent_id,
                surface,
                origin,
                kind,
                status,
                terminal_reason,
                started_at,
                finished_at,
                steps,
                segment: String::new(),
                last_seq: Seq(0),
            });
        }
        let mut world = World {
            scenario,
            now,
            head: 1,
            ring: VecDeque::new(),
            surfaces,
            agents,
            messages,
            runs: mock_runs,
            queue: VecDeque::new(),
            subscribers: Vec::new(),
            posted: HashMap::new(),
            media,
            public: HashMap::new(),
            cursors: HashMap::new(),
            groups: Vec::new(),
            my_name,
            my_description,
            tasks,
            task_runs: HashMap::new(),
            fires: Vec::new(),
            avatars: HashMap::new(),
            next_message,
            next_input: 1,
        };
        world.read_everything();
        world
    }

    fn seed_messages(&mut self, base: OffsetDateTime) {
        let plan = [
            (SurfaceId(1), AgentId(1), 30),
            (SurfaceId(2), AgentId(3), 18),
            (SurfaceId(3), AgentId(2), 12),
        ];
        let mut minute = 0;
        for (surface, lead, count) in plan {
            for index in 0..count {
                minute += 7;
                let at = base + TimeDuration::minutes(minute);
                let message = match index % 6 {
                    0 => self.seed_user(surface, at, Channel::Telegram, "Что нового?"),
                    1 => self.seed_answer(surface, lead, at),
                    2 => self.seed_user(surface, at, Channel::Desktop, "А подробнее?"),
                    3 => self.seed_answer(surface, lead, at),
                    4 => self.seed_post(surface, lead, at),
                    5 => self.seed_side(surface, lead, at),
                    other => unreachable!("index % 6 is {other}"),
                };
                self.messages.push(message);
            }
        }
    }

    fn seed_user(
        &mut self,
        surface: SurfaceId,
        at: OffsetDateTime,
        channel: Channel,
        text: &str,
    ) -> Message {
        Message {
            id: self.next_message(),
            surface_id: surface,
            kind: MessageKind::User,
            author: Author {
                kind: AuthorKind::User,
                agent_id: None,
            },
            addressed_agent_id: None,
            reply_to_message_id: None,
            text: text.into(),
            run_id: None,
            origin: "user".into(),
            channel: Some(channel),
            client_message_id: None,
            created_at: at,
            run_summary: None,
            attachments: Vec::new(),
        }
    }

    fn seed_answer(&mut self, surface: SurfaceId, agent: AgentId, at: OffsetDateTime) -> Message {
        let run = fresh_run();
        self.runs.push(MockRun {
            id: run.clone(),
            agent,
            surface,
            origin: "user".into(),
            kind: "user".into(),
            status: RunStatus::Ok,
            terminal_reason: Some("success".into()),
            started_at: at - TimeDuration::seconds(9),
            finished_at: Some(at),
            steps: vec![
                StepRow {
                    seq: 1,
                    kind: RowKind::Text,
                    tool_use_id: None,
                    name: None,
                    input: None,
                    output: Some("Проверю.".into()),
                    status: None,
                    started_at: at - TimeDuration::seconds(8),
                    finished_at: None,
                },
                StepRow {
                    seq: 2,
                    kind: RowKind::Text,
                    tool_use_id: None,
                    name: None,
                    input: None,
                    output: Some("Всё спокойно, новых событий нет.".into()),
                    status: None,
                    started_at: at - TimeDuration::seconds(2),
                    finished_at: None,
                },
            ],
            segment: String::new(),
            last_seq: Seq(0),
        });
        Message {
            id: self.next_message(),
            surface_id: surface,
            kind: MessageKind::Answer,
            author: Author {
                kind: AuthorKind::Agent,
                agent_id: Some(agent),
            },
            addressed_agent_id: None,
            reply_to_message_id: None,
            text: "Всё спокойно, новых событий нет.".into(),
            run_id: Some(run),
            origin: "user".into(),
            channel: None,
            client_message_id: None,
            created_at: at,
            run_summary: Some(RunSummary {
                status: RunStatus::Ok,
                step_count: 2,
                tool_count: 0,
                duration_ms: 9000,
            }),
            attachments: Vec::new(),
        }
    }

    fn seed_post(&mut self, surface: SurfaceId, agent: AgentId, at: OffsetDateTime) -> Message {
        Message {
            id: self.next_message(),
            surface_id: surface,
            kind: MessageKind::Post,
            author: Author {
                kind: AuthorKind::Agent,
                agent_id: Some(agent),
            },
            addressed_agent_id: None,
            reply_to_message_id: None,
            text: "**Сводка за утро**\n\n- новых релизов: 2\n- задач в очереди: 0".into(),
            run_id: None,
            origin: "scheduled".into(),
            channel: None,
            client_message_id: None,
            created_at: at,
            run_summary: None,
            attachments: Vec::new(),
        }
    }

    fn seed_side(&mut self, surface: SurfaceId, agent: AgentId, at: OffsetDateTime) -> Message {
        if surface == SurfaceId(1) {
            return Message {
                id: self.next_message(),
                surface_id: surface,
                kind: MessageKind::A2a,
                author: Author {
                    kind: AuthorKind::Agent,
                    agent_id: Some(agent),
                },
                addressed_agent_id: Some(AgentId(3)),
                reply_to_message_id: None,
                text: "@magnet_feed что вышло за неделю?".into(),
                run_id: None,
                origin: "a2a".into(),
                channel: None,
                client_message_id: None,
                created_at: at,
                run_summary: None,
                attachments: Vec::new(),
            };
        }
        Message {
            id: self.next_message(),
            surface_id: surface,
            kind: MessageKind::Notice,
            author: Author {
                kind: AuthorKind::System,
                agent_id: Some(agent),
            },
            addressed_agent_id: None,
            reply_to_message_id: None,
            text: "Агент перезапущен после обновления.".into(),
            run_id: None,
            origin: "system".into(),
            channel: None,
            client_message_id: None,
            created_at: at,
            run_summary: None,
            attachments: Vec::new(),
        }
    }

    fn seed_voice(&mut self, base: OffsetDateTime) {
        let id = AttachmentId(1);
        self.media.insert(id, Media::Embedded(TONE));
        let message = Message {
            id: self.next_message(),
            surface_id: SurfaceId(2),
            kind: MessageKind::User,
            author: Author {
                kind: AuthorKind::User,
                agent_id: None,
            },
            addressed_agent_id: None,
            reply_to_message_id: None,
            text: "[Voice message]\nЧто вышло за неделю из сериалов?".into(),
            run_id: None,
            origin: "user".into(),
            channel: Some(Channel::Telegram),
            client_message_id: None,
            created_at: base + TimeDuration::hours(6),
            run_summary: None,
            attachments: vec![Attachment {
                id,
                kind: AttachmentKind::Voice,
                mime: "audio/ogg".into(),
                size_bytes: u64::try_from(TONE.len()).unwrap_or(u64::MAX),
                duration_ms: Some(3006),
            }],
        };
        self.messages.push(message);
    }

    fn seed_live_run(&mut self) {
        let started_at = self.now - TimeDuration::seconds(40);
        self.head += 4;
        self.runs.push(MockRun {
            id: RunId("0b9d2c4e-5a61-4f7e-8c3d-1e2f3a4b5c6d".into()),
            agent: AgentId(3),
            surface: SurfaceId(2),
            origin: "scheduled".into(),
            kind: "scheduled".into(),
            status: RunStatus::Running,
            terminal_reason: None,
            started_at,
            finished_at: None,
            steps: vec![
                StepRow {
                    seq: 1,
                    kind: RowKind::Text,
                    tool_use_id: None,
                    name: None,
                    input: None,
                    output: Some("Проверяю новые релизы.".into()),
                    status: None,
                    started_at: started_at + TimeDuration::seconds(2),
                    finished_at: None,
                },
                StepRow {
                    seq: 2,
                    kind: RowKind::Tool,
                    tool_use_id: Some(ToolUseId("toolu_seed".into())),
                    name: Some("WebFetch".into()),
                    input: Some(json!({"url": "https://example.org/releases"})),
                    output: Some("3 new entries".into()),
                    status: Some("ok".into()),
                    started_at: started_at + TimeDuration::seconds(4),
                    finished_at: Some(started_at + TimeDuration::seconds(9)),
                },
                StepRow {
                    seq: 3,
                    kind: RowKind::Status,
                    tool_use_id: None,
                    name: Some("compacting".into()),
                    input: None,
                    output: Some("context 91%".into()),
                    status: None,
                    started_at: started_at + TimeDuration::seconds(20),
                    finished_at: None,
                },
            ],
            segment: "Нашёл три новых релиза, ".into(),
            last_seq: Seq(self.head),
        });
    }

    fn next_message(&mut self) -> MessageId {
        self.next_message += 1;
        MessageId(self.next_message)
    }

    fn next_input(&mut self) -> InputId {
        self.next_input += 1;
        InputId(self.next_input)
    }

    fn user_message(
        &mut self,
        surface: SurfaceId,
        text: &str,
        channel: Channel,
        client: Option<ClientMessageId>,
    ) -> Message {
        let message = Message {
            id: self.next_message(),
            surface_id: surface,
            kind: MessageKind::User,
            author: Author {
                kind: AuthorKind::User,
                agent_id: None,
            },
            addressed_agent_id: None,
            reply_to_message_id: None,
            text: text.into(),
            run_id: None,
            origin: "user".into(),
            channel: Some(channel),
            client_message_id: client,
            created_at: self.now,
            run_summary: None,
            attachments: Vec::new(),
        };
        self.messages.push(message.clone());
        message
    }

    fn live(&self, run: &RunId) -> Option<usize> {
        for (index, candidate) in self.runs.iter().enumerate() {
            if &candidate.id == run && candidate.finished_at.is_none() {
                return Some(index);
            }
        }
        None
    }

    fn run_index(&self, run: &RunId) -> Option<usize> {
        for (index, candidate) in self.runs.iter().enumerate() {
            if &candidate.id == run {
                return Some(index);
            }
        }
        None
    }

    fn floor(&self) -> i64 {
        match self.ring.front() {
            Some((seq, _)) => *seq,
            None => self.head,
        }
    }

    fn unavailable(&mut self) -> bool {
        if !self.scenario.unavailable_once {
            return false;
        }
        self.scenario.unavailable_once = false;
        true
    }

    fn get(&mut self, path: &str) -> Result<Value, ApiError> {
        if self.unavailable() {
            return Err(ApiError::Unavailable);
        }
        let (route, query) = match path.split_once('?') {
            Some((route, query)) => (route, query),
            None => (path, ""),
        };
        let segments = segments(route);
        match segments.as_slice() {
            ["surfaces"] => to_json(&self.surfaces_view()),
            ["groups"] => to_json(&self.groups_view()),
            ["agents"] => to_json(&self.agents_view()),
            ["me"] => to_json(&self.me_view()),
            ["tasks"] => to_json(&self.tasks_view(query)),
            ["tasks", id, "runs"] => {
                let id = TaskId((*id).to_string());
                if self.task_index(&id).is_none() {
                    return Err(ApiError::NotFound);
                }
                let mut runs = self.task_runs.get(&id).cloned().unwrap_or_default();
                runs.reverse();
                to_json(&runs)
            }
            ["surfaces", id, "messages"] => {
                let surface = parse_surface(id)?;
                self.page(surface, query)
            }
            ["runs", id] => {
                let Some(index) = self.run_index(&RunId((*id).to_string())) else {
                    return Err(ApiError::NotFound);
                };
                to_json(&self.detail(index))
            }
            [..] => Err(ApiError::NotFound),
        }
    }

    fn fetch(&mut self, path: &str) -> Result<Vec<u8>, ApiError> {
        if self.unavailable() {
            return Err(ApiError::Unavailable);
        }
        let (route, query) = match path.split_once('?') {
            Some((route, query)) => (route, query),
            None => (path, ""),
        };
        let segments = segments(route);
        let id = match segments.as_slice() {
            ["attachments", id] => id,
            ["agents", id, "avatar"] => {
                let owner = AvatarOwner::Agent(parse_agent(id)?);
                return self.avatar_bytes(owner, query);
            }
            ["me", "avatar"] => return self.avatar_bytes(AvatarOwner::Me, query),
            [..] => return Err(ApiError::NotFound),
        };
        let Ok(id) = id.parse::<i64>() else {
            return Err(ApiError::NotFound);
        };
        let Some(media) = self.media.get(&AttachmentId(id)) else {
            return Err(ApiError::NotFound);
        };
        match media {
            Media::Embedded(bytes) => Ok(bytes.to_vec()),
            Media::Owned(bytes) => Ok(bytes.clone()),
            Media::File(path) => std::fs::read(path)
                .map_err(|error| ApiError::Transport(format!("{}: {error}", path.display()))),
        }
    }

    fn post(&mut self, path: &str, body: Option<Value>) -> Result<Value, ApiError> {
        if self.unavailable() {
            return Err(ApiError::Unavailable);
        }
        let segments = segments(path);
        match segments.as_slice() {
            ["surfaces", id, "messages"] => {
                let surface = parse_surface(id)?;
                let Some(body) = body else {
                    return Err(ApiError::Invalid("a post needs a body".into()));
                };
                let post: Post = serde_json::from_value(body)
                    .map_err(|error| ApiError::Invalid(error.to_string()))?;
                to_json(&self.accept(surface, post, Vec::new())?)
            }
            ["groups"] => self.create_group(body),
            ["surfaces", id, "read"] => {
                let surface = parse_surface(id)?;
                self.mark_read(surface, body)
            }
            ["runs", id, "interrupt"] => {
                self.interrupt(&RunId((*id).to_string()))?;
                Ok(Value::Null)
            }
            ["tasks", id, "pause"] => self.transition(id, TaskStatus::Active, TaskStatus::Paused),
            ["tasks", id, "resume"] => self.transition(id, TaskStatus::Paused, TaskStatus::Active),
            [..] => Err(ApiError::NotFound),
        }
    }

    fn voice(
        &mut self,
        surface: SurfaceId,
        query: &str,
        method: Method,
        body: Body,
    ) -> Result<Value, ApiError> {
        let (
            Method::Post,
            Body::Voice {
                kind,
                bytes,
                client_message_id,
            },
        ) = (method, body)
        else {
            return Err(ApiError::NotFound);
        };
        if bytes.is_empty() {
            return Err(ApiError::Invalid("the recording is empty".into()));
        }
        if bytes.len() > VOICE_LIMIT {
            return Err(ApiError::Refused {
                code: "too_large".into(),
                message: "the recording must be at most 20971520 bytes".into(),
            });
        }
        let mut addressed_agent_id = None;
        for pair in query.split('&') {
            let Some(("addressed_agent_id", agent)) = pair.split_once('=') else {
                continue;
            };
            addressed_agent_id = Some(parse_agent(agent)?);
        }
        let post = Post {
            text: format!("[Voice message]\n{VOICE_TRANSCRIPT}"),
            addressed_agent_id,
            client_message_id,
        };
        if self.posted.contains_key(&post.client_message_id) {
            return to_json(&self.accept(surface, post, Vec::new())?);
        }
        let id = AttachmentId(VOICE_ATTACHMENTS + i64::try_from(self.media.len()).unwrap_or(0));
        let attachment = Attachment {
            id,
            kind: AttachmentKind::Voice,
            mime: kind.mime().into(),
            size_bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            duration_ms: None,
        };
        self.media.insert(id, Media::Owned(bytes));
        to_json(&self.accept(surface, post, vec![attachment])?)
    }

    fn accept(
        &mut self,
        surface: SurfaceId,
        post: Post,
        attachments: Vec<Attachment>,
    ) -> Result<Posted, ApiError> {
        if let Some((first, posted)) = self.posted.get(&post.client_message_id) {
            if *first != surface {
                return Err(ApiError::Conflict);
            }
            return Ok(*posted);
        }
        let Some(found) = self.surface(surface) else {
            return Err(ApiError::NotFound);
        };
        let mut wired = None;
        if let Some(addressed) = post.addressed_agent_id {
            for wiring in &found.agents {
                if wiring.agent_id == addressed {
                    wired = Some(addressed);
                }
            }
        }
        let agent = match wired {
            Some(addressed) => addressed,
            None => {
                let Some(lead) = found.lead_agent_id else {
                    return Err(ApiError::Invalid("the surface has no lead".into()));
                };
                lead
            }
        };
        let mut message = self.user_message(
            surface,
            &post.text,
            Channel::Desktop,
            Some(post.client_message_id.clone()),
        );
        message.addressed_agent_id = post.addressed_agent_id;
        message.attachments = attachments.clone();
        if let Some(stored) = self.messages.last_mut() {
            stored.addressed_agent_id = post.addressed_agent_id;
            stored.attachments = attachments;
        }
        let input = self.next_input();
        let posted = Posted {
            message_id: message.id,
            input_id: Some(input),
            agent_id: Some(agent),
        };
        self.posted
            .insert(post.client_message_id, (surface, posted));
        self.queue.push_back(Script::Accepted(InputAccepted {
            input_id: input,
            surface_id: surface,
            agent_id: agent,
        }));
        self.queue.push_back(Script::Created(message));
        let run = run_script(Begin {
            id: fresh_run(),
            agent,
            surface,
            inputs: vec![input],
            origin: "user".into(),
        });
        self.queue.extend(run);
        Ok(posted)
    }

    fn interrupt(&mut self, run: &RunId) -> Result<(), ApiError> {
        if self.live(run).is_none() {
            return Err(ApiError::Conflict);
        }
        let mut kept = VecDeque::new();
        for script in self.queue.drain(..) {
            if script.run() != Some(run) {
                kept.push_back(script);
            }
        }
        self.queue = kept;
        self.queue.push_front(Script::Finished(
            run.clone(),
            RunFinished {
                is_error: false,
                error: None,
                terminal_reason: "interrupted".into(),
                usage: None,
                context_usage: None,
            },
        ));
        Ok(())
    }

    fn surface(&self, surface: SurfaceId) -> Option<&Surface> {
        let mut found = None;
        for candidate in &self.surfaces {
            if candidate.id == surface {
                found = Some(candidate);
                break;
            }
        }
        found
    }

    fn surfaces_view(&self) -> Vec<Surface> {
        let mut surfaces = Vec::new();
        for surface in &self.surfaces {
            let mut surface = surface.clone();
            for message in &self.messages {
                if message.surface_id == surface.id {
                    surface.last_message_at = Some(message.created_at);
                }
            }
            for run in &self.runs {
                if run.surface == surface.id && run.finished_at.is_none() {
                    surface.live_run = Some(SurfaceRun {
                        run_id: run.id.clone(),
                        agent_id: run.agent,
                    });
                }
            }
            surface.topic_name = surface.name.clone();
            if let Some(display) = &surface.display_name {
                surface.name = display.clone();
            }
            let cursor = self.cursors.get(&surface.id).copied();
            surface.last_read_message_id = cursor;
            surface.unread = self.unread(surface.id, cursor);
            surfaces.push(surface);
        }
        surfaces
    }

    fn groups_view(&self) -> Vec<Group> {
        let mut groups = self.groups.clone();
        groups.sort_by_key(|group| (group.sort_order, group.id));
        groups
    }

    fn surface_row(&self, surface: SurfaceId) -> Option<Surface> {
        let mut found = None;
        for candidate in self.surfaces_view() {
            if candidate.id == surface {
                found = Some(candidate);
            }
        }
        found
    }

    fn announce_surface(&mut self, surface: SurfaceId) {
        let Some(view) = self.surface_row(surface) else {
            return;
        };
        self.persist(
            "surface.updated",
            Some(surface),
            None,
            &json!({ "surface": view }),
        );
    }

    fn announce_groups(&mut self) {
        let groups = self.groups_view();
        self.persist("groups.changed", None, None, &json!({ "groups": groups }));
    }

    fn group_index(&self, group: GroupId) -> Option<usize> {
        let mut found = None;
        for (index, candidate) in self.groups.iter().enumerate() {
            if candidate.id == group {
                found = Some(index);
            }
        }
        found
    }

    fn create_group(&mut self, body: Option<Value>) -> Result<Value, ApiError> {
        let Some(body) = body else {
            return Err(ApiError::Invalid("a group needs a name".into()));
        };
        let name = body.get("name").and_then(Value::as_str).unwrap_or_default();
        let name = name.trim();
        if name.is_empty() || name.chars().count() > NAME_LIMIT {
            return Err(ApiError::Invalid(
                "a group name is 1 to 64 characters".into(),
            ));
        }
        let mut next = 1;
        let mut last = -1;
        for group in &self.groups {
            let GroupId(id) = group.id;
            next = next.max(id + 1);
            last = last.max(group.sort_order);
        }
        let group = Group {
            id: GroupId(next),
            name: name.to_string(),
            emoji: body
                .get("emoji")
                .and_then(Value::as_str)
                .map(str::to_string),
            sort_order: last + 1,
        };
        self.groups.push(group.clone());
        self.announce_groups();
        to_json(&group)
    }

    fn change_group(&mut self, id: &str, method: Method, body: Body) -> Result<Value, ApiError> {
        let Ok(raw) = id.parse::<i64>() else {
            return Err(ApiError::NotFound);
        };
        let Some(index) = self.group_index(GroupId(raw)) else {
            return Err(ApiError::NotFound);
        };
        match method {
            Method::Patch => {
                let Body::Json(json) = body else {
                    return Err(ApiError::Invalid("a JSON body is required".into()));
                };
                if let Some(name) = json.get("name").and_then(Value::as_str) {
                    let name = name.trim();
                    if name.is_empty() {
                        return Err(ApiError::Invalid("a group needs a name".into()));
                    }
                    self.groups[index].name = name.to_string();
                }
                if let Some(emoji) = json.get("emoji") {
                    self.groups[index].emoji = emoji.as_str().map(str::to_string);
                }
                if let Some(order) = json.get("sort_order").and_then(Value::as_i64) {
                    self.groups[index].sort_order = order;
                }
                let group = self.groups[index].clone();
                self.announce_groups();
                to_json(&group)
            }
            Method::Delete => {
                let gone = self.groups.remove(index).id;
                let mut moved = Vec::new();
                for surface in &mut self.surfaces {
                    if surface.group_id == Some(gone) {
                        surface.group_id = None;
                        moved.push(surface.id);
                    }
                }
                self.announce_groups();
                for surface in moved {
                    self.announce_surface(surface);
                }
                Ok(Value::Null)
            }
            Method::Post => Err(ApiError::NotFound),
            Method::Put => Err(ApiError::NotFound),
        }
    }

    fn patch_surface(
        &mut self,
        surface: SurfaceId,
        method: Method,
        body: Body,
    ) -> Result<Value, ApiError> {
        let index = self.surface_index(surface)?;
        let (Method::Patch, Body::Json(json)) = (method, body) else {
            return Err(ApiError::NotFound);
        };
        if let Some(group) = json.get("group_id") {
            let group = match group.as_i64() {
                Some(raw) => {
                    if self.group_index(GroupId(raw)).is_none() {
                        return Err(ApiError::NotFound);
                    }
                    Some(GroupId(raw))
                }
                None => None,
            };
            self.surfaces[index].group_id = group;
        }
        if let Some(name) = json.get("display_name") {
            let name = name.as_str().map(str::trim).filter(|name| !name.is_empty());
            if name.is_some_and(|name| name.chars().count() > NAME_LIMIT) {
                return Err(ApiError::Invalid(
                    "a display name is at most 64 characters".into(),
                ));
            }
            self.surfaces[index].display_name = name.map(str::to_string);
        }
        if let Some(archived) = json.get("archived").and_then(Value::as_bool) {
            self.surfaces[index].archived_at = match archived {
                true => Some(self.now),
                false => None,
            };
        }
        self.announce_surface(surface);
        self.surface_view(surface)
    }

    fn reorder(&mut self, method: Method, body: Body) -> Result<Value, ApiError> {
        let (Method::Put, Body::Json(json)) = (method, body) else {
            return Err(ApiError::NotFound);
        };
        let placements: Vec<Placement> =
            serde_json::from_value(json).map_err(|error| ApiError::Invalid(error.to_string()))?;
        for Placement {
            id,
            group_id,
            sort_order: _,
        } in &placements
        {
            self.surface_index(*id)?;
            if let Some(group) = group_id
                && self.group_index(*group).is_none()
            {
                return Err(ApiError::NotFound);
            }
        }
        for Placement {
            id,
            group_id,
            sort_order,
        } in placements
        {
            if let Ok(index) = self.surface_index(id) {
                self.surfaces[index].group_id = group_id;
                self.surfaces[index].sort_order = sort_order;
            }
            self.announce_surface(id);
        }
        to_json(&self.surfaces_view())
    }

    fn unread(&self, surface: SurfaceId, cursor: Option<MessageId>) -> u32 {
        let mut unread = 0;
        for message in &self.messages {
            if message.surface_id != surface {
                continue;
            }
            if cursor.is_some_and(|cursor| message.id <= cursor) {
                continue;
            }
            let counted = match message.author.kind {
                AuthorKind::User => false,
                AuthorKind::Agent => true,
                AuthorKind::System => true,
                AuthorKind::Unknown => true,
            };
            if counted {
                unread += 1;
            }
        }
        unread
    }

    fn mark_read(&mut self, surface: SurfaceId, body: Option<Value>) -> Result<Value, ApiError> {
        if self.surface(surface).is_none() {
            return Err(ApiError::NotFound);
        }
        let Some(MessageId(wanted)) = body
            .as_ref()
            .and_then(|body| body.get("message_id"))
            .and_then(Value::as_i64)
            .filter(|id| *id > 0)
            .map(MessageId)
        else {
            return Err(ApiError::Invalid(
                "message_id must be a positive integer".into(),
            ));
        };
        let mut found = false;
        for message in &self.messages {
            if message.id == MessageId(wanted) && message.surface_id == surface {
                found = true;
            }
        }
        if !found {
            return Err(ApiError::Invalid(
                "the message is not on this surface".into(),
            ));
        }
        let cursor = match self.cursors.get(&surface) {
            Some(MessageId(old)) => MessageId((*old).max(wanted)),
            None => MessageId(wanted),
        };
        self.cursors.insert(surface, cursor);
        let answer = ReadAnswer {
            last_read_message_id: cursor,
            unread: self.unread(surface, Some(cursor)),
        };
        self.persist("surface.read", Some(surface), None, &json!(answer));
        to_json(&answer)
    }

    fn task_index(&self, id: &TaskId) -> Option<usize> {
        let mut found = None;
        for (index, task) in self.tasks.iter().enumerate() {
            if task.id == *id {
                found = Some(index);
            }
        }
        found
    }

    fn tasks_view(&self, query: &str) -> Vec<Task> {
        let all = query.split('&').any(|pair| pair == "status=all");
        let mut tasks = Vec::new();
        for task in &self.tasks {
            let listed = match task.status {
                TaskStatus::Active => true,
                TaskStatus::Paused => true,
                TaskStatus::Completed => all,
                TaskStatus::Cancelled => all,
                TaskStatus::Unknown => all,
            };
            if listed {
                tasks.push(task.clone());
            }
        }
        tasks
    }

    fn transition(
        &mut self,
        id: &str,
        from: TaskStatus,
        to: TaskStatus,
    ) -> Result<Value, ApiError> {
        let id = TaskId(id.to_string());
        let Some(index) = self.task_index(&id) else {
            return Err(ApiError::NotFound);
        };
        if self.tasks[index].status != from {
            return Err(ApiError::Conflict);
        }
        self.tasks[index].status = to;
        to_json(&self.tasks[index])
    }

    fn fire(&mut self, id: &TaskId, outcome: Outcome) {
        let Some(index) = self.task_index(id) else {
            return;
        };
        self.now += TimeDuration::seconds(1);
        let at = self.now;
        let task = &mut self.tasks[index];
        task.last_run_at = Some(at);
        task.last_outcome = Some(outcome);
        let surface = task.surface_id;
        self.task_runs.entry(id.clone()).or_default().push(TaskRun {
            at,
            outcome,
            duration_ms: 1200,
            error: None,
        });
        let mark = FireMark {
            task_id: id.clone(),
            at: Some(at),
            outcome,
            run_id: None,
            message_id: None,
            error: None,
        };
        if let Some(surface) = surface {
            self.fires.push((surface, mark.clone()));
        }
        self.persist(
            "task.fired",
            surface,
            None,
            &json!({"task_id": mark.task_id, "outcome": mark.outcome}),
        );
    }

    fn seed_tasks(&mut self, base: OffsetDateTime) {
        let task =
            |id: &str, agent: i64, surface: i64, prompt: &str, kind: ScheduleKind, value: &str| {
                Task {
                    id: TaskId(id.to_string()),
                    agent_id: Some(AgentId(agent)),
                    surface_id: Some(SurfaceId(surface)),
                    prompt: prompt.to_string(),
                    schedule: Schedule {
                        kind,
                        value: value.to_string(),
                    },
                    recurring: false,
                    condition: None,
                    status: TaskStatus::Active,
                    next_run_at: None,
                    last_run_at: None,
                    last_outcome: None,
                    active_from: None,
                    active_until: None,
                    created_at: Some(base - TimeDuration::days(12)),
                }
            };
        let mut digest = task(
            "task-weekly-releases",
            3,
            2,
            "Check the trackers for new releases and post a short digest",
            ScheduleKind::Cron,
            "0 9 * * 1",
        );
        digest.next_run_at = Some(base + TimeDuration::days(2));
        let mut poll = task(
            "task-download-done",
            3,
            2,
            "Tell me when Touch of Evil finishes downloading",
            ScheduleKind::PollUntil,
            "15m",
        );
        poll.condition = Some("check-torrent.sh touch-of-evil".into());
        poll.next_run_at = Some(base + TimeDuration::hours(8));
        let mut lights = task(
            "task-evening-lights",
            2,
            3,
            "Dim the living room lights at sunset",
            ScheduleKind::Interval,
            "24h",
        );
        lights.status = TaskStatus::Paused;
        let mut jobs = task(
            "task-mac-jobs",
            1,
            1,
            "Report when a long job on the Mac finishes",
            ScheduleKind::Event,
            "tuclaw.jobs.done.>",
        );
        jobs.recurring = true;
        self.tasks = vec![digest, poll, lights, jobs];
        let fires = [
            ("task-download-done", 2, 3, Outcome::Skipped),
            ("task-download-done", 2, 3, Outcome::Skipped),
            ("task-download-done", 2, 3, Outcome::Skipped),
            ("task-download-done", 2, 4, Outcome::Ran),
            ("task-mac-jobs", 1, 3, Outcome::Ran),
            ("task-mac-jobs", 1, 4, Outcome::Silent),
        ];
        let mut minute = 0;
        for (id, surface, hour, outcome) in fires {
            minute += 15;
            let at = base + TimeDuration::hours(hour) + TimeDuration::minutes(minute % 60);
            let id = TaskId(id.to_string());
            self.task_runs.entry(id.clone()).or_default().push(TaskRun {
                at,
                outcome,
                duration_ms: 900,
                error: None,
            });
            let mut agent = None;
            if let Some(index) = self.task_index(&id) {
                self.tasks[index].last_run_at = Some(at);
                self.tasks[index].last_outcome = Some(outcome);
                agent = self.tasks[index].agent_id;
            }
            let message_id = match outcome {
                Outcome::Ran => self.answer_after(SurfaceId(surface), agent, at),
                Outcome::Silent => None,
                Outcome::Skipped => None,
                Outcome::Failed => None,
                Outcome::Unknown => None,
            };
            self.fires.push((
                SurfaceId(surface),
                FireMark {
                    task_id: id,
                    at: Some(at),
                    outcome,
                    run_id: None,
                    message_id,
                    error: None,
                },
            ));
        }
    }

    fn answer_after(
        &self,
        surface: SurfaceId,
        agent: Option<AgentId>,
        at: OffsetDateTime,
    ) -> Option<MessageId> {
        let mut found = None;
        for message in &self.messages {
            if message.surface_id != surface || message.created_at < at {
                continue;
            }
            if agent.is_some() && message.author.agent_id == agent {
                found = Some(message.id);
                break;
            }
        }
        found
    }

    fn me_view(&self) -> Me {
        Me {
            name: self.my_name.clone(),
            description: self.my_description.clone(),
            avatar_url: self
                .avatars
                .get(&AvatarOwner::Me)
                .map(|avatar| avatar.url(AvatarOwner::Me)),
        }
    }

    fn avatar_bytes(&self, owner: AvatarOwner, query: &str) -> Result<Vec<u8>, ApiError> {
        let Some(avatar) = self.avatars.get(&owner) else {
            return Err(ApiError::NotFound);
        };
        for pair in query.split('&') {
            let Some(("v", version)) = pair.split_once('=') else {
                continue;
            };
            if version != avatar.version {
                return Err(ApiError::NotFound);
            }
        }
        Ok(avatar.bytes.clone())
    }

    fn send(&mut self, request: Request) -> Result<Value, ApiError> {
        if self.unavailable() {
            return Err(ApiError::Unavailable);
        }
        let Request { method, path, body } = request;
        let (route, query) = match path.split_once('?') {
            Some((route, query)) => (route, query),
            None => (path.as_str(), ""),
        };
        let segments = segments(route);
        let owner = match segments.as_slice() {
            ["surfaces", id, "voice"] => {
                let surface = parse_surface(id)?;
                return self.voice(surface, query, method, body);
            }
            ["agents", id, "avatar"] => {
                let agent = parse_agent(id)?;
                let mut known = false;
                for candidate in &self.agents {
                    if candidate.id == agent {
                        known = true;
                        break;
                    }
                }
                if !known {
                    return Err(ApiError::NotFound);
                }
                AvatarOwner::Agent(agent)
            }
            ["me", "avatar"] => AvatarOwner::Me,
            ["me"] => return self.rename(method, body),
            ["tasks", id] => {
                let (Method::Delete, Body::Empty) = (method, body) else {
                    return Err(ApiError::NotFound);
                };
                let id = TaskId((*id).to_string());
                let Some(index) = self.task_index(&id) else {
                    return Err(ApiError::NotFound);
                };
                self.tasks[index].status = TaskStatus::Cancelled;
                return Ok(Value::Null);
            }
            ["agents", id] => return self.patch_agent(parse_agent(id)?, method, body),
            ["surfaces", "order"] => return self.reorder(method, body),
            ["surfaces", id] => return self.patch_surface(parse_surface(id)?, method, body),
            ["groups", id] => return self.change_group(id, method, body),
            ["surfaces", surface, "agents", agent] => {
                let surface = parse_surface(surface)?;
                let agent = parse_agent(agent)?;
                return match method {
                    Method::Put => match body {
                        Body::Json(json) => self.wire(surface, agent, &json),
                        Body::Empty => Err(ApiError::Invalid("a body is required".into())),
                        Body::Image { kind: _, bytes: _ }
                        | Body::Voice {
                            kind: _,
                            bytes: _,
                            client_message_id: _,
                        } => Err(ApiError::Invalid("a JSON body is required".into())),
                    },
                    Method::Delete => self.unwire(surface, agent),
                    Method::Patch | Method::Post => Err(ApiError::NotFound),
                };
            }
            [..] => return Err(ApiError::NotFound),
        };
        match method {
            Method::Put => match body {
                Body::Image { kind: _, bytes } => self.store_avatar(owner, bytes),
                Body::Empty
                | Body::Json(_)
                | Body::Voice {
                    kind: _,
                    bytes: _,
                    client_message_id: _,
                } => Err(ApiError::Invalid("an image body is required".into())),
            },
            Method::Delete => match body {
                Body::Empty => {
                    self.avatars.remove(&owner);
                    Ok(Value::Null)
                }
                Body::Json(_)
                | Body::Image { kind: _, bytes: _ }
                | Body::Voice {
                    kind: _,
                    bytes: _,
                    client_message_id: _,
                } => Err(ApiError::Invalid("no body expected".into())),
            },
            Method::Patch | Method::Post => Err(ApiError::NotFound),
        }
    }

    fn agent_index(&self, agent: AgentId) -> Result<usize, ApiError> {
        let mut found = None;
        for (index, candidate) in self.agents.iter().enumerate() {
            if candidate.id == agent {
                found = Some(index);
                break;
            }
        }
        found.ok_or(ApiError::NotFound)
    }

    fn surface_index(&self, surface: SurfaceId) -> Result<usize, ApiError> {
        let mut found = None;
        for (index, candidate) in self.surfaces.iter().enumerate() {
            if candidate.id == surface {
                found = Some(index);
                break;
            }
        }
        found.ok_or(ApiError::NotFound)
    }

    fn patch_agent(
        &mut self,
        agent: AgentId,
        method: Method,
        body: Body,
    ) -> Result<Value, ApiError> {
        let (Method::Patch, Body::Json(json)) = (method, body) else {
            return Err(ApiError::NotFound);
        };
        self.update_agent(agent, &json)
    }

    fn update_agent(&mut self, agent: AgentId, json: &Value) -> Result<Value, ApiError> {
        let index = self.agent_index(agent)?;
        let Value::Object(fields) = json else {
            return Err(ApiError::Invalid("an object is expected".into()));
        };
        let description = match fields.get("description") {
            None => None,
            Some(Value::String(text)) if text.chars().count() > DESCRIPTION_LIMIT => {
                return Err(ApiError::Invalid(format!(
                    "description is longer than {DESCRIPTION_LIMIT} characters"
                )));
            }
            Some(Value::String(text)) => Some(text.clone()),
            Some(_) => return Err(ApiError::Invalid("description must be a string".into())),
        };
        let model = match fields.get("model") {
            None => None,
            Some(Value::Null) => Some(DEFAULT_MODEL.to_string()),
            Some(Value::String(spec)) if spec.trim().is_empty() => Some(DEFAULT_MODEL.to_string()),
            Some(Value::String(spec)) if spec.chars().any(char::is_whitespace) => {
                return Err(ApiError::Invalid("model must not contain spaces".into()));
            }
            Some(Value::String(spec)) => Some(spec.clone()),
            Some(_) => return Err(ApiError::Invalid("model must be a string or null".into())),
        };
        let stored = &mut self.agents[index];
        if let Some(description) = description {
            stored.description = description;
        }
        if let Some(model) = model {
            stored.model = model;
        }
        let mut view = None;
        for candidate in self.agents_view() {
            if candidate.id == agent {
                view = Some(candidate);
                break;
            }
        }
        to_json(&view)
    }

    fn surface_view(&self, surface: SurfaceId) -> Result<Value, ApiError> {
        for candidate in self.surfaces_view() {
            if candidate.id == surface {
                return to_json(&candidate);
            }
        }
        Err(ApiError::NotFound)
    }

    fn wire(
        &mut self,
        surface: SurfaceId,
        agent: AgentId,
        json: &Value,
    ) -> Result<Value, ApiError> {
        let index = self.surface_index(surface)?;
        self.agent_index(agent)?;
        let Ok(WiringChange { role, listens }) =
            serde_json::from_value::<WiringChange>(json.clone())
        else {
            return Err(ApiError::Invalid("role and listens are required".into()));
        };
        let target = &mut self.surfaces[index];
        match role {
            Role::Lead => {
                for wiring in &mut target.agents {
                    if wiring.agent_id != agent && wiring.role == Role::Lead {
                        wiring.role = Role::Mention;
                    }
                }
                target.lead_agent_id = Some(agent);
            }
            Role::Mention => {
                if target.lead_agent_id == Some(agent) {
                    return Err(ApiError::Conflict);
                }
            }
            Role::Unknown => return Err(ApiError::Invalid("role must be lead or mention".into())),
        }
        let mut updated = false;
        for wiring in &mut target.agents {
            if wiring.agent_id == agent {
                wiring.role = role;
                wiring.listens = listens;
                updated = true;
            }
        }
        if !updated {
            target.agents.push(Wiring {
                agent_id: agent,
                role,
                listens,
            });
        }
        self.surface_view(surface)
    }

    fn unwire(&mut self, surface: SurfaceId, agent: AgentId) -> Result<Value, ApiError> {
        let index = self.surface_index(surface)?;
        self.agent_index(agent)?;
        let target = &mut self.surfaces[index];
        if target.lead_agent_id == Some(agent) {
            return Err(ApiError::Conflict);
        }
        let mut kept = Vec::new();
        for wiring in std::mem::take(&mut target.agents) {
            if wiring.agent_id != agent {
                kept.push(wiring);
            }
        }
        target.agents = kept;
        Ok(Value::Null)
    }

    fn store_avatar(&mut self, owner: AvatarOwner, bytes: Vec<u8>) -> Result<Value, ApiError> {
        if bytes.len() > AVATAR_LIMIT {
            return Err(ApiError::Invalid("the image is larger than 2 MiB".into()));
        }
        let Some(_kind) = ImageKind::sniff(&bytes) else {
            return Err(ApiError::Invalid(
                "the body is not a png, jpeg or webp image".into(),
            ));
        };
        let avatar = Avatar::new(bytes);
        let avatar_url = avatar.url(owner);
        self.avatars.insert(owner, avatar);
        to_json(&AvatarSet { avatar_url })
    }

    fn rename(&mut self, method: Method, body: Body) -> Result<Value, ApiError> {
        let (Method::Patch, Body::Json(json)) = (method, body) else {
            return Err(ApiError::NotFound);
        };
        let Ok(MePatch { name, description }) = serde_json::from_value::<MePatch>(json) else {
            return Err(ApiError::Invalid(
                "name and description must be strings".into(),
            ));
        };
        let name = match name {
            Some(name) if name.trim().is_empty() => {
                return Err(ApiError::Invalid("name must not be empty".into()));
            }
            Some(name) => Some(name.trim().to_string()),
            None => None,
        };
        if let Some(description) = &description
            && description.chars().count() > PROFILE_LIMIT
        {
            return Err(ApiError::Invalid(format!(
                "description is longer than {PROFILE_LIMIT} characters"
            )));
        }
        if let Some(name) = name {
            self.my_name = name;
        }
        if let Some(description) = description {
            self.my_description = description;
        }
        to_json(&self.me_view())
    }

    fn agents_view(&self) -> Vec<Agent> {
        let mut agents = Vec::new();
        for agent in &self.agents {
            let mut agent = agent.clone();
            let owner = AvatarOwner::Agent(agent.id);
            if let Some(avatar) = self.avatars.get(&owner) {
                agent.avatar_url = Some(avatar.url(owner));
            }
            for run in &self.runs {
                if run.agent == agent.id && run.finished_at.is_none() {
                    agent.state = AgentState::Running;
                    agent.live_run = Some(AgentRun {
                        run_id: run.id.clone(),
                        surface_id: run.surface,
                    });
                }
            }
            agents.push(agent);
        }
        agents
    }

    fn page(&self, surface: SurfaceId, query: &str) -> Result<Value, ApiError> {
        let mut limit = 50;
        let mut before = None;
        for pair in query.split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            match key {
                "limit" => {
                    limit = value
                        .parse::<usize>()
                        .map_err(|error| ApiError::Invalid(error.to_string()))?
                        .clamp(1, 200)
                }
                "before" => {
                    before = Some(
                        value
                            .parse::<i64>()
                            .map_err(|error| ApiError::Invalid(error.to_string()))?,
                    )
                }
                other => return Err(ApiError::Invalid(format!("unknown parameter {other}"))),
            }
        }
        if self.surface(surface).is_none() {
            return Err(ApiError::NotFound);
        }
        let mut matching = Vec::new();
        for message in &self.messages {
            if message.surface_id != surface {
                continue;
            }
            if let Some(before) = before
                && message.id.0 >= before
            {
                continue;
            }
            matching.push(message.clone());
        }
        let has_more = matching.len() > limit;
        let start = matching.len().saturating_sub(limit);
        let messages = matching.split_off(start);
        let from = messages.first().map(|message| message.created_at);
        let until = match before {
            Some(_) => messages.last().map(|message| message.created_at),
            None => None,
        };
        let mut automations = Vec::new();
        for (on, mark) in &self.fires {
            if *on != surface {
                continue;
            }
            let Some(at) = mark.at else {
                continue;
            };
            if from.is_some_and(|from| at < from) && has_more {
                continue;
            }
            if until.is_some_and(|until| at > until) {
                continue;
            }
            automations.push(mark.clone());
        }
        to_json(&MessagesPage {
            messages,
            has_more,
            automations,
        })
    }

    fn detail(&self, index: usize) -> RunDetail {
        let run = &self.runs[index];
        RunDetail {
            run: RunRow {
                id: run.id.clone(),
                agent_id: run.agent,
                surface_id: Some(run.surface),
                origin: run.origin.clone(),
                kind: run.kind.clone(),
                status: run.status,
                terminal_reason: run.terminal_reason.clone(),
                error: None,
                started_at: run.started_at,
                finished_at: run.finished_at,
                usage: None,
                context: None,
            },
            steps: run.steps.clone(),
        }
    }

    fn connect(&mut self, since: Option<Seq>) -> Connection {
        let (frames_tx, frames) = mpsc::unbounded();
        let (control, control_rx) = mpsc::unbounded();
        let subscriber = Subscriber {
            frames: frames_tx,
            control: control_rx,
            focus: BTreeSet::new(),
        };
        let hello = Hello {
            head: Seq(self.head),
            floor: Seq(self.floor()),
            server_time: self.now,
            capabilities: Capabilities {
                events: vec![
                    "run.started".into(),
                    "step.text".into(),
                    "step.tool_started".into(),
                    "step.tool_finished".into(),
                    "step.task".into(),
                    "step.status".into(),
                    "run.reset".into(),
                    "run.finished".into(),
                    "message.created".into(),
                    "text.delta".into(),
                    "input.accepted".into(),
                ],
                ops: vec!["focus".into()],
                auth: AuthMode::None,
            },
        };
        self.deliver_to(&subscriber, &self.line("hello", None, None, None, &hello));
        if let Some(Seq(since)) = since {
            let gap = self.scenario.gap_once || since + 1 < self.floor() || since > self.head;
            if gap {
                self.scenario.gap_once = false;
                let gap = Gap {
                    floor: Seq(self.floor()),
                };
                self.deliver_to(&subscriber, &self.line("gap", None, None, None, &gap));
            } else {
                for (seq, line) in &self.ring {
                    if *seq > since {
                        self.deliver_to(&subscriber, line);
                    }
                }
            }
        }
        for index in 0..self.runs.len() {
            if self.runs[index].finished_at.is_none() {
                let line = self.snapshot_line(index);
                self.deliver_to(&subscriber, &line);
            }
        }
        self.subscribers.push(subscriber);
        Connection { frames, control }
    }

    fn deliver_to(&self, subscriber: &Subscriber, line: &str) {
        let Ok(frame) = decode(line) else {
            return;
        };
        subscriber.frames.unbounded_send(frame).ok();
    }

    fn snapshot_line(&self, index: usize) -> String {
        let run = &self.runs[index];
        let snapshot = RunSnapshot {
            run_id: run.id.clone(),
            agent_id: run.agent,
            surface_id: run.surface,
            started_at: run.started_at,
            as_of_seq: run.last_seq,
            text: run.segment.clone(),
            steps: run.steps.clone(),
        };
        self.line(
            "run.snapshot",
            None,
            Some(run.surface),
            Some(&run.id),
            &snapshot,
        )
    }

    fn drain_control(&mut self) {
        let mut snapshots = Vec::new();
        let mut open = Vec::new();
        for (position, subscriber) in self.subscribers.iter_mut().enumerate() {
            loop {
                match subscriber.control.try_recv() {
                    Ok(ClientFrame::Focus { surface_ids }) => {
                        let mut focus = BTreeSet::new();
                        for surface in surface_ids {
                            focus.insert(surface);
                            if !subscriber.focus.contains(&surface) {
                                snapshots.push((position, surface));
                            }
                        }
                        subscriber.focus = focus;
                    }
                    Err(TryRecvError::Empty) => {
                        open.push(position);
                        break;
                    }
                    Err(TryRecvError::Closed) => break,
                }
            }
        }
        for (position, surface) in snapshots {
            for index in 0..self.runs.len() {
                if self.runs[index].surface == surface && self.runs[index].finished_at.is_none() {
                    let line = self.snapshot_line(index);
                    let Ok(frame) = decode(&line) else {
                        continue;
                    };
                    self.subscribers[position].frames.unbounded_send(frame).ok();
                }
            }
        }
        let mut kept = Vec::new();
        for (position, subscriber) in self.subscribers.drain(..).enumerate() {
            if open.contains(&position) && !subscriber.frames.is_closed() {
                kept.push(subscriber);
            }
        }
        self.subscribers = kept;
    }

    fn line<T: Serialize>(
        &self,
        type_name: &str,
        seq: Option<i64>,
        surface: Option<SurfaceId>,
        run: Option<&RunId>,
        payload: &T,
    ) -> String {
        let at = self.now.format(&Rfc3339).unwrap_or_default();
        let mut envelope = json!({
            "v": 1,
            "type": type_name,
            "surface_id": surface,
            "run_id": run,
            "at": at,
            "payload": payload,
        });
        if let (Some(seq), Value::Object(fields)) = (seq, &mut envelope) {
            fields.insert("seq".into(), json!(seq));
        }
        envelope.to_string()
    }

    fn persist(
        &mut self,
        type_name: &str,
        surface: Option<SurfaceId>,
        run: Option<&RunId>,
        payload: &impl Serialize,
    ) {
        self.head += 1;
        let seq = self.head;
        let line = self.line(type_name, Some(seq), surface, run, payload);
        if let Some(run) = run
            && let Some(index) = self.run_index(run)
        {
            self.runs[index].last_seq = Seq(seq);
        }
        self.ring.push_back((seq, line.clone()));
        while self.ring.len() > RING {
            self.ring.pop_front();
        }
        self.broadcast(&line, Reach::Everyone);
    }

    fn broadcast(&mut self, line: &str, reach: Reach) {
        let Ok(frame) = decode(line) else {
            return;
        };
        for subscriber in &self.subscribers {
            let reached = match reach {
                Reach::Everyone => true,
                Reach::Focused(surface) => subscriber.focus.contains(&surface),
            };
            if reached {
                subscriber.frames.unbounded_send(frame.clone()).ok();
            }
        }
    }

    fn emit(&mut self, script: Script) {
        self.now += TimeDuration::seconds(1);
        match script {
            Script::Accepted(accepted) => {
                let line = self.line(
                    "input.accepted",
                    None,
                    Some(accepted.surface_id),
                    None,
                    &accepted,
                );
                self.broadcast(&line, Reach::Focused(accepted.surface_id));
            }
            Script::Created(message) => {
                let mut known = false;
                for stored in &self.messages {
                    if stored.id == message.id {
                        known = true;
                    }
                }
                if !known {
                    self.messages.push(message.clone());
                }
                let surface = message.surface_id;
                let run = message.run_id.clone();
                self.persist(
                    "message.created",
                    Some(surface),
                    run.as_ref(),
                    &json!({"message": message}),
                );
            }
            Script::Started(begin) => self.start(begin),
            Script::Delta(run, text) => {
                let Some(index) = self.live(&run) else {
                    return;
                };
                self.runs[index].segment.push_str(&text);
                let surface = self.runs[index].surface;
                let line = self.line(
                    "text.delta",
                    None,
                    Some(surface),
                    Some(&run),
                    &json!({"text": text}),
                );
                self.broadcast(&line, Reach::Focused(surface));
            }
            Script::Text(run, text) => {
                let Some(index) = self.live(&run) else {
                    return;
                };
                let row = self.row(index, RowKind::Text, None, Some(text.clone()));
                self.runs[index].steps.push(row);
                self.runs[index].segment.clear();
                let surface = self.runs[index].surface;
                self.persist("step.text", Some(surface), Some(&run), &StepText { text });
            }
            Script::ToolStart(run, started) => {
                let Some(index) = self.live(&run) else {
                    return;
                };
                let mut row = self.row(index, RowKind::Tool, Some(started.name.clone()), None);
                row.tool_use_id = Some(started.tool_use_id.clone());
                row.input = Some(started.input.clone());
                row.status = Some("running".into());
                self.runs[index].steps.push(row);
                let surface = self.runs[index].surface;
                self.persist("step.tool_started", Some(surface), Some(&run), &started);
            }
            Script::ToolFinish(run, finished) => {
                let Some(index) = self.live(&run) else {
                    return;
                };
                let now = self.now;
                for step in &mut self.runs[index].steps {
                    if step.tool_use_id.as_ref() == Some(&finished.tool_use_id) {
                        step.output = Some(finished.summary.clone());
                        step.status = Some(if finished.is_error { "error" } else { "ok" }.into());
                        step.finished_at = Some(now);
                    }
                }
                let surface = self.runs[index].surface;
                self.persist("step.tool_finished", Some(surface), Some(&run), &finished);
            }
            Script::Task(run, task) => {
                let Some(index) = self.live(&run) else {
                    return;
                };
                let body = serde_json::to_string(&task).unwrap_or_default();
                let mut row = self.row(
                    index,
                    RowKind::Task,
                    Some(task.task_type.clone()),
                    Some(body),
                );
                row.status = Some(task.state.clone());
                self.runs[index].steps.push(row);
                let surface = self.runs[index].surface;
                self.persist("step.task", Some(surface), Some(&run), &task);
            }
            Script::Finished(run, finished) => self.finish(run, finished),
        }
    }

    fn row(
        &self,
        index: usize,
        kind: RowKind,
        name: Option<String>,
        output: Option<String>,
    ) -> StepRow {
        StepRow {
            seq: i64::try_from(self.runs[index].steps.len()).unwrap_or(i64::MAX) + 1,
            kind,
            tool_use_id: None,
            name,
            input: None,
            output,
            status: None,
            started_at: self.now,
            finished_at: None,
        }
    }

    fn start(&mut self, begin: Begin) {
        let Begin {
            id,
            agent,
            surface,
            inputs,
            origin,
        } = begin;
        self.runs.push(MockRun {
            id: id.clone(),
            agent,
            surface,
            origin: origin.clone(),
            kind: origin.clone(),
            status: RunStatus::Running,
            terminal_reason: None,
            started_at: self.now,
            finished_at: None,
            steps: Vec::new(),
            segment: String::new(),
            last_seq: Seq(self.head),
        });
        let started = RunStarted {
            agent_id: agent,
            input_ids: inputs,
            origin: origin.clone(),
            turn_kind: origin,
        };
        self.persist("run.started", Some(surface), Some(&id), &started);
    }

    fn finish(&mut self, run: RunId, finished: RunFinished) {
        let Some(index) = self.live(&run) else {
            return;
        };
        let surface = self.runs[index].surface;
        let agent = self.runs[index].agent;
        let interrupted = finished.terminal_reason == "interrupted";
        if !interrupted {
            let mut answer = String::new();
            for step in &self.runs[index].steps {
                if step.kind == RowKind::Text
                    && let Some(output) = &step.output
                {
                    answer = output.clone();
                }
            }
            let summary = self.summary(index);
            let message = Message {
                id: self.next_message(),
                surface_id: surface,
                kind: MessageKind::Answer,
                author: Author {
                    kind: AuthorKind::Agent,
                    agent_id: Some(agent),
                },
                addressed_agent_id: None,
                reply_to_message_id: None,
                text: answer,
                run_id: Some(run.clone()),
                origin: self.runs[index].origin.clone(),
                channel: None,
                client_message_id: None,
                created_at: self.now,
                run_summary: Some(summary),
                attachments: Vec::new(),
            };
            self.messages.push(message.clone());
            self.persist(
                "message.created",
                Some(surface),
                Some(&run),
                &json!({"message": message}),
            );
        }
        let now = self.now;
        let status = if interrupted {
            RunStatus::Interrupted
        } else if finished.is_error {
            RunStatus::Error
        } else {
            RunStatus::Ok
        };
        let target = &mut self.runs[index];
        target.status = status;
        target.terminal_reason = Some(finished.terminal_reason.clone());
        target.finished_at = Some(now);
        target.segment.clear();
        self.persist("run.finished", Some(surface), Some(&run), &finished);
    }

    fn summary(&self, index: usize) -> RunSummary {
        let run = &self.runs[index];
        let mut tools = 0;
        for step in &run.steps {
            if step.kind == RowKind::Tool {
                tools += 1;
            }
        }
        RunSummary {
            status: RunStatus::Running,
            step_count: u32::try_from(run.steps.len()).unwrap_or(u32::MAX),
            tool_count: tools,
            duration_ms: u64::try_from((self.now - run.started_at).whole_milliseconds())
                .unwrap_or(0),
        }
    }
}

fn segments(path: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    for segment in path.trim_start_matches('/').split('/') {
        segments.push(segment);
    }
    segments
}

fn to_json<T: Serialize>(value: &T) -> Result<Value, ApiError> {
    serde_json::to_value(value).map_err(|error| ApiError::Decode(error.to_string()))
}

fn parse_agent(id: &str) -> Result<AgentId, ApiError> {
    let Ok(id) = id.parse::<i64>() else {
        return Err(ApiError::NotFound);
    };
    Ok(AgentId(id))
}

fn parse_surface(id: &str) -> Result<SurfaceId, ApiError> {
    let Ok(id) = id.parse::<i64>() else {
        return Err(ApiError::NotFound);
    };
    Ok(SurfaceId(id))
}

fn seed_surfaces() -> Vec<Surface> {
    vec![
        Surface {
            id: SurfaceId(1),
            kind: SurfaceKind::Channel,
            name: "General".into(),
            sort_order: 0,
            last_message_at: None,
            lead_agent_id: Some(AgentId(1)),
            agents: vec![
                Wiring {
                    agent_id: AgentId(1),
                    role: Role::Lead,
                    listens: true,
                },
                Wiring {
                    agent_id: AgentId(3),
                    role: Role::Mention,
                    listens: false,
                },
            ],
            bindings: vec![Binding {
                channel: Channel::Telegram,
                external_id: "-1003614621196:0".into(),
                mirror: Mirror::AgentOnly,
            }],
            live_run: None,
            topic_name: String::new(),
            display_name: None,
            group_id: None,
            archived_at: None,
            last_read_message_id: None,
            unread: 0,
        },
        Surface {
            id: SurfaceId(2),
            kind: SurfaceKind::Channel,
            name: "Magnet Feed".into(),
            sort_order: 1,
            last_message_at: None,
            lead_agent_id: Some(AgentId(3)),
            agents: vec![Wiring {
                agent_id: AgentId(3),
                role: Role::Lead,
                listens: true,
            }],
            bindings: vec![Binding {
                channel: Channel::Telegram,
                external_id: "-1003614621196:918".into(),
                mirror: Mirror::AgentOnly,
            }],
            live_run: None,
            topic_name: String::new(),
            display_name: None,
            group_id: None,
            archived_at: None,
            last_read_message_id: None,
            unread: 0,
        },
        Surface {
            id: SurfaceId(3),
            kind: SurfaceKind::Channel,
            name: "Smart Home".into(),
            sort_order: 2,
            last_message_at: None,
            lead_agent_id: Some(AgentId(2)),
            agents: vec![Wiring {
                agent_id: AgentId(2),
                role: Role::Lead,
                listens: true,
            }],
            bindings: vec![Binding {
                channel: Channel::Telegram,
                external_id: "-1003614621196:731".into(),
                mirror: Mirror::AgentOnly,
            }],
            live_run: None,
            topic_name: String::new(),
            display_name: None,
            group_id: None,
            archived_at: None,
            last_read_message_id: None,
            unread: 0,
        },
    ]
}

fn seed_agents() -> Vec<Agent> {
    let agent =
        |id: i64, name: &str, ident: &str, description: &str, model: &str, home: Option<i64>| {
            Agent {
                id: AgentId(id),
                name: name.into(),
                ident: ident.into(),
                description: description.into(),
                bot_username: Some(format!("{ident}_bot")),
                model: model.into(),
                state: AgentState::Idle,
                live_run: None,
                home_surface_id: home.map(SurfaceId),
                avatar_url: None,
            }
        };
    vec![
        agent(
            1,
            "Jarvis",
            "tuclaw",
            "The house butler",
            "opus[1m]:medium",
            Some(1),
        ),
        agent(
            2,
            "Home",
            "home",
            "Lights, heating and the robot vacuum",
            "sonnet",
            Some(3),
        ),
        agent(
            3,
            "Magnet Feed",
            "magnet_feed",
            "Tracks torrent releases",
            "sonnet",
            Some(2),
        ),
        agent(4, "Scout", "scout", "Research on request", "opus", None),
    ]
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use futures::executor::block_on;

    use super::*;
    use crate::v3::client::{Client, Pause, TaskScope};
    use crate::v3::dto::{AgentPatch, AudioKind, ModelChange, VoicePost};

    fn stepped() -> (MockTransport, Client) {
        let mock = MockTransport::new(Scenario::default(), Pace::Stepped);
        let client = Client::mock(&mock);
        (mock, client)
    }

    fn kind(frame: &Frame) -> &'static str {
        match frame {
            Frame::Hello(_) => "hello",
            Frame::Gap(_) => "gap",
            Frame::RunSnapshot(_) => "run.snapshot",
            Frame::RunStarted(_) => "run.started",
            Frame::StepText(_) => "step.text",
            Frame::ToolStarted(_) => "step.tool_started",
            Frame::ToolFinished(_) => "step.tool_finished",
            Frame::Task(_) => "step.task",
            Frame::Status(_) => "step.status",
            Frame::RunReset(_) => "run.reset",
            Frame::RunFinished(_) => "run.finished",
            Frame::MessageCreated(_) => "message.created",
            Frame::TaskFired(_) => "task.fired",
            Frame::SurfaceRead(_) => "surface.read",
            Frame::SurfaceUpdated(_) => "surface.updated",
            Frame::GroupsChanged(_) => "groups.changed",
            Frame::TextDelta(_) => "text.delta",
            Frame::InputAccepted(_) => "input.accepted",
            Frame::Unknown(_) => "unknown",
        }
    }

    fn drain(connection: &mut Connection) -> Vec<Frame> {
        let mut frames = Vec::new();
        while let Ok(frame) = connection.frames.try_recv() {
            frames.push(frame);
        }
        frames
    }

    fn kinds(frames: &[Frame]) -> Vec<&'static str> {
        let mut kinds = Vec::new();
        for frame in frames {
            kinds.push(kind(frame));
        }
        kinds
    }

    fn collapse(kinds: Vec<&'static str>) -> Vec<&'static str> {
        let mut collapsed: Vec<&'static str> = Vec::new();
        for kind in kinds {
            if collapsed.last() != Some(&kind) {
                collapsed.push(kind);
            }
        }
        collapsed
    }

    fn post(text: &str, addressed: Option<AgentId>) -> Post {
        Post {
            text: text.into(),
            addressed_agent_id: addressed,
            client_message_id: ClientMessageId::random(),
        }
    }

    fn connected(mock: &MockTransport, client: &Client, focus: Vec<SurfaceId>) -> Connection {
        let mut connection = block_on(client.connect(None)).expect("the mock connects");
        drain(&mut connection);
        connection.focus(focus).expect("the socket is open");
        mock.pump_control();
        drain(&mut connection);
        connection
    }

    fn voice(bytes: &[u8], client_message_id: &ClientMessageId) -> VoicePost {
        VoicePost {
            kind: AudioKind::M4a,
            bytes: bytes.to_vec(),
            addressed_agent_id: None,
            client_message_id: client_message_id.clone(),
        }
    }

    #[test]
    fn a_voice_post_stores_the_recording_and_wakes_the_lead() {
        let (_mock, client) = stepped();
        let key = ClientMessageId::random();
        let posted = block_on(client.post_voice(SurfaceId(1), voice(b"m4a bytes", &key)))
            .expect("the voice is accepted");
        assert_eq!(posted.agent_id, Some(AgentId(1)));
        let again = block_on(client.post_voice(SurfaceId(1), voice(b"m4a bytes", &key)))
            .expect("a retry answers the first ids");
        assert_eq!(again, posted);
        let page = block_on(client.messages(SurfaceId(1), 200)).expect("page");
        let Some(stored) = page.messages.last() else {
            panic!("the voice message is stored");
        };
        assert_eq!(stored.id, posted.message_id);
        assert!(stored.text.starts_with("[Voice message]"));
        assert_eq!(stored.attachments.len(), 1);
        assert_eq!(stored.attachments[0].mime, "audio/mp4");
        let bytes = block_on(client.attachment(stored.attachments[0].id)).expect("bytes");
        assert_eq!(bytes, b"m4a bytes");
        let empty =
            block_on(client.post_voice(SurfaceId(1), voice(b"", &ClientMessageId::random())));
        let Err(ApiError::Invalid(_)) = empty else {
            panic!("an empty recording is refused");
        };
    }

    #[test]
    fn reading_moves_the_cursor_forward_and_is_announced() {
        let (mock, client) = stepped();
        let mut connection = connected(&mock, &client, Vec::new());
        mock.agent_posts(SurfaceId(1), AgentId(1), "one");
        mock.agent_posts(SurfaceId(1), AgentId(1), "two");
        while mock.step() {}
        drain(&mut connection);
        let general = |client: &Client| {
            let surfaces = block_on(client.surfaces()).expect("surfaces");
            surfaces[0].clone()
        };
        assert_eq!(general(&client).unread, 2);
        let page = block_on(client.messages(SurfaceId(1), 200)).expect("page");
        let newest = page.messages[page.messages.len() - 1].id;
        let older = page.messages[page.messages.len() - 2].id;
        let answer = block_on(client.mark_read(SurfaceId(1), older)).expect("reads");
        assert_eq!(answer.unread, 1);
        let answer = block_on(client.mark_read(SurfaceId(1), newest)).expect("reads");
        assert_eq!(
            answer,
            ReadAnswer {
                last_read_message_id: newest,
                unread: 0
            }
        );
        let back = block_on(client.mark_read(SurfaceId(1), older)).expect("never moves back");
        assert_eq!(back.last_read_message_id, newest);
        assert_eq!(general(&client).unread, 0);
        let mut announced = 0;
        for frame in drain(&mut connection) {
            if let Frame::SurfaceRead(_) = frame {
                announced += 1;
            }
        }
        assert_eq!(announced, 3);
        let other = block_on(client.mark_read(SurfaceId(2), newest));
        let Err(ApiError::Invalid(_)) = other else {
            panic!("a message of another surface is refused");
        };
    }

    #[test]
    fn automations_list_pause_resume_and_cancel() {
        let (_mock, client) = stepped();
        let live = block_on(client.tasks(TaskScope::Live)).expect("tasks");
        assert_eq!(live.len(), 4);
        let lights = TaskId("task-evening-lights".into());
        let resumed = block_on(client.set_task_paused(&lights, Pause::Resume)).expect("resumes");
        assert_eq!(resumed.status, TaskStatus::Active);
        let again = block_on(client.set_task_paused(&lights, Pause::Resume));
        let Err(ApiError::Conflict) = again else {
            panic!("resuming an active automation conflicts");
        };
        let paused = block_on(client.set_task_paused(&lights, Pause::Pause)).expect("pauses");
        assert_eq!(paused.status, TaskStatus::Paused);
        block_on(client.cancel_task(&lights)).expect("cancels");
        assert_eq!(
            block_on(client.tasks(TaskScope::Live))
                .expect("tasks")
                .len(),
            3
        );
        let recent = block_on(client.tasks(TaskScope::Recent)).expect("tasks");
        assert_eq!(recent.len(), 4);
        let unknown = block_on(client.task_runs(&TaskId("gone".into())));
        let Err(ApiError::NotFound) = unknown else {
            panic!("an unknown automation has no runs");
        };
    }

    #[test]
    fn a_fire_is_persisted_on_its_surface_and_lands_on_the_page() {
        let (mock, client) = stepped();
        let mut connection = connected(&mock, &client, Vec::new());
        let jobs = TaskId("task-mac-jobs".into());
        let before = block_on(client.messages(SurfaceId(1), 200)).expect("page");
        mock.fire_task(&jobs, Outcome::Ran);
        let mut fired = None;
        for frame in drain(&mut connection) {
            if let Frame::TaskFired(frame) = frame {
                fired = Some(frame);
            }
        }
        let fired = fired.expect("task.fired is sent");
        assert_eq!(fired.surface_id, Some(SurfaceId(1)));
        assert_eq!(fired.mark.task_id, jobs);
        assert_eq!(fired.mark.outcome, Outcome::Ran);
        let after = block_on(client.messages(SurfaceId(1), 200)).expect("page");
        assert_eq!(after.automations.len(), before.automations.len() + 1);
        let runs = block_on(client.task_runs(&jobs)).expect("runs");
        assert_eq!(runs[0].outcome, Outcome::Ran);
        assert!(runs[0].at > runs[runs.len() - 1].at);
    }

    #[test]
    fn a_surface_without_fires_still_carries_an_empty_list() {
        let (_mock, client) = stepped();
        let page = block_on(client.messages(SurfaceId(3), 200)).expect("page");
        assert!(page.automations.is_empty());
        let raw = to_json(&page).expect("encodes");
        assert_eq!(raw["automations"], json!([]));
    }

    #[test]
    fn the_world_decodes_through_the_contract_types() {
        let (_mock, client) = stepped();
        let surfaces = block_on(client.surfaces()).expect("surfaces");
        assert_eq!(surfaces.len(), 3);
        assert_eq!(surfaces[0].agents.len(), 2);
        assert!(surfaces[1].live_run.is_some());
        assert!(surfaces[0].last_message_at.is_some());
        let agents = block_on(client.agents()).expect("agents");
        assert_eq!(agents.len(), 4);
        assert_eq!(agents[2].state, AgentState::Running);
        assert_eq!(agents[3].home_surface_id, None);
        let page = block_on(client.messages(SurfaceId(1), 10)).expect("page");
        assert_eq!(page.messages.len(), 10);
        assert!(page.has_more);
        assert!(page.messages[0].created_at < page.messages[9].created_at);
        let mut total = 0;
        for surface in &surfaces {
            total += block_on(client.messages(surface.id, 200))
                .expect("page")
                .messages
                .len();
        }
        assert_eq!(total, 61);
        let live = surfaces[1].live_run.clone().expect("a live run");
        let detail = block_on(client.run(&live.run_id)).expect("the live run");
        assert_eq!(detail.run.status, RunStatus::Running);
        assert_eq!(detail.steps.len(), 3);
        assert_eq!(
            block_on(client.run(&RunId("nope".into()))),
            Err(ApiError::NotFound)
        );
    }

    #[test]
    fn a_post_plays_the_contract_order_with_the_answer_before_the_finish() {
        let (mock, client) = stepped();
        let mut connection = connected(&mock, &client, vec![SurfaceId(1)]);
        let posted = block_on(client.post(SurfaceId(1), &post("Лисички?", None))).expect("posted");
        assert_eq!(posted.agent_id, Some(AgentId(1)));
        assert!(mock.play_all() > 10);
        let frames = drain(&mut connection);
        assert_eq!(
            collapse(kinds(&frames)),
            vec![
                "input.accepted",
                "message.created",
                "run.started",
                "text.delta",
                "step.text",
                "step.tool_started",
                "step.tool_finished",
                "step.task",
                "text.delta",
                "step.text",
                "message.created",
                "run.finished",
            ]
        );
        let Frame::MessageCreated(user) = &frames[1] else {
            panic!("expected the user message");
        };
        assert_eq!(user.message.channel, Some(Channel::Desktop));
        assert_eq!(user.message.id, posted.message_id);
        let mut seqs = Vec::new();
        for frame in &frames {
            if let Some(Seq(seq)) = frame.seq() {
                seqs.push(seq);
            }
        }
        let mut sorted = seqs.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(seqs, sorted);
        let page = block_on(client.messages(SurfaceId(1), 2)).expect("page");
        assert_eq!(page.messages[0].id, posted.message_id);
        assert_eq!(page.messages[1].kind, MessageKind::Answer);
    }

    #[test]
    fn posts_route_to_the_addressed_agent_or_the_lead() {
        let (_mock, client) = stepped();
        let addressed =
            block_on(client.post(SurfaceId(1), &post("@magnet_feed?", Some(AgentId(3)))))
                .expect("posted");
        assert_eq!(addressed.agent_id, Some(AgentId(3)));
        let lead = block_on(client.post(SurfaceId(3), &post("свет", None))).expect("posted");
        assert_eq!(lead.agent_id, Some(AgentId(2)));
        let unwired = block_on(client.post(SurfaceId(1), &post("@scout", Some(AgentId(4)))))
            .expect("an unwired agent routes to the lead");
        assert_eq!(unwired.agent_id, Some(AgentId(1)));
        assert_eq!(
            block_on(client.post(SurfaceId(9), &post("x", None))),
            Err(ApiError::NotFound)
        );
    }

    #[test]
    fn a_repeated_client_message_id_returns_the_first_answer_and_queues_nothing() {
        let (mock, client) = stepped();
        let first = post("Лисички?", None);
        let posted = block_on(client.post(SurfaceId(1), &first)).expect("posted");
        let pending = mock.pending();
        let again = block_on(client.post(SurfaceId(1), &first)).expect("posted");
        assert_eq!(again, posted);
        assert_eq!(mock.pending(), pending);
        assert_eq!(
            block_on(client.post(SurfaceId(3), &first)),
            Err(ApiError::Conflict)
        );
        assert_eq!(mock.pending(), pending);
    }

    #[test]
    fn a_since_beyond_the_head_gets_a_gap() {
        let (mock, client) = stepped();
        let Seq(head) = mock.head();
        let mut ahead = block_on(client.connect(Some(Seq(head + 5)))).expect("connects");
        assert_eq!(
            kinds(&drain(&mut ahead)),
            vec!["hello", "gap", "run.snapshot"]
        );
        let mut current = block_on(client.connect(Some(Seq(head)))).expect("connects");
        assert_eq!(kinds(&drain(&mut current)), vec!["hello", "run.snapshot"]);
    }

    #[test]
    fn an_interrupt_ends_the_run_without_an_answer() {
        let (mock, client) = stepped();
        let mut connection = connected(&mock, &client, vec![SurfaceId(1)]);
        block_on(client.post(SurfaceId(1), &post("Лисички?", None))).expect("posted");
        for _ in 0..5 {
            mock.step();
        }
        let frames = drain(&mut connection);
        let mut run = None;
        for frame in &frames {
            if let Frame::RunStarted(event) = frame {
                run = Some(event.run_id.clone());
            }
        }
        let run = run.expect("the run started");
        assert_eq!(block_on(client.interrupt(&run)), Ok(()));
        mock.play_all();
        let frames = drain(&mut connection);
        assert_eq!(kinds(&frames), vec!["run.finished"]);
        let Frame::RunFinished(finished) = &frames[0] else {
            panic!("expected run.finished");
        };
        assert_eq!(finished.body.terminal_reason, "interrupted");
        assert!(!finished.body.is_error);
        assert_eq!(block_on(client.interrupt(&run)), Err(ApiError::Conflict));
        let detail = block_on(client.run(&run)).expect("the run");
        assert_eq!(detail.run.status, RunStatus::Interrupted);
    }

    #[test]
    fn a2a_plays_two_runs_on_one_surface() {
        let (mock, client) = stepped();
        let mut connection = connected(&mock, &client, vec![SurfaceId(1)]);
        mock.play_a2a();
        mock.play_all();
        let mut started = Vec::new();
        for frame in drain(&mut connection) {
            if let Frame::RunStarted(event) = frame {
                assert_eq!(event.surface_id, Some(SurfaceId(1)));
                started.push(event.body.agent_id);
            }
        }
        assert_eq!(started, vec![AgentId(1), AgentId(3)]);
    }

    #[test]
    fn a_telegram_tick_posts_a_telegram_message_and_its_run() {
        let (mock, client) = stepped();
        let mut connection = connected(&mock, &client, vec![SurfaceId(1)]);
        mock.telegram_tick();
        mock.play_all();
        let frames = drain(&mut connection);
        let Frame::MessageCreated(user) = &frames[0] else {
            panic!("expected the user message first");
        };
        assert_eq!(user.message.channel, Some(Channel::Telegram));
        assert_eq!(kind(&frames[frames.len() - 1]), "run.finished");
    }

    #[test]
    fn connect_sends_hello_and_a_snapshot_per_live_run() {
        let (mock, client) = stepped();
        let mut connection = block_on(client.connect(None)).expect("the mock connects");
        let frames = drain(&mut connection);
        assert_eq!(kinds(&frames), vec!["hello", "run.snapshot"]);
        let Frame::Hello(hello) = &frames[0] else {
            panic!("expected hello");
        };
        assert_eq!(hello.head, mock.head());
        let Frame::RunSnapshot(snapshot) = &frames[1] else {
            panic!("expected a snapshot");
        };
        assert_eq!(snapshot.surface_id, SurfaceId(2));
        assert_eq!(snapshot.as_of_seq, mock.head());
        assert_eq!(snapshot.text, "Нашёл три новых релиза, ");
    }

    #[test]
    fn focus_sends_snapshots_of_newly_focused_surfaces_only() {
        let (mock, client) = stepped();
        let mut connection = block_on(client.connect(None)).expect("the mock connects");
        drain(&mut connection);
        connection.focus(vec![SurfaceId(2)]).expect("open");
        mock.pump_control();
        assert_eq!(kinds(&drain(&mut connection)), vec!["run.snapshot"]);
        connection
            .focus(vec![SurfaceId(1), SurfaceId(2)])
            .expect("open");
        mock.pump_control();
        assert!(drain(&mut connection).is_empty());
    }

    #[test]
    fn ephemeral_frames_reach_only_focused_surfaces() {
        let (mock, client) = stepped();
        let mut watching = connected(&mock, &client, vec![SurfaceId(1)]);
        let mut elsewhere = connected(&mock, &client, vec![SurfaceId(3)]);
        block_on(client.post(SurfaceId(1), &post("Лисички?", None))).expect("posted");
        mock.play_all();
        let watched = kinds(&drain(&mut watching));
        let other = kinds(&drain(&mut elsewhere));
        assert!(watched.contains(&"text.delta"));
        assert!(watched.contains(&"input.accepted"));
        assert!(!other.contains(&"text.delta"));
        assert!(!other.contains(&"input.accepted"));
        assert!(other.contains(&"run.finished"));
    }

    #[test]
    fn a_reconnect_replays_exactly_the_missed_persisted_frames() {
        let (mock, client) = stepped();
        let mut first = connected(&mock, &client, vec![SurfaceId(1)]);
        block_on(client.post(SurfaceId(1), &post("Лисички?", None))).expect("posted");
        for _ in 0..6 {
            mock.step();
        }
        let mut last = None;
        for frame in drain(&mut first) {
            if let Some(seq) = frame.seq() {
                last = Some(seq);
            }
        }
        mock.disconnect_all();
        assert_eq!(block_on(first.frames.next()), None);
        mock.play_all();
        let mut second = block_on(client.connect(last)).expect("the mock reconnects");
        let frames = drain(&mut second);
        assert_eq!(kind(&frames[0]), "hello");
        let mut replayed = Vec::new();
        let mut snapshots = 0;
        for frame in &frames[1..] {
            match (frame.seq(), kind(frame)) {
                (Some(Seq(seq)), _) => {
                    assert_eq!(snapshots, 0, "the replay comes before the snapshots");
                    replayed.push(seq);
                }
                (None, "run.snapshot") => snapshots += 1,
                (None, other) => panic!("a replay carries no ephemeral frames, got {other}"),
            }
        }
        assert_eq!(snapshots, 1);
        let Some(Seq(last)) = last else {
            panic!("the first connection saw persisted frames");
        };
        let Seq(head) = mock.head();
        let mut expected = Vec::new();
        for seq in last + 1..=head {
            expected.push(seq);
        }
        assert_eq!(replayed, expected);
    }

    #[test]
    fn a_since_below_the_ring_or_the_gap_knob_gets_a_gap() {
        let (mock, client) = stepped();
        block_on(client.post(SurfaceId(1), &post("Лисички?", None))).expect("posted");
        mock.play_all();
        let mut old = block_on(client.connect(Some(Seq(5)))).expect("connects");
        assert_eq!(
            collapse(kinds(&drain(&mut old))),
            vec!["hello", "gap", "run.snapshot"]
        );
        let knobbed = MockTransport::new(
            Scenario {
                gap_once: true,
                unavailable_once: false,
            },
            Pace::Stepped,
        );
        let client = Client::mock(&knobbed);
        let head = knobbed.head();
        let mut first = block_on(client.connect(Some(head))).expect("connects");
        assert_eq!(
            kinds(&drain(&mut first)),
            vec!["hello", "gap", "run.snapshot"]
        );
        let mut second = block_on(client.connect(Some(head))).expect("connects");
        assert_eq!(kinds(&drain(&mut second)), vec!["hello", "run.snapshot"]);
    }

    #[test]
    fn the_unavailable_knob_fails_one_call() {
        let mock = MockTransport::new(
            Scenario {
                gap_once: false,
                unavailable_once: true,
            },
            Pace::Stepped,
        );
        let client = Client::mock(&mock);
        assert_eq!(block_on(client.surfaces()), Err(ApiError::Unavailable));
        assert!(block_on(client.surfaces()).is_ok());
    }

    #[test]
    fn the_built_in_voice_message_serves_its_recording() {
        let (_mock, client) = stepped();
        let page = block_on(client.messages(SurfaceId(2), 50)).expect("page");
        let mut voice = None;
        for message in &page.messages {
            if let Some(attachment) = message.attachments.first() {
                voice = Some(attachment.clone());
            }
        }
        let voice = voice.expect("Magnet Feed carries a voice message");
        assert_eq!(voice.kind, AttachmentKind::Voice);
        assert_eq!(voice.mime, "audio/ogg");
        let bytes = block_on(client.attachment(voice.id)).expect("the recording");
        assert_eq!(u64::try_from(bytes.len()).ok(), Some(voice.size_bytes));
        assert!(bytes.starts_with(b"OggS"));
        assert_eq!(
            block_on(client.attachment(AttachmentId(99))),
            Err(ApiError::NotFound)
        );
    }

    #[test]
    fn avatars_are_served_replaced_and_cleared() {
        let mock = MockTransport::new(Scenario::default(), Pace::Stepped);
        let client = Client::mock(&mock);
        let agents = block_on(client.agents()).expect("agents");
        let Some(first) = agents[0].avatar_url.clone() else {
            panic!("the mock seeds an avatar for the first agent");
        };
        assert_eq!(agents[1].avatar_url, None);
        let bytes = block_on(client.avatar(&first)).expect("the avatar is served");
        assert_eq!(ImageKind::sniff(&bytes), Some(ImageKind::Png));
        let replaced = block_on(client.set_avatar(
            AvatarOwner::Agent(AgentId(1)),
            ImageKind::Png,
            MY_AVATAR.to_vec(),
        ))
        .expect("stored");
        assert_ne!(replaced, first);
        assert_eq!(block_on(client.avatar(&first)), Err(ApiError::NotFound));
        assert!(block_on(client.avatar(&replaced)).is_ok());
        assert_eq!(
            block_on(client.agents()).expect("agents")[0].avatar_url,
            Some(replaced)
        );
        block_on(client.clear_avatar(AvatarOwner::Agent(AgentId(1)))).expect("cleared");
        assert_eq!(
            block_on(client.agents()).expect("agents")[0].avatar_url,
            None
        );
        assert_eq!(block_on(client.avatar(&first)), Err(ApiError::NotFound));
    }

    #[test]
    fn an_upload_is_judged_by_its_bytes_not_its_declared_type() {
        let mock = MockTransport::new(Scenario::default(), Pace::Stepped);
        let client = Client::mock(&mock);
        let mislabeled =
            block_on(client.set_avatar(AvatarOwner::Me, ImageKind::Jpeg, MY_AVATAR.to_vec()));
        assert!(mislabeled.is_ok());
        let refused =
            block_on(client.set_avatar(AvatarOwner::Me, ImageKind::Png, b"GIF89a....".to_vec()));
        assert!(matches!(refused, Err(ApiError::Invalid(_))));
        let huge = block_on(client.set_avatar(
            AvatarOwner::Me,
            ImageKind::Png,
            vec![0x89; AVATAR_LIMIT + 1],
        ));
        assert!(matches!(huge, Err(ApiError::Invalid(_))));
        let unknown = block_on(client.set_avatar(
            AvatarOwner::Agent(AgentId(99)),
            ImageKind::Png,
            MY_AVATAR.to_vec(),
        ));
        assert_eq!(unknown, Err(ApiError::NotFound));
    }

    #[test]
    fn an_agent_description_and_model_are_patched_and_reset() {
        let mock = MockTransport::new(Scenario::default(), Pace::Stepped);
        let client = Client::mock(&mock);
        let patched = block_on(client.update_agent(
            AgentId(3),
            &AgentPatch {
                description: Some("Pulls releases".into()),
                model: ModelChange::Set("opus[1m]:high".into()),
            },
        ))
        .expect("patched");
        assert_eq!(patched.description, "Pulls releases");
        assert_eq!(patched.model, "opus[1m]:high");
        let reset = block_on(client.update_agent(
            AgentId(3),
            &AgentPatch {
                description: None,
                model: ModelChange::Default,
            },
        ))
        .expect("reset");
        assert_eq!(reset.description, "Pulls releases");
        assert_eq!(reset.model, DEFAULT_MODEL);
        let long = block_on(client.update_agent(
            AgentId(3),
            &AgentPatch {
                description: Some("x".repeat(DESCRIPTION_LIMIT + 1)),
                model: ModelChange::Keep,
            },
        ));
        assert!(matches!(long, Err(ApiError::Invalid(_))));
        let unknown = block_on(client.update_agent(
            AgentId(99),
            &AgentPatch {
                description: None,
                model: ModelChange::Keep,
            },
        ));
        assert_eq!(unknown, Err(ApiError::NotFound));
    }

    #[test]
    fn a_new_lead_demotes_the_old_one_and_a_lead_cannot_be_removed() {
        let mock = MockTransport::new(Scenario::default(), Pace::Stepped);
        let client = Client::mock(&mock);
        let surface = SurfaceId(1);
        let before = block_on(client.surfaces()).expect("surfaces");
        let old_lead = before[0].lead_agent_id.expect("General has a lead");
        let fresh = AgentId(4);
        let added = block_on(client.set_wiring(
            surface,
            fresh,
            WiringChange {
                role: Role::Mention,
                listens: true,
            },
        ))
        .expect("added");
        assert_eq!(added.lead_agent_id, Some(old_lead));
        let promoted = block_on(client.set_wiring(
            surface,
            fresh,
            WiringChange {
                role: Role::Lead,
                listens: true,
            },
        ))
        .expect("promoted");
        assert_eq!(promoted.lead_agent_id, Some(fresh));
        let mut leads = 0;
        for wiring in &promoted.agents {
            if wiring.role == Role::Lead {
                leads += 1;
            }
        }
        assert_eq!(leads, 1);
        assert_eq!(
            block_on(client.remove_wiring(surface, fresh)),
            Err(ApiError::Conflict)
        );
        let demoted = block_on(client.set_wiring(
            surface,
            fresh,
            WiringChange {
                role: Role::Mention,
                listens: true,
            },
        ));
        assert_eq!(demoted, Err(ApiError::Conflict));
        block_on(client.remove_wiring(surface, old_lead)).expect("the demoted lead leaves");
        block_on(client.remove_wiring(surface, old_lead)).expect("removing twice is fine");
        let after = block_on(client.surfaces()).expect("surfaces");
        let mut still_wired = false;
        for wiring in &after[0].agents {
            if wiring.agent_id == old_lead {
                still_wired = true;
            }
        }
        assert!(!still_wired);
        assert_eq!(
            block_on(client.remove_wiring(SurfaceId(99), fresh)),
            Err(ApiError::NotFound)
        );
    }

    #[test]
    fn a_wiring_with_an_unknown_role_is_refused() {
        let mock = MockTransport::new(Scenario::default(), Pace::Stepped);
        let refused = block_on(mock.send(Request {
            method: Method::Put,
            path: "/surfaces/1/agents/4".into(),
            body: Body::Json(json!({"role": "boss", "listens": true})),
        }));
        assert!(matches!(refused, Err(ApiError::Invalid(_))));
        let not_bool = block_on(mock.send(Request {
            method: Method::Put,
            path: "/surfaces/1/agents/4".into(),
            body: Body::Json(json!({"role": "mention", "listens": "yes"})),
        }));
        assert!(matches!(not_bool, Err(ApiError::Invalid(_))));
    }

    #[test]
    fn me_is_renamed_and_falls_back_to_you() {
        let mock = MockTransport::new(Scenario::default(), Pace::Stepped);
        let client = Client::mock(&mock);
        assert_eq!(block_on(client.me()).expect("me").name, "You");
        let renamed = block_on(client.update_me(&MePatch {
            name: Some("Pavel".into()),
            description: Some("Builds tuclaw".into()),
        }))
        .expect("renamed");
        assert_eq!(renamed.description, "Builds tuclaw");
        assert_eq!(block_on(client.me()).expect("me").name, "Pavel");
        let blank = block_on(client.update_me(&MePatch {
            name: Some("  ".into()),
            description: None,
        }));
        assert!(matches!(blank, Err(ApiError::Invalid(_))));
    }

    #[test]
    fn a_seeded_attachment_is_read_from_its_file() {
        let path = std::env::temp_dir().join(format!("tuclaw-media-{}.bin", std::process::id()));
        std::fs::write(&path, b"voice bytes").expect("written");
        let seed = Seed {
            surfaces: Vec::new(),
            agents: Vec::new(),
            messages: Vec::new(),
            runs: Vec::new(),
            media: vec![SeedMedia {
                id: AttachmentId(7),
                path: path.clone(),
            }],
            me: None,
            tasks: Vec::new(),
        };
        let mock = MockTransport::seeded(seed, Scenario::default(), Pace::Stepped);
        let client = Client::mock(&mock);
        assert_eq!(
            block_on(client.attachment(AttachmentId(7))),
            Ok(b"voice bytes".to_vec())
        );
        std::fs::remove_file(&path).expect("removed");
        let Err(ApiError::Transport(_)) = block_on(client.attachment(AttachmentId(7))) else {
            panic!("a missing file is a transport error");
        };
    }

    #[test]
    fn fail_next_call_fails_exactly_one_call() {
        let (mock, client) = stepped();
        mock.fail_next_call();
        assert_eq!(block_on(client.agents()), Err(ApiError::Unavailable));
        assert!(block_on(client.agents()).is_ok());
    }

    #[test]
    fn a_seeded_world_answers_from_its_seed() {
        let surfaces: Vec<Surface> =
            serde_json::from_str(include_str!("../../testdata/v3/surfaces.json")).unwrap();
        let agents: Vec<Agent> =
            serde_json::from_str(include_str!("../../testdata/v3/agents.json")).unwrap();
        let page: MessagesPage =
            serde_json::from_str(include_str!("../../testdata/v3/messages_page.json")).unwrap();
        let detail: RunDetail =
            serde_json::from_str(include_str!("../../testdata/v3/run.json")).unwrap();
        let seed = Seed {
            surfaces,
            agents,
            messages: page.messages,
            runs: vec![detail.clone()],
            media: Vec::new(),
            me: None,
            tasks: Vec::new(),
        };
        let mock = MockTransport::seeded(seed, Scenario::default(), Pace::Stepped);
        let client = Client::mock(&mock);
        let surfaces = block_on(client.surfaces()).expect("surfaces");
        assert_eq!(surfaces.len(), 2);
        assert_eq!(surfaces[0].name, "General");
        let page = block_on(client.messages(SurfaceId(1), 50)).expect("page");
        assert_eq!(page.messages.len(), 2);
        assert_eq!(page.messages[1].id, MessageId(9192));
        let run = block_on(client.run(&detail.run.id)).expect("the seeded run");
        assert_eq!(run.steps.len(), 2);
        assert_eq!(run.run.status, RunStatus::Ok);
        let posted = block_on(client.post(SurfaceId(1), &post("Привет", None))).expect("posted");
        assert_eq!(posted.message_id, MessageId(9193));
        assert_eq!(posted.agent_id, Some(AgentId(1)));
        let mut connection = block_on(client.connect(None)).expect("connects");
        assert_eq!(kinds(&drain(&mut connection)), vec!["hello"]);
    }

    #[test]
    fn realtime_plays_the_queue_by_itself() {
        let mock = MockTransport::new(Scenario::default(), Pace::Realtime);
        let client = Client::mock(&mock);
        let mut connection = block_on(client.connect(None)).expect("connects");
        block_on(client.post(SurfaceId(1), &post("Лисички?", None))).expect("posted");
        let finished = handle().block_on(async {
            tokio::time::timeout(Duration::from_secs(20), async {
                while let Some(frame) = connection.frames.next().await {
                    if let Frame::RunFinished(event) = frame {
                        return Some(event.body.terminal_reason);
                    }
                }
                None
            })
            .await
        });
        assert_eq!(finished, Ok(Some("success".to_string())));
    }
}
