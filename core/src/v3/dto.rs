//! Wire shapes of the v3 REST bodies, field for field as the contract spells them.
//!
//! Every optional field defaults when absent, unknown fields are ignored, and every enum-valued
//! field has an `Unknown` variant, so a value this build does not know never fails a whole body.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

/// Identifies a surface (today a Telegram topic).
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::SurfaceId;
///
/// let id: SurfaceId = serde_json::from_str("7").unwrap();
/// assert_eq!(id, SurfaceId(7));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SurfaceId(pub i64);

/// Identifies an agent.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::AgentId;
///
/// let id: AgentId = serde_json::from_str("3").unwrap();
/// assert_eq!(id, AgentId(3));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentId(pub i64);

/// Identifies a message.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::MessageId;
///
/// let id: MessageId = serde_json::from_str("9192").unwrap();
/// assert_eq!(id, MessageId(9192));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageId(pub i64);

/// Identifies an input the daemon queued for an agent.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::InputId;
///
/// let id: InputId = serde_json::from_str("42").unwrap();
/// assert_eq!(id, InputId(42));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InputId(pub i64);

/// Identifies a run; a UUID string chosen by the agent.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::RunId;
///
/// let id: RunId = serde_json::from_str("\"6763eb02\"").unwrap();
/// assert_eq!(id, RunId("6763eb02".into()));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunId(pub String);

/// Identifies a tool call inside a run.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::ToolUseId;
///
/// let id: ToolUseId = serde_json::from_str("\"toolu_01\"").unwrap();
/// assert_eq!(id, ToolUseId("toolu_01".into()));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolUseId(pub String);

/// The client-chosen UUID that makes a post idempotent and matches the echoed message.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::ClientMessageId;
///
/// let first = ClientMessageId::random();
/// assert_ne!(first, ClientMessageId::random());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClientMessageId(pub String);

impl ClientMessageId {
    /// Returns a fresh random (v4) UUID.
    pub fn random() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

/// Positions a persisted event in the daemon's event log.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::Seq;
///
/// assert!(Seq(1201) > Seq(1200));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Seq(pub i64);

/// The kind of a surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKind {
    /// A shared channel; every surface in v3.0.
    Channel,
    /// A value this build does not know.
    #[serde(other)]
    Unknown,
}

/// How an agent is wired to a surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The agent answers every unaddressed message.
    Lead,
    /// The agent answers only when addressed.
    Mention,
    /// A value this build does not know.
    #[serde(other)]
    Unknown,
}

/// A channel a message can be typed in or a surface can be bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// Telegram.
    Telegram,
    /// A v3 desktop client.
    Desktop,
    /// A value this build does not know.
    #[serde(other)]
    Unknown,
}

/// What a surface binding mirrors to its channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mirror {
    /// Only agent-authored messages are mirrored.
    AgentOnly,
    /// A value this build does not know.
    #[serde(other)]
    Unknown,
}

/// One agent wired to a surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wiring {
    /// The wired agent.
    pub agent_id: AgentId,
    /// Whether it leads the surface or answers on mention.
    pub role: Role,
    /// Whether it hears the messages it does not answer.
    #[serde(default)]
    pub listens: bool,
}

/// One external channel a surface is bound to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    /// The channel.
    pub channel: Channel,
    /// The channel's own address of the surface, `"{chat}:{topic}"` for Telegram.
    pub external_id: String,
    /// What is mirrored to the channel.
    pub mirror: Mirror,
}

/// The live run on a surface, as `GET /surfaces` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceRun {
    /// The run.
    pub run_id: RunId,
    /// The agent running it.
    pub agent_id: AgentId,
}

/// One entry of `GET /surfaces`.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::{Surface, SurfaceId};
///
/// let json = r#"{"id": 1, "kind": "channel", "name": "General", "sort_order": 0}"#;
/// let surface: Surface = serde_json::from_str(json).unwrap();
/// assert_eq!(surface.id, SurfaceId(1));
/// assert!(surface.agents.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Surface {
    /// The surface's identifier.
    pub id: SurfaceId,
    /// Its kind.
    pub kind: SurfaceKind,
    /// Its display name.
    pub name: String,
    /// Its position in the sidebar.
    #[serde(default)]
    pub sort_order: i64,
    /// When the newest message on it was written.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_message_at: Option<OffsetDateTime>,
    /// The agent that answers unaddressed messages.
    #[serde(default)]
    pub lead_agent_id: Option<AgentId>,
    /// Every agent wired to it.
    #[serde(default)]
    pub agents: Vec<Wiring>,
    /// Every external channel it is bound to.
    #[serde(default)]
    pub bindings: Vec<Binding>,
    /// The run stamped with this surface, while one is live.
    #[serde(default)]
    pub live_run: Option<SurfaceRun>,
}

