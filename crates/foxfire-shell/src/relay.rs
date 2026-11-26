use crate::hosted::single_session_view;
use agent_client_protocol::v2::schema::Implementation;
use foxfire_acp::chat::Participant;
use foxfire_acp::websocket::{self, Endpoint};
use foxfire_core::connection::{ClientInfo, Connection};
use foxfire_ui::backend::Project;
use foxfire_ui::driver::drive;
use foxfire_ui::platform::ChatPlatform;
use rouge::core::View;
use rouge::persist::{Storage, app_storage};
use rouge::platform::open_url;
use std::rc::Rc;

/// Opens the relay's existing session when its row is selected. Reconnects never provision a thread.
pub fn relay_view(
    endpoint: Endpoint,
    participant: Participant,
    platform: Rc<dyn ChatPlatform>,
    storage: Rc<dyn Storage>,
) -> impl View {
    let project = Project { id: "relay".to_owned(), name: "Relay".to_owned() };
    single_session_view(
        project,
        move |resume| {
            let (endpoint, participant) = (endpoint.clone(), participant.clone());
            drive(|_| async move {
                let transport = websocket::connect(&endpoint).await?;
                let program = Implementation::new(CLIENT, env!("CARGO_PKG_VERSION"));
                let client = ClientInfo { program, participant: Some(participant) };
                let connection = Connection::connect_to(client, transport).await?;
                Ok(connection.open_remote(resume).await?.into())
            })
        },
        platform,
        storage,
    )
}

/// Shows a relay session using this host's saved preferences and browser for links.
pub fn connect(url: &str, name: &str) -> impl View + use<> {
    let participant = Participant { id: name.to_lowercase(), name: name.to_owned() };
    relay_view(Endpoint::new(url), participant, Rc::new(RelayPlatform), app_storage("foxfire"))
}

struct RelayPlatform;

impl ChatPlatform for RelayPlatform {
    fn open(&self, target: &str) {
        if target.starts_with("https://") || target.starts_with("http://") {
            let _ = open_url(target);
        }
    }
}

const CLIENT: &str = if cfg!(any(target_os = "ios", target_os = "android")) { "foxfire-mobile" } else { "foxfire-web" };
