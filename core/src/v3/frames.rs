//! Frames of the v3 event socket: decoding what the server sends, encoding what the client sends.
//!
//! [`decode`] reads the envelope first and the payload by `type`; a type this build does not know
//! becomes [`Frame::Unknown`], never an error, so a newer daemon cannot break an older client.

use std::fmt;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use super::dto::{
    AgentId, ContextUsage, FireMark, InputId, Message, MessageId, Outcome, RunId, Seq, StepRow,
    SurfaceId, TaskId, ToolUseId, Usage, null_as_empty,
};

/// The envelope version this build speaks.
pub const VERSION: u32 = 1;

/// Whether the server wants a bearer token, as `hello.capabilities.auth` says.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::AuthMode;
///
/// assert_eq!(AuthMode::default(), AuthMode::Unknown);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    /// No header is needed; any `Authorization` header is ignored.
    None,
    /// Every request must carry `Authorization: Bearer <token>`.
    Bearer,
    /// The server did not say, or said something this build does not know.
    #[default]
    #[serde(other)]
    Unknown,
}

/// The event types and operations the server announces in `hello`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Capabilities {
    /// The event types it sends.
    #[serde(default)]
    pub events: Vec<String>,
    /// The operations it accepts.
    #[serde(default)]
    pub ops: Vec<String>,
    /// Whether it wants a bearer token.
    #[serde(default)]
    pub auth: AuthMode,
}

/// The first frame of every connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// The newest seq in the log.
    pub head: Seq,
    /// The oldest seq still kept.
    pub floor: Seq,
    /// The server's clock.
    #[serde(with = "time::serde::rfc3339")]
    pub server_time: OffsetDateTime,
    /// What the server speaks.
    #[serde(default)]
    pub capabilities: Capabilities,
}

/// Sent instead of a replay when the requested `since` fell out of the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    /// The oldest seq still kept.
    pub floor: Seq,
}

/// The whole state of one live run, sent after the replay and on focus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSnapshot {
    /// The run.
    pub run_id: RunId,
    /// The agent running it.
    pub agent_id: AgentId,
    /// The surface it is stamped with.
    pub surface_id: SurfaceId,
    /// When it started.
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    /// The newest seq whose effects `steps` already contain.
    pub as_of_seq: Seq,
    /// The text segment in progress.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub text: String,
    /// The finished steps.
    #[serde(default)]
    pub steps: Vec<StepRow>,
}

/// A persisted event of one run: the envelope's stamp around the payload.
#[derive(Debug, Clone, PartialEq)]
pub struct RunEvent<T> {
    /// The event's position in the log.
    pub seq: Seq,
    /// The run it belongs to.
    pub run_id: RunId,
    /// The surface the run is stamped with.
    pub surface_id: Option<SurfaceId>,
    /// When the daemon logged it.
    pub at: Option<OffsetDateTime>,
    /// The payload.
    pub body: T,
}

/// The payload of `run.started`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStarted {
    /// The agent running it.
    pub agent_id: AgentId,
    /// The wake inputs it claimed.
    #[serde(default)]
    pub input_ids: Vec<InputId>,
    /// What caused it.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub origin: String,
    /// Its metrics kind.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub turn_kind: String,
}

/// The payload of `step.text`: one finished text segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepText {
    /// The segment.
    pub text: String,
}

/// The payload of `step.tool_started`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolStarted {
    /// The tool call.
    pub tool_use_id: ToolUseId,
    /// The tool.
    pub name: String,
    /// The tool's JSON input, or `{"truncated": "..."}` over 16 KB.
    #[serde(default)]
    pub input: Value,
    /// The tool call that spawned this one, inside a subagent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<ToolUseId>,
}

