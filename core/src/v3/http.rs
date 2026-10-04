use std::time::Duration;

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::future::BoxFuture;
use futures::{SinkExt, StreamExt};
use reqwest::{RequestBuilder, Response};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::runtime::Handle;
use tokio::time::{Instant, sleep_until, timeout};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use super::dto::Seq;
use super::frames::{ClientFrame, Frame, decode, encode};
use super::runtime::{handle, spawn};
use super::transport::{ApiError, Connection, Transport};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const HEARTBEAT_DEADLINE: Duration = Duration::from_secs(60);

/// The bearer token the daemon's optional `TUCLAW_CLIENT_TOKEN` holds.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::ClientToken;
///
/// let ClientToken(raw) = ClientToken("secret".into());
/// assert_eq!(raw, "secret");
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct ClientToken(pub String);

impl std::fmt::Debug for ClientToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ClientToken(..)")
    }
}

/// The daemon's `/api/v3` over HTTP and its event socket over WebSocket.
///
/// Every call runs on the `core` I/O runtime and returns a future that needs no runtime in the
/// caller and aborts its request when dropped. The event socket is served by one task that reads
/// frames, sends the client's frames and closes the socket when the server falls silent for the
/// heartbeat deadline.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::{ClientToken, HttpTransport};
///
/// let transport = HttpTransport::new("http://192.168.1.10:9090", Some(ClientToken("t".into())));
/// assert!(transport.is_ok());
/// assert!(HttpTransport::new("http://192.168.1.10:9090", None).is_ok());
/// assert!(HttpTransport::new("ftp://example", None).is_err());
/// ```
#[derive(Clone)]
pub struct HttpTransport {
    client: reqwest::Client,
    api: String,
    events: String,
    token: Option<ClientToken>,
    handle: Handle,
    heartbeat_deadline: Duration,
}

impl HttpTransport {
    /// Creates a transport for the daemon at `base_url` (`http://host:port`); every request carries
    /// `token` as a bearer when one is given, and no `Authorization` header otherwise.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Invalid`] when `base_url` is not an `http://` URL, and
    /// [`ApiError::Transport`] when the HTTP client cannot be built.
    pub fn new(base_url: &str, token: Option<ClientToken>) -> Result<HttpTransport, ApiError> {
        let base_url = base_url.trim_end_matches('/');
        let Some(authority) = base_url.strip_prefix("http://") else {
            return Err(ApiError::Invalid(format!(
                "the daemon URL must start with http://, got {base_url}"
            )));
        };
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .no_proxy()
            .build()
            .map_err(|error| ApiError::Transport(error.to_string()))?;
        Ok(HttpTransport {
            client,
            api: format!("{base_url}/api/v3"),
            events: format!("ws://{authority}/api/v3/events"),
            token,
            handle: handle(),
            heartbeat_deadline: HEARTBEAT_DEADLINE,
        })
    }

    /// Runs the transport's I/O on the caller's tokio runtime instead of the `core` one.
    pub fn with_handle(self, handle: Handle) -> HttpTransport {
        HttpTransport { handle, ..self }
    }

    /// Changes how long the event socket waits for any server message before it closes.
    pub fn with_heartbeat_deadline(self, heartbeat_deadline: Duration) -> HttpTransport {
        HttpTransport {
            heartbeat_deadline,
            ..self
        }
    }

    fn authorized(&self, request: RequestBuilder) -> RequestBuilder {
        let Some(ClientToken(token)) = &self.token else {
            return request;
        };
        request.bearer_auth(token)
    }
}

