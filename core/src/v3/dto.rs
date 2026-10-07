//! Wire shapes of the v3 REST bodies, field for field as the contract spells them.
//!
//! Every optional field defaults when absent, unknown fields are ignored, and every enum-valued
//! field has an `Unknown` variant, so a value this build does not know never fails a whole body.
//! A defaulted string field also reads `null` as empty, since the daemon writes `null` for values
//! older rows never had.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

pub(crate) fn null_as_empty<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

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

/// Identifies an attachment of a message (v3.1 draft).
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::AttachmentId;
///
/// let id: AttachmentId = serde_json::from_str("5").unwrap();
/// assert_eq!(id, AttachmentId(5));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AttachmentId(pub i64);

/// Where an avatar's bytes live: a versioned path under the daemon's base URL, fetched with the
/// same token as every v3 call; a new picture is a new URL.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::AvatarUrl;
///
/// let url: AvatarUrl = serde_json::from_str(r#""/api/v3/agents/7/avatar?v=ab12""#).unwrap();
/// assert_eq!(url, AvatarUrl("/api/v3/agents/7/avatar?v=ab12".into()));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AvatarUrl(pub String);

/// The image formats an avatar can be stored in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageKind {
    /// `image/png`.
    Png,
    /// `image/jpeg`.
    Jpeg,
    /// `image/webp`.
    Webp,
}

impl ImageKind {
    /// Returns the MIME type sent as the upload's `Content-Type`.
    pub fn mime(self) -> &'static str {
        match self {
            ImageKind::Png => "image/png",
            ImageKind::Jpeg => "image/jpeg",
            ImageKind::Webp => "image/webp",
        }
    }

    /// Recognizes an image by its leading bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuclaw_core::v3::ImageKind;
    ///
    /// assert_eq!(ImageKind::sniff(b"\x89PNG\r\n\x1a\n...."), Some(ImageKind::Png));
    /// assert_eq!(ImageKind::sniff(b"GIF89a"), None);
    /// ```
    pub fn sniff(bytes: &[u8]) -> Option<ImageKind> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Some(ImageKind::Png);
        }
        if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            return Some(ImageKind::Jpeg);
        }
        if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
            return Some(ImageKind::Webp);
        }
        None
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
    /// Its own name in its channel, e.g. the Telegram topic's.
    #[serde(default)]
    pub topic_name: String,
    /// The name the user gave it in a client; [`None`] when it uses the topic's.
    #[serde(default)]
    pub display_name: Option<String>,
    /// The group it is filed under in the sidebar.
    #[serde(default)]
    pub group_id: Option<GroupId>,
    /// When it was archived; an archived surface is left out of the sidebar.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub archived_at: Option<OffsetDateTime>,
    /// The newest message read on it; `None` before anything was read.
    #[serde(default)]
    pub last_read_message_id: Option<MessageId>,
    /// The messages after the read cursor not written by the user.
    #[serde(default)]
    pub unread: u32,
    /// Those of them that answer the user: agent answers and posts of runs the user started.
    #[serde(default)]
    pub unread_replies: u32,
    /// Whether the user marked it unread; cleared by the next read.
    #[serde(default)]
    pub marked_unread: bool,
}

/// Identifies a sidebar group.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::GroupId;
///
/// let id: GroupId = serde_json::from_str("3").unwrap();
/// assert_eq!(id, GroupId(3));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GroupId(pub i64);

/// A named section of the sidebar that surfaces are filed under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    /// The group's identifier.
    pub id: GroupId,
    /// Its title.
    pub name: String,
    /// The emoji drawn before its title, if any.
    #[serde(default)]
    pub emoji: Option<String>,
    /// Its position among the groups.
    #[serde(default)]
    pub sort_order: i64,
}

/// A field a patch leaves alone, sets, or clears to `null`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change<T> {
    /// Leaves the field as it is; the key is not sent.
    Keep,
    /// Sets the field.
    Set(T),
    /// Clears the field; sent as `null`.
    Clear,
}

impl<T> Change<T> {
    fn is_keep(&self) -> bool {
        match self {
            Change::Keep => true,
            Change::Set(_) => false,
            Change::Clear => false,
        }
    }
}