impl ToolStarted {
    /// Returns whether the agent replaced the input with its 16 KB `truncated` clamp.
    ///
    /// # Examples
    ///
    /// ```
    /// use serde_json::json;
    /// use tuclaw_core::v3::{ToolStarted, ToolUseId};
    ///
    /// let tool = ToolStarted {
    ///     tool_use_id: ToolUseId("toolu_1".into()),
    ///     name: "Write".into(),
    ///     input: json!({"truncated": "{\"content\": \"..."}),
    ///     parent_tool_use_id: None,
    /// };
    /// assert!(tool.is_truncated());
    /// ```
    pub fn is_truncated(&self) -> bool {
        let Value::Object(fields) = &self.input else {
            return false;
        };
        let Some(Value::String(_)) = fields.get("truncated") else {
            return false;
        };
        fields.len() == 1
    }
}

/// The payload of `step.tool_finished`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolFinished {
    /// The tool call.
    pub tool_use_id: ToolUseId,
    /// Whether the tool failed.
    #[serde(default)]
    pub is_error: bool,
    /// The error text, clamped to 2 KB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The result text, clamped to 2 KB.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub summary: String,
}

/// The payload of `step.task`: a background subagent or Bash update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskUpdate {
    /// The task.
    pub task_id: String,
    /// Its type.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub task_type: String,
    /// Its state.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub state: String,
    /// What it does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// What it reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// The payload of `step.status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusUpdate {
    /// The status.
    pub status: String,
    /// Its detail.
    #[serde(default, deserialize_with = "null_as_empty")]
    pub detail: String,
}

/// The empty payload of `run.reset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RunReset {}

/// The payload of `run.finished`, the only terminal signal of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunFinished {
    /// Whether the run failed.
    #[serde(default)]
    pub is_error: bool,
    /// The error it ended with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Why it ended: `success`, `interrupted`, `agent_crashed`, ...
    #[serde(default, deserialize_with = "null_as_empty")]
    pub terminal_reason: String,
    /// The run's token counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// The context window after the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_usage: Option<ContextUsage>,
}

/// `message.created`: a message was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageCreated {
    /// The event's position in the log.
    pub seq: Seq,
    /// When the daemon logged it.
    pub at: Option<OffsetDateTime>,
    /// The message, as in a messages page.
    pub message: Message,
}

/// `task.fired`: an automation fired on a surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskFired {
    /// The event's position in the log.
    pub seq: Seq,
    /// The surface the automation reports to.
    pub surface_id: Option<SurfaceId>,
    /// The fire; its `at` is the event's.
    pub mark: FireMark,
}

#[derive(Debug, Deserialize)]
struct FiredPayload {
    task_id: TaskId,
    outcome: Outcome,
    #[serde(default)]
    run_id: Option<RunId>,
    #[serde(default)]
    message_id: Option<MessageId>,
    #[serde(default)]
    error: Option<String>,
}

/// `surface.read`: the read cursor of a surface moved, or was confirmed where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceRead {
    /// The event's position in the log.
    pub seq: Seq,
    /// The surface that was read.
    pub surface_id: SurfaceId,
    /// The newest message read on it.
    pub last_read_message_id: MessageId,
    /// The messages still unread on it after the move.
    pub unread: u32,
}

#[derive(Debug, Deserialize)]
struct ReadPayload {
    last_read_message_id: MessageId,
    unread: u32,
}

/// `text.delta`: streamed text of a run on a focused surface, never replayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextDelta {
    /// The run it belongs to.
    pub run_id: RunId,
    /// The surface the run is stamped with.
    pub surface_id: Option<SurfaceId>,
    /// The text to append.
    pub text: String,
}

/// `input.accepted`: the daemon took a posted message; never replayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputAccepted {
    /// The input queued.
    pub input_id: InputId,
    /// The surface it was posted on.
    pub surface_id: SurfaceId,
    /// The agent that will run it.
    pub agent_id: AgentId,
}

/// A frame of a type this build does not know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownFrame {
    /// The frame's `type`.
    pub type_name: String,
    /// Its seq, when it is persisted.
    pub seq: Option<Seq>,
}

