//! An in-process daemon speaking the v3 contract, for development and tests.
//!
//! [`MockTransport`] answers REST from a scripted world and streams scripted frames. Every frame
//! is built as the JSON line the daemon would send and goes through [`decode`], so the mock drives
//! the same decoder as the socket. With [`Pace::Stepped`] nothing plays until the caller's
//! [`MockTransport::step`] or [`MockTransport::play_all`], so tests stay on their own thread; with
//! [`Pace::Realtime`] a task on the `core` runtime plays the queue with real delays.

use std::collections::{BTreeSet, HashMap, VecDeque};
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

use super::dto::{
    Agent, AgentId, AgentRun, AgentState, Attachment, AttachmentId, AttachmentKind, Author,
    AuthorKind, Binding, Channel, ClientMessageId, ContextUsage, InputId, Message, MessageId,
    MessageKind, MessagesPage, Mirror, Post, Posted, Role, RowKind, RunDetail, RunId, RunRow,
    RunStatus, RunSummary, Seq, StepRow, Surface, SurfaceId, SurfaceKind, SurfaceRun, ToolUseId,
    Usage, Wiring,
};
use super::frames::{
    AuthMode, Capabilities, ClientFrame, Frame, Gap, Hello, InputAccepted, RunFinished,
    RunSnapshot, RunStarted, StepText, TaskUpdate, ToolFinished, ToolStarted, decode,
};
use super::runtime::handle;
use super::transport::{ApiError, Connection, Transport};

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

#[derive(Debug, Clone)]
enum Media {
    Embedded(&'static [u8]),
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

    /// Applies the client frames sent so far (a `focus` sends snapshots right away).
    pub fn pump_control(&self) {
        self.lock().drain_control();
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

    fn connect(&self, since: Option<Seq>) -> BoxFuture<'static, Result<Connection, ApiError>> {
        let connection = self.lock().connect(since);
        ready(Ok(connection)).boxed()
    }
}

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
            next_message: 9000,
            next_input: 40,
        };
        world.seed_messages(base);
        world.seed_live_run();
        world.seed_voice(base);
        world
    }

    fn from_seed(seed: Seed, scenario: Scenario) -> World {
        let Seed {
            surfaces,
            agents,
            messages,
            runs,
            media: files,
        } = seed;
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
        World {
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
            next_message,
            next_input: 1,
        }
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
            ["agents"] => to_json(&self.agents_view()),
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
        let segments = segments(path);
        let ["attachments", id] = segments.as_slice() else {
            return Err(ApiError::NotFound);
        };
        let Ok(id) = id.parse::<i64>() else {
            return Err(ApiError::NotFound);
        };
        let Some(media) = self.media.get(&AttachmentId(id)) else {
            return Err(ApiError::NotFound);
        };
        match media {
            Media::Embedded(bytes) => Ok(bytes.to_vec()),
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
                to_json(&self.accept(surface, post)?)
            }
            ["runs", id, "interrupt"] => {
                self.interrupt(&RunId((*id).to_string()))?;
                Ok(Value::Null)
            }
            [..] => Err(ApiError::NotFound),
        }
    }

    fn accept(&mut self, surface: SurfaceId, post: Post) -> Result<Posted, ApiError> {
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
        if let Some(stored) = self.messages.last_mut() {
            stored.addressed_agent_id = post.addressed_agent_id;
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
            surfaces.push(surface);
        }
        surfaces
    }

    fn agents_view(&self) -> Vec<Agent> {
        let mut agents = Vec::new();
        for agent in &self.agents {
            let mut agent = agent.clone();
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
        to_json(&MessagesPage { messages, has_more })
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
    use crate::v3::client::Client;

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
