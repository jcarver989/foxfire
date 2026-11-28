use agent_client_protocol::Error;
use agent_client_protocol::v2::schema::{
    NewSessionRequest, ReplayFrom, ReplayFromStart, ResumeSessionRequest, SessionId,
};
use foxfire_acp::artifact::{
    Artifact, ArtifactContent, ArtifactFetchParams, ArtifactFetchResponse, ArtifactId, ArtifactKind, ArtifactPayload,
    ImageFormat,
};
use foxfire_acp::client::{AcpClient, AcpClientError, AcpEvent, connect_acp_client};
use foxfire_acp::testing::{FakeAgent, FakeAgentRequests, SessionTools, initialize_request, run_local};
use foxfire_artifacts::ArtifactHost;
use foxfire_artifacts::store::InMemory;
use serde_json::{Value, json};
use std::fs;
use std::io;
use tempfile::TempDir;
use thiserror::Error as ThisError;

#[test]
fn a_changed_file_is_announced_as_the_next_version_of_its_artifact() -> Result<(), TestError> {
    run_local(async {
        let mut hosted = Hosted::open(FakeAgent::new(SESSION).mcp_acp()).await?;
        let page = hosted.write("pricing.html", "<h1>v1</h1>")?;
        let first = hosted.submit(&page, Some("Pricing")).await?;
        let (_, one) = hosted.announced().await?;
        assert_eq!(
            (one.title.as_str(), one.name.as_str(), one.kind, hosted.fetch(SESSION, one.id).await?),
            ("Pricing", "pricing.html", ArtifactKind::Html, html("<h1>v1</h1>"))
        );

        let same = hosted.submit(&page, None).await?;
        assert_eq!(hosted.fetch(SESSION, one.id).await?, html("<h1>v1</h1>"), "submitting it unchanged keeps it");
        hosted.write("pricing.html", "<h1>v2</h1>")?;
        let second = hosted.submit(&format!("file://{page}"), None).await?;
        assert_eq!((&first["version"], &same, &second["version"]), (&json!(1), &first, &json!(2)));
        let (_, two) = hosted.announced().await?;
        assert_eq!((&two.slot, two.version), (&one.slot, 2));
        assert_eq!(hosted.fetch(SESSION, two.id).await?, html("<h1>v2</h1>"));
        Ok(())
    })
}

#[test]
fn an_image_is_shown_as_an_image() -> Result<(), TestError> {
    run_local(async {
        let mut hosted = Hosted::open(FakeAgent::new(SESSION).mcp_acp()).await?;
        let chart = hosted.write("chart.PNG", [0x89, b'P', b'N', b'G'])?;
        hosted.submit(&chart, None).await?;
        let (_, shown) = hosted.announced().await?;
        let png = ArtifactContent::Image(ImageFormat::Png, vec![0x89, b'P', b'N', b'G'].into());
        assert_eq!((shown.kind, hosted.fetch(SESSION, shown.id).await?), (png.kind(), png));
        Ok(())
    })
}

#[test]
fn clients_identify_an_artifact_without_learning_where_it_is() -> Result<(), TestError> {
    run_local(async {
        let mut hosted = Hosted::open(FakeAgent::new(SESSION).mcp_acp()).await?;
        hosted.submit(&hosted.write("pricing.html", "<h1>v1</h1>")?, None).await?;
        let (_, shown) = hosted.announced().await?;
        let announced = json!(shown).to_string();
        let folder = hosted.workspace.path().to_string_lossy().into_owned();
        assert!(!announced.contains(&folder), "{announced}");
        Ok(())
    })
}

#[test]
fn a_large_artifact_can_be_submitted_and_fetched() -> Result<(), TestError> {
    run_local(async {
        let mut hosted = Hosted::open(FakeAgent::new(SESSION).mcp_acp()).await?;
        let content = format!("<p>{}</p>", "x".repeat(1024 * 1024));
        let page = hosted.write("large.html", &content)?;
        hosted.submit(&page, None).await?;
        let (_, shown) = hosted.announced().await?;
        assert_eq!(hosted.fetch(SESSION, shown.id).await?, html(&content));
        Ok(())
    })
}

#[test]
fn what_cannot_be_shown_is_refused() -> Result<(), TestError> {
    run_local(async {
        let hosted = Hosted::open(FakeAgent::new(SESSION).mcp_acp()).await?;
        let refused = [
            hosted.write("notes.txt", "hi")?,
            hosted.write("binary.html", [0xff, 0xfe])?,
            "relative.html".to_owned(),
            "https://example.com/a.html".to_owned(),
        ];
        for refused in refused {
            assert!(hosted.submit(&refused, None).await.is_err(), "{refused}");
        }
        Ok(())
    })
}

#[test]
fn a_session_keeps_every_artifact_it_is_given() -> Result<(), TestError> {
    run_local(async {
        let hosted = Hosted::open(FakeAgent::new(SESSION).mcp_acp()).await?;
        for n in 0..32 {
            hosted.submit(&hosted.write(&format!("{n}.html"), format!("{n}").repeat(256 * 1024 / 2))?, None).await?;
        }
        Ok(())
    })
}