impl<T: Serialize> Serialize for Change<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Change::Keep => serializer.serialize_none(),
            Change::Set(value) => value.serialize(serializer),
            Change::Clear => serializer.serialize_none(),
        }
    }
}

/// The body of `PATCH /surfaces/{id}`; kept fields are not sent.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::{Change, GroupId, SurfacePatch};
///
/// let patch = SurfacePatch {
///     display_name: Change::Clear,
///     archived: None,
///     group_id: Change::Set(GroupId(2)),
/// };
/// assert_eq!(
///     serde_json::to_string(&patch).unwrap(),
///     r#"{"display_name":null,"group_id":2}"#
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SurfacePatch {
    /// The name shown in clients; cleared, the topic's name shows again.
    #[serde(skip_serializing_if = "Change::is_keep")]
    pub display_name: Change<String>,
    /// Archives or restores the surface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    /// Files the surface under a group; cleared, it is ungrouped.
    #[serde(skip_serializing_if = "Change::is_keep")]
    pub group_id: Change<GroupId>,
}

/// The body of `POST /groups`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewGroup {
    /// Its title.
    pub name: String,
    /// The emoji drawn before its title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
}

/// The body of `PATCH /groups/{id}`; kept fields are not sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GroupPatch {
    /// The new title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The new emoji, or none.
    #[serde(skip_serializing_if = "Change::is_keep")]
    pub emoji: Change<String>,
    /// The new position among the groups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<i64>,
}

/// One surface's place in `PUT /surfaces/order`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    /// The surface.
    pub id: SurfaceId,
    /// The group it is filed under, or none.
    pub group_id: Option<GroupId>,
    /// Its position within that group.
    pub sort_order: i64,
}

/// The answer to marking a surface read or unread: its read state now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadAnswer {
    /// The newest message read on the surface; [`None`] before anything was read.
    pub last_read_message_id: Option<MessageId>,
    /// The messages still unread on it.
    pub unread: u32,
    /// Those of them that answer the user.
    #[serde(default)]
    pub unread_replies: u32,
    /// Whether it is marked unread.
    #[serde(default)]
    pub marked_unread: bool,
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
    #[serde(default, deserialize_with = "null_as_empty")]
    pub description: String,
    /// Its own Telegram bot, when it has one.
    #[serde(default)]
    pub bot_username: Option<String>,
    /// The effective model spec; empty for the SDK default.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub model: String,
    /// Whether it is running a turn.
    pub state: AgentState,
    /// Its live run, while running.
    #[serde(default)]
    pub live_run: Option<AgentRun>,
    /// The surface its session lives on.
    #[serde(default)]
    pub home_surface_id: Option<SurfaceId>,
    /// Its avatar; `None` draws its initials.
    #[serde(default)]
    pub avatar_url: Option<AvatarUrl>,
}

/// The person using the client, as `GET /me` answers.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::Me;
///
/// let me: Me = serde_json::from_str(r#"{"name": "You", "avatar_url": null}"#).unwrap();
/// assert!(me.avatar_url.is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Me {
    /// The display name; "You" when the daemon knows none.
    pub name: String,
    /// A few words about the user; empty when unset.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub description: String,
    /// The avatar; `None` draws the initials.
    #[serde(default)]
    pub avatar_url: Option<AvatarUrl>,
}

/// The answer to an avatar upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvatarSet {
    /// Where the stored picture is served from now.
    pub avatar_url: AvatarUrl,
}

/// How `PATCH /agents/{id}` changes an agent's model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelChange {
    /// Leaves the model as it is.
    Keep,
    /// Sets a model spec such as `opus[1m]:medium`.
    Set(String),
    /// Clears the agent's own model, so it runs on the daemon default.
    Default,
}

impl ModelChange {
    fn is_keep(&self) -> bool {
        match self {
            ModelChange::Keep => true,
            ModelChange::Set(_) => false,
            ModelChange::Default => false,
        }
    }
}

impl Serialize for ModelChange {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            ModelChange::Keep => serializer.serialize_none(),
            ModelChange::Set(model) => serializer.serialize_str(model),
            ModelChange::Default => serializer.serialize_none(),
        }
    }
}

