//! Test doubles for the daemon side of `/api/v3`.
//!
//! [`FakeDaemon`](crate::testing::FakeDaemon) is a real loopback HTTP and WebSocket server on the `core` I/O runtime, so the
//! HTTP transport is exercised end to end without a daemon.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{
    Callback, ErrorResponse, Request, Response,
};
use tokio_tungstenite::tungstenite::http::StatusCode;

use crate::v3::runtime::handle;

/// How the fake answers one REST route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// A status code and a JSON body (may be empty).
    Json {
        /// The HTTP status.
        status: u16,
        /// The body text.
        body: String,
    },
    /// Never answers; the fake counts the request in [`FakeDaemon::dropped_requests`] once the
    /// client gives up on it.
    Hang,
}

/// How the fake serves the event socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Events {
    /// Sends these lines after the handshake, then stays open, recording the client's frames.
    Lines(Vec<String>),
    /// Sends these lines after the handshake, then closes the socket.
    LinesThenClose(Vec<String>),
    /// Rejects the upgrade with this status and the contract's error envelope.
    Reject(u16),
    /// Accepts the TCP connection and never answers the handshake.
    NoHandshake,
}

/// One request the fake received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// The HTTP method.
    pub method: String,
    /// The path with its query string.
    pub target: String,
    /// The `Authorization` header, if any.
    pub authorization: Option<String>,
    /// The request body.
    pub body: String,
}

#[derive(Debug)]
struct State {
    routes: HashMap<(String, String), Reply>,
    events: Events,
    requests: Vec<Recorded>,
    upgrades: Vec<Recorded>,
    client_frames: Vec<String>,
    sockets_closed: usize,
    dropped_requests: usize,
}

/// A canned loopback daemon answering `/api/v3` routes and the event socket.
///
/// # Examples
///
/// ```
/// use tuclaw_core::testing::{FakeDaemon, Reply};
///
/// let daemon = FakeDaemon::start();
/// daemon.route("GET", "/api/v3/surfaces", Reply::Json { status: 200, body: "[]".into() });
/// assert!(daemon.url().starts_with("http://127.0.0.1:"));
/// ```
pub struct FakeDaemon {
    addr: SocketAddr,
    state: Arc<Mutex<State>>,
    server: JoinHandle<()>,
}

