use super::artifacts::ArtifactsHandle;
use super::file::ArtifactFile;
use agent_client_protocol::mcp_server::McpServer;
use agent_client_protocol::v2::schema::SessionId;
use agent_client_protocol::{Conductor, Error, RunWithConnectionTo, tool_fn};
use agent_client_protocol_rmcp::rmcp_3::McpServerExt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};

#[derive(Deserialize, JsonSchema)]
struct Submit {
    /// An absolute path or file:// URL on the agent's machine, to an HTML, PNG, JPEG, GIF or WebP file of up to 256 KiB.
    url: String,
    /// What to call the artifact. Defaults to the file name.
    title: Option<String>,
}

/// Receipt returned by `submit_artifact`; the content itself goes only to the clients.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Submitted {
    pub(crate) version: u32,
}

pub(crate) fn tool(
    session: Arc<OnceLock<SessionId>>,
    artifacts: ArtifactsHandle,
) -> McpServer<Conductor, impl RunWithConnectionTo<Conductor>> {
    McpServer::builder("foxfire")
        .tool_fn(
            "submit_artifact",
            include_str!("submit_artifact.md"),
            async move |input: Submit, _| {
                let id = session.get().ok_or_else(|| Error::invalid_request().data("the session is not open"))?;
                let snapshot = ArtifactFile::locate(&input.url).await?.read(input.title).await?;
                artifacts.submit(id.clone(), snapshot).await
            },
            tool_fn!(),
        )
        .build()
}
