use agent_client_protocol::Error;
use agent_client_protocol::v2::schema::{NewSessionRequest, SessionId};
use foxfire_acp::artifact::{Artifact, ArtifactContent, ArtifactId, ArtifactPayload};
use foxfire_acp::client::{AcpClient, AcpClientError, AcpEvent, connect_acp_client};
use foxfire_acp::testing::{FakeAgent, SessionTools, initialize_request, run_local};
use foxfire_artifacts::ArtifactHost;
use foxfire_artifacts::client::{ArtifactLoader, LoadError};
use foxfire_artifacts::store::InMemory;
use serde_json::json;
use std::fs;
use std::io;
use std::path::PathBuf;
use tempfile::TempDir;
use thiserror::Error as ThisError;

#[test]
fn an_artifact_is_fetched_once_and_then_loaded_from_the_clients_store() -> Result<(), TestError> {
    run_local(async {
        let mut shown = Shown::open().await?;
        let mut loader = ArtifactLoader::new(shown.client.handle.clone());
        let first = shown.submit("pricing.html", "<h1>v1</h1>").await?;
        assert_eq!(load(&mut loader, first.id).await?, html("<h1>v1</h1>"));

        shown.submit("pricing.html", "<h1>v2</h1>").await?;
        assert_eq!(
            load(&mut loader, first.id).await?,
            html("<h1>v1</h1>"),
            "the host no longer keeps the first version, so the client kept it"
        );
        Ok(())
    })
}

#[test]
fn loads_asked_for_together_are_each_answered_in_turn() -> Result<(), TestError> {
    run_local(async {
        let mut shown = Shown::open().await?;
        let mut loader = ArtifactLoader::new(shown.client.handle.clone());
        let pricing = shown.submit("pricing.html", "<h1>Pricing</h1>").await?;
        let roadmap = shown.submit("roadmap.html", "<h1>Roadmap</h1>").await?;

        assert!(loader.load(SessionId::new(SESSION), pricing.id).is_none());
        assert!(loader.load(SessionId::new(SESSION), roadmap.id).is_none());
        let (first, second) = (loader.finished().await, loader.finished().await);
        assert_eq!((first.id, first.result?), (pricing.id, html("<h1>Pricing</h1>")));
        assert_eq!((second.id, second.result?), (roadmap.id, html("<h1>Roadmap</h1>")));
        assert!(!loader.is_loading());
        Ok(())
    })
}

#[test]
fn content_the_session_does_not_show_is_not_loaded() -> Result<(), TestError> {
    run_local(async {
        let shown = Shown::open().await?;
        let mut loader = ArtifactLoader::new(shown.client.handle.clone());
        let result = load(&mut loader, html("<h1>Never shown</h1>").id()).await;
        assert!(matches!(result, Err(LoadError::Fetch(_))), "{result:?}");
        Ok(())
    })
}

const SESSION: &str = "session";

#[derive(Debug, ThisError)]
enum TestError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Client(#[from] AcpClientError),
    #[error("the tool refused: {0}")]
    Tool(#[from] Error),
    #[error(transparent)]
    Load(#[from] LoadError),
    #[error("connection closed")]
    Closed,
}

struct Shown {
    client: AcpClient,
    tools: SessionTools,
    workspace: TempDir,
}

impl Shown {
    async fn open() -> Result<Self, TestError> {
        let (agent, mut requests) = FakeAgent::new(SESSION).mcp_acp().capture();
        let host = ArtifactHost::new(agent.serve(), InMemory::default());
        let client = connect_acp_client(host, initialize_request()).await?;
        let workspace = TempDir::new()?;
        client.handle.request(NewSessionRequest::new(workspace.path())).await?;
        let tools = requests.opened_tools().await.ok_or(TestError::Closed)?;
        Ok(Self { client, tools, workspace })
    }

    async fn submit(&mut self, name: &str, page: &str) -> Result<Artifact, TestError> {
        let path: PathBuf = self.workspace.path().join(name);
        fs::write(&path, page)?;
        self.tools.call("submit_artifact", json!({ "url": path })).await?;
        while let Some(event) = self.client.event_rx.recv().await {
            if let AcpEvent::Artifact(ArtifactPayload { artifact, .. }) = event {
                return Ok(artifact);
            }
        }
        Err(TestError::Closed)
    }
}

async fn load(loader: &mut ArtifactLoader, id: ArtifactId) -> Result<ArtifactContent, LoadError> {
    if let Some(answered) = loader.load(SessionId::new(SESSION), id) {
        return answered.result;
    }
    let finished = loader.finished().await;
    assert_eq!(finished.id, id);
    finished.result
}

fn html(page: &str) -> ArtifactContent {
    ArtifactContent::Html(page.to_owned())
}
