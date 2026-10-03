use std::fmt;
use std::time::Duration;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures::future::BoxFuture;
use serde_json::Value;

use super::dto::{ErrorBody, ErrorDetail, Seq, SurfaceId};
use super::frames::{ClientFrame, Frame};

/// Why a call to the daemon failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// The token is missing or wrong (`401`).
    Unauthorized,
    /// The surface, run or route does not exist (`404`).
    NotFound,
    /// The daemon rejected the request (`400`), with its explanation.
    Invalid(String),
    /// The request conflicts with the current state (`409`), e.g. interrupting a finished run.
    Conflict,
    /// The daemon cannot serve the request right now (`503`).
    Unavailable,
    /// The request did not complete: connection, timeout, or an unexpected answer.
    Transport(String),
    /// The answer does not match the contract.
    Decode(String),
}

impl ApiError {
    /// Maps a non-2xx status and its body, the contract's error envelope when it is one.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuclaw_core::v3::ApiError;
    ///
    /// let body = br#"{"error": {"code": "conflict", "message": "run is not live"}}"#;
    /// assert_eq!(ApiError::from_status(409, body), ApiError::Conflict);
    /// assert_eq!(ApiError::from_status(401, b""), ApiError::Unauthorized);
    /// ```
    pub fn from_status(status: u16, body: &[u8]) -> ApiError {
        let Ok(ErrorBody {
            error: ErrorDetail { code, message },
        }) = serde_json::from_slice::<ErrorBody>(body)
        else {
            if status == 401 {
                return ApiError::Unauthorized;
            }
            return ApiError::Transport(format!("HTTP {status}"));
        };
        match code.as_str() {
            "unauthorized" => ApiError::Unauthorized,
            "not_found" => ApiError::NotFound,
            "invalid_request" => ApiError::Invalid(message),
            "conflict" => ApiError::Conflict,
            "unavailable" => ApiError::Unavailable,
            other => ApiError::Transport(format!("HTTP {status} {other}: {message}")),
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Unauthorized => write!(f, "the daemon rejected the token"),
            ApiError::NotFound => write!(f, "not found"),
            ApiError::Invalid(message) => write!(f, "invalid request: {message}"),
            ApiError::Conflict => write!(f, "conflict"),
            ApiError::Unavailable => write!(f, "the daemon is unavailable"),
            ApiError::Transport(reason) => write!(f, "transport: {reason}"),
            ApiError::Decode(reason) => write!(f, "decode: {reason}"),
        }
    }
}

impl std::error::Error for ApiError {}

/// One open event socket.
///
/// Frames arrive on `frames` in order; `frames` ends when the socket closes for any reason, which
/// is how a caller learns to reconnect. Dropping the connection (or `control`) closes the socket.
#[derive(Debug)]
pub struct Connection {
    /// The decoded server frames.
    pub frames: UnboundedReceiver<Frame>,
    /// The frames to send to the server.
    pub control: UnboundedSender<ClientFrame>,
}

impl Connection {
    /// Tells the server which surfaces the client shows.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Transport`] when the socket is already closed.
    pub fn focus(&self, surface_ids: Vec<SurfaceId>) -> Result<(), ApiError> {
        let sent = self
            .control
            .unbounded_send(ClientFrame::Focus { surface_ids });
        let Ok(()) = sent else {
            return Err(ApiError::Transport("the event socket is closed".into()));
        };
        Ok(())
    }
}

/// The seam between the typed client and the daemon: three calls, each an executor-agnostic
/// future that needs no runtime in the caller.
pub trait Transport: Send + Sync {
    /// Sends `GET /api/v3{path}` and returns the JSON body (`Null` for an empty one).
    fn get(&self, path: &str) -> BoxFuture<'static, Result<Value, ApiError>>;

    /// Sends `POST /api/v3{path}` with an optional JSON body and returns the JSON answer.
    fn post(&self, path: &str, body: Option<Value>) -> BoxFuture<'static, Result<Value, ApiError>>;

    /// Opens the event socket, replaying after `since` when given.
    fn connect(&self, since: Option<Seq>) -> BoxFuture<'static, Result<Connection, ApiError>>;
}

