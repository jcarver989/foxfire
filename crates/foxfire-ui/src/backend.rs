use agent_client_protocol::v2::schema::SessionId;
use chrono::{DateTime, Utc};
use foxfire_acp::client::AcpClientError;
use foxfire_acp::websocket;
use foxfire_core::command::Command;
use foxfire_core::connection::ConnectionError;
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
    /// A session is open, with whatever its agent replayed while it opened.
    Connected(Box<SessionCoreConfig>, Vec<Input>),
    OpenReview,
    ConnectFailed(ConnectFailure),
    Input(Input),
}

/// Why a session could not be opened, and so what would help.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ConnectFailure {
    /// A later attempt may succeed.
    #[error("{0}")]
    Retryable(String),
    /// The user has to sign in first.
    #[error("{0}")]
    Unauthenticated(String),
    /// The session is over, and cannot be opened again.
    #[error("{0}")]
    Ended(String),
    #[error("{0}")]
    Other(String),
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
    /// What the thread is called, once it has been prompted or renamed.
    pub title: Option<String>,
    pub created_by: User,
    pub created_at: DateTime<Utc>,
    /// When the backend ends the thread, if it does so by itself.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Someone who uses a hosted backend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct User {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostedState {
    Starting,
    Ready,
    /// Keeps its session, and resumes when it is opened.
    Suspended,
    /// Can no longer be opened.
    Ended,
    Failed(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ThreadTitle {
    /// Taken from the first prompt, and kept only if the thread has no title yet.
    Suggested(String),
    Chosen(String),
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BackendError {
    #[error("Sign in to continue")]
    Unauthenticated,
    #[error("Unknown project: {0}")]
    UnknownProject(String),
    #[error("Thread not found: {}", .0.0)]
    UnknownThread(ThreadId),
    #[error("{0}")]
    Unavailable(String),
}

pub type BackendFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, BackendError>> + 'a>>;

/// Lifecycle operations required by a thread list, independent of a particular HTTP API.
pub trait HostedBackend: 'static {
    /// Attaches to an existing thread, resuming it if it is suspended, and optionally resuming its ACP session.
    fn open(&self, target: ThreadId, resume: Option<SessionId>) -> SessionLink;
    fn projects(&self) -> BackendFuture<'_, Vec<Project>>;
    fn list_threads(&self) -> BackendFuture<'_, Vec<HostedThread>>;
    fn launch(&self, project: String) -> BackendFuture<'_, HostedThread>;
    fn rename(&self, id: ThreadId, title: ThreadTitle) -> BackendFuture<'_, ()>;
    fn suspend(&self, id: ThreadId) -> BackendFuture<'_, ()>;
    fn stop(&self, id: ThreadId) -> BackendFuture<'_, ()>;
}

impl From<BackendError> for ConnectFailure {
    fn from(error: BackendError) -> Self {
        let reason = match &error {
            BackendError::Unauthenticated => Self::Unauthenticated,
            BackendError::Unavailable(_) => Self::Retryable,
            BackendError::UnknownThread(_) => Self::Ended,
            BackendError::UnknownProject(_) => Self::Other,
        };
        reason(error.to_string())
    }
}

/// A refused handshake says nothing about the user or the session, because a browser cannot see why it was refused.
/// Whatever hands out endpoints answers that on the next attempt.
impl From<websocket::ConnectError> for ConnectFailure {
    fn from(error: websocket::ConnectError) -> Self {
        let reason = if error.is_retryable() { Self::Retryable } else { Self::Other };
        reason(error.to_string())
    }
}

impl From<ConnectionError> for ConnectFailure {
    fn from(error: ConnectionError) -> Self {
        let reason = match &error {
            ConnectionError::Acp(AcpClientError::AgentCrashed(_)) => Self::Retryable,
            ConnectionError::Acp(AcpClientError::InvalidAgentCommand(_) | AcpClientError::Protocol(_))
            | ConnectionError::MissingRemoteContract => Self::Other,
        };
        reason(error.to_string())
    }
}
