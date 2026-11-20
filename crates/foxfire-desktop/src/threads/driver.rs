use crate::target::{AgentConnector, ConnectError, SessionTarget};
use agent_client_protocol::v2::schema::SessionId;
use foxfire_core::command::Command;
use foxfire_core::native::runtime::{ReviewOverride, SessionRuntime};
use foxfire_core::session_core::Input;
use foxfire_ui::backend::{SessionEvent, SessionLink};
use rouge::reactive::spawn_tokio;
use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

pub(super) fn open(connector: Arc<AgentConnector>, target: SessionTarget, resume: Option<SessionId>) -> SessionLink {
    let (commands, commands_rx) = unbounded_channel();
    let (messages, events) = unbounded_channel();
    spawn_tokio(drive(connector, target, resume, commands_rx, messages));
    SessionLink { commands, events }
}

async fn drive(
    connector: Arc<AgentConnector>,
    mut target: SessionTarget,
    session_id: Option<SessionId>,
    commands: UnboundedReceiver<Command>,
    messages: UnboundedSender<SessionEvent>,
) {
    match connector.launch(&target).await {
        Ok(Some(launched)) => {
            target.set_instance(launched.clone());
            if messages.send(SessionEvent::Launched(launched)).is_err() {
                return;
            }
        }
        Ok(None) => {}
        Err(error) => {
            let _ = messages.send(SessionEvent::ConnectFailed(error.to_string()));
            return;
        }
    }
    let session = match connector.connect(&target, session_id).await {
        Ok(session) => session,
        Err(error) => {
            let _ = messages.send(SessionEvent::ConnectFailed(error.to_string()));
            return;
        }
    };
    let (config, mut runtime) = session.into_core();
    if messages.send(SessionEvent::Connected(Box::new(config))).is_ok() {
        forward(&mut runtime, connector.review_override(&target), commands, &messages).await;
    }
    runtime.shutdown().await;
}

async fn forward(
    runtime: &mut SessionRuntime,
    review: impl Future<Output = Result<Option<ReviewOverride>, ConnectError>>,
    mut commands: UnboundedReceiver<Command>,
    messages: &UnboundedSender<SessionEvent>,
) {
    let mut review = pin!(review);
    let mut review_pending = true;
    loop {
        let message = tokio::select! {
            source = &mut review, if review_pending => {
                review_pending = false;
                runtime.set_review_override(
                    source.unwrap_or_else(|error| Some(ReviewOverride::Unavailable(error.to_string()))),
                );
                SessionEvent::OpenReview
            }
            input = runtime.next() => SessionEvent::Input(input),
            command = commands.recv() => match command {
                Some(command) => {
                    runtime.dispatch(command);
                    continue;
                }
                None => return,
            },
        };
        let closed =
            matches!(message, SessionEvent::Input(Input::Event(foxfire_acp::client::AcpEvent::ConnectionClosed)));
        if messages.send(message).is_err() || closed {
            return;
        }
    }
}
