use crate::agents::{AgentProfile, AgentTransport, CommandLine, EndpointSource, expand_command_line, expand_endpoint};
use crate::variables::VarError;
use agent_client_protocol::v2::schema::{Implementation, SessionId};
use agent_client_protocol::{self as acp, Agent, ByteStreams, Client, ConnectTo, DynConnectTo};
use clankerdiff_client::ConnectionHeader;
use clankerdiff_remote::{AcpTarget, Channel, Invocation, RemoteError, RemoteSpec, Remotes};
use foxfire_acp::chat::Participant;
use foxfire_acp::websocket::{self, Endpoint};
use foxfire_core::connection::{ClientInfo, Connection, ConnectionError, Session};
use foxfire_core::native::runtime::ReviewOverride;
use foxfire_core::session_model::WorkspaceAccess;
use serde::de::DeserializeOwned;
use std::env::var;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::watch;
use tokio::task::{JoinError, spawn_blocking};
use tokio::time::{Instant, sleep_until, timeout_at};
use tokio_tungstenite::tungstenite;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionTarget {
    Agent { agent: AgentProfile, cwd: PathBuf },
    Remote { remote: RemoteSpec, instance: Option<String> },
}

pub const STARTUP_WINDOW: Duration = Duration::from_mins(5);

pub struct AgentConnector {
    remotes: Arc<dyn Remotes>,
    transport: Arc<dyn Transport>,
    identity: watch::Sender<Option<Participant>>,
}

pub type TransportFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ConnectError>> + Send + 'a>>;

pub trait Transport: Send + Sync + 'static {
    fn stdio<'a>(&'a self, line: &'a CommandLine, dir: &'a Path) -> TransportFuture<'a, DynConnectTo<Client>>;

    fn websocket<'a>(&'a self, endpoint: &'a Endpoint, startup: Duration) -> TransportFuture<'a, DynConnectTo<Client>>;

    fn run<'a>(&'a self, line: &'a CommandLine, dir: &'a Path) -> TransportFuture<'a, Vec<u8>>;
}