/// The body of `PATCH /agents/{id}`; absent fields stay as they are.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::{AgentPatch, ModelChange};
///
/// let reset = AgentPatch { description: None, model: ModelChange::Default };
/// assert_eq!(serde_json::to_string(&reset).unwrap(), r#"{"model":null}"#);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentPatch {
    /// The new description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The model change.
    #[serde(skip_serializing_if = "ModelChange::is_keep")]
    pub model: ModelChange,
}

/// The body of `PUT /surfaces/{id}/agents/{agent}`: how the agent is wired there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WiringChange {
    /// Lead or answers on mention; a new lead demotes the old one.
    pub role: Role,
    /// Whether it hears the messages it does not answer.
    pub listens: bool,
}

/// The body of `PATCH /me`; absent fields stay as they are.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::MePatch;
///
/// let patch = MePatch { name: None, description: Some("Builds tuclaw".into()) };
/// assert_eq!(serde_json::to_string(&patch).unwrap(), r#"{"description":"Builds tuclaw"}"#);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MePatch {
    /// The new display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The new description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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

/// What an attachment holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    /// A voice recording the message's text transcribes.
    Voice,
    /// A value this build does not know.
    #[serde(other)]
    Unknown,
}

/// A file attached to a message, fetched through `GET /attachments/{id}` (v3.1 draft).
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::{Attachment, AttachmentKind};
///
/// let json = r#"{"id": 5, "kind": "voice", "mime": "audio/ogg", "size_bytes": 1200, "duration_ms": 3000}"#;
/// let attachment: Attachment = serde_json::from_str(json).unwrap();
/// assert_eq!(attachment.kind, AttachmentKind::Voice);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// The attachment's identifier.
    pub id: AttachmentId,
    /// What it holds.
    pub kind: AttachmentKind,
    /// Its media type, e.g. `audio/ogg` or `audio/mp4`.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub mime: String,
    /// Its size.
    #[serde(default)]
    pub size_bytes: u64,
    /// Its length, when known.
    #[serde(default)]
    pub duration_ms: Option<u64>,
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
    #[serde(default, deserialize_with = "null_as_empty")]
    pub text: String,
    /// The run that produced it or that it started.
    #[serde(default)]
    pub run_id: Option<RunId>,
    /// What caused it: `user`, `a2a`, `scheduled`, ...; empty on rows written before step B2,
    /// where the daemon sends `null`.
    #[serde(default, deserialize_with = "null_as_empty")]
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
    /// Its attachments (v3.1 draft); empty when it has none.
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    /// The one-tap replies an agent attached to its answer (v3.12); [`None`] without any.
    #[serde(default)]
    pub suggested_replies: Option<SuggestedReplies>,
}

/// The one-tap replies of an answer (v3.12).
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::SuggestedReplies;
///
/// let replies: SuggestedReplies =
///     serde_json::from_str(r#"{"options": ["Do it", "Skip"], "open": true, "chosen": null}"#)
///         .unwrap();
/// assert!(replies.open);
/// assert_eq!(replies.options, vec!["Do it".to_string(), "Skip".to_string()]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestedReplies {
    /// One to three options, in the agent's order.
    pub options: Vec<String>,
    /// Whether nothing was written on the surface after the answer yet.
    pub open: bool,
    /// The option the user picked, when the first message after the answer was one.
    #[serde(default)]
    pub chosen: Option<String>,
}

/// The body of `POST /messages/{id}/reply` (v3.12).
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::{ClientMessageId, ReplyPost};
///
/// let tap = ReplyPost {
///     option: "Do it".into(),
///     client_message_id: ClientMessageId("8b0c4f2e-1d7a-4c39-9e65-3a2b1c0d9f87".into()),
/// };
/// let json = serde_json::to_value(&tap).unwrap();
/// assert_eq!(json["option"], "Do it");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyPost {
    /// The option tapped, verbatim.
    pub option: String,
    /// The idempotency key.
    pub client_message_id: ClientMessageId,
}

