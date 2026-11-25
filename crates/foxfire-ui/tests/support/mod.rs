use agent_client_protocol::v2::schema::{Implementation, SessionId};
use foxfire_acp::testing::{Channel, FakeAgent};
use foxfire_core::connection::{Connection, ConnectionError, Session};
use tokio::task::{JoinHandle, spawn_local};

/// Start a real ACP fake agent and retain its task so tests can stop or await it.
pub(crate) fn fake_agent(agent: FakeAgent) -> (Channel, JoinHandle<()>) {
    let (agent_end, client_end) = Channel::duplex();
    let task = spawn_local(async move {
        let _ = agent.agent().connect_to(agent_end).await;
    });
    (client_end, task)
}

/// Connect to the fake agent and open either a new or resumed session.
pub(crate) async fn open(client: Channel, resume: Option<SessionId>) -> Result<Session, ConnectionError> {
    let connection = Connection::connect_to(Implementation::new("foxfire-test", "1"), client).await?;
    connection.open(resume, "/work".into()).await
}
