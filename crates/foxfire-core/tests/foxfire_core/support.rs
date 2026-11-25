use agent_client_protocol::v2::schema::Implementation;
use clankerdiff_client::ClientState;
use clankerdiff_core::DiffDocument;
use foxfire_acp::client::{AcpClientError, AcpEvent};
use foxfire_acp::conversation::{ConversationContent, Notice, NoticeLevel};
use foxfire_core::command::CommandResult;
use foxfire_core::connection::ConnectionError;
use foxfire_core::runtime::SessionRuntime;
use foxfire_core::session_core::{Input, SessionCore};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub(crate) enum TestError {
    #[error(transparent)]
    Client(#[from] AcpClientError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Connection(#[from] ConnectionError),
}

pub(crate) fn client_info() -> Implementation {
    Implementation::new("client-core-test", "1")
}

pub(crate) use foxfire_acp::testing::run_local;
pub(crate) use foxfire_core::testing::LiveSession;

pub(crate) fn deliver(core: &mut SessionCore, result: CommandResult) {
    core.on_input(Input::Result(result));
}

pub(crate) fn receive(core: &mut SessionCore, event: impl Into<AcpEvent>) {
    core.on_input(Input::Event(event.into()));
}

pub(crate) fn notices(core: &SessionCore, level: NoticeLevel) -> Vec<&str> {
    core.conversation()
        .items()
        .iter()
        .filter_map(|item| match item.content() {
            ConversationContent::Notice(Notice { level: shown, text }) if *shown == level => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

pub(crate) fn offline() -> AcpClientError {
    AcpClientError::Protocol(agent_client_protocol::Error::new(-32603, "offline"))
}

pub(crate) async fn next_result(runtime: &mut SessionRuntime) -> CommandResult {
    loop {
        if let Input::Result(result) = runtime.next().await {
            return result;
        }
    }
}

pub(crate) async fn next_review_state(runtime: &mut SessionRuntime) -> Arc<ClientState> {
    loop {
        if let CommandResult::GitReview(state) = next_result(runtime).await {
            return state;
        }
    }
}

pub(crate) async fn next_document(runtime: &mut SessionRuntime) -> Arc<DiffDocument> {
    loop {
        if let Some(document) = next_review_state(runtime).await.snapshot.clone() {
            return document;
        }
    }
}