impl FakeDaemon {
    /// Starts the fake on a free loopback port; unknown routes answer `404 not_found` and the
    /// event socket starts as [`Events::Lines`] with no lines.
    ///
    /// # Panics
    ///
    /// Panics when no loopback port can be bound.
    pub fn start() -> FakeDaemon {
        let runtime = handle();
        let listener = runtime
            .block_on(TcpListener::bind("127.0.0.1:0"))
            .expect("a loopback port is free");
        let addr = listener.local_addr().expect("the listener has an address");
        let state = Arc::new(Mutex::new(State {
            routes: HashMap::new(),
            events: Events::Lines(Vec::new()),
            requests: Vec::new(),
            upgrades: Vec::new(),
            client_frames: Vec::new(),
            sockets_closed: 0,
            dropped_requests: 0,
        }));
        let shared = state.clone();
        let server = runtime.spawn(async move {
            loop {
                let Ok((stream, _peer)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(serve(stream, shared.clone()));
            }
        });
        FakeDaemon {
            addr,
            state,
            server,
        }
    }

    /// Returns the base URL a transport is pointed at.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Sets the reply for `method` on `path` (without the query string).
    pub fn route(&self, method: &str, path: &str, reply: Reply) {
        lock(&self.state)
            .routes
            .insert((method.to_string(), path.to_string()), reply);
    }

    /// Sets how the next event socket connections are served.
    pub fn events(&self, events: Events) {
        lock(&self.state).events = events;
    }

    /// Returns the REST requests received so far.
    pub fn requests(&self) -> Vec<Recorded> {
        lock(&self.state).requests.clone()
    }

    /// Returns the WebSocket upgrade requests received so far.
    pub fn upgrades(&self) -> Vec<Recorded> {
        lock(&self.state).upgrades.clone()
    }

    /// Returns the text frames clients sent on the event socket.
    pub fn client_frames(&self) -> Vec<String> {
        lock(&self.state).client_frames.clone()
    }

    /// Returns how many event sockets the clients closed.
    pub fn sockets_closed(&self) -> usize {
        lock(&self.state).sockets_closed
    }

    /// Returns how many [`Reply::Hang`] requests the clients abandoned.
    pub fn dropped_requests(&self) -> usize {
        lock(&self.state).dropped_requests
    }
}

impl Drop for FakeDaemon {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    match state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

async fn serve(mut stream: TcpStream, state: Arc<Mutex<State>>) {
    let Some(head) = peek_head(&stream).await else {
        return;
    };
    let mut lines = head.lines();
    let Some(request_line) = lines.next() else {
        return;
    };
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return;
    };
    let path = target.split('?').next().unwrap_or(target);
    if path == "/api/v3/events" {
        serve_events(stream, state).await;
        return;
    }
    let mut authorization = None;
    let mut length = 0;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("authorization") {
            authorization = Some(value.to_string());
        }
        if name.eq_ignore_ascii_case("content-length") {
            length = value.parse().unwrap_or(0);
        }
    }
    let mut consumed = vec![0; head.len() + 4];
    if stream.read_exact(&mut consumed).await.is_err() {
        return;
    }
    let mut body = vec![0; length];
    if stream.read_exact(&mut body).await.is_err() {
        return;
    }
    let reply = {
        let mut state = lock(&state);
        state.requests.push(Recorded {
            method: method.to_string(),
            target: target.to_string(),
            authorization,
            body: String::from_utf8_lossy(&body).into_owned(),
        });
        state
            .routes
            .get(&(method.to_string(), path.to_string()))
            .cloned()
    };
    let reply = reply.unwrap_or(Reply::Json {
        status: 404,
        body: envelope("not_found"),
    });
    match reply {
        Reply::Json { status, body } => {
            let response = format!(
                "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                reason(status),
                body.len()
            );
            stream.write_all(response.as_bytes()).await.ok();
            stream.shutdown().await.ok();
        }
        Reply::Hang => {
            let mut buf = [0; 512];
            loop {
                match stream.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            lock(&state).dropped_requests += 1;
        }
    }
}

async fn peek_head(stream: &TcpStream) -> Option<String> {
    let mut buf = vec![0; 16 * 1024];
    for _ in 0..400 {
        let seen = stream.peek(&mut buf).await.ok()?;
        if seen == 0 {
            return None;
        }
        let text = String::from_utf8_lossy(&buf[..seen]);
        if let Some(end) = text.find("\r\n\r\n") {
            return Some(text[..end].to_string());
        }
        sleep(Duration::from_millis(5)).await;
    }
    None
}

async fn serve_events(stream: TcpStream, state: Arc<Mutex<State>>) {
    let events = lock(&state).events.clone();
    if events == Events::NoHandshake {
        let mut stream = stream;
        let mut buf = [0; 512];
        while let Ok(read) = stream.read(&mut buf).await {
            if read == 0 {
                break;
            }
        }
        return;
    }
    let rejection = match &events {
        Events::Reject(status) => Some(*status),
        Events::Lines(_) => None,
        Events::LinesThenClose(_) => None,
        Events::NoHandshake => None,
    };
    let callback = Upgrade {
        state: state.clone(),
        rejection,
    };
    let Ok(mut socket) = accept_hdr_async(stream, callback).await else {
        return;
    };
    let (lines, close) = match events {
        Events::Lines(lines) => (lines, false),
        Events::LinesThenClose(lines) => (lines, true),
        Events::Reject(_) => return,
        Events::NoHandshake => return,
    };
    for line in lines {
        if socket.send(Message::text(line)).await.is_err() {
            return;
        }
    }
    if close {
        socket.close(None).await.ok();
        return;
    }
    while let Some(Ok(message)) = socket.next().await {
        match message {
            Message::Text(text) => lock(&state).client_frames.push(text.as_str().to_string()),
            Message::Close(_) => break,
            Message::Binary(_) => {}
            Message::Ping(_) => {}
            Message::Pong(_) => {}
            Message::Frame(_) => {}
        }
    }
    lock(&state).sockets_closed += 1;
}

struct Upgrade {
    state: Arc<Mutex<State>>,
    rejection: Option<u16>,
}

impl Callback for Upgrade {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        let authorization = request
            .headers()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        lock(&self.state).upgrades.push(Recorded {
            method: request.method().to_string(),
            target: request.uri().to_string(),
            authorization,
            body: String::new(),
        });
        let Some(status) = self.rejection else {
            return Ok(response);
        };
        let mut rejected = ErrorResponse::new(Some(envelope(&code_for(status))));
        *rejected.status_mut() =
            StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        Err(rejected)
    }
}

fn envelope(code: &str) -> String {
    format!(r#"{{"error": {{"code": "{code}", "message": "fake daemon"}}}}"#)
}

fn code_for(status: u16) -> String {
    match status {
        400 => "invalid_request",
        401 => "unauthorized",
        404 => "not_found",
        409 => "conflict",
        503 => "unavailable",
        other => return format!("status_{other}"),
    }
    .to_string()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        409 => "Conflict",
        503 => "Service Unavailable",
        _other => "Status",
    }
}