pub struct ProcessTransport;

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error(transparent)]
    Variable(#[from] VarError),
    #[error("failed to start `{program}`: {source}")]
    Spawn {
        program: String,
        #[source]
        source: io::Error,
    },
    #[error("`{program}` failed ({status}): {stderr}")]
    CommandFailed { program: String, status: ExitStatus, stderr: String },
    #[error("`{program}` printed unexpected output: {source}")]
    CommandOutput {
        program: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid header `{name}` for a review endpoint")]
    InvalidReviewHeader { name: String },
    #[error(transparent)]
    WebSocket(#[from] websocket::ConnectError),
    #[error("the agent did not complete the WebSocket handshake in time")]
    WebSocketHandshakeTimeout,
    #[error(transparent)]
    Client(#[from] ConnectionError),
    #[error(transparent)]
    Remote(#[from] RemoteError),
    #[error("{remote} serves no agent")]
    NoAgent { remote: String },
    #[error("{remote}'s agent needs an `acp.cwd` to work in, since it does not report one of its own")]
    NoCwd { remote: String },
    #[error("the thread's instance has not been launched")]
    NotLaunched,
    #[error("the remote's command was interrupted: {0}")]
    Interrupted(#[from] JoinError),
}

impl SessionTarget {
    pub fn agent(cwd: PathBuf, agent: AgentProfile) -> Self {
        Self::Agent { agent, cwd }
    }

    pub fn remote(remote: &RemoteSpec, instance: Option<String>) -> Self {
        let instance = instance.or_else(|| remote.is_static().then(|| remote.name.clone()));
        Self::Remote { remote: remote.clone(), instance }
    }

    pub fn set_instance(&mut self, launched: String) {
        if let Self::Remote { instance, .. } = self {
            *instance = Some(launched);
        }
    }

    pub fn cwd(&self) -> Option<&Path> {
        match self {
            Self::Agent { cwd, .. } => Some(cwd),
            Self::Remote { .. } => None,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Agent { agent, .. } => &agent.name,
            Self::Remote { remote, .. } => &remote.name,
        }
    }

    pub fn remote_name(&self) -> Option<&str> {
        match self {
            Self::Agent { .. } => None,
            Self::Remote { remote, .. } => Some(&remote.name),
        }
    }

    pub fn instance(&self) -> Option<&str> {
        match self {
            Self::Agent { .. } => None,
            Self::Remote { instance, .. } => instance.as_deref(),
        }
    }
}

impl AgentConnector {
    pub fn new(remotes: Arc<dyn Remotes>, transport: Arc<dyn Transport>) -> Self {
        Self { remotes, transport, identity: watch::Sender::default() }
    }

    /// Says who is using the connections made from now on.
    pub fn identify(&self, participant: Participant) {
        self.identity.send_replace(Some(participant));
    }

    async fn introduce(&self, agent: DynConnectTo<Client>) -> Result<Connection, ConnectError> {
        let started = Instant::now();
        let mut participant = self.identity.borrow().clone();
        if let Some(unnamed) = participant.as_mut().filter(|participant| participant.name.is_empty()) {
            unnamed.name = default_name().await;
        }
        let client = ClientInfo { program: client_info(), participant };
        let connection = Connection::connect_to(client, agent).await;
        tracing::debug!(target: "foxfire_startup", elapsed_ms = started.elapsed().as_millis(), ok = connection.is_ok(), "ACP initialize finished");
        Ok(connection?)
    }

    pub async fn launch(&self, target: &SessionTarget) -> Result<Option<String>, ConnectError> {
        match target {
            SessionTarget::Remote { remote, instance: None } => {
                let (remotes, remote) = (self.remotes.clone(), remote.clone());
                Ok(Some(spawn_blocking(move || remotes.launch(&remote)).await??))
            }
            SessionTarget::Remote { instance: Some(_), .. } | SessionTarget::Agent { .. } => Ok(None),
        }
    }

    pub async fn connect(
        &self,
        target: &SessionTarget,
        session_id: Option<SessionId>,
    ) -> Result<Session, ConnectError> {
        match target {
            SessionTarget::Agent { agent, cwd } => {
                Ok(self.connect_agent(agent, cwd).await?.open(session_id, cwd.clone()).await?)
            }
            SessionTarget::Remote { remote, instance } => {
                let instance = instance.as_deref().ok_or(ConnectError::NotLaunched)?;
                let AcpTarget { transport, cwd } =
                    remote.acp(Some(instance))?.ok_or_else(|| ConnectError::NoAgent { remote: remote.name.clone() })?;
                let agent = match transport {
                    Channel::Stdio(invocation) => {
                        self.transport.stdio(&command_line(&invocation), &invocation.dir).await?
                    }
                    Channel::Websocket(invocation) => {
                        let startup = if remote.is_static() { Duration::ZERO } else { STARTUP_WINDOW };
                        let started = Instant::now();
                        let endpoint = self.endpoint(&invocation).await;
                        tracing::debug!(target: "foxfire_startup", elapsed_ms = started.elapsed().as_millis(), ok = endpoint.is_ok(), "Endpoint command finished");
                        self.transport.websocket(&endpoint?, startup).await?
                    }
                };
                let connection = self.introduce(agent).await?;
                let started = Instant::now();
                let session = match cwd {
                    Some(cwd) => connection
                        .open_with_access(session_id, cwd.into(), WorkspaceAccess::Remote)
                        .await
                        .map_err(ConnectError::from),
                    None => connection.open_remote(session_id).await.map_err(|error| match error {
                        ConnectionError::MissingRemoteContract => ConnectError::NoCwd { remote: remote.name.clone() },
                        error @ ConnectionError::Acp(_) => error.into(),
                    }),
                };
                tracing::debug!(target: "foxfire_startup", elapsed_ms = started.elapsed().as_millis(), ok = session.is_ok(), "ACP session open finished");
                session
            }
        }
    }

    pub async fn connect_agent(&self, agent: &AgentProfile, dir: &Path) -> Result<Connection, ConnectError> {
        let transport = match &agent.config.transport {
            AgentTransport::Stdio(command) => {
                self.transport.stdio(&expand_command_line(command, &agent.vars())?, dir).await?
            }
            AgentTransport::WebSocket(source) => {
                self.transport.websocket(&self.resolve(source, agent).await?, Duration::ZERO).await?
            }
        };
        self.introduce(transport).await
    }

    pub async fn review_override(&self, target: &SessionTarget) -> Result<Option<ReviewOverride>, ConnectError> {
        match target {
            SessionTarget::Agent { agent, .. } => match &agent.config.diff {
                Some(source) => Ok(Some(review_at(self.resolve(source, agent).await?)?)),
                None => Ok(None),
            },
            SessionTarget::Remote { remote, instance } => match remote.diff(instance.as_deref())? {
                Some(Channel::Stdio(invocation)) => Ok(Some(ReviewOverride::Stdio(invocation))),
                Some(Channel::Websocket(invocation)) => Ok(Some(review_at(self.endpoint(&invocation).await?)?)),
                None => Ok(None),
            },
        }
    }

    async fn resolve(&self, source: &EndpointSource, agent: &AgentProfile) -> Result<Endpoint, ConnectError> {
        match source {
            EndpointSource::Fixed(endpoint) => Ok(expand_endpoint(endpoint, &agent.vars())?),
            EndpointSource::Command(line) => {
                self.run_for_json(&expand_command_line(line, &agent.vars())?, &agent.workspace).await
            }
        }
    }

    async fn endpoint(&self, invocation: &Invocation) -> Result<Endpoint, ConnectError> {
        self.run_for_json(&command_line(invocation), &invocation.dir).await
    }

    async fn run_for_json<T: DeserializeOwned>(&self, line: &CommandLine, dir: &Path) -> Result<T, ConnectError> {
        let output = self.transport.run(line, dir).await?;
        serde_json::from_slice(&output)
            .map_err(|source| ConnectError::CommandOutput { program: line.command().display().to_string(), source })
    }
}

impl Transport for ProcessTransport {
    fn stdio<'a>(&'a self, line: &'a CommandLine, dir: &'a Path) -> TransportFuture<'a, DynConnectTo<Client>> {
        Box::pin(async move {
            let mut child = command(line, dir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .map_err(|source| ConnectError::Spawn { program: line.command().display().to_string(), source })?;
            let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
                unreachable!("both pipes were requested at spawn");
            };
            Ok(DynConnectTo::new(AgentProcess { child, stdin, stdout }))
        })
    }

    fn websocket<'a>(&'a self, endpoint: &'a Endpoint, startup: Duration) -> TransportFuture<'a, DynConnectTo<Client>> {
        Box::pin(async move {
            let started = Instant::now();
            let deadline = (!startup.is_zero()).then_some(started + startup);
            let mut delay = FIRST_RETRY_DELAY;
            let mut attempts = 0;
            let transport = loop {
                let attempt = websocket::connect(endpoint);
                attempts += 1;
                let result = match deadline {
                    Some(deadline) => timeout_at(deadline.min(Instant::now() + HANDSHAKE_TIMEOUT), attempt)
                        .await
                        .map_err(|_| ConnectError::WebSocketHandshakeTimeout)
                        .and_then(|result| result.map_err(ConnectError::from)),
                    None => attempt.await.map_err(ConnectError::from),
                };
                let error = match result {
                    Ok(transport) => break Ok(transport),
                    Err(error) => error,
                };
                let retry_at = Instant::now() + delay;
                if deadline.is_none_or(|deadline| retry_at >= deadline) || !is_starting(&error) {
                    break Err(error);
                }
                tracing::debug!(target: "foxfire_startup", attempts, timed_out = matches!(error, ConnectError::WebSocketHandshakeTimeout), delay_ms = delay.as_millis(), "Retrying WebSocket upgrade");
                sleep_until(retry_at).await;
                delay = (delay * 2).min(MAX_RETRY_DELAY);
            };
            tracing::debug!(target: "foxfire_startup", elapsed_ms = started.elapsed().as_millis(), attempts, ok = transport.is_ok(), "WebSocket upgrade finished");
            Ok(DynConnectTo::new(transport?))
        })
    }

    fn run<'a>(&'a self, line: &'a CommandLine, dir: &'a Path) -> TransportFuture<'a, Vec<u8>> {
        Box::pin(async move {
            let output = command(line, dir)
                .stdin(Stdio::null())
                .output()
                .await
                .map_err(|source| ConnectError::Spawn { program: line.command().display().to_string(), source })?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let last_line = stderr.lines().map(str::trim).rfind(|line| !line.is_empty()).unwrap_or_default();
                return Err(ConnectError::CommandFailed {
                    program: line.command().display().to_string(),
                    status: output.status,
                    stderr: last_line.to_owned(),
                });
            }
            Ok(output.stdout)
        })
    }
}

