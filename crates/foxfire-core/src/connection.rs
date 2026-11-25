use crate::runtime::SessionRuntime;
use crate::session_core::SessionCoreConfig;
use crate::session_model::{WorkspaceAccess, initial_workspace_status};
use agent_client_protocol::v2::schema::{
    AbsolutePath, Implementation, ListSessionsRequest, NewSessionRequest, NewSessionResponse, ReplayFrom,
    ReplayFromStart, ResumeSessionRequest, SessionId, SessionInfo,
};
use agent_client_protocol::{Client, ConnectTo};
use foxfire_acp::aether::{RemoteServerInfo, SessionPreviewParams};
use foxfire_acp::chat::{ClientMeta, FoxfireMeta, Participant};
use foxfire_acp::client::{AcpClient, AcpClientError, AcpClientHandle, connect_acp_client, initialize_request};
use std::path::{Path, PathBuf};

pub struct Connection {
    client: AcpClient,
    participant: Option<Participant>,
}

/// The program connecting, and the person using it if they chat.
pub struct ClientInfo {
    pub program: Implementation,
    pub participant: Option<Participant>,
}

pub struct Session {
    client: AcpClient,
    response: NewSessionResponse,
    working_dir: PathBuf,
    workspace_access: WorkspaceAccess,
    participant: Option<Participant>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("the agent did not report a working directory of its own, so one must be given to open a session")]
    MissingRemoteContract,
    #[error("{0}")]
    Acp(#[from] AcpClientError),
}

impl Connection {
    pub async fn connect_to(
        client: impl Into<ClientInfo>,
        agent: impl ConnectTo<Client> + 'static,
    ) -> Result<Self, ConnectionError> {
        let ClientInfo { program, participant } = client.into();
        let mut request = initialize_request(program);
        if participant.is_some() {
            request = request.meta(ClientMeta { participant: participant.clone() }.to_meta());
        }
        let client = connect_acp_client(agent, request).await?;
        Ok(Self { client, participant })
    }

    pub async fn open(self, session_id: Option<SessionId>, working_dir: PathBuf) -> Result<Session, ConnectionError> {
        self.open_with_access(session_id, working_dir, WorkspaceAccess::Local).await
    }

    pub async fn open_with_access(
        self,
        session_id: Option<SessionId>,
        working_dir: PathBuf,
        workspace_access: WorkspaceAccess,
    ) -> Result<Session, ConnectionError> {
        if self.remote_server().is_some() {
            return self.open_remote(session_id).await;
        }
        self.start(session_id, working_dir, workspace_access).await
    }

    pub async fn open_remote(self, requested_session: Option<SessionId>) -> Result<Session, ConnectionError> {
        let remote = self.remote_server().ok_or(ConnectionError::MissingRemoteContract)?;
        let (selected, working_dir) = match (requested_session, remote.session_id) {
            (Some(requested), live) if live.as_ref() != Some(&requested) => {
                let preview = SessionPreviewParams { session_id: requested.to_string() };
                (Some(requested), self.client.handle.request(preview).await?.cwd)
            }
            (requested, live) => (requested.or(live), remote.cwd),
        };
        self.start(selected, working_dir, WorkspaceAccess::Remote).await
    }

    pub async fn list_sessions(&self, cwd: &Path) -> Result<Vec<SessionInfo>, ConnectionError> {
        Ok(list_sessions(&self.client.handle, Some(cwd)).await?)
    }

    fn remote_server(&self) -> Option<RemoteServerInfo> {
        RemoteServerInfo::from_meta(self.client.initialize_response.meta.as_ref())
    }

    async fn start(
        self,
        session_id: Option<SessionId>,
        working_dir: PathBuf,
        workspace_access: WorkspaceAccess,
    ) -> Result<Session, ConnectionError> {
        let Self { client, participant } = self;
        let response = match session_id {
            Some(session_id) => resume(&client, session_id, working_dir.clone()).await?,
            None => client.handle.request(NewSessionRequest::new(working_dir.clone())).await?,
        };
        Ok(Session { client, response, working_dir, workspace_access, participant })
    }
}

impl From<Implementation> for ClientInfo {
    fn from(program: Implementation) -> Self {
        Self { program, participant: None }
    }
}

impl Session {
    pub fn session_id(&self) -> &SessionId {
        &self.response.session_id
    }

    pub fn working_dir(&self) -> &Path {
        &self.working_dir
    }

    pub fn client(&self) -> &AcpClient {
        &self.client
    }

    pub fn into_core(self) -> (SessionCoreConfig, SessionRuntime) {
        let Self { client, response, working_dir, workspace_access, participant } = self;
        let config = SessionCoreConfig {
            initialize_response: client.initialize_response,
            session_response: response,
            workspace_status: initial_workspace_status(&working_dir),
            working_dir,
            workspace_access,
            participant,
        };
        (config, SessionRuntime::new(client.handle, client.event_rx))
    }
}

pub(crate) async fn list_sessions(
    handle: &AcpClientHandle,
    cwd: Option<&Path>,
) -> Result<Vec<SessionInfo>, AcpClientError> {
    let mut sessions = Vec::new();
    let mut cursor = None;
    loop {
        let request = ListSessionsRequest::new().cwd(cwd.map(AbsolutePath::new)).cursor(cursor);
        let page = handle.request(request).await?;
        sessions.extend(page.sessions);
        cursor = page.next_cursor;
        if cursor.is_none() {
            return Ok(sessions);
        }
    }
}

pub(crate) fn resume_request(session_id: SessionId, cwd: PathBuf) -> ResumeSessionRequest {
    ResumeSessionRequest::new(session_id, cwd).replay_from(ReplayFrom::Start(ReplayFromStart::new()))
}

async fn resume(
    client: &AcpClient,
    session_id: SessionId,
    cwd: PathBuf,
) -> Result<NewSessionResponse, ConnectionError> {
    let resumed = client.handle.request(resume_request(session_id.clone(), cwd)).await?;
    Ok(NewSessionResponse::new(session_id).config_options(resumed.config_options))
}
