use crate::wire;
use agent_client_protocol::v2::schema::{InitializeResponse, Meta, SessionId};
use agent_client_protocol::{RawJsonRpcId, TransportFrame};
use clankerdiff_protocol::client::{ClientCommand, LocalClientMessageTransport};
use clankerdiff_server::{DiffServer, ServerOptions};
use foxfire_acp::aether::{
    AetherCapabilities, AetherMeta, GitDiffEventPayload, RemoteServerInfo, SessionPreviewResponse,
};
use foxfire_core::session_model::local_status;
use futures::channel::mpsc::UnboundedSender;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tokio::task::spawn_blocking;

pub(crate) struct Workspace {
    cwd: PathBuf,
    diff: Option<DiffServer>,
}

pub(crate) type DiffBridge = mpsc::UnboundedSender<ClientCommand>;

impl Workspace {
    pub(crate) async fn open(cwd: PathBuf) -> Self {
        let diff = match DiffServer::open(&cwd, ServerOptions::default()).await {
            Ok(server) => Some(server),
            Err(error) => {
                tracing::warn!(%error, cwd = %cwd.display(), "serving no review, as the workspace is not a repository");
                None
            }
        };
        Self { cwd, diff }
    }

    pub(crate) fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub(crate) fn advertise(&self, mut response: InitializeResponse) -> InitializeResponse {
        let mut capabilities = AetherCapabilities::of(&response.capabilities);
        capabilities.git_diff = self.diff.is_some();
        capabilities.workspace_status = true;
        let mut session = response.capabilities.session.take().unwrap_or_default();
        session.meta = Some(merge(session.meta.take(), capabilities.to_meta()));
        response.capabilities.session = Some(session);
        let remote = RemoteServerInfo { cwd: self.cwd.clone(), session_id: None };
        response.meta = Some(merge(response.meta.take(), remote.to_meta()));
        response
    }

    pub(crate) fn bridge(&self, session_id: &str, outgoing: UnboundedSender<TransportFrame>) -> Option<DiffBridge> {
        let transport = self.diff.as_ref()?.connect_messages().ok()?;
        let (commands, received) = mpsc::unbounded_channel();
        tokio::spawn(run_bridge(transport, received, SessionId::new(session_id), outgoing));
        Some(commands)
    }

    pub(crate) fn report_status(&self, id: RawJsonRpcId, outgoing: UnboundedSender<TransportFrame>) {
        let cwd = self.cwd.clone();
        tokio::spawn(async move {
            if let Ok(status) = spawn_blocking(move || local_status(&cwd)).await {
                let _ = outgoing.unbounded_send(wire::result(id, status));
            }
        });
    }

    pub(crate) fn preview(&self, session_id: String) -> SessionPreviewResponse {
        SessionPreviewResponse {
            session_id,
            cwd: self.cwd.clone(),
            created_at: String::new(),
            model: String::new(),
            selected_mode: None,
            transcript: Vec::new(),
            tool_call_count: 0,
            truncated: false,
        }
    }
}

async fn run_bridge(
    transport: LocalClientMessageTransport,
    mut commands: mpsc::UnboundedReceiver<ClientCommand>,
    session_id: SessionId,
    outgoing: UnboundedSender<TransportFrame>,
) {
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else { break };
                if transport.send(command).await.is_err() {
                    break;
                }
            }
            event = transport.recv() => {
                let Ok(event) = event else { break };
                let payload = GitDiffEventPayload { session_id: session_id.clone(), event };
                if outgoing.unbounded_send(wire::notification("_aether/git_diff_event", payload)).is_err() {
                    break;
                }
            }
        }
    }
    transport.close();
}

fn merge(meta: Option<Meta>, addition: Meta) -> Meta {
    let mut meta = meta.unwrap_or_default();
    for (namespace, value) in addition {
        match (meta.get_mut(&namespace), value) {
            (Some(Value::Object(existing)), Value::Object(added)) => existing.extend(added),
            (_, value) => {
                meta.insert(namespace, value);
            }
        }
    }
    meta
}
