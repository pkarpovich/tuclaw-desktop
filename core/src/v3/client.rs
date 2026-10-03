use std::future::Future;
use std::sync::Arc;

use futures::FutureExt;
use futures::future::ready;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::dto::{Agent, MessagesPage, Post, Posted, RunDetail, RunId, Seq, Surface, SurfaceId};
use super::http::{ClientToken, HttpTransport};
use super::transport::{ApiError, Connection, Transport};

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
/// let client = Client::http("http://192.168.1.10:9090", ClientToken("t".into())).unwrap();
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
    pub fn http(base_url: &str, token: ClientToken) -> Result<Client, ApiError> {
        Ok(Client::new(Arc::new(HttpTransport::new(base_url, token)?)))
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

    /// Opens the event socket, replaying after `since` when given.
    pub fn connect(
        &self,
        since: Option<Seq>,
    ) -> impl Future<Output = Result<Connection, ApiError>> + Send + 'static {
        self.transport.connect(since)
    }
}

fn body<T: DeserializeOwned>(value: Value) -> Result<T, ApiError> {
    serde_json::from_value(value).map_err(|error| ApiError::Decode(error.to_string()))
}
