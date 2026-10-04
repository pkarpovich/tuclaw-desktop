use std::future::Future;
use std::sync::Arc;

use futures::FutureExt;
use futures::future::{BoxFuture, ready};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::dto::{
    Agent, AgentId, AgentPatch, AttachmentId, AvatarSet, AvatarUrl, ImageKind, Me, MePatch,
    MessageId, MessagesPage, Post, Posted, RunDetail, RunId, Seq, Surface, SurfaceId, WiringChange,
};
use super::http::{ClientToken, HttpTransport};
use super::mock::MockTransport;
use super::transport::{ApiError, Body, Connection, Method, Request, Transport};

/// The typed `/api/v3` client: every call of the contract over one [`Transport`].
///
/// Every call returns a `Send + 'static` future, so a UI can hand it to a background executor
/// without borrowing the client. The request starts when the call is made, not when the future is
/// first polled, and dropping the future aborts it. Paths and query strings are built here and nowhere else.
///
/// # Examples
///
/// ```
/// use tuclaw_core::v3::{Client, ClientToken};
///
/// let client = Client::http("http://192.168.1.10:9090", Some(ClientToken("t".into()))).unwrap();
/// let _surfaces = client.surfaces();
/// ```
#[derive(Clone)]
pub struct Client {
    transport: Arc<dyn Transport>,
}

impl Client {
    /// Creates a client over any transport.
    pub fn new(transport: Arc<dyn Transport>) -> Client {
        Client { transport }
    }

    /// Creates a client for the daemon at `base_url` (`http://host:port`).
    ///
    /// # Errors
    ///
    /// Returns the [`ApiError`] of [`HttpTransport::new`].
    pub fn http(base_url: &str, token: Option<ClientToken>) -> Result<Client, ApiError> {
        Ok(Client::new(Arc::new(HttpTransport::new(base_url, token)?)))
    }

    /// Creates a client over the in-process mock daemon; the caller keeps `transport` to drive it.
    pub fn mock(transport: &MockTransport) -> Client {
        Client::new(Arc::new(transport.clone()))
    }

