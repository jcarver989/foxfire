//! Run in a browser against the fake agent, with `scripts/test-web`.
#![cfg(target_arch = "wasm32")]

use agent_client_protocol::v2::schema::Implementation;
use foxfire_acp::websocket::{ConnectError, Endpoint, connect};
use foxfire_core::composer::submission::Submission;
use foxfire_core::connection::{Connection, ConnectionError};
use foxfire_core::runtime::SessionRuntime;
use foxfire_core::session_core::{Effect, SessionCore};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn a_session_core_holds_a_conversation() -> Result<(), TestError> {
    let (mut core, mut runtime) = open().await?;
    core.submit(Submission::from("hello"));
    run_until(&mut core, &mut runtime, |core| {
        core.conversation().items().iter().any(|item| item.text().is_some_and(|text| text.contains(REPLY)))
    })
    .await;
    run_until(&mut core, &mut runtime, |core| !core.is_agent_busy()).await;
    runtime.shutdown().await;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum TestError {
    #[error("run these tests with `scripts/test-web`")]
    NoServer,
    #[error(transparent)]
    Socket(#[from] ConnectError),
    #[error(transparent)]
    Connection(#[from] ConnectionError),
}

const PROTOCOL: &str = "fake-agent.v1";
const REPLY: &str = "hello from the fake agent";

async fn open() -> Result<(SessionCore, SessionRuntime), TestError> {
    let url = option_env!("FAKE_AGENT_WS_URL").ok_or(TestError::NoServer)?;
    let endpoint = Endpoint { subprotocols: vec![PROTOCOL.to_owned()], ..Endpoint::new(format!("{url}/protocol")) };
    let connection =
        Connection::connect_to(Implementation::new("foxfire-test", "1"), connect(&endpoint).await?).await?;
    let (config, runtime) = connection.open(None, "/work".into()).await?.into_core();
    Ok((SessionCore::new(config), runtime))
}

async fn run_until(core: &mut SessionCore, runtime: &mut SessionRuntime, done: impl Fn(&SessionCore) -> bool) {
    loop {
        for effect in core.take_effects() {
            if let Effect::Dispatch(command) = effect {
                runtime.dispatch(command);
            }
        }
        if done(core) {
            return;
        }
        core.on_input(runtime.next().await);
    }
}