pub(crate) fn client_info() -> Implementation {
    Implementation::new(crate::branding::CLIENT_NAME, env!("CARGO_PKG_VERSION"))
}

/// What to call someone who has not chosen a name.
async fn default_name() -> String {
    let git = Command::new("git").args(["config", "--get", "user.name"]).stdin(Stdio::null()).output().await;
    let configured = git.ok().map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    let mut names = configured.into_iter().chain(["USER", "USERNAME"].into_iter().filter_map(|name| var(name).ok()));
    names.find(|name| !name.trim().is_empty()).unwrap_or_else(|| "User".to_owned())
}

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);
const FIRST_RETRY_DELAY: Duration = Duration::from_millis(250);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(1);

struct AgentProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl ConnectTo<Client> for AgentProcess {
    async fn connect_to(self, client: impl ConnectTo<Agent>) -> Result<(), acp::Error> {
        let Self { child: _child, stdin, stdout } = self;
        ConnectTo::<Client>::connect_to(ByteStreams::new(stdin.compat_write(), stdout.compat()), client).await
    }
}

fn command_line(invocation: &Invocation) -> CommandLine {
    CommandLine::new(&invocation.program).args(&invocation.args).envs(&invocation.env)
}

fn command(line: &CommandLine, dir: &Path) -> Command {
    let mut command = Command::new(line.command());
    command.args(line.arguments()).envs(line.environment()).current_dir(dir).kill_on_drop(true);
    command
}

fn review_at(endpoint: Endpoint) -> Result<ReviewOverride, ConnectError> {
    let header = |(name, value): (String, String)| {
        format!("{name}: {value}").parse().map_err(|_| ConnectError::InvalidReviewHeader { name })
    };
    let headers = endpoint.headers.into_iter().map(header).collect::<Result<Vec<ConnectionHeader>, _>>()?;
    Ok(ReviewOverride::WebSocket { url: endpoint.url, headers })
}

fn is_starting(error: &ConnectError) -> bool {
    match error {
        ConnectError::WebSocketHandshakeTimeout => true,
        ConnectError::WebSocket(websocket::ConnectError::Handshake(error)) => match error.as_ref() {
            tungstenite::Error::Io(_) => true,
            tungstenite::Error::Http(response) => response.status().is_server_error(),
            _ => false,
        },
        _ => false,
    }
}