/// One page of `GET /surfaces/{id}/messages`, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessagesPage {
    /// The messages, oldest first.
    pub messages: Vec<Message>,
    /// Whether older messages exist.
    #[serde(default)]
    pub has_more: bool,
    /// The automation fires within the page's time range (the last 30 days only).
    #[serde(default)]
    pub automations: Vec<FireMark>,
}

/// Identifies an automation (a scheduled task); the daemon names tasks with strings.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::TaskId;
///
/// let id: TaskId = serde_json::from_str(r#""task-1759-a1b2""#).unwrap();
/// assert_eq!(id, TaskId("task-1759-a1b2".into()));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(pub String);

/// What fires an automation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleKind {
    /// Once, at a moment.
    Once,
    /// On a cron expression.
    Cron,
    /// Every interval.
    Interval,
    /// Repeatedly until the agent answers `[DONE]`.
    PollUntil,
    /// On a matching NATS event.
    Event,
    /// A kind this build does not know.
    #[serde(other)]
    Unknown,
}

/// When an automation fires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    /// The trigger kind.
    #[serde(rename = "type")]
    pub kind: ScheduleKind,
    /// The kind's value: a timestamp, a cron expression, a duration or a subject filter.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub value: String,
}

/// Where an automation is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Firing as scheduled.
    Active,
    /// Paused by the user.
    Paused,
    /// Done: a one-shot fired or its window ended.
    Completed,
    /// Cancelled.
    Cancelled,
    /// A status this build does not know.
    #[serde(other)]
    Unknown,
}

/// How one fire of an automation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The agent ran and answered.
    Ran,
    /// The agent ran and stayed silent.
    Silent,
    /// The condition said there was nothing to do.
    Skipped,
    /// The fire failed.
    Failed,
    /// An outcome this build does not know.
    #[serde(other)]
    Unknown,
}

/// One automation, as `GET /tasks` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// The task's identifier.
    pub id: TaskId,
    /// The agent that runs it; `None` when its session is gone.
    #[serde(default)]
    pub agent_id: Option<AgentId>,
    /// The surface it reports to.
    #[serde(default)]
    pub surface_id: Option<SurfaceId>,
    /// What the agent is asked.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub prompt: String,
    /// When it fires.
    pub schedule: Schedule,
    /// Whether an event task stays subscribed after a fire.
    #[serde(default)]
    pub recurring: bool,
    /// The pre-check command, when it has one.
    #[serde(default)]
    pub condition: Option<String>,
    /// Its status.
    pub status: TaskStatus,
    /// When it fires next.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub next_run_at: Option<OffsetDateTime>,
    /// When it last fired.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_run_at: Option<OffsetDateTime>,
    /// How its newest attempt ended.
    #[serde(default)]
    pub last_outcome: Option<Outcome>,
    /// The start of its activity window.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub active_from: Option<OffsetDateTime>,
    /// The end of its activity window.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub active_until: Option<OffsetDateTime>,
    /// When it was created.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub created_at: Option<OffsetDateTime>,
}

/// One attempt of an automation, as `GET /tasks/{id}/runs` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRun {
    /// When it ran.
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    /// How it ended: ran, failed or skipped.
    pub outcome: Outcome,
    /// How long it took.
    #[serde(default)]
    pub duration_ms: u64,
    /// The failure, when it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One fire of an automation on a surface: a `task.fired` event or a page's mark.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FireMark {
    /// The automation.
    pub task_id: TaskId,
    /// When it fired.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub at: Option<OffsetDateTime>,
    /// How it ended.
    pub outcome: Outcome,
    /// The run it started, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    /// The message it posted (its answer, or its failure notice), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<MessageId>,
    /// The failure, when it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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

/// The audio containers a voice message can be uploaded in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioKind {
    /// AAC in an MPEG-4 container, `audio/mp4`.
    M4a,
    /// Opus in Ogg, `audio/ogg`.
    Ogg,
}

impl AudioKind {
    /// Returns the MIME type sent as the upload's `Content-Type`.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuclaw_core::v3::AudioKind;
    ///
    /// assert_eq!(AudioKind::M4a.mime(), "audio/mp4");
    /// ```
    pub fn mime(self) -> &'static str {
        match self {
            AudioKind::M4a => "audio/mp4",
            AudioKind::Ogg => "audio/ogg",
        }
    }
}

