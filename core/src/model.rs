use time::OffsetDateTime;

/// Identifies a channel.
///
/// # Examples
///
/// ```
/// use tuclaw_core::model::ChannelId;
///
/// let ChannelId(raw) = ChannelId(7);
/// assert_eq!(raw, 7);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelId(pub i64);

/// Identifies a message.
///
/// # Examples
///
/// ```
/// use tuclaw_core::model::MessageId;
///
/// let MessageId(raw) = MessageId(42);
/// assert_eq!(raw, 42);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageId(pub i64);

/// Identifies an agent.
///
/// # Examples
///
/// ```
/// use tuclaw_core::model::AgentId;
///
/// let AgentId(raw) = AgentId(3);
/// assert_eq!(raw, 3);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AgentId(pub i64);

/// Distinguishes a named channel from a direct conversation with one agent.
///
/// # Examples
///
/// ```
/// use tuclaw_core::model::{AgentId, ChannelKind};
///
/// let kind = ChannelKind::Direct(AgentId(1));
/// assert_ne!(kind, ChannelKind::Channel);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    /// A named channel that any agent may post in.
    Channel,
    /// A direct conversation with the given agent.
    Direct(AgentId),
}

/// Names who wrote a message.
///
/// # Examples
///
/// ```
/// use tuclaw_core::model::{AgentId, Author};
///
/// let author = Author::Agent(AgentId(2));
/// assert_ne!(author, Author::User);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Author {
    /// The person using the app.
    User,
    /// The agent with the given identifier.
    Agent(AgentId),
    /// The daemon itself, e.g. a notice that a run failed.
    System,
}

/// Reports whether an agent is working, and on what.
///
/// # Examples
///
/// ```
/// use tuclaw_core::model::AgentStatus;
///
/// let status = AgentStatus::Busy("syncing feeds".to_string());
/// assert_ne!(status, AgentStatus::Idle);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    /// The agent has nothing in flight.
    Idle,
    /// The agent is working on the described task.
    Busy(String),
}

/// An agent the workspace can talk to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    /// The agent's identifier.
    pub id: AgentId,
    /// The display name, as shown in the sidebar and on message rows.
    pub name: String,
    /// The two letters drawn in the agent's chip.
    pub initials: String,
    /// The one-line description of what the agent does.
    pub role: String,
    /// Whether the agent is idle or busy.
    pub status: AgentStatus,
    /// The position of the agent in the rendered order, ascending.
    pub sort_index: i64,
}

/// A conversation in the sidebar, either a channel or a direct message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    /// The channel's identifier.
    pub id: ChannelId,
    /// The display name, without a leading `#`.
    pub name: String,
    /// The sidebar section the channel belongs to, or [`None`] when ungrouped.
    pub group: Option<String>,
    /// Whether the channel is a named channel or a direct conversation.
    pub kind: ChannelKind,
    /// How many messages the user has not read.
    pub unread: usize,
    /// The position of the channel in the rendered order, ascending.
    pub sort_index: i64,
}

/// One message in a conversation.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// The message's identifier.
    pub id: MessageId,
    /// Who wrote the message.
    pub author: Author,
    /// The body, as the sequence of spans it renders to.
    pub body: Vec<Span>,
    /// When the message was sent.
    pub sent_at: OffsetDateTime,
}

/// One run of a message body: plain text, an agent mention, or inline code.
///
/// # Examples
///
/// ```
/// use tuclaw_core::model::Span;
///
/// let body = vec![
///     Span::Text("ask ".to_string()),
///     Span::Mention("allspeak".to_string()),
/// ];
/// assert_eq!(body.len(), 2);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Span {
    /// Plain text, rendered word by word.
    Text(String),
    /// An agent mention, rendered as a chip.
    Mention(String),
    /// Inline code, rendered as a chip.
    Code(String),
}