    /// Fetches the sidebar: every surface, ordered by `sort_order`.
    pub fn surfaces(
        &self,
    ) -> impl Future<Output = Result<Vec<Surface>, ApiError>> + Send + 'static {
        let request = self.transport.get("/surfaces");
        async move { body(request.await?) }
    }

    /// Fetches every agent.
    pub fn agents(&self) -> impl Future<Output = Result<Vec<Agent>, ApiError>> + Send + 'static {
        let request = self.transport.get("/agents");
        async move { body(request.await?) }
    }

    /// Fetches the newest page of a surface's messages, oldest first.
    pub fn messages(
        &self,
        surface: SurfaceId,
        limit: u32,
    ) -> impl Future<Output = Result<MessagesPage, ApiError>> + Send + 'static {
        let SurfaceId(surface) = surface;
        let request = self
            .transport
            .get(&format!("/surfaces/{surface}/messages?limit={limit}"));
        async move { body(request.await?) }
    }

    /// Fetches the page of a surface's messages just older than `before`, oldest first.
    pub fn messages_before(
        &self,
        surface: SurfaceId,
        before: MessageId,
        limit: u32,
    ) -> impl Future<Output = Result<MessagesPage, ApiError>> + Send + 'static {
        let SurfaceId(surface) = surface;
        let MessageId(before) = before;
        let request = self.transport.get(&format!(
            "/surfaces/{surface}/messages?before={before}&limit={limit}"
        ));
        async move { body(request.await?) }
    }

    /// Posts a message to a surface; the reply arrives on the event socket.
    pub fn post(
        &self,
        surface: SurfaceId,
        post: &Post,
    ) -> impl Future<Output = Result<Posted, ApiError>> + Send + 'static {
        let SurfaceId(surface) = surface;
        let request = match serde_json::to_value(post) {
            Ok(encoded) => self
                .transport
                .post(&format!("/surfaces/{surface}/messages"), Some(encoded)),
            Err(error) => ready(Err(ApiError::Decode(error.to_string()))).boxed(),
        };
        async move { body(request.await?) }
    }

    /// Fetches a run with its steps.
    pub fn run(
        &self,
        run: &RunId,
    ) -> impl Future<Output = Result<RunDetail, ApiError>> + Send + 'static {
        let RunId(run) = run;
        let request = self.transport.get(&format!("/runs/{run}"));
        async move { body(request.await?) }
    }

    /// Stops a live run; it then ends with `run.finished{terminal_reason: "interrupted"}`.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Conflict`] when the run is not live.
    pub fn interrupt(
        &self,
        run: &RunId,
    ) -> impl Future<Output = Result<(), ApiError>> + Send + 'static {
        let RunId(run) = run;
        let request = self.transport.post(&format!("/runs/{run}/interrupt"), None);
        async move {
            request.await?;
            Ok(())
        }
    }

    /// Fetches an attachment's bytes (v3.1 draft).
    pub fn attachment(
        &self,
        attachment: AttachmentId,
    ) -> impl Future<Output = Result<Vec<u8>, ApiError>> + Send + 'static {
        let AttachmentId(attachment) = attachment;
        self.transport.fetch(&format!("/attachments/{attachment}"))
    }

    /// Fetches the person using the client.
    pub fn me(&self) -> impl Future<Output = Result<Me, ApiError>> + Send + 'static {
        let request = self.transport.get("/me");
        async move { body(request.await?) }
    }

    /// Fetches an avatar's bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Decode`] when the URL is not under `/api/v3`, and
    /// [`ApiError::NotFound`] when the picture is gone, which a view shows as initials.
    pub fn avatar(
        &self,
        url: &AvatarUrl,
    ) -> impl Future<Output = Result<Vec<u8>, ApiError>> + Send + 'static {
        let AvatarUrl(url) = url;
        match url.strip_prefix(API_PREFIX) {
            Some(path) => self.transport.fetch(path),
            None => ready(Err(ApiError::Decode(format!(
                "avatar url {url} is outside {API_PREFIX}"
            ))))
            .boxed(),
        }
    }

    /// Stores a new avatar and returns where it is served from.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Invalid`] when the daemon refuses the image (type or size).
    pub fn set_avatar(
        &self,
        owner: AvatarOwner,
        kind: ImageKind,
        bytes: Vec<u8>,
    ) -> impl Future<Output = Result<AvatarUrl, ApiError>> + Send + 'static {
        let request = self.transport.send(Request {
            method: Method::Put,
            path: avatar_path(owner),
            body: Body::Image { kind, bytes },
        });
        async move {
            let AvatarSet { avatar_url } = body(request.await?)?;
            Ok(avatar_url)
        }
    }

    /// Removes an avatar, so its owner is drawn with initials again.
    pub fn clear_avatar(
        &self,
        owner: AvatarOwner,
    ) -> impl Future<Output = Result<(), ApiError>> + Send + 'static {
        let request = self.transport.send(Request {
            method: Method::Delete,
            path: avatar_path(owner),
            body: Body::Empty,
        });
        async move {
            request.await?;
            Ok(())
        }
    }

    /// Changes the user's name or description and returns the profile as stored.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Invalid`] for an empty name or a description over the limit.
    pub fn update_me(
        &self,
        patch: &MePatch,
    ) -> impl Future<Output = Result<Me, ApiError>> + Send + 'static {
        let request = self.json_write(Method::Patch, "/me".into(), patch);
        async move { body(request.await?) }
    }

    /// Changes an agent's description or model and returns the agent as stored.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Invalid`] for a description over the limit or a malformed model.
    pub fn update_agent(
        &self,
        agent: AgentId,
        patch: &AgentPatch,
    ) -> impl Future<Output = Result<Agent, ApiError>> + Send + 'static {
        let AgentId(agent) = agent;
        let request = self.json_write(Method::Patch, format!("/agents/{agent}"), patch);
        async move { body(request.await?) }
    }

    /// Wires an agent to a surface or changes its role there and returns the surface as stored.
    pub fn set_wiring(
        &self,
        surface: SurfaceId,
        agent: AgentId,
        change: WiringChange,
    ) -> impl Future<Output = Result<Surface, ApiError>> + Send + 'static {
        let request = self.json_write(Method::Put, wiring_path(surface, agent), &change);
        async move { body(request.await?) }
    }

    /// Takes an agent off a surface.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Conflict`] when the agent leads the surface.
    pub fn remove_wiring(
        &self,
        surface: SurfaceId,
        agent: AgentId,
    ) -> impl Future<Output = Result<(), ApiError>> + Send + 'static {
        let request = self.transport.send(Request {
            method: Method::Delete,
            path: wiring_path(surface, agent),
            body: Body::Empty,
        });
        async move {
            request.await?;
            Ok(())
        }
    }

    fn json_write<T: Serialize>(
        &self,
        method: Method,
        path: String,
        value: &T,
    ) -> BoxFuture<'static, Result<Value, ApiError>> {
        match serde_json::to_value(value) {
            Ok(encoded) => self.transport.send(Request {
                method,
                path,
                body: Body::Json(encoded),
            }),
            Err(error) => ready(Err(ApiError::Decode(error.to_string()))).boxed(),
        }
    }

    /// Opens the event socket, replaying after `since` when given.
    pub fn connect(
        &self,
        since: Option<Seq>,
    ) -> impl Future<Output = Result<Connection, ApiError>> + Send + 'static {
        self.transport.connect(since)
    }
}

/// Whose avatar a write changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AvatarOwner {
    /// An agent's.
    Agent(AgentId),
    /// The user's.
    Me,
}

const API_PREFIX: &str = "/api/v3";

fn wiring_path(surface: SurfaceId, agent: AgentId) -> String {
    let SurfaceId(surface) = surface;
    let AgentId(agent) = agent;
    format!("/surfaces/{surface}/agents/{agent}")
}

fn avatar_path(owner: AvatarOwner) -> String {
    match owner {
        AvatarOwner::Agent(AgentId(agent)) => format!("/agents/{agent}/avatar"),
        AvatarOwner::Me => "/me/avatar".into(),
    }
}

fn body<T: DeserializeOwned>(value: Value) -> Result<T, ApiError> {
    serde_json::from_value(value).map_err(|error| ApiError::Decode(error.to_string()))
}