/// A recorded voice message to post into a surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoicePost {
    /// The recording's container.
    pub kind: AudioKind,
    /// The encoded recording.
    pub bytes: Vec<u8>,
    /// The agent the message is addressed to, if any.
    pub addressed_agent_id: Option<AgentId>,
    /// The idempotency key.
    pub client_message_id: ClientMessageId,
}

/// The `202` answer to a post.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Posted {
    /// The user message written.
    pub message_id: MessageId,
    /// The input queued for the agent; `None` when the message was stored but not queued (the
    /// daemon posted an error notice, or restarted before queuing). Retrying with the same
    /// `client_message_id` queues nothing; a retry needs a new one.
    pub input_id: Option<InputId>,
    /// The agent actually woken; `None` exactly when `input_id` is.
    pub agent_id: Option<AgentId>,
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
    #[serde(default, deserialize_with = "null_as_empty")]
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
    #[serde(default, deserialize_with = "null_as_empty")]
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
    #[serde(default, deserialize_with = "null_as_empty")]
    pub origin: String,
    /// Its metrics kind.
    #[serde(default, deserialize_with = "null_as_empty")]
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
    #[serde(default, deserialize_with = "null_as_empty")]
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
    const POSTED: &str = include_str!("../../testdata/v3/posted.json");
    const POSTED_WITHOUT_INPUT: &str = include_str!("../../testdata/v3/posted_without_input.json");
    const ERROR: &str = include_str!("../../testdata/v3/error.json");
    const ME: &str = include_str!("../../testdata/v3/me.json");
    const TASKS: &str = include_str!("../../testdata/v3/tasks.json");
    const TASK_RUNS: &str = include_str!("../../testdata/v3/task_runs.json");

    #[test]
    fn automations_and_their_runs_decode() {
        let tasks: Vec<Task> = serde_json::from_str(TASKS).unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(
            tasks[0].id,
            TaskId("task-1759500000000000000-1a2b3c4d".into())
        );
        assert_eq!(tasks[0].schedule.kind, ScheduleKind::Cron);
        assert_eq!(tasks[0].last_outcome, Some(Outcome::Skipped));
        assert_eq!(tasks[0].condition.as_deref(), Some("check-feeds.sh"));
        assert_eq!(tasks[1].agent_id, None);
        assert_eq!(tasks[1].status, TaskStatus::Paused);
        assert!(tasks[1].recurring);
        assert!(tasks[1].active_until.is_some());
        let runs: Vec<TaskRun> = serde_json::from_str(TASK_RUNS).unwrap();
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[1].outcome, Outcome::Failed);
        assert_eq!(runs[1].error.as_deref(), Some("agent silent"));
        assert_eq!(runs[2].error, None);
    }

    #[test]
    fn a_page_carries_its_automation_marks() {
        let page: MessagesPage = serde_json::from_str(MESSAGES_PAGE).unwrap();
        assert_eq!(page.automations.len(), 1);
        assert_eq!(page.automations[0].outcome, Outcome::Skipped);
        assert!(page.automations[0].run_id.is_none());
        let encoded = serde_json::to_value(&page.automations[0]).unwrap();
        assert!(encoded.get("run_id").is_none());
    }
    const AVATAR_SET: &str = include_str!("../../testdata/v3/avatar_set.json");

    #[test]
    fn me_and_an_avatar_answer_decode() {
        let Me {
            name,
            description,
            avatar_url,
        } = serde_json::from_str(ME).unwrap();
        assert_eq!(description, "Builds tuclaw");
        assert_eq!(name, "Pavel");
        assert_eq!(
            avatar_url,
            Some(AvatarUrl("/api/v3/me/avatar?v=AgADq2wx".into()))
        );
        let AvatarSet { avatar_url } = serde_json::from_str(AVATAR_SET).unwrap();
        assert_eq!(
            avatar_url,
            AvatarUrl("/api/v3/agents/1/avatar?v=AQADcVty".into())
        );
    }

    #[test]
    fn images_are_recognized_by_their_leading_bytes() {
        let mut webp = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
        webp.extend_from_slice(&[0; 4]);
        assert_eq!(ImageKind::sniff(&webp), Some(ImageKind::Webp));
        assert_eq!(
            ImageKind::sniff(&[0xff, 0xd8, 0xff, 0xe0]),
            Some(ImageKind::Jpeg)
        );
        assert_eq!(ImageKind::sniff(b"RIFF\0\0\0\0WAVE"), None);
        assert_eq!(ImageKind::Webp.mime(), "image/webp");
    }

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
            topic_name,
            display_name,
            group_id,
            archived_at,
            last_read_message_id,
            unread,
            unread_replies,
            marked_unread,
        } = &surfaces[0];
        assert_eq!(*unread_replies, 1);
        assert!(!*marked_unread);
        assert!(surfaces[1].marked_unread);
        assert_eq!(*last_read_message_id, Some(MessageId(9191)));
        assert_eq!(topic_name, "General");
        assert_eq!(*display_name, None);
        assert_eq!(*group_id, None);
        assert_eq!(*archived_at, None);
        assert_eq!(surfaces[1].name, "Torrents");
        assert_eq!(surfaces[1].display_name.as_deref(), Some("Torrents"));
        let groups: Vec<Group> =
            serde_json::from_str(include_str!("../../testdata/v3/groups.json")).unwrap();
        assert_eq!(groups[0].id, GroupId(2));
        assert_eq!(*unread, 1);
        assert_eq!(surfaces[1].last_read_message_id, None);
        let answer: ReadAnswer =
            serde_json::from_str(include_str!("../../testdata/v3/read_answer.json")).unwrap();
        assert_eq!(
            answer,
            ReadAnswer {
                last_read_message_id: Some(MessageId(9192)),
                unread: 0,
                unread_replies: 0,
                marked_unread: false,
            }
        );
        let answer: ReadAnswer =
            serde_json::from_str(include_str!("../../testdata/v3/unread_answer.json")).unwrap();
        assert_eq!(
            answer,
            ReadAnswer {
                last_read_message_id: Some(MessageId(9192)),
                unread: 0,
                unread_replies: 0,
                marked_unread: true,
            }
        );
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
                external_id: "-1001234567890:0".into(),
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
        assert_eq!(
            agents[0].avatar_url,
            Some(AvatarUrl("/api/v3/agents/1/avatar?v=AQADbVsx".into()))
        );
        assert_eq!(agents[1].avatar_url, None);
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
        assert_eq!(user.suggested_replies, None);
        assert_eq!(
            answer.suggested_replies,
            Some(SuggestedReplies {
                options: vec!["Ещё вариант".into(), "Спасибо".into()],
                open: true,
                chosen: None,
            })
        );
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
                agent_id: Some(AgentId(1)),
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

    #[test]
    fn the_posted_fixtures_decode() {
        let posted: Posted = serde_json::from_str(POSTED).unwrap();
        assert_eq!(posted.input_id, Some(InputId(42)));
        let orphan: Posted = serde_json::from_str(POSTED_WITHOUT_INPUT).unwrap();
        assert_eq!(orphan.input_id, None);
        assert_eq!(orphan.agent_id, None);
        assert_eq!(orphan.message_id, MessageId(9194));
    }

    #[test]
    fn a_null_string_field_reads_as_empty() {
        let message: Message = serde_json::from_str(
            r#"{"id": 6017, "surface_id": 3, "kind": "answer", "author": {"kind": "agent", "agent_id": 2},
                "text": "ok", "origin": null, "created_at": "2026-07-31T22:17:32Z", "run_summary": null}"#,
        )
        .expect("a pre-B2 answer decodes");
        assert_eq!(message.origin, "");
        assert_eq!(message.run_summary, None);
    }

    #[test]
    fn the_error_fixture_decodes() {
        let ErrorBody { error } = serde_json::from_str(ERROR).unwrap();
        assert_eq!(error.code, "conflict");
        assert!(error.message.ends_with("is not live"));
    }
}