#[test]
fn each_session_counts_its_own_versions_and_is_shown_them_again_when_replayed() -> Result<(), TestError> {
    run_local(async {
        let mut hosted = Hosted::open(FakeAgent::new(SESSION).mcp_acp()).await?;
        let page = hosted.write("pricing.html", "<h1>v1</h1>")?;
        hosted.submit(&page, None).await?;
        hosted.write("pricing.html", "<h1>v2</h1>")?;
        hosted.submit(&page, None).await?;
        let (_, _first) = hosted.announced().await?;
        let (session, newest) = hosted.announced().await?;
        assert_eq!((session.as_str(), newest.version), (SESSION, 2));

        hosted.resume("other").await?;
        hosted.submit(&page, None).await?;
        let (session, other) = hosted.announced().await?;
        assert_eq!((session.as_str(), other.version), ("other", 1));

        hosted.replay(SESSION).await?;
        assert_eq!(hosted.announced().await?, (SESSION.to_owned(), newest));
        Ok(())
    })
}

#[test]
fn content_one_session_moved_on_from_can_still_be_fetched_for_another() -> Result<(), TestError> {
    run_local(async {
        let mut hosted = Hosted::open(FakeAgent::new(SESSION).mcp_acp()).await?;
        let page = hosted.write("pricing.html", "<h1>v1</h1>")?;
        hosted.submit(&page, None).await?;
        let (_, shared) = hosted.announced().await?;

        hosted.resume("other").await?;
        hosted.submit(&page, None).await?;
        hosted.write("pricing.html", "<h1>v2</h1>")?;
        hosted.submit(&page, None).await?;
        hosted.announced().await?;
        hosted.announced().await?;

        assert_eq!(hosted.fetch(SESSION, shared.id).await?, html("<h1>v1</h1>"));
        Ok(())
    })
}

#[test]
fn a_session_can_only_fetch_what_it_shows() -> Result<(), TestError> {
    run_local(async {
        let mut hosted = Hosted::open(FakeAgent::new(SESSION).mcp_acp()).await?;
        let page = hosted.write("pricing.html", "<h1>v1</h1>")?;
        hosted.submit(&page, None).await?;
        let (_, first) = hosted.announced().await?;
        hosted.write("pricing.html", "<h1>v2</h1>")?;
        hosted.submit(&page, None).await?;
        let (_, second) = hosted.announced().await?;

        assert!(hosted.fetch(SESSION, first.id).await.is_err(), "a replaced version is gone");
        assert!(hosted.fetch("other", second.id).await.is_err(), "another session never showed it");
        Ok(())
    })
}

#[test]
fn an_agent_that_cannot_reach_the_tool_is_not_given_it() -> Result<(), TestError> {
    run_local(async {
        let hosted = Hosted::open(FakeAgent::new(SESSION)).await?;
        assert_eq!(hosted.tools.servers(), []);
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
    #[error("connection closed")]
    Closed,
}

struct Hosted {
    client: AcpClient,
    agent: FakeAgentRequests,
    tools: SessionTools,
    workspace: TempDir,
}

impl Hosted {
    async fn open(agent: FakeAgent) -> Result<Self, TestError> {
        let (agent, mut requests) = agent.capture();
        let host = ArtifactHost::new(agent.serve(), InMemory::default());
        let client = connect_acp_client(host, initialize_request()).await?;
        let workspace = TempDir::new()?;
        client.handle.request(NewSessionRequest::new(workspace.path())).await?;
        let tools = requests.opened_tools().await.ok_or(TestError::Closed)?;
        Ok(Self { client, agent: requests, tools, workspace })
    }

    fn write(&self, name: &str, content: impl AsRef<[u8]>) -> Result<String, TestError> {
        let path = self.workspace.path().join(name);
        fs::write(&path, content)?;
        Ok(path.to_string_lossy().into_owned())
    }

    async fn submit(&self, url: &str, title: Option<&str>) -> Result<Value, TestError> {
        Ok(self.tools.call("submit_artifact", json!({ "url": url, "title": title })).await?)
    }

    async fn fetch(&self, session: &str, id: ArtifactId) -> Result<ArtifactContent, TestError> {
        let request = ArtifactFetchParams { session_id: SessionId::new(session), id };
        let ArtifactFetchResponse { content } = self.client.handle.request(request).await?;
        Ok(content)
    }

    async fn resume(&mut self, session: &str) -> Result<(), TestError> {
        self.client.handle.request(ResumeSessionRequest::new(session, self.workspace.path())).await?;
        self.tools = self.agent.resumed_tools().await.ok_or(TestError::Closed)?;
        Ok(())
    }

    async fn replay(&mut self, session: &str) -> Result<(), TestError> {
        let replayed = ResumeSessionRequest::new(session, self.workspace.path())
            .replay_from(ReplayFrom::from(ReplayFromStart::default()));
        self.client.handle.request(replayed).await?;
        self.tools = self.agent.resumed_tools().await.ok_or(TestError::Closed)?;
        Ok(())
    }

    async fn announced(&mut self) -> Result<(String, Artifact), TestError> {
        while let Some(event) = self.client.event_rx.recv().await {
            if let AcpEvent::Artifact(ArtifactPayload { session_id, artifact }) = event {
                return Ok((SessionId::to_string(&session_id), artifact));
            }
        }
        Err(TestError::Closed)
    }
}

fn html(page: &str) -> ArtifactContent {
    ArtifactContent::Html(page.to_owned())
}
