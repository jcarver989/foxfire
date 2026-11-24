use crate::hub::{ClientEvent, ClientId, Hub, LiveSession};
use crate::wire;
use crate::workspace::Workspace;
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::v2::schema::{
    AbsolutePath, Implementation, InitializeResponse, ListSessionsRequest, ListSessionsResponse, SessionInfo,
};
use agent_client_protocol::{
    Agent, ByteStreams, Client, ConnectTo, ConnectionDriver, RawJsonRpcError, RawJsonRpcId, RawJsonRpcMessage,
    RawJsonRpcRequest, RawJsonRpcResponse, Role, TransportFrame,
};
use chrono::{DateTime, FixedOffset};
use foxfire_acp::client::initialize_request;
use foxfire_acp::websocket::WebSocketTransport;
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures::future::OptionFuture;
use serde::Serialize;
use serde_json::Value;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;
use tokio_tungstenite::accept_async;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tokio_util::task::TaskTracker;

#[derive(Debug, Error)]
pub enum RelayError {
    #[error("failed to {context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },
    #[error("the spawned agent has no {0} pipe")]
    MissingAgentPipe(&'static str),
    #[error("the agent refused to initialize: {0}")]
    Initialize(String),
    #[error("the agent exited")]
    AgentExited,
    #[error("the agent exited ({0})")]
    AgentFailed(ExitStatus),
}

pub struct Relay {
    cwd: PathBuf,
}

impl Relay {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }

    pub async fn run(self, address: SocketAddr, agent: Command) -> Result<(), RelayError> {
        let AgentProcess { mut child, stdin, stdout } = AgentProcess::spawn(agent, &self.cwd)?;
        let listener = TcpListener::bind(address)
            .await
            .map_err(|source| RelayError::Io { context: format!("listen on {address}"), source })?;
        let served = self.serve(listener, ByteStreams::new(stdin.compat_write(), stdout.compat())).await;
        match (served, child.wait().await) {
            (Err(RelayError::AgentExited), Ok(status)) if !status.success() => Err(RelayError::AgentFailed(status)),
            (served, _) => served,
        }
    }

    pub async fn serve(self, listener: TcpListener, agent: impl ConnectTo<Client>) -> Result<(), RelayError> {
        let tasks = TaskTracker::new();
        let (from_agent, to_agent, driver) = split_transport(agent);
        if let Some(driver) = driver {
            tasks.spawn(driver);
        }
        let mut startup = AgentStartup { from_agent, to_agent, requests: 0 };
        let initialized = startup.initialize().await?;
        let live = startup.find_latest_session(&self.cwd).await?;
        let AgentStartup { from_agent, to_agent, .. } = startup;
        let workspace = Workspace::open(self.cwd).await;
        tracing::info!(cwd = %workspace.cwd().display(), address = ?listener.local_addr().ok(), "relaying the agent");

        let (events, received) = mpsc::unbounded_channel();
        let accepting = tokio::spawn(accept_clients(listener, events, tasks.clone()));
        Hub::create(to_agent, initialized, workspace, live).run(from_agent, received).await;
        accepting.abort();
        tasks.close();
        tasks.wait().await;
        Err(RelayError::AgentExited)
    }
}

struct AgentProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl AgentProcess {
    fn spawn(mut command: Command, cwd: &Path) -> Result<Self, RelayError> {
        let mut child = command
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|source| RelayError::Io { context: "start the agent".to_owned(), source })?;
        let stdin = child.stdin.take().ok_or(RelayError::MissingAgentPipe("stdin"))?;
        let stdout = child.stdout.take().ok_or(RelayError::MissingAgentPipe("stdout"))?;
        Ok(Self { child, stdin, stdout })
    }
}

fn split_transport<T: Role>(
    transport: impl ConnectTo<T>,
) -> (UnboundedReceiver<TransportFrame>, UnboundedSender<TransportFrame>, Option<ConnectionDriver>) {
    let (channel, driver) = transport.into_channel_and_future();
    let (from, to) = channel.into_parts();
    (from, to, driver)
}

struct AgentStartup {
    from_agent: UnboundedReceiver<TransportFrame>,
    to_agent: UnboundedSender<TransportFrame>,
    requests: u64,
}