/// Whether an agent is running a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    /// No live run.
    Idle,
    /// A live run somewhere.
    Running,
    /// A value this build does not know.
    #[serde(other)]
    Unknown,
}

/// The live run of an agent, as `GET /agents` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRun {
    /// The run.
    pub run_id: RunId,
    /// The surface it is stamped with.
    pub surface_id: SurfaceId,
}

/// One entry of `GET /agents`.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::{Agent, AgentState};
///
/// let json = r#"{"id": 1, "name": "Jarvis", "ident": "tuclaw_bot", "state": "idle"}"#;
/// let agent: Agent = serde_json::from_str(json).unwrap();
/// assert_eq!(agent.state, AgentState::Idle);
/// assert!(agent.live_run.is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    /// The agent's identifier.
    pub id: AgentId,
    /// Its display name.
    pub name: String,
    /// What an `@mention` of it resolves to.
    pub ident: String,
    /// Its description.
    #[serde(default)]
    pub description: String,
    /// Its own Telegram bot, when it has one.
    #[serde(default)]
    pub bot_username: Option<String>,
    /// The effective model spec; empty for the SDK default.
    #[serde(default)]
    pub model: String,
    /// Whether it is running a turn.
    pub state: AgentState,
    /// Its live run, while running.
    #[serde(default)]
    pub live_run: Option<AgentRun>,
    /// The surface its session lives on.
    #[serde(default)]
    pub home_surface_id: Option<SurfaceId>,
}

/// The kind of a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    /// Typed by the user.
    User,
    /// An agent's answer to a run.
    Answer,
    /// An agent's post outside a run's answer.
    Post,
    /// A notice the system wrote.
    Notice,
    /// One agent addressing another.
    A2a,
    /// A value this build does not know.
    #[serde(other)]
    Unknown,
}

/// Who wrote a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorKind {
    /// The user.
    User,
    /// An agent.
    Agent,
    /// The daemon.
    System,
    /// A value this build does not know.
    #[serde(other)]
    Unknown,
}

/// The author of a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Author {
    /// Who wrote it.
    pub kind: AuthorKind,
    /// The agent, for agent messages and notices posted for one.
    #[serde(default)]
    pub agent_id: Option<AgentId>,
}

/// The state of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Live.
    Running,
    /// Finished successfully.
    Ok,
    /// Finished with an error.
    Error,
    /// Stopped by an interrupt.
    Interrupted,
    /// A value this build does not know.
    #[serde(other)]
    Unknown,
}

/// The counts a message carries about the run that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSummary {
    /// The run's state.
    pub status: RunStatus,
    /// How many steps it has.
    #[serde(default)]
    pub step_count: u32,
    /// How many of them are tool calls.
    #[serde(default)]
    pub tool_count: u32,
    /// How long it took, or has taken so far.
    #[serde(default)]
    pub duration_ms: u64,
}

/// One message of a surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// The message's identifier.
    pub id: MessageId,
    /// The surface it belongs to.
    pub surface_id: SurfaceId,
    /// Its kind.
    pub kind: MessageKind,
    /// Who wrote it.
    pub author: Author,
    /// The agent it was addressed to.
    #[serde(default)]
    pub addressed_agent_id: Option<AgentId>,
    /// The message it replies to.
    #[serde(default)]
    pub reply_to_message_id: Option<MessageId>,
    /// The Markdown text, verbatim.
    #[serde(default)]
    pub text: String,
    /// The run that produced it or that it started.
    #[serde(default)]
    pub run_id: Option<RunId>,
    /// What caused it: `user`, `a2a`, `scheduled`, ...
    #[serde(default)]
    pub origin: String,
    /// Where a user message was typed.
    #[serde(default)]
    pub channel: Option<Channel>,
    /// The id a v3 client posted it with.
    #[serde(default)]
    pub client_message_id: Option<ClientMessageId>,
    /// When it was written.
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// The counts of its run.
    #[serde(default)]
    pub run_summary: Option<RunSummary>,
}

/// One page of `GET /surfaces/{id}/messages`, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessagesPage {
    /// The messages, oldest first.
    pub messages: Vec<Message>,
    /// Whether older messages exist.
    #[serde(default)]
    pub has_more: bool,
}