const INITIAL_DELAY: Duration = Duration::from_millis(500);
const MAX_DELAY: Duration = Duration::from_secs(30);

/// The reconnect policy: 500 ms doubling to a 30 s cap, plus jitter.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use tuclaw_core::v3::Backoff;
///
/// let mut backoff = Backoff::default();
/// assert_eq!(backoff.next_delay(0.0), Duration::from_millis(500));
/// assert_eq!(backoff.next_delay(0.5), Duration::from_millis(1500));
/// backoff.reset();
/// assert_eq!(backoff.next_delay(0.0), Duration::from_millis(500));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backoff {
    next: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Backoff {
            next: INITIAL_DELAY,
        }
    }
}

impl Backoff {
    /// Returns the delay before the next attempt and doubles the following one.
    ///
    /// `jitter` is a fraction in `[0, 1)` (random in the app, fixed in tests); the delay grows by
    /// that fraction of itself. Values outside the range are clamped.
    pub fn next_delay(&mut self, jitter: f64) -> Duration {
        let delay = self.next;
        self.next = (delay * 2).min(MAX_DELAY);
        delay + delay.mul_f64(jitter.clamp(0.0, 1.0))
    }

    /// Starts over from the initial delay, after a successful connection.
    pub fn reset(&mut self) {
        self.next = INITIAL_DELAY;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_envelopes_map_to_their_codes() {
        let cases = [
            (401, "unauthorized", ApiError::Unauthorized),
            (404, "not_found", ApiError::NotFound),
            (400, "invalid_request", ApiError::Invalid("bad".into())),
            (409, "conflict", ApiError::Conflict),
            (503, "unavailable", ApiError::Unavailable),
            (
                418,
                "teapot",
                ApiError::Transport("HTTP 418 teapot: bad".into()),
            ),
        ];
        for (status, code, expected) in cases {
            let body = format!(r#"{{"error": {{"code": "{code}", "message": "bad"}}}}"#);
            assert_eq!(
                ApiError::from_status(status, body.as_bytes()),
                expected,
                "{code}"
            );
        }
    }

    #[test]
    fn bodies_that_are_not_envelopes_map_by_status() {
        assert_eq!(ApiError::from_status(401, b"nope"), ApiError::Unauthorized);
        assert_eq!(
            ApiError::from_status(502, b"<html>bad gateway</html>"),
            ApiError::Transport("HTTP 502".into())
        );
    }

    #[test]
    fn backoff_doubles_to_the_cap_and_resets() {
        let mut backoff = Backoff::default();
        let mut delays = Vec::new();
        for _ in 0..9 {
            delays.push(backoff.next_delay(0.0).as_millis());
        }
        assert_eq!(
            delays,
            vec![500, 1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000]
        );
        backoff.reset();
        assert_eq!(backoff.next_delay(0.0), INITIAL_DELAY);
    }

    #[test]
    fn backoff_jitter_is_a_clamped_fraction_of_the_delay() {
        let mut backoff = Backoff::default();
        assert_eq!(backoff.next_delay(0.25), Duration::from_millis(625));
        assert_eq!(backoff.next_delay(7.0), Duration::from_millis(2000));
        assert_eq!(backoff.next_delay(-1.0), Duration::from_millis(2000));
    }

    #[test]
    fn focus_on_a_closed_connection_is_a_transport_error() {
        let (control, control_rx) = futures::channel::mpsc::unbounded();
        let (_frames_tx, frames) = futures::channel::mpsc::unbounded();
        let connection = Connection { frames, control };
        assert_eq!(connection.focus(vec![SurfaceId(1)]), Ok(()));
        drop(control_rx);
        assert!(matches_transport(connection.focus(vec![SurfaceId(1)])));
    }

    fn matches_transport(result: Result<(), ApiError>) -> bool {
        match result {
            Err(ApiError::Transport(_)) => true,
            Err(ApiError::Unauthorized) => false,
            Err(ApiError::NotFound) => false,
            Err(ApiError::Invalid(_)) => false,
            Err(ApiError::Conflict) => false,
            Err(ApiError::Unavailable) => false,
            Err(ApiError::Decode(_)) => false,
            Ok(()) => false,
        }
    }
}