/// One decoded server frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    /// `hello`.
    Hello(Hello),
    /// `gap`.
    Gap(Gap),
    /// `run.snapshot`.
    RunSnapshot(RunSnapshot),
    /// `run.started`.
    RunStarted(RunEvent<RunStarted>),
    /// `step.text`.
    StepText(RunEvent<StepText>),
    /// `step.tool_started`.
    ToolStarted(RunEvent<ToolStarted>),
    /// `step.tool_finished`.
    ToolFinished(RunEvent<ToolFinished>),
    /// `step.task`.
    Task(RunEvent<TaskUpdate>),
    /// `step.status`.
    Status(RunEvent<StatusUpdate>),
    /// `run.reset`.
    RunReset(RunEvent<RunReset>),
    /// `run.finished`.
    RunFinished(RunEvent<RunFinished>),
    /// `message.created`.
    MessageCreated(MessageCreated),
    /// `task.fired`.
    TaskFired(TaskFired),
    /// `surface.read`.
    SurfaceRead(SurfaceRead),
    /// `text.delta`.
    TextDelta(TextDelta),
    /// `input.accepted`.
    InputAccepted(InputAccepted),
    /// A type this build does not know.
    Unknown(UnknownFrame),
}

impl Frame {
    /// Returns the frame's seq; `None` for ephemeral and connection frames.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuclaw_core::v3::{decode, Seq};
    ///
    /// let line = r#"{"v": 1, "seq": 7, "type": "run.reset", "run_id": "r1", "payload": {}}"#;
    /// assert_eq!(decode(line).unwrap().seq(), Some(Seq(7)));
    /// ```
    pub fn seq(&self) -> Option<Seq> {
        match self {
            Frame::Hello(_) => None,
            Frame::Gap(_) => None,
            Frame::RunSnapshot(_) => None,
            Frame::RunStarted(event) => Some(event.seq),
            Frame::StepText(event) => Some(event.seq),
            Frame::ToolStarted(event) => Some(event.seq),
            Frame::ToolFinished(event) => Some(event.seq),
            Frame::Task(event) => Some(event.seq),
            Frame::Status(event) => Some(event.seq),
            Frame::RunReset(event) => Some(event.seq),
            Frame::RunFinished(event) => Some(event.seq),
            Frame::MessageCreated(created) => Some(created.seq),
            Frame::TaskFired(fired) => Some(fired.seq),
            Frame::SurfaceRead(read) => Some(read.seq),
            Frame::TextDelta(_) => None,
            Frame::InputAccepted(_) => None,
            Frame::Unknown(unknown) => unknown.seq,
        }
    }

    /// Returns the run the frame belongs to, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuclaw_core::v3::{decode, RunId};
    ///
    /// let line = r#"{"v": 1, "type": "text.delta", "run_id": "r1", "payload": {"text": "hi"}}"#;
    /// assert_eq!(decode(line).unwrap().run_id(), Some(&RunId("r1".into())));
    /// ```
    pub fn run_id(&self) -> Option<&RunId> {
        match self {
            Frame::Hello(_) => None,
            Frame::Gap(_) => None,
            Frame::RunSnapshot(snapshot) => Some(&snapshot.run_id),
            Frame::RunStarted(event) => Some(&event.run_id),
            Frame::StepText(event) => Some(&event.run_id),
            Frame::ToolStarted(event) => Some(&event.run_id),
            Frame::ToolFinished(event) => Some(&event.run_id),
            Frame::Task(event) => Some(&event.run_id),
            Frame::Status(event) => Some(&event.run_id),
            Frame::RunReset(event) => Some(&event.run_id),
            Frame::RunFinished(event) => Some(&event.run_id),
            Frame::MessageCreated(created) => created.message.run_id.as_ref(),
            Frame::TaskFired(fired) => fired.mark.run_id.as_ref(),
            Frame::SurfaceRead(_) => None,
            Frame::TextDelta(delta) => Some(&delta.run_id),
            Frame::InputAccepted(_) => None,
            Frame::Unknown(_) => None,
        }
    }
}

