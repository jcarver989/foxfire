use agent_client_protocol::v2::schema::SessionId;
use foxfire_core::command::Command;
use foxfire_core::session_core::Input;
use foxfire_core::session_core::SessionCoreConfig;
use std::future::Future;
use std::pin::Pin;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// The command and event channels for one attachment. Dropping them disconnects only this client.
pub struct SessionLink {
    pub commands: UnboundedSender<Command>,
    pub events: UnboundedReceiver<SessionEvent>,
}

/// Events sent by the backend to the shared thread controller.
#[allow(clippy::large_enum_variant, reason = "streamed inputs are the common case; keep them inline in the channel")]
pub enum SessionEvent {
    Launched(String),
    Connected(Box<SessionCoreConfig>),
    OpenReview,
    ConnectFailed(String),
    Input(Input),
}

/// An opaque, stable ID assigned by a hosted backend.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ThreadId(pub String);

/// A launchable project in a hosted backend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
}

/// A hosted thread that can be opened without provisioning again.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostedThread {
    pub id: ThreadId,
    pub project: Project,
    pub state: HostedState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostedState {
    Starting,
    Ready,
    Failed(String),
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("Unknown project: {0}")]
    UnknownProject(String),
    #[error("Thread not found: {}", .0.0)]
    UnknownThread(ThreadId),
    #[error("{0}")]
    Unavailable(String),
}

pub type BackendFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, BackendError>> + 'a>>;

/// Lifecycle operations required by the web shell, independent of a particular HTTP API.
pub trait HostedBackend: 'static {
    /// Attaches to an existing thread, optionally resuming its ACP session.
    fn open(&self, target: ThreadId, resume: Option<SessionId>) -> SessionLink;
    fn projects(&self) -> BackendFuture<'_, Vec<Project>>;
    fn list_threads(&self) -> BackendFuture<'_, Vec<HostedThread>>;
    fn launch(&self, project: String) -> BackendFuture<'_, HostedThread>;
    fn stop(&self, id: ThreadId) -> BackendFuture<'_, ()>;
}