impl Transport for HttpTransport {
    fn get(&self, path: &str) -> BoxFuture<'static, Result<Value, ApiError>> {
        let request = self.authorized(self.client.get(format!("{}{path}", self.api)));
        spawn(
            &self.handle,
            async move { answer(request.send().await).await },
        )
    }

    fn fetch(&self, path: &str) -> BoxFuture<'static, Result<Vec<u8>, ApiError>> {
        let request = self.authorized(self.client.get(format!("{}{path}", self.api)));
        spawn(
            &self.handle,
            async move { bytes(request.send().await).await },
        )
    }

    fn post(&self, path: &str, body: Option<Value>) -> BoxFuture<'static, Result<Value, ApiError>> {
        let request = self.authorized(self.client.post(format!("{}{path}", self.api)));
        let request = match body {
            Some(body) => request.json(&body),
            None => request,
        };
        spawn(
            &self.handle,
            async move { answer(request.send().await).await },
        )
    }

    fn connect(&self, since: Option<Seq>) -> BoxFuture<'static, Result<Connection, ApiError>> {
        let url = match since {
            Some(Seq(since)) => format!("{}?since={since}", self.events),
            None => self.events.clone(),
        };
        let token = self.token.clone();
        let deadline = self.heartbeat_deadline;
        spawn(&self.handle, async move {
            let mut request = url
                .as_str()
                .into_client_request()
                .map_err(handshake_error)?;
            if let Some(ClientToken(token)) = token {
                let bearer = HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|error| ApiError::Invalid(error.to_string()))?;
                request.headers_mut().insert(AUTHORIZATION, bearer);
            }
            let Ok(connected) = timeout(CONNECT_TIMEOUT, connect_async(request)).await else {
                return Err(ApiError::Transport(
                    "timed out opening the event socket".into(),
                ));
            };
            let (socket, _response) = connected.map_err(handshake_error)?;
            let (frames_tx, frames) = mpsc::unbounded();
            let (control, control_rx) = mpsc::unbounded();
            tokio::spawn(pump(socket, frames_tx, control_rx, deadline));
            Ok(Connection { frames, control })
        })
    }
}

async fn answer(sent: reqwest::Result<Response>) -> Result<Value, ApiError> {
    let response = sent.map_err(request_error)?;
    let status = response.status();
    let body = response.bytes().await.map_err(request_error)?;
    if !status.is_success() {
        return Err(ApiError::from_status(status.as_u16(), &body));
    }
    if body.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&body).map_err(|error| ApiError::Decode(error.to_string()))
}

async fn bytes(sent: reqwest::Result<Response>) -> Result<Vec<u8>, ApiError> {
    let response = sent.map_err(request_error)?;
    let status = response.status();
    let body = response.bytes().await.map_err(request_error)?;
    if !status.is_success() {
        return Err(ApiError::from_status(status.as_u16(), &body));
    }
    Ok(body.to_vec())
}

fn request_error(error: reqwest::Error) -> ApiError {
    if error.is_timeout() {
        return ApiError::Transport("the daemon did not answer in time".into());
    }
    ApiError::Transport(error.to_string())
}

fn handshake_error(error: WsError) -> ApiError {
    let WsError::Http(response) = &error else {
        return ApiError::Transport(error.to_string());
    };
    let status = response.status().as_u16();
    let body = response.body().clone().unwrap_or_default();
    ApiError::from_status(status, &body)
}