/// The body of `POST /surfaces/{id}/messages`.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::{ClientMessageId, Post};
///
/// let post = Post {
///     text: "hello".into(),
///     addressed_agent_id: None,
///     client_message_id: ClientMessageId("8b0c".into()),
/// };
/// let json = serde_json::to_value(&post).unwrap();
/// assert_eq!(json["addressed_agent_id"], serde_json::Value::Null);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Post {
    /// The Markdown text.
    pub text: String,
    /// The agent the message mentions, when it leads with one.
    pub addressed_agent_id: Option<AgentId>,
    /// The idempotency key.
    pub client_message_id: ClientMessageId,
}

/// The `202` answer to a post.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Posted {
    /// The user message written.
    pub message_id: MessageId,
    /// The input queued for the agent; `None` when the message was stored but its wake could not
    /// be queued (the daemon then posts an error notice on the surface).
    pub input_id: Option<InputId>,
    /// The agent that will answer.
    pub agent_id: AgentId,
}

/// Token counts of one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Input tokens.
    #[serde(default)]
    pub input_tokens: u64,
    /// Output tokens.
    #[serde(default)]
    pub output_tokens: u64,
    /// Tokens read from the prompt cache.
    #[serde(default)]
    pub cache_read_tokens: u64,
    /// Tokens written to the prompt cache.
    #[serde(default)]
    pub cache_creation_tokens: u64,
    /// Model turns the run took; reported on `run.finished`.
    #[serde(default)]
    pub num_turns: u64,
    /// Time spent in API calls; reported on `run.finished`.
    #[serde(default)]
    pub duration_api_ms: u64,
}

/// The context window after a run, as `run.finished` reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextUsage {
    /// Tokens in the window.
    #[serde(default)]
    pub total_tokens: u64,
    /// The window's size.
    #[serde(default)]
    pub max_tokens: u64,
    /// How full the window is, 0-100.
    #[serde(default)]
    pub percentage: f64,
    /// The model measured.
    #[serde(default)]
    pub model: String,
}

/// The context window after a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextWindow {
    /// Tokens in the window.
    #[serde(default)]
    pub tokens: u64,
    /// The window's size.
    #[serde(default)]
    pub max_tokens: u64,
    /// The model measured.
    #[serde(default)]
    pub model: String,
}

/// A run as `GET /runs/{id}` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRow {
    /// The run's identifier.
    pub id: RunId,
    /// The agent that ran it.
    pub agent_id: AgentId,
    /// The surface it is stamped with.
    #[serde(default)]
    pub surface_id: Option<SurfaceId>,
    /// What caused it.
    #[serde(default)]
    pub origin: String,
    /// Its metrics kind.
    #[serde(default)]
    pub kind: String,
    /// Its state.
    pub status: RunStatus,
    /// Why it ended.
    #[serde(default)]
    pub terminal_reason: Option<String>,
    /// The error it ended with.
    #[serde(default)]
    pub error: Option<String>,
    /// When it started.
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    /// When it ended.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub finished_at: Option<OffsetDateTime>,
    /// Its token counts.
    #[serde(default)]
    pub usage: Option<Usage>,
    /// The context window after it.
    #[serde(default)]
    pub context: Option<ContextWindow>,
}

/// The kind of a step row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
    /// A finished text segment.
    Text,
    /// A tool call.
    Tool,
    /// A background task update.
    Task,
    /// A status line.
    Status,
    /// A value this build does not know.
    #[serde(other)]
    Unknown,
}

/// One step of a run, the contract's generic row for every kind.
///
/// Per kind: `text` carries the segment in `output`; `tool` carries `tool_use_id`, `name`,
/// `input`, `output` and `status`; `task` carries the task type in `name`, its state in `status`
/// and the JSON body in `output`; `status` carries the status in `name` and the detail in `output`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepRow {
    /// The step's position in its run.
    pub seq: i64,
    /// Its kind.
    pub kind: RowKind,
    /// The tool call, for tool steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<ToolUseId>,
    /// The tool, task type or status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The tool's JSON input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    /// The text, result, task body or detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// The tool's or task's state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// When the step started.
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    /// When the step finished.
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    pub finished_at: Option<OffsetDateTime>,
}

/// The body of `GET /runs/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunDetail {
    /// The run.
    pub run: RunRow,
    /// Its steps in order.
    #[serde(default)]
    pub steps: Vec<StepRow>,
}

/// The code and text of an error envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorDetail {
    /// The snake_case code: `unauthorized`, `not_found`, `invalid_request`, `conflict`, `unavailable`.
    pub code: String,
    /// The human text.
    #[serde(default)]
    pub message: String,
}

