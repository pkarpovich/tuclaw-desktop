use anyhow::Result;
use serde::{Deserialize, Serialize};
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
    /// How many replies the message's thread holds.
    pub reply_count: usize,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Span {
    /// Plain text, rendered word by word.
    Text(String),
    /// An agent mention, rendered as a chip.
    Mention(String),
    /// Inline code, rendered as a chip.
    Code(String),
}

/// Encodes a message body as the JSON stored in the `body` column.
///
/// # Panics
///
/// Panics if `serde_json` cannot serialize the spans, which cannot happen for
/// the string payloads [`Span`] carries.
///
/// # Examples
///
/// ```
/// use tuclaw_core::model::{encode, Span};
///
/// let json = encode(&[Span::Text("on it".to_string())]);
/// assert_eq!(json, r#"[{"Text":"on it"}]"#);
/// ```
pub fn encode(body: &[Span]) -> String {
    serde_json::to_string(body).expect("spans hold only strings and always serialize")
}

/// Decodes a message body from the JSON stored in the `body` column.
///
/// # Errors
///
/// Returns an error if the input is not the JSON [`encode`] produces.
///
/// # Examples
///
/// ```
/// use tuclaw_core::model::{decode, Span};
///
/// let body = decode(r#"[{"Code":"make run"}]"#).unwrap();
/// assert_eq!(body, vec![Span::Code("make run".to_string())]);
/// assert!(decode("not json").is_err());
/// ```
pub fn decode(json: &str) -> Result<Vec<Span>> {
    let body = serde_json::from_str(json)?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::{Span, decode, encode};

    fn round_trip(body: Vec<Span>) {
        let json = encode(&body);
        let decoded = decode(&json).expect("encoded body decodes");
        assert_eq!(decoded, body);
    }

    #[test]
    fn plain_text_round_trips() {
        round_trip(vec![Span::Text("watched it last night".to_string())]);
    }

    #[test]
    fn mention_mid_sentence_round_trips() {
        round_trip(vec![
            Span::Text("asking ".to_string()),
            Span::Mention("allspeak".to_string()),
            Span::Text(" for subtitles".to_string()),
        ]);
    }

    #[test]
    fn inline_code_round_trips() {
        round_trip(vec![
            Span::Text("dropped it in ".to_string()),
            Span::Code("~/Media/inbox".to_string()),
        ]);
    }

    #[test]
    fn several_spans_in_one_body_round_trip() {
        round_trip(vec![
            Span::Text("hey ".to_string()),
            Span::Mention("magnet feed sync".to_string()),
            Span::Text(", check ".to_string()),
            Span::Code("/tmp/list.txt".to_string()),
            Span::Text(" and report back".to_string()),
            Span::Mention("media review".to_string()),
        ]);
    }

    #[test]
    fn empty_body_round_trips() {
        round_trip(Vec::new());
    }

    #[test]
    fn punctuation_and_escapes_round_trip() {
        round_trip(vec![
            Span::Text(r#"a [bracket] a {brace} a "quote" a \backslash"#.to_string()),
            Span::Code(r#"{"key": "value\\"}"#.to_string()),
        ]);
    }

    #[test]
    fn non_ascii_round_trips() {
        round_trip(vec![
            Span::Text("привет 🐢 ".to_string()),
            Span::Mention("allspeak".to_string()),
        ]);
    }

    #[test]
    fn encoded_form_is_externally_tagged() {
        let json = encode(&[
            Span::Text("on it ".to_string()),
            Span::Mention("allspeak".to_string()),
        ]);
        assert_eq!(json, r#"[{"Text":"on it "},{"Mention":"allspeak"}]"#);
    }

    #[test]
    fn malformed_json_is_an_error() {
        assert!(decode("").is_err());
        assert!(decode("[{\"Text\":").is_err());
        assert!(decode(r#"[{"Unknown":"x"}]"#).is_err());
        assert!(decode(r#"{"Text":"x"}"#).is_err());
    }
}
