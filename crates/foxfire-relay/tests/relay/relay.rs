use crate::support::{RelayBuilder, TestError, until};
use foxfire_acp::client::AcpEvent;
use foxfire_acp::testing::{FakeAgent, run_local};
use foxfire_relay::RelayError;

#[test]
fn the_relay_disconnects_every_client_and_stops_when_the_agent_exits() -> Result<(), TestError> {
    run_local(async {
        let mut relay = RelayBuilder::new(FakeAgent::default()).build().await?;
        let mut clients = [relay.client().await?, relay.client().await?];

        relay.stop_agent().await;

        for client in &mut clients {
            until(client, |event| matches!(event, AcpEvent::ConnectionClosed).then_some(())).await?;
        }
        assert!(matches!(relay.stopped().await, Err(RelayError::AgentExited)));
        Ok(())
    })
}