async fn pump(
    mut socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    frames: UnboundedSender<Frame>,
    mut control: UnboundedReceiver<ClientFrame>,
    deadline: Duration,
) {
    let mut heard = Instant::now();
    loop {
        tokio::select! {
            inbound = socket.next() => {
                let Some(Ok(message)) = inbound else {
                    break;
                };
                heard = Instant::now();
                match message {
                    Message::Text(text) => {
                        let Ok(frame) = decode(text.as_str()) else {
                            continue;
                        };
                        if frames.unbounded_send(frame).is_err() {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    Message::Binary(_) => {}
                    Message::Ping(_) => {}
                    Message::Pong(_) => {}
                    Message::Frame(_) => {}
                }
            }
            outbound = control.next() => {
                let Some(frame) = outbound else {
                    break;
                };
                if socket.send(Message::text(encode(&frame))).await.is_err() {
                    break;
                }
            }
            () = sleep_until(heard + deadline) => break,
        }
    }
    socket.close(None).await.ok();
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::testing::{Events, FakeDaemon, Reply};
    use crate::v3::client::Client;
    use crate::v3::dto::{
        AgentId, ClientMessageId, InputId, MessageId, Post, Posted, RunId, SurfaceId,
    };
    use crate::v3::frames::Frame;

    const TOKEN: &str = "s3cret";

    fn within<T>(future: impl Future<Output = T>) -> T {
        handle()
            .block_on(async { timeout(Duration::from_secs(5), future).await })
            .expect("the future finished in time")
    }

    fn transport(daemon: &FakeDaemon) -> HttpTransport {
        HttpTransport::new(&daemon.url(), Some(ClientToken(TOKEN.into())))
            .expect("the URL is valid")
    }

    fn client(daemon: &FakeDaemon) -> Client {
        Client::new(Arc::new(transport(daemon)))
    }

    fn json_reply(status: u16, body: &str) -> Reply {
        Reply::Json {
            status,
            body: body.to_string(),
        }
    }

    fn eventually(check: impl Fn() -> bool) -> bool {
        for _ in 0..200 {
            if check() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn rest_calls_carry_the_bearer_token_and_decode_the_body() {
        let daemon = FakeDaemon::start();
        daemon.route(
            "GET",
            "/api/v3/surfaces",
            json_reply(200, include_str!("../../testdata/v3/surfaces.json")),
        );
        let surfaces = within(client(&daemon).surfaces()).expect("the surfaces load");
        assert_eq!(surfaces.len(), 2);
        let requests = daemon.requests();
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].target, "/api/v3/surfaces");
        assert_eq!(requests[0].authorization.as_deref(), Some("Bearer s3cret"));
    }

    #[test]
    fn messages_ask_for_the_newest_page_with_a_limit() {
        let daemon = FakeDaemon::start();
        daemon.route(
            "GET",
            "/api/v3/surfaces/1/messages",
            json_reply(200, include_str!("../../testdata/v3/messages_page.json")),
        );
        let page = within(client(&daemon).messages(SurfaceId(1), 50)).expect("the page loads");
        assert!(page.has_more);
        assert_eq!(
            daemon.requests()[0].target,
            "/api/v3/surfaces/1/messages?limit=50"
        );
    }

    #[test]
    fn error_statuses_map_to_api_errors() {
        let daemon = FakeDaemon::start();
        let cases = [
            (
                401,
                r#"{"error": {"code": "unauthorized", "message": "x"}}"#,
                ApiError::Unauthorized,
            ),
            (
                404,
                r#"{"error": {"code": "not_found", "message": "x"}}"#,
                ApiError::NotFound,
            ),
            (
                400,
                r#"{"error": {"code": "invalid_request", "message": "no agent"}}"#,
                ApiError::Invalid("no agent".into()),
            ),
            (
                409,
                r#"{"error": {"code": "conflict", "message": "x"}}"#,
                ApiError::Conflict,
            ),
            (
                503,
                r#"{"error": {"code": "unavailable", "message": "x"}}"#,
                ApiError::Unavailable,
            ),
            (
                500,
                "<html>oops</html>",
                ApiError::Transport("HTTP 500".into()),
            ),
        ];
        for (status, body, expected) in cases {
            daemon.route("GET", "/api/v3/agents", json_reply(status, body));
            assert_eq!(within(client(&daemon).agents()), Err(expected), "{status}");
        }
    }

    #[test]
    fn an_unknown_route_is_not_found() {
        let daemon = FakeDaemon::start();
        assert_eq!(within(client(&daemon).agents()), Err(ApiError::NotFound));
    }

    #[test]
    fn a_body_off_the_contract_is_a_decode_error() {
        let daemon = FakeDaemon::start();
        daemon.route(
            "GET",
            "/api/v3/agents",
            json_reply(200, r#"{"agents": []}"#),
        );
        let result = within(client(&daemon).agents());
        let Err(ApiError::Decode(_)) = result else {
            panic!("expected a decode error, got {result:?}");
        };
    }

    #[test]
    fn post_sends_the_body_and_decodes_the_202() {
        let daemon = FakeDaemon::start();
        daemon.route(
            "POST",
            "/api/v3/surfaces/1/messages",
            json_reply(
                202,
                r#"{"message_id": 9193, "input_id": 42, "agent_id": 1}"#,
            ),
        );
        let post = Post {
            text: "Лисички?".into(),
            addressed_agent_id: Some(AgentId(3)),
            client_message_id: ClientMessageId("8b0c".into()),
        };
        let posted = within(client(&daemon).post(SurfaceId(1), &post)).expect("the post lands");
        assert_eq!(
            posted,
            Posted {
                message_id: MessageId(9193),
                input_id: Some(InputId(42)),
                agent_id: AgentId(1),
            }
        );
        let sent: serde_json::Value =
            serde_json::from_str(&daemon.requests()[0].body).expect("the body is JSON");
        assert_eq!(
            sent,
            json!({"text": "Лисички?", "addressed_agent_id": 3, "client_message_id": "8b0c"})
        );
    }

    #[test]
    fn attachments_are_fetched_raw_with_the_bearer() {
        let daemon = FakeDaemon::start();
        daemon.route(
            "GET",
            "/api/v3/attachments/5",
            json_reply(200, "OggS-bytes"),
        );
        let bytes = within(client(&daemon).attachment(crate::v3::dto::AttachmentId(5)))
            .expect("the attachment");
        assert_eq!(bytes, b"OggS-bytes".to_vec());
        assert_eq!(
            daemon.requests()[0].authorization.as_deref(),
            Some("Bearer s3cret")
        );
        assert_eq!(
            within(client(&daemon).attachment(crate::v3::dto::AttachmentId(6))),
            Err(ApiError::NotFound)
        );
    }

    #[test]
    fn interrupt_accepts_an_empty_202_and_reports_a_conflict() {
        let daemon = FakeDaemon::start();
        let run = RunId("r1".into());
        daemon.route("POST", "/api/v3/runs/r1/interrupt", json_reply(202, ""));
        assert_eq!(within(client(&daemon).interrupt(&run)), Ok(()));
        assert_eq!(daemon.requests()[0].body, "");
        daemon.route(
            "POST",
            "/api/v3/runs/r1/interrupt",
            json_reply(
                409,
                r#"{"error": {"code": "conflict", "message": "not live"}}"#,
            ),
        );
        assert_eq!(
            within(client(&daemon).interrupt(&run)),
            Err(ApiError::Conflict)
        );
    }

    #[test]
    fn dropping_a_pending_call_aborts_its_request() {
        let daemon = FakeDaemon::start();
        daemon.route("GET", "/api/v3/agents", Reply::Hang);
        let pending = client(&daemon).agents();
        assert!(eventually(|| daemon.requests().len() == 1));
        drop(pending);
        assert!(eventually(|| daemon.dropped_requests() == 1));
    }

    #[test]
    fn a_hung_request_times_out() {
        let daemon = FakeDaemon::start();
        daemon.route("GET", "/api/v3/agents", Reply::Hang);
        let result = handle().block_on(client(&daemon).agents());
        assert_eq!(
            result,
            Err(ApiError::Transport(
                "the daemon did not answer in time".into()
            ))
        );
    }

    #[test]
    fn connect_sends_since_and_the_bearer_on_the_upgrade() {
        let daemon = FakeDaemon::start();
        let connection =
            within(client(&daemon).connect(Some(Seq(1200)))).expect("the socket opens");
        let upgrades = daemon.upgrades();
        assert_eq!(upgrades[0].target, "/api/v3/events?since=1200");
        assert_eq!(upgrades[0].authorization.as_deref(), Some("Bearer s3cret"));
        drop(connection);
        within(client(&daemon).connect(None)).expect("the socket opens");
        assert_eq!(daemon.upgrades()[1].target, "/api/v3/events");
    }

    #[test]
    fn without_a_token_no_authorization_header_is_sent() {
        let daemon = FakeDaemon::start();
        daemon.route(
            "GET",
            "/api/v3/surfaces",
            json_reply(200, include_str!("../../testdata/v3/surfaces.json")),
        );
        let open = Client::new(Arc::new(
            HttpTransport::new(&daemon.url(), None).expect("the URL is valid"),
        ));
        within(open.surfaces()).expect("the surfaces load");
        assert_eq!(daemon.requests()[0].authorization, None);
        within(open.connect(None)).expect("the socket opens");
        assert_eq!(daemon.upgrades()[0].authorization, None);
    }

    #[test]
    fn a_rejected_upgrade_maps_its_status() {
        let daemon = FakeDaemon::start();
        daemon.events(Events::Reject(401));
        let result = within(client(&daemon).connect(None));
        let Err(error) = result else {
            panic!("expected a rejected upgrade");
        };
        assert_eq!(error, ApiError::Unauthorized);
    }

    #[test]
    fn a_handshake_that_never_finishes_times_out() {
        let daemon = FakeDaemon::start();
        daemon.events(Events::NoHandshake);
        let result = handle().block_on(client(&daemon).connect(None));
        let Err(error) = result else {
            panic!("expected a timeout");
        };
        assert_eq!(
            error,
            ApiError::Transport("timed out opening the event socket".into())
        );
    }

    #[test]
    fn scripted_lines_arrive_in_order_and_a_malformed_one_is_skipped() {
        let daemon = FakeDaemon::start();
        daemon.events(Events::LinesThenClose(vec![
            include_str!("../../testdata/v3/frames/hello.json")
                .trim()
                .to_string(),
            "not json".to_string(),
            include_str!("../../testdata/v3/frames/run_started.json")
                .trim()
                .to_string(),
        ]));
        let mut connection = within(client(&daemon).connect(None)).expect("the socket opens");
        let Some(Frame::Hello(_)) = within(connection.frames.next()) else {
            panic!("expected hello first");
        };
        let Some(Frame::RunStarted(_)) = within(connection.frames.next()) else {
            panic!("expected run.started second");
        };
        assert_eq!(within(connection.frames.next()), None);
    }

    #[test]
    fn focus_reaches_the_server_as_the_contract_frame() {
        let daemon = FakeDaemon::start();
        let connection = within(client(&daemon).connect(None)).expect("the socket opens");
        connection
            .focus(vec![SurfaceId(1), SurfaceId(3)])
            .expect("the socket is open");
        assert!(eventually(|| !daemon.client_frames().is_empty()));
        assert_eq!(
            daemon.client_frames(),
            vec![include_str!("../../testdata/v3/frames/focus.json").to_string()]
        );
    }

    #[test]
    fn a_silent_server_is_dropped_after_the_heartbeat_deadline() {
        let daemon = FakeDaemon::start();
        let transport = transport(&daemon).with_heartbeat_deadline(Duration::from_millis(200));
        let mut connection = within(transport.connect(None)).expect("the socket opens");
        assert_eq!(within(connection.frames.next()), None);
        assert!(eventually(|| daemon.sockets_closed() == 1));
    }

    #[test]
    fn dropping_the_connection_closes_the_socket() {
        let daemon = FakeDaemon::start();
        let connection = within(client(&daemon).connect(None)).expect("the socket opens");
        assert_eq!(daemon.sockets_closed(), 0);
        drop(connection);
        assert!(eventually(|| daemon.sockets_closed() == 1));
    }

    #[test]
    fn only_http_urls_are_accepted() {
        let cases = ["https://example", "ws://example", "example:9090"];
        for url in cases {
            let Err(ApiError::Invalid(_)) =
                HttpTransport::new(url, Some(ClientToken(TOKEN.into())))
            else {
                panic!("{url} should be rejected");
            };
        }
        let trailing = HttpTransport::new("http://host:9090/", Some(ClientToken(TOKEN.into())))
            .expect("a trailing slash is fine");
        assert_eq!(trailing.api, "http://host:9090/api/v3");
        assert_eq!(trailing.events, "ws://host:9090/api/v3/events");
    }

    #[test]
    fn the_token_is_not_printed() {
        assert_eq!(
            format!("{:?}", ClientToken(TOKEN.into())),
            "ClientToken(..)"
        );
    }
}
