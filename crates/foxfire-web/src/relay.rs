use agent_client_protocol::v2::schema::Implementation;
use foxfire_acp::chat::Participant;
use foxfire_acp::websocket::{self, Endpoint};
use foxfire_core::connection::{ClientInfo, Connection};
use foxfire_shell::hosted::single_session_view;
use foxfire_ui::backend::Project;
use foxfire_ui::driver::drive;
use foxfire_ui::platform::ChatPlatform;
use rouge::core::View;
use rouge::persist::Storage;
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
                let program = Implementation::new("foxfire-web", env!("CARGO_PKG_VERSION"));
                let client = ClientInfo { program, participant: Some(participant) };
                let connection = Connection::connect_to(client, transport).await?;
                Ok(connection.open_remote(resume).await?.into())
            })
        },
        platform,
        storage,
    )
}