impl AgentStartup {
    async fn initialize(&mut self) -> Result<InitializeResponse, RelayError> {
        let request = initialize_request(Implementation::new("foxfire-relay", env!("CARGO_PKG_VERSION")));
        let result =
            self.request("initialize", request).await?.map_err(|error| RelayError::Initialize(error.message))?;
        let version = result.get("protocolVersion").cloned().unwrap_or(Value::Null);
        if serde_json::from_value::<ProtocolVersion>(version.clone()).ok() != Some(ProtocolVersion::V2) {
            return Err(RelayError::Initialize(format!("it chose ACP version {version}, but the relay needs 2")));
        }
        serde_json::from_value(result).map_err(|error| RelayError::Initialize(error.to_string()))
    }

    async fn find_latest_session(&mut self, cwd: &Path) -> Result<Option<LiveSession>, RelayError> {
        let mut sessions = Vec::new();
        let mut cursor = None;
        loop {
            let request = ListSessionsRequest::new().cwd(AbsolutePath::new(cwd)).cursor(cursor);
            let page = match self.request("session/list", request).await? {
                Ok(page) => serde_json::from_value::<ListSessionsResponse>(page).map_err(|error| error.to_string()),
                Err(error) => Err(error.message),
            };
            match page {
                Ok(page) => {
                    sessions.extend(page.sessions);
                    cursor = page.next_cursor;
                }
                Err(error) => {
                    tracing::warn!(%error, "adopting no earlier session, as the agent did not list its sessions");
                    return Ok(None);
                }
            }
            if cursor.is_none() {
                break;
            }
        }
        let latest = sessions.into_iter().rev().max_by_key(updated_at);
        Ok(latest.map(|session| LiveSession { id: session.session_id.to_string(), cwd: session.cwd.into_inner() }))
    }

    async fn request(
        &mut self,
        method: &str,
        params: impl Serialize,
    ) -> Result<Result<Value, Box<RawJsonRpcError>>, RelayError> {
        self.requests += 1;
        let sent = RawJsonRpcId::Str(format!("startup-{}", self.requests));
        let request = RawJsonRpcRequest { id: sent.clone(), method: method.into(), params: wire::params(params) };
        let _ = self.to_agent.unbounded_send(TransportFrame::Single(request.into()));
        while let Some(frame) = self.from_agent.next().await {
            match frame {
                TransportFrame::Single(RawJsonRpcMessage::Response(RawJsonRpcResponse::Result { id, result }))
                    if id == sent =>
                {
                    return Ok(Ok(result));
                }
                TransportFrame::Single(RawJsonRpcMessage::Response(RawJsonRpcResponse::Error { id, error }))
                    if id == sent =>
                {
                    return Ok(Err(error));
                }
                _ => {}
            }
        }
        Err(RelayError::AgentExited)
    }
}

fn updated_at(session: &SessionInfo) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(session.updated_at.as_deref()?).ok()
}

async fn accept_clients(listener: TcpListener, events: mpsc::UnboundedSender<ClientEvent>, tasks: TaskTracker) {
    let mut accepted = 0;
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                accepted += 1;
                tasks.spawn(serve_client(stream, ClientId(accepted), events.clone()));
            }
            Err(error) => tracing::warn!(%error, "failed to accept a client"),
        }
    }
}

async fn serve_client(stream: TcpStream, client: ClientId, events: mpsc::UnboundedSender<ClientEvent>) {
    let socket = match accept_async(stream).await {
        Ok(socket) => socket,
        Err(error) => return tracing::debug!(%error, "a client failed the WebSocket handshake"),
    };
    let (mut from_client, outgoing, driver) = split_transport::<Agent>(WebSocketTransport::new(socket));
    if events.send(ClientEvent::Connected { client, outgoing }).is_err() {
        return;
    }
    let receive = async {
        while let Some(frame) = from_client.next().await {
            if events.send(ClientEvent::Received { client, frame }).is_err() {
                return;
            }
        }
        let _ = events.send(ClientEvent::Disconnected(client));
    };
    let drive = async {
        if let Some(Err(error)) = OptionFuture::from(driver).await {
            tracing::debug!(%error, ?client, "client socket failed");
        }
    };
    tokio::join!(receive, drive);
}
