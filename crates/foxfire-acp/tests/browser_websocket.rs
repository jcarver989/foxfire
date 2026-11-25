//! Run in a browser against the fake agent, with `scripts/test-web`.
#![cfg(target_arch = "wasm32")]

use agent_client_protocol::v2::schema::{Implementation, NewSessionRequest};
use foxfire_acp::client::{AcpClientError, AcpEvent, connect_acp_client, initialize_request};
use foxfire_acp::websocket::{ConnectError, Endpoint, connect};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn acp_runs_over_a_socket_with_a_negotiated_subprotocol() -> Result<(), TestError> {
    let endpoint = offering_protocol("/protocol")?;
    let client = connect_acp_client(connect(&endpoint).await?, initialize_request(client_info())).await?;
    let session = client.handle.request(NewSessionRequest::new("/work")).await?;
    assert_eq!(session.session_id.0.as_ref(), "fake-session");
    client.handle.disconnect().await;
    Ok(())
}

#[wasm_bindgen_test]
async fn dropping_the_client_closes_its_connection() -> Result<(), TestError> {
    let endpoint = offering_protocol("/protocol")?;
    let mut client = connect_acp_client(connect(&endpoint).await?, initialize_request(client_info())).await?;
    drop(client.handle);
    while let Some(event) = client.event_rx.recv().await {
        if matches!(event, AcpEvent::ConnectionClosed) {
            return Ok(());
        }
    }
    Err(TestError::NeverClosed)
}

#[wasm_bindgen_test]
async fn a_server_that_selects_no_subprotocol_refuses_the_handshake() -> Result<(), TestError> {
    let refused = connect(&offering_protocol("/basic")?).await.err();
    assert!(matches!(refused, Some(ConnectError::Handshake(_))), "{refused:?}");
    Ok(())
}

#[wasm_bindgen_test]
async fn a_refused_handshake_is_retryable() -> Result<(), TestError> {
    let refused = connect(&Endpoint::new("ws://127.0.0.1:9")).await.err();
    assert!(refused.is_some_and(|error| error.is_retryable()));
    Ok(())
}

#[wasm_bindgen_test]
async fn an_invalid_url_is_not_retryable() -> Result<(), TestError> {
    let invalid = connect(&Endpoint::new("ws://[")).await.err();
    assert!(matches!(invalid, Some(ConnectError::InvalidUrl { .. })), "{invalid:?}");
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum TestError {
    #[error("run these tests with `scripts/test-web`")]
    NoServer,
    #[error("the connection never closed")]
    NeverClosed,
    #[error(transparent)]
    Connect(#[from] ConnectError),
    #[error(transparent)]
    Client(#[from] AcpClientError),
}

const PROTOCOL: &str = "fake-agent.v1";

fn offering_protocol(scenario: &str) -> Result<Endpoint, TestError> {
    let url = option_env!("FAKE_AGENT_WS_URL").ok_or(TestError::NoServer)?;
    Ok(Endpoint { subprotocols: vec![PROTOCOL.to_owned()], ..Endpoint::new(format!("{url}{scenario}")) })
}

fn client_info() -> Implementation {
    Implementation::new("foxfire-test", "1")
}