/// The body of every non-2xx answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    /// The error.
    pub error: ErrorDetail,
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use time::macros::datetime;
    use uuid::Uuid;

    use super::*;

    const SURFACES: &str = include_str!("../../testdata/v3/surfaces.json");
    const AGENTS: &str = include_str!("../../testdata/v3/agents.json");
    const MESSAGES_PAGE: &str = include_str!("../../testdata/v3/messages_page.json");
    const POST_MESSAGE: &str = include_str!("../../testdata/v3/post_message.json");
    const RUN: &str = include_str!("../../testdata/v3/run.json");

    #[test]
    fn surfaces_decode_with_wiring_bindings_and_live_run() {
        let surfaces: Vec<Surface> = serde_json::from_str(SURFACES).unwrap();
        assert_eq!(surfaces.len(), 2);
        let Surface {
            id,
            kind,
            name,
            sort_order,
            last_message_at,
            lead_agent_id,
            agents,
            bindings,
            live_run,
        } = &surfaces[0];
        assert_eq!(*id, SurfaceId(1));
        assert_eq!(*kind, SurfaceKind::Channel);
        assert_eq!(name, "General");
        assert_eq!(*sort_order, 0);
        assert_eq!(*last_message_at, Some(datetime!(2026-10-03 15:26:00 UTC)));
        assert_eq!(*lead_agent_id, Some(AgentId(1)));
        assert_eq!(
            agents,
            &vec![
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
            ]
        );
        assert_eq!(
            bindings,
            &vec![Binding {
                channel: Channel::Telegram,
                external_id: "-1003614621196:0".into(),
                mirror: Mirror::AgentOnly,
            }]
        );
        assert_eq!(
            live_run,
            &Some(SurfaceRun {
                run_id: RunId("6763eb02-7f3e-4c4d-9b1a-2f0c5d8e9a11".into()),
                agent_id: AgentId(1),
            })
        );
        assert_eq!(surfaces[1].last_message_at, None);
        assert_eq!(surfaces[1].live_run, None);
    }

    #[test]
    fn agents_decode_idle_and_running() {
        let agents: Vec<Agent> = serde_json::from_str(AGENTS).unwrap();
        assert_eq!(agents[0].state, AgentState::Idle);
        assert_eq!(agents[0].live_run, None);
        assert_eq!(agents[0].bot_username.as_deref(), Some("tuclaw_bot"));
        assert_eq!(agents[0].home_surface_id, Some(SurfaceId(1)));
        assert_eq!(agents[1].state, AgentState::Running);
        assert_eq!(
            agents[1].live_run,
            Some(AgentRun {
                run_id: RunId("0b9d2c4e-5a61-4f7e-8c3d-1e2f3a4b5c6d".into()),
                surface_id: SurfaceId(4),
            })
        );
    }

    #[test]
    fn messages_page_decodes_user_row_and_answer_row() {
        let page: MessagesPage = serde_json::from_str(MESSAGES_PAGE).unwrap();
        assert!(page.has_more);
        let user = &page.messages[0];
        assert_eq!(user.kind, MessageKind::User);
        assert_eq!(user.author.kind, AuthorKind::User);
        assert_eq!(user.author.agent_id, None);
        assert_eq!(user.channel, Some(Channel::Desktop));
        assert_eq!(
            user.client_message_id,
            Some(ClientMessageId(
                "8b0c4f2e-1d7a-4c39-9e65-3a2b1c0d9f87".into()
            ))
        );
        assert_eq!(user.run_summary, None);
        let answer = &page.messages[1];
        assert_eq!(answer.kind, MessageKind::Answer);
        assert_eq!(answer.author.agent_id, Some(AgentId(1)));
        assert_eq!(answer.channel, None);
        assert_eq!(answer.created_at, datetime!(2026-10-03 15:26:13 UTC));
        assert!(answer.text.starts_with("## Лисички"));
        assert_eq!(
            answer.run_summary,
            Some(RunSummary {
                status: RunStatus::Ok,
                step_count: 6,
                tool_count: 1,
                duration_ms: 13029,
            })
        );
    }

    #[test]
    fn post_round_trips_through_the_fixture() {
        let post: Post = serde_json::from_str(POST_MESSAGE).unwrap();
        assert_eq!(post.addressed_agent_id, None);
        let fixture: Value = serde_json::from_str(POST_MESSAGE).unwrap();
        assert_eq!(serde_json::to_value(&post).unwrap(), fixture);
    }

    #[test]
    fn run_detail_decodes_run_and_generic_steps() {
        let detail: RunDetail = serde_json::from_str(RUN).unwrap();
        let RunDetail { run, steps } = detail;
        assert_eq!(run.status, RunStatus::Ok);
        assert_eq!(run.terminal_reason.as_deref(), Some("success"));
        assert_eq!(run.finished_at, Some(datetime!(2026-10-03 15:26:13 UTC)));
        assert_eq!(run.usage.map(|usage| usage.output_tokens), Some(412));
        assert_eq!(run.context.map(|context| context.tokens), Some(323_968));
        assert_eq!(steps[0].kind, RowKind::Text);
        assert_eq!(
            steps[0].output.as_deref(),
            Some("Посмотрю, что есть в заметках.")
        );
        assert_eq!(steps[0].finished_at, None);
        assert_eq!(steps[1].kind, RowKind::Tool);
        assert_eq!(
            steps[1].tool_use_id,
            Some(ToolUseId("toolu_01A2b3C4d5E6f7G8h9".into()))
        );
        assert_eq!(
            steps[1].input,
            Some(json!({"command": "rg -i лисич ~/notes"}))
        );
        assert_eq!(steps[1].status.as_deref(), Some("ok"));
    }

    #[test]
    fn unknown_enum_values_fall_back_without_failing_the_body() {
        let page = json!({
            "messages": [{
                "id": 1, "surface_id": 1, "kind": "card", "author": {"kind": "bot"},
                "channel": "slack", "created_at": "2026-10-03T15:26:00Z",
                "run_summary": {"status": "paused"}
            }]
        });
        let page: MessagesPage = serde_json::from_value(page).unwrap();
        let message = &page.messages[0];
        assert_eq!(message.kind, MessageKind::Unknown);
        assert_eq!(message.author.kind, AuthorKind::Unknown);
        assert_eq!(message.channel, Some(Channel::Unknown));
        assert_eq!(
            message.run_summary.map(|summary| summary.status),
            Some(RunStatus::Unknown)
        );
        assert!(!page.has_more);

        let row = json!({"seq": 1, "kind": "thinking", "started_at": "2026-10-03T15:26:00Z"});
        let row: StepRow = serde_json::from_value(row).unwrap();
        assert_eq!(row.kind, RowKind::Unknown);

        let surface = json!({
            "id": 1, "kind": "dm", "name": "x",
            "agents": [{"agent_id": 1, "role": "observer"}],
            "bindings": [{"channel": "matrix", "external_id": "!a", "mirror": "everything"}]
        });
        let surface: Surface = serde_json::from_value(surface).unwrap();
        assert_eq!(surface.kind, SurfaceKind::Unknown);
        assert_eq!(surface.agents[0].role, Role::Unknown);
        assert_eq!(surface.bindings[0].channel, Channel::Unknown);
        assert_eq!(surface.bindings[0].mirror, Mirror::Unknown);

        let agent = json!({"id": 1, "name": "x", "ident": "x", "state": "sleeping"});
        let agent: Agent = serde_json::from_value(agent).unwrap();
        assert_eq!(agent.state, AgentState::Unknown);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let posted =
            json!({"message_id": 9193, "input_id": 42, "agent_id": 1, "thread_root_id": 5});
        let posted: Posted = serde_json::from_value(posted).unwrap();
        assert_eq!(
            posted,
            Posted {
                message_id: MessageId(9193),
                input_id: Some(InputId(42)),
                agent_id: AgentId(1),
            }
        );
    }

    #[test]
    fn a_post_whose_wake_failed_has_no_input() {
        let posted = json!({"message_id": 9193, "input_id": null, "agent_id": 1});
        let posted: Posted = serde_json::from_value(posted).unwrap();
        assert_eq!(posted.input_id, None);
    }

    #[test]
    fn error_envelope_decodes() {
        let body = json!({"error": {"code": "conflict", "message": "run is not live"}});
        let ErrorBody { error } = serde_json::from_value(body).unwrap();
        assert_eq!(error.code, "conflict");
        assert_eq!(error.message, "run is not live");
    }

    #[test]
    fn a_malformed_timestamp_fails_the_body() {
        let row = json!({"seq": 1, "kind": "text", "started_at": "yesterday"});
        assert!(serde_json::from_value::<StepRow>(row).is_err());
    }

    #[test]
    fn random_client_message_ids_are_unique_uuids() {
        let ClientMessageId(first) = ClientMessageId::random();
        let ClientMessageId(second) = ClientMessageId::random();
        assert!(Uuid::parse_str(&first).is_ok());
        assert!(Uuid::parse_str(&second).is_ok());
        assert_ne!(first, second);
    }
}