/// Why a line is not a frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The line is not a JSON envelope.
    Envelope(String),
    /// The envelope's `v` is not one this build speaks.
    Version(u32),
    /// A run event without a `run_id`, or a persisted event without a `seq`.
    Missing {
        /// The frame's `type`.
        type_name: String,
        /// The missing envelope field.
        field: &'static str,
    },
    /// The payload does not match its `type`.
    Payload {
        /// The frame's `type`.
        type_name: String,
        /// The serde error.
        reason: String,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Envelope(reason) => write!(f, "malformed envelope: {reason}"),
            DecodeError::Version(version) => write!(f, "unsupported envelope version {version}"),
            DecodeError::Missing { type_name, field } => write!(f, "{type_name}: missing {field}"),
            DecodeError::Payload { type_name, reason } => write!(f, "{type_name}: {reason}"),
        }
    }
}

impl std::error::Error for DecodeError {}

#[derive(Deserialize)]
struct Envelope {
    v: u32,
    #[serde(rename = "type")]
    type_name: String,
    #[serde(default)]
    seq: Option<Seq>,
    #[serde(default)]
    surface_id: Option<SurfaceId>,
    #[serde(default)]
    run_id: Option<RunId>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    at: Option<OffsetDateTime>,
    #[serde(default)]
    payload: Value,
}

#[derive(Deserialize)]
struct MessagePayload {
    message: Message,
}

#[derive(Deserialize)]
struct DeltaPayload {
    text: String,
}

impl Envelope {
    fn payload<T: DeserializeOwned>(&self) -> Result<T, DecodeError> {
        let payload = if self.payload.is_null() {
            Value::Object(serde_json::Map::new())
        } else {
            self.payload.clone()
        };
        serde_json::from_value(payload).map_err(|error| DecodeError::Payload {
            type_name: self.type_name.clone(),
            reason: error.to_string(),
        })
    }

    fn seq(&self) -> Result<Seq, DecodeError> {
        let Some(seq) = self.seq else {
            return Err(self.missing("seq"));
        };
        Ok(seq)
    }

    fn run_id(&self) -> Result<RunId, DecodeError> {
        let Some(run_id) = &self.run_id else {
            return Err(self.missing("run_id"));
        };
        Ok(run_id.clone())
    }

    fn missing(&self, field: &'static str) -> DecodeError {
        DecodeError::Missing {
            type_name: self.type_name.clone(),
            field,
        }
    }

    fn run_event<T: DeserializeOwned>(&self) -> Result<RunEvent<T>, DecodeError> {
        Ok(RunEvent {
            seq: self.seq()?,
            run_id: self.run_id()?,
            surface_id: self.surface_id,
            at: self.at,
            body: self.payload()?,
        })
    }
}

/// Decodes one text message of the event socket.
///
/// # Errors
///
/// Returns [`DecodeError`] when the line is not a v1 envelope, when a run event lacks its
/// `run_id` or `seq`, or when a known type's payload does not match it. An unknown `type` is not
/// an error; it decodes to [`Frame::Unknown`].
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::{decode, Frame};
///
/// let frame = decode(r#"{"v": 1, "type": "presence.changed", "payload": {}}"#).unwrap();
/// let Frame::Unknown(unknown) = frame else { panic!("expected an unknown frame") };
/// assert_eq!(unknown.type_name, "presence.changed");
/// assert!(decode("not json").is_err());
/// ```
pub fn decode(line: &str) -> Result<Frame, DecodeError> {
    let envelope: Envelope =
        serde_json::from_str(line).map_err(|error| DecodeError::Envelope(error.to_string()))?;
    if envelope.v != VERSION {
        return Err(DecodeError::Version(envelope.v));
    }
    let frame = match envelope.type_name.as_str() {
        "hello" => Frame::Hello(envelope.payload()?),
        "gap" => Frame::Gap(envelope.payload()?),
        "run.snapshot" => Frame::RunSnapshot(envelope.payload()?),
        "run.started" => Frame::RunStarted(envelope.run_event()?),
        "step.text" => Frame::StepText(envelope.run_event()?),
        "step.tool_started" => Frame::ToolStarted(envelope.run_event()?),
        "step.tool_finished" => Frame::ToolFinished(envelope.run_event()?),
        "step.task" => Frame::Task(envelope.run_event()?),
        "step.status" => Frame::Status(envelope.run_event()?),
        "run.reset" => Frame::RunReset(envelope.run_event()?),
        "run.finished" => Frame::RunFinished(envelope.run_event()?),
        "message.created" => {
            let MessagePayload { message } = envelope.payload()?;
            Frame::MessageCreated(MessageCreated {
                seq: envelope.seq()?,
                at: envelope.at,
                message,
            })
        }
        "surface.read" => {
            let ReadPayload {
                last_read_message_id,
                unread,
            } = envelope.payload()?;
            let Some(surface_id) = envelope.surface_id else {
                return Err(DecodeError::Missing {
                    type_name: "surface.read".to_string(),
                    field: "surface_id",
                });
            };
            Frame::SurfaceRead(SurfaceRead {
                seq: envelope.seq()?,
                surface_id,
                last_read_message_id,
                unread,
            })
        }
        "task.fired" => {
            let FiredPayload {
                task_id,
                outcome,
                run_id,
                message_id,
                error,
            } = envelope.payload()?;
            Frame::TaskFired(TaskFired {
                seq: envelope.seq()?,
                surface_id: envelope.surface_id,
                mark: FireMark {
                    task_id,
                    at: envelope.at,
                    outcome,
                    run_id,
                    message_id,
                    error,
                },
            })
        }
        "text.delta" => {
            let DeltaPayload { text } = envelope.payload()?;
            Frame::TextDelta(TextDelta {
                run_id: envelope.run_id()?,
                surface_id: envelope.surface_id,
                text,
            })
        }
        "input.accepted" => Frame::InputAccepted(envelope.payload()?),
        other => Frame::Unknown(UnknownFrame {
            type_name: other.to_string(),
            seq: envelope.seq,
        }),
    };
    Ok(frame)
}

