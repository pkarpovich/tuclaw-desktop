use std::future::Future;
use std::sync::Arc;

use futures::FutureExt;
use futures::future::{BoxFuture, ready};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::dto::{
    Agent, AgentId, AgentPatch, AttachmentId, AvatarSet, AvatarUrl, Group, GroupId, GroupPatch,
    ImageKind, Me, MePatch, MessageId, MessagesPage, NewGroup, Placement, Post, Posted, ReadAnswer,
    ReplyPost, RunDetail, RunId, Seq, Surface, SurfaceId, SurfacePatch, Task, TaskId, TaskRun,
    VoicePost, WiringChange,
};
use super::http::{ClientToken, HttpTransport};
use super::mock::MockTransport;
use super::transport::{ApiError, Body, Connection, Method, PublicUrl, Request, Transport};

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

    /// Taps one of an answer's suggested replies; the posted message arrives on the event socket.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Conflict`] once the replies are closed, and [`ApiError::Invalid`] for
    /// an option the answer does not offer.
    pub fn reply(
        &self,
        message: MessageId,
        tap: &ReplyPost,
    ) -> impl Future<Output = Result<Posted, ApiError>> + Send + 'static {
        let MessageId(message) = message;
        let request = match serde_json::to_value(tap) {
            Ok(encoded) => self
                .transport
                .post(&format!("/messages/{message}/reply"), Some(encoded)),
            Err(error) => ready(Err(ApiError::Decode(error.to_string()))).boxed(),
        };
        async move { body(request.await?) }
    }

    /// Posts a recorded voice message; the daemon answers once it has the transcript.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Conflict`] when the idempotency key was used on another surface.
    pub fn post_voice(
        &self,
        surface: SurfaceId,
        voice: VoicePost,
    ) -> impl Future<Output = Result<Posted, ApiError>> + Send + 'static {
        let SurfaceId(surface) = surface;
        let VoicePost {
            kind,
            bytes,
            addressed_agent_id,
            client_message_id,
        } = voice;
        let path = match addressed_agent_id {
            Some(AgentId(agent)) => format!("/surfaces/{surface}/voice?addressed_agent_id={agent}"),
            None => format!("/surfaces/{surface}/voice"),
        };
        let request = self.transport.send(Request {
            method: Method::Post,
            path,
            body: Body::Voice {
                kind,
                bytes,
                client_message_id,
            },
        });
        async move { body(request.await?) }
    }

    /// Fetches the sidebar groups, in their order.
    pub fn groups(&self) -> impl Future<Output = Result<Vec<Group>, ApiError>> + Send + 'static {
        let request = self.transport.get("/groups");
        async move { body(request.await?) }
    }

    /// Creates a sidebar group at the end of the list.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Invalid`] for an empty name.
    pub fn create_group(
        &self,
        group: &NewGroup,
    ) -> impl Future<Output = Result<Group, ApiError>> + Send + 'static {
        let request = match serde_json::to_value(group) {
            Ok(encoded) => self.transport.post("/groups", Some(encoded)),
            Err(error) => ready(Err(ApiError::Decode(error.to_string()))).boxed(),
        };
        async move { body(request.await?) }
    }

    /// Renames, re-emojis or moves a group.
    pub fn update_group(
        &self,
        group: GroupId,
        patch: &GroupPatch,
    ) -> impl Future<Output = Result<Group, ApiError>> + Send + 'static {
        let GroupId(group) = group;
        let request = self.json_write(Method::Patch, format!("/groups/{group}"), patch);
        async move { body(request.await?) }
    }

    /// Deletes a group; its surfaces become ungrouped.
    pub fn delete_group(
        &self,
        group: GroupId,
    ) -> impl Future<Output = Result<(), ApiError>> + Send + 'static {
        let GroupId(group) = group;
        let request = self.transport.send(Request {
            method: Method::Delete,
            path: format!("/groups/{group}"),
            body: Body::Empty,
        });
        async move {
            request.await?;
            Ok(())
        }
    }

    /// Renames, archives or regroups a surface and returns it as `GET /surfaces` lists it.
    pub fn update_surface(
        &self,
        surface: SurfaceId,
        patch: &SurfacePatch,
    ) -> impl Future<Output = Result<Surface, ApiError>> + Send + 'static {
        let SurfaceId(surface) = surface;
        let request = self.json_write(Method::Patch, format!("/surfaces/{surface}"), patch);
        async move { body(request.await?) }
    }

    /// Places surfaces in groups and orders them, all at once; answers with every surface.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::NotFound`] for an unknown surface or group, and changes nothing.
    pub fn reorder_surfaces(
        &self,
        placements: &[Placement],
    ) -> impl Future<Output = Result<Vec<Surface>, ApiError>> + Send + 'static {
        let request = self.json_write(Method::Put, "/surfaces/order".to_string(), placements);
        async move { body(request.await?) }
    }

    /// Marks a surface read up to `message` and clears its unread mark; the
    /// cursor only ever moves forward, and without a message only the mark is
    /// cleared.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Invalid`] when the message is not on the surface.
    pub fn mark_read(
        &self,
        surface: SurfaceId,
        message: Option<MessageId>,
    ) -> impl Future<Output = Result<ReadAnswer, ApiError>> + Send + 'static {
        let SurfaceId(surface) = surface;
        let payload = match message {
            Some(MessageId(message)) => serde_json::json!({ "message_id": message }),
            None => serde_json::json!({}),
        };
        let request = self
            .transport
            .post(&format!("/surfaces/{surface}/read"), Some(payload));
        async move { body(request.await?) }
    }

    /// Marks a surface unread, leaving its cursor where it is; the next read
    /// clears the mark.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::NotFound`] for an unknown surface.
    pub fn mark_unread(
        &self,
        surface: SurfaceId,
    ) -> impl Future<Output = Result<ReadAnswer, ApiError>> + Send + 'static {
        let SurfaceId(surface) = surface;
        let request = self
            .transport
            .post(&format!("/surfaces/{surface}/unread"), None);
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

    /// Fetches a public picture a message links to; the daemon's token is never sent.
    pub fn public_picture(
        &self,
        url: &PublicUrl,
    ) -> impl Future<Output = Result<Vec<u8>, ApiError>> + Send + 'static {
        self.transport.fetch_public(url)
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

    fn json_write<T: Serialize + ?Sized>(
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

    /// Fetches the automations: active and paused ones, or every one of the last 7 days too.
    pub fn tasks(
        &self,
        everything: TaskScope,
    ) -> impl Future<Output = Result<Vec<Task>, ApiError>> + Send + 'static {
        let path = match everything {
            TaskScope::Live => "/tasks",
            TaskScope::Recent => "/tasks?status=all",
        };
        let request = self.transport.get(path);
        async move { body(request.await?) }
    }

    /// Fetches an automation's newest attempts, newest first.
    pub fn task_runs(
        &self,
        task: &TaskId,
    ) -> impl Future<Output = Result<Vec<TaskRun>, ApiError>> + Send + 'static {
        let TaskId(task) = task;
        let request = self.transport.get(&format!("/tasks/{task}/runs?limit=20"));
        async move { body(request.await?) }
    }

    /// Pauses or resumes an automation and returns it as stored.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::Conflict`] when the automation is not in the state the change needs.
    pub fn set_task_paused(
        &self,
        task: &TaskId,
        paused: Pause,
    ) -> impl Future<Output = Result<Task, ApiError>> + Send + 'static {
        let TaskId(task) = task;
        let verb = match paused {
            Pause::Pause => "pause",
            Pause::Resume => "resume",
        };
        let request = self.transport.post(&format!("/tasks/{task}/{verb}"), None);
        async move { body(request.await?) }
    }

    /// Cancels an automation; its history stays.
    pub fn cancel_task(
        &self,
        task: &TaskId,
    ) -> impl Future<Output = Result<(), ApiError>> + Send + 'static {
        let TaskId(task) = task;
        let request = self.transport.send(Request {
            method: Method::Delete,
            path: format!("/tasks/{task}"),
            body: Body::Empty,
        });
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

/// Which automations `GET /tasks` lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskScope {
    /// Active and paused ones.
    Live,
    /// Those plus the completed and cancelled ones of the last 7 days.
    Recent,
}

/// Whether to pause or resume an automation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pause {
    /// Stop firing.
    Pause,
    /// Fire again.
    Resume,
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
