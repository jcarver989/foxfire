use crate::support::{RelayBuilder, TestError, until};
use agent_client_protocol::v2::schema::Implementation;
use clankerdiff_core::{DiffScope, ReviewTarget};
use clankerdiff_protocol::client::ClientCommand;
use clankerdiff_protocol::shared::{Event, LIVE_PROTOCOL_VERSION};
use foxfire_acp::aether::{
    AetherCapabilities, GitDiffCommandPayload, RemoteServerInfo, SessionPreviewParams, WorkspaceStatusPayload,
};
use foxfire_acp::client::AcpEvent;
use foxfire_acp::testing::{FakeAgent, run_local};
use std::fs;

#[test]
fn a_repository_is_reviewed_and_reported_over_the_acp_socket() -> Result<(), TestError> {
    run_local(async {
        let agent = FakeAgent::default().agent_info(Implementation::new("wrapped", "1.0")).prompt_history(&[]);
        let mut relay = RelayBuilder::new(agent).repository().build().await?;
        fs::write(relay.cwd().join("README.md"), "changed\n")?;
        let mut client = relay.client().await?;

        let response = &client.initialize_response;
        let capabilities = AetherCapabilities::of(&response.capabilities);
        assert_eq!(response.info.name, "wrapped");
        assert!(capabilities.git_diff && capabilities.workspace_status);
        assert!(capabilities.prompt_search, "the agent's own extensions stay advertised");
        assert_eq!(RemoteServerInfo::from_meta(response.meta.as_ref()).map(|remote| remote.cwd), Some(relay.cwd()));

        let status = client.handle.request(WorkspaceStatusPayload { session_id: "any".to_owned() }).await?;
        assert_eq!(status.git_ref.as_deref(), Some("main"));
        let preview = client.handle.request(SessionPreviewParams { session_id: "saved".to_owned() }).await?;
        assert_eq!(preview.cwd, relay.cwd());
        assert!(relay.agent.extensions.try_recv().is_err(), "the relay answers for the agent");

        let target = ReviewTarget::WorkingTree(DiffScope::Both);
        let command = ClientCommand::Initialize { protocol_version: LIVE_PROTOCOL_VERSION, target };
        client.handle.notify(GitDiffCommandPayload { session_id: "any".to_owned(), command })?;
        let changed = until(&mut client, |event| match event {
            AcpEvent::GitDiffEvent(payload) => match payload.event {
                Event::Document(document) => Some(document.files.len()),
                _ => None,
            },
            _ => None,
        })
        .await?;
        assert_eq!(changed, 1);
        Ok(())
    })
}

#[test]
fn a_workspace_that_is_not_a_repository_offers_no_review() -> Result<(), TestError> {
    run_local(async {
        let relay = RelayBuilder::new(FakeAgent::default()).build().await?;
        let client = relay.client().await?;
        let capabilities = AetherCapabilities::of(&client.initialize_response.capabilities);
        assert!(!capabilities.git_diff && capabilities.workspace_status);
        Ok(())
    })
}