/// A frame the client sends.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::{encode, ClientFrame, SurfaceId};
///
/// let focus = ClientFrame::Focus { surface_ids: vec![SurfaceId(1), SurfaceId(3)] };
/// assert_eq!(encode(&focus), r#"{"v":1,"type":"focus","payload":{"surface_ids":[1,3]}}"#);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientFrame {
    /// The surfaces the client shows; ephemeral frames flow only for them.
    Focus {
        /// The shown surfaces.
        surface_ids: Vec<SurfaceId>,
    },
}

/// Encodes a client frame as the one-line JSON text message the socket sends.
pub fn encode(frame: &ClientFrame) -> String {
    match frame {
        ClientFrame::Focus { surface_ids } => {
            let mut ids = Vec::new();
            for SurfaceId(id) in surface_ids {
                ids.push(Value::from(*id));
            }
            let ids = Value::Array(ids);
            format!(r#"{{"v":{VERSION},"type":"focus","payload":{{"surface_ids":{ids}}}}}"#)
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use time::macros::datetime;

    use super::*;
    use crate::v3::dto::{ClientMessageId, MessageKind, RowKind, RunStatus};
    use crate::v3::golden::fixture;

    #[test]
    fn surface_read_carries_the_cursor_and_the_count() {
        let Frame::SurfaceRead(read) = crate::v3::golden::frame("surface_read") else {
            panic!("surface.read decodes");
        };
        assert_eq!(
            read,
            SurfaceRead {
                seq: Seq(1300),
                surface_id: SurfaceId(1),
                last_read_message_id: MessageId(9192),
                unread: 0,
            }
        );
        assert_eq!(Frame::SurfaceRead(read).run_id(), None);
    }

    #[test]
    fn task_fired_carries_the_mark_with_the_envelope_time() {
        let Frame::TaskFired(fired) = crate::v3::golden::frame("task_fired") else {
            panic!("task.fired decodes");
        };
        assert_eq!(fired.seq, Seq(1290));
        assert_eq!(fired.surface_id, Some(SurfaceId(4)));
        assert_eq!(fired.mark.outcome, Outcome::Ran);
        assert_eq!(fired.mark.message_id, Some(MessageId(9301)));
        assert_eq!(fired.mark.at, Some(datetime!(2026-10-04 07:00:19 UTC)));
        assert_eq!(
            Frame::TaskFired(fired.clone()).run_id(),
            Some(&RunId("0b9d2c4e-5a61-4f7e-8c3d-1e2f3a4b5c6d".into()))
        );
    }

    const RUN_ID: &str = "6763eb02-7f3e-4c4d-9b1a-2f0c5d8e9a11";

    fn frame(name: &str) -> Frame {
        decode(fixture(name)).unwrap()
    }

    fn run_id() -> RunId {
        RunId(RUN_ID.into())
    }

    #[test]
    fn every_fixture_has_the_expected_seq_and_run() {
        let cases = [
            ("event_frame", Some(1201), None),
            ("hello", None, None),
            ("gap", None, None),
            ("run_snapshot", None, Some(RUN_ID)),
            ("run_started", Some(1202), Some(RUN_ID)),
            ("step_text", Some(1203), Some(RUN_ID)),
            ("step_tool_started", Some(1204), Some(RUN_ID)),
            ("step_tool_started_truncated", Some(1209), Some(RUN_ID)),
            ("step_tool_finished", Some(1205), Some(RUN_ID)),
            ("step_task", Some(1206), Some(RUN_ID)),
            ("step_status", Some(1207), Some(RUN_ID)),
            ("run_reset", Some(1208), Some(RUN_ID)),
            ("run_finished", Some(1211), Some(RUN_ID)),
            ("run_finished_interrupted", Some(1211), Some(RUN_ID)),
            ("message_created", Some(1210), Some(RUN_ID)),
            ("text_delta", None, Some(RUN_ID)),
            ("input_accepted", None, None),
            ("unknown", Some(1212), None),
        ];
        for (name, seq, run) in cases {
            let decoded = frame(name);
            assert_eq!(decoded.seq(), seq.map(Seq), "{name}");
            assert_eq!(
                decoded.run_id(),
                run.map(|id| RunId(id.into())).as_ref(),
                "{name}"
            );
        }
    }

    #[test]
    fn hello_carries_head_floor_and_capabilities() {
        let Frame::Hello(hello) = frame("hello") else {
            panic!("expected hello");
        };
        assert_eq!(hello.head, Seq(1200));
        assert_eq!(hello.floor, Seq(17));
        assert_eq!(hello.server_time, datetime!(2026-10-03 15:25:58 UTC));
        assert!(
            hello
                .capabilities
                .events
                .contains(&"text.delta".to_string())
        );
        assert_eq!(hello.capabilities.ops, vec!["focus".to_string()]);
        assert_eq!(hello.capabilities.auth, AuthMode::None);
    }

    #[test]
    fn an_absent_or_unknown_auth_mode_is_unknown() {
        for (raw, mode) in [
            (r#"{"events": [], "ops": []}"#, AuthMode::Unknown),
            (r#"{"auth": "oidc"}"#, AuthMode::Unknown),
            (r#"{"auth": "bearer"}"#, AuthMode::Bearer),
        ] {
            let capabilities: Capabilities = serde_json::from_str(raw).expect("decodes");
            assert_eq!(capabilities.auth, mode);
        }
    }

    #[test]
    fn gap_carries_floor() {
        let Frame::Gap(gap) = frame("gap") else {
            panic!("expected gap");
        };
        assert_eq!(gap, Gap { floor: Seq(17) });
    }

    #[test]
    fn run_snapshot_carries_as_of_seq_segment_and_every_row_kind() {
        let Frame::RunSnapshot(snapshot) = frame("run_snapshot") else {
            panic!("expected run.snapshot");
        };
        assert_eq!(snapshot.as_of_seq, Seq(1207));
        assert_eq!(snapshot.agent_id, AgentId(1));
        assert_eq!(snapshot.surface_id, SurfaceId(1));
        assert_eq!(snapshot.text, "Сливки лучше брать");
        let mut kinds = Vec::new();
        for row in &snapshot.steps {
            kinds.push(row.kind);
        }
        assert_eq!(
            kinds,
            vec![RowKind::Text, RowKind::Tool, RowKind::Task, RowKind::Status]
        );
        assert_eq!(snapshot.steps[2].name.as_deref(), Some("local_agent"));
        assert_eq!(snapshot.steps[3].output.as_deref(), Some("context 91%"));
    }

    #[test]
    fn run_started_carries_agent_and_inputs() {
        let Frame::RunStarted(event) = frame("run_started") else {
            panic!("expected run.started");
        };
        assert_eq!(event.run_id, run_id());
        assert_eq!(event.surface_id, Some(SurfaceId(1)));
        assert_eq!(event.at, Some(datetime!(2026-10-03 15:26:00 UTC)));
        assert_eq!(
            event.body,
            RunStarted {
                agent_id: AgentId(1),
                input_ids: vec![InputId(42)],
                origin: "user".into(),
                turn_kind: "user".into(),
            }
        );
    }

    #[test]
    fn step_payloads_decode() {
        let Frame::StepText(text) = frame("step_text") else {
            panic!("expected step.text");
        };
        assert_eq!(text.body.text, "Посмотрю, что есть в заметках.");

        let Frame::ToolStarted(started) = frame("step_tool_started") else {
            panic!("expected step.tool_started");
        };
        assert_eq!(started.body.name, "Bash");
        assert_eq!(started.body.input["command"], json!("rg -i лисич ~/notes"));
        assert_eq!(started.body.parent_tool_use_id, None);
        assert!(!started.body.is_truncated());

        let Frame::ToolStarted(truncated) = frame("step_tool_started_truncated") else {
            panic!("expected step.tool_started");
        };
        assert!(truncated.body.is_truncated());
        assert_eq!(
            truncated.body.parent_tool_use_id,
            Some(ToolUseId("toolu_01A2b3C4d5E6f7G8h9".into()))
        );

        let Frame::ToolFinished(finished) = frame("step_tool_finished") else {
            panic!("expected step.tool_finished");
        };
        assert!(!finished.body.is_error);
        assert_eq!(finished.body.error, None);
        assert_eq!(finished.body.summary, "recipes.md: лисички со сливками");

        let Frame::Task(task) = frame("step_task") else {
            panic!("expected step.task");
        };
        assert_eq!(task.body.task_type, "local_agent");
        assert_eq!(task.body.state, "running");
        assert_eq!(task.body.summary, None);

        let Frame::Status(status) = frame("step_status") else {
            panic!("expected step.status");
        };
        assert_eq!(status.body.status, "compacting");
        assert_eq!(status.body.detail, "context 91%");

        let Frame::RunReset(reset) = frame("run_reset") else {
            panic!("expected run.reset");
        };
        assert_eq!(reset.body, RunReset {});
    }

    #[test]
    fn run_finished_carries_reason_usage_and_context() {
        let Frame::RunFinished(ok) = frame("run_finished") else {
            panic!("expected run.finished");
        };
        assert!(!ok.body.is_error);
        assert_eq!(ok.body.terminal_reason, "success");
        assert_eq!(
            ok.body.usage,
            Some(Usage {
                input_tokens: 12,
                output_tokens: 412,
                cache_read_tokens: 321_004,
                cache_creation_tokens: 2950,
                num_turns: 2,
                duration_api_ms: 11_840,
            })
        );
        assert_eq!(
            ok.body.context_usage,
            Some(ContextUsage {
                total_tokens: 323_968,
                max_tokens: 1_000_000,
                percentage: 32.4,
                model: "claude-opus-5-5[1m]".into(),
            })
        );

        let Frame::RunFinished(interrupted) = frame("run_finished_interrupted") else {
            panic!("expected run.finished");
        };
        assert!(!interrupted.body.is_error);
        assert_eq!(interrupted.body.terminal_reason, "interrupted");
        assert_eq!(interrupted.body.usage, None);
    }

    #[test]
    fn message_created_carries_the_message() {
        let Frame::MessageCreated(answer) = frame("message_created") else {
            panic!("expected message.created");
        };
        assert_eq!(answer.message.kind, MessageKind::Answer);
        assert_eq!(answer.message.run_id, Some(run_id()));
        assert_eq!(
            answer.message.run_summary.map(|summary| summary.status),
            Some(RunStatus::Running)
        );

        let Frame::MessageCreated(user) = frame("event_frame") else {
            panic!("expected message.created");
        };
        assert_eq!(user.message.kind, MessageKind::User);
        assert_eq!(
            user.message.client_message_id,
            Some(ClientMessageId(
                "8b0c4f2e-1d7a-4c39-9e65-3a2b1c0d9f87".into()
            ))
        );
    }

    #[test]
    fn ephemeral_frames_decode_without_seq() {
        let Frame::TextDelta(delta) = frame("text_delta") else {
            panic!("expected text.delta");
        };
        assert_eq!(delta.text, " с жирностью 20%");
        assert_eq!(delta.surface_id, Some(SurfaceId(1)));

        let Frame::InputAccepted(accepted) = frame("input_accepted") else {
            panic!("expected input.accepted");
        };
        assert_eq!(
            accepted,
            InputAccepted {
                input_id: InputId(42),
                surface_id: SurfaceId(1),
                agent_id: AgentId(1),
            }
        );
    }

    #[test]
    fn unknown_types_and_step_kinds_decode_to_unknown() {
        let Frame::Unknown(unknown) = frame("unknown") else {
            panic!("expected unknown");
        };
        assert_eq!(unknown.type_name, "presence.changed");
        assert_eq!(unknown.seq, Some(Seq(1212)));

        let line = json!({"v": 1, "seq": 9, "type": "step.thinking", "run_id": "r", "payload": {"text": "hm"}});
        let Frame::Unknown(step) = decode(&line.to_string()).unwrap() else {
            panic!("expected unknown");
        };
        assert_eq!(step.type_name, "step.thinking");
        assert_eq!(step.seq, Some(Seq(9)));
    }

    #[test]
    fn extra_fields_are_ignored() {
        let line = json!({
            "v": 1, "seq": 3, "type": "step.status", "run_id": "r", "trace": "x",
            "payload": {"status": "api_retry", "detail": "attempt 2", "attempt": 2}
        });
        let Frame::Status(status) = decode(&line.to_string()).unwrap() else {
            panic!("expected step.status");
        };
        assert_eq!(status.body.status, "api_retry");
        assert_eq!(status.surface_id, None);
        assert_eq!(status.at, None);
    }

    #[test]
    fn a_missing_payload_reads_as_empty() {
        let line = json!({"v": 1, "seq": 4, "type": "run.reset", "run_id": "r"});
        let Frame::RunReset(reset) = decode(&line.to_string()).unwrap() else {
            panic!("expected run.reset");
        };
        assert_eq!(reset.seq, Seq(4));
    }

    #[test]
    fn malformed_lines_are_errors() {
        let cases = [
            ("not json", "envelope"),
            (r#"{"type": "hello"}"#, "envelope"),
            (r#"{"v": 2, "type": "hello", "payload": {}}"#, "version"),
            (
                r#"{"v": 1, "seq": 1, "type": "step.text", "payload": {"text": "x"}}"#,
                "run_id",
            ),
            (
                r#"{"v": 1, "type": "run.reset", "run_id": "r", "payload": {}}"#,
                "seq",
            ),
            (
                r#"{"v": 1, "seq": 1, "type": "step.text", "run_id": "r", "payload": {"text": 5}}"#,
                "payload",
            ),
            (
                r#"{"v": 1, "type": "hello", "payload": {"head": 1}}"#,
                "payload",
            ),
        ];
        for (line, expected) in cases {
            let error = decode(line).unwrap_err();
            let kind = match &error {
                DecodeError::Envelope(_) => "envelope",
                DecodeError::Version(_) => "version",
                DecodeError::Missing { field, .. } => field,
                DecodeError::Payload { .. } => "payload",
            };
            assert_eq!(kind, expected, "{line}: {error}");
        }
    }

    #[test]
    fn focus_encodes_byte_equal_to_the_fixture() {
        let focus = ClientFrame::Focus {
            surface_ids: vec![SurfaceId(1), SurfaceId(3)],
        };
        assert_eq!(encode(&focus), fixture("focus"));
        let empty = ClientFrame::Focus {
            surface_ids: Vec::new(),
        };
        assert_eq!(
            encode(&empty),
            r#"{"v":1,"type":"focus","payload":{"surface_ids":[]}}"#
        );
    }
}
