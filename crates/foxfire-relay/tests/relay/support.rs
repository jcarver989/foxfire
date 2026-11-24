use agent_client_protocol::ByteStreams;
use agent_client_protocol::v2::schema::{
    ContentBlock, NewSessionRequest, PromptRequest, ReplayFrom, ReplayFromStart, ResumeSessionRequest, SessionId,
    SessionInfo, SessionUpdate, StateUpdate,
};
use clankerdiff_git::testing::RepoFixtureBuilder;
use foxfire_acp::aether::{RemoteServerInfo, SessionPreviewParams};
use foxfire_acp::client::{AcpClient, AcpClientError, AcpEvent, connect_acp_client};
use foxfire_acp::testing::{FakeAgent, FakeAgentRequests, initialize_request};
use foxfire_acp::websocket::WebSocketTransport;
use foxfire_relay::{Relay, RelayError};
use std::any::Any;
use std::io;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::task::{JoinHandle, spawn_local};
use tokio_tungstenite::connect_async;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

pub(crate) const SESSION: &str = "shared";

#[derive(Debug, thiserror::Error)]
pub(crate) enum TestError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Acp(#[from] AcpClientError),
    #[error(transparent)]
    Protocol(#[from] agent_client_protocol::Error),
    #[error(transparent)]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("the fake agent's recorder closed")]
    Closed,
}

pub(crate) struct RelayBuilder {
    agent: FakeAgent,
    repository: bool,
    saved_sessions: Vec<(&'static str, &'static str)>,
}

pub(crate) struct TestRelay {
    pub(crate) agent: FakeAgentRequests,
    url: String,
    cwd: PathBuf,
    _workspace: Box<dyn Any>,
    served: JoinHandle<Result<(), RelayError>>,
    agent_task: JoinHandle<()>,
}

impl RelayBuilder {
    pub(crate) fn new(agent: FakeAgent) -> Self {
        Self { agent, repository: false, saved_sessions: Vec::new() }
    }

    pub(crate) fn saved_session(mut self, session_id: &'static str, updated_at: &'static str) -> Self {
        self.saved_sessions.push((session_id, updated_at));
        self
    }

    pub(crate) fn repository(mut self) -> Self {
        self.repository = true;
        self
    }

    pub(crate) async fn build(self) -> Result<TestRelay, TestError> {
        let (cwd, workspace): (PathBuf, Box<dyn Any>) = if self.repository {
            let repository = RepoFixtureBuilder::new().file("README.md", "first\n").committed().build();
            (repository.root().to_path_buf(), Box::new(repository))
        } else {
            let directory = TempDir::new()?;
            (directory.path().to_path_buf(), Box::new(directory))
        };
        let mut agent = self.agent;
        if !self.saved_sessions.is_empty() {
            let saved = self
                .saved_sessions
                .iter()
                .map(|(id, updated_at)| SessionInfo::new(*id, cwd.clone()).updated_at((*updated_at).to_owned()));
            agent = agent.sessions(saved.collect());
        }
        let (agent, requests) = agent.capture();
        let (relay_end, agent_end) = tokio::io::duplex(1 << 20);
        let (from_relay, to_relay) = tokio::io::split(agent_end);
        let agent_task = spawn_local(async move {
            let _ = agent.agent().connect_to(ByteStreams::new(to_relay.compat_write(), from_relay.compat())).await;
        });
        let (from_agent, to_agent) = tokio::io::split(relay_end);
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("ws://{}", listener.local_addr()?);
        let agent = ByteStreams::new(to_agent.compat_write(), from_agent.compat());
        let served = tokio::spawn(Relay::new(cwd.clone()).serve(listener, agent));
        Ok(TestRelay { agent: requests, url, cwd, _workspace: workspace, served, agent_task })
    }
}

impl TestRelay {
    pub(crate) fn cwd(&self) -> PathBuf {
        self.cwd.clone()
    }

    pub(crate) async fn client(&self) -> Result<AcpClient, TestError> {
        let (socket, _) = connect_async(self.url.as_str()).await?;
        Ok(connect_acp_client(WebSocketTransport::new(socket), initialize_request()).await?)
    }

    pub(crate) async fn creator(&self) -> Result<AcpClient, TestError> {
        let client = self.client().await?;
        client.handle.request(NewSessionRequest::new(self.cwd())).await?;
        Ok(client)
    }

    pub(crate) async fn joiner(&self, replay: bool) -> Result<AcpClient, TestError> {
        let client = self.client().await?;
        client.handle.request(resume(&self.cwd, replay)).await?;
        Ok(client)
    }

    pub(crate) async fn settle(&self, client: &AcpClient) -> Result<(), TestError> {
        client.handle.request(SessionPreviewParams { session_id: SESSION.to_owned() }).await?;
        Ok(())
    }

    pub(crate) async fn stop_agent(&mut self) {
        self.agent_task.abort();
        let _ = (&mut self.agent_task).await;
    }

    pub(crate) async fn stopped(self) -> Result<(), RelayError> {
        let joined = self.served.await;
        joined.map_err(|error| RelayError::Io { context: "join the relay".into(), source: error.into() })?
    }
}

pub(crate) fn resume(cwd: &Path, replay: bool) -> ResumeSessionRequest {
    let request = ResumeSessionRequest::new(SESSION, cwd.to_path_buf());
    if replay { request.replay_from(ReplayFrom::Start(ReplayFromStart::new())) } else { request }
}

pub(crate) fn live_session(client: &AcpClient) -> Option<SessionId> {
    RemoteServerInfo::from_meta(client.initialize_response.meta.as_ref())?.session_id
}

pub(crate) fn prompt(text: &str) -> PromptRequest {
    PromptRequest::new(SESSION, vec![text.into()])
}

pub(crate) async fn until<T>(
    client: &mut AcpClient,
    mut pick: impl FnMut(AcpEvent) -> Option<T>,
) -> Result<T, TestError> {
    loop {
        let event = client.event_rx.recv().await.ok_or_else(|| io::Error::other("the client's events ended"))?;
        if let Some(picked) = pick(event) {
            return Ok(picked);
        }
    }
}

pub(crate) async fn transcript(client: &mut AcpClient) -> Result<Vec<String>, TestError> {
    let mut texts = Vec::new();
    until(client, |event| {
        let AcpEvent::SessionUpdate(notification) = event else {
            return None;
        };
        match notification.update {
            SessionUpdate::StateUpdate(StateUpdate::Idle(_)) => return Some(()),
            SessionUpdate::UserMessage(message) => {
                texts.extend(message.content.value().into_iter().flatten().filter_map(text));
            }
            SessionUpdate::AgentMessageChunk(chunk) => texts.extend(text(&chunk.content)),
            _ => {}
        }
        None
    })
    .await?;
    Ok(texts)
}

fn text(block: &ContentBlock) -> Option<String> {
    match block {
        ContentBlock::Text(text) => Some(text.text.clone()),
        _ => None,
    }
}
