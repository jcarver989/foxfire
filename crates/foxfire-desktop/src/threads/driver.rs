use super::thread::Thread;
use crate::target::{AgentConnector, ConnectError, SessionTarget};
use agent_client_protocol::v2::schema::SessionId;
use foxfire_core::command::Command;
use foxfire_core::native::runtime::{ReviewOverride, SessionRuntime};
use foxfire_core::session_core::{Input, SessionCoreConfig};
use rouge::reactive::{Task, spawn, spawn_tokio};
use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

pub(crate) enum DriverMessage {
    Launched(String),
    Connected(Box<SessionCoreConfig>),
    OpenReview,
    ConnectFailed(ConnectError),
    Input(Input),
}

pub(crate) struct Driver {
    commands: UnboundedSender<Command>,
    _connection: Task<()>,
    receive: Task<()>,
}

impl Driver {
    pub(crate) fn spawn(
        connector: Arc<AgentConnector>,
        target: SessionTarget,
        session_id: Option<SessionId>,
        thread: Thread,
    ) -> Self {
        let (commands, commands_rx) = unbounded_channel();
        let (messages, mut messages_rx) = unbounded_channel();
        let connection = spawn_tokio(drive(connector, target, session_id, commands_rx, messages));
        let receive = spawn(async move {
            while let Some(message) = messages_rx.recv().await {
                let batch = std::iter::once(message).chain(std::iter::from_fn(|| messages_rx.try_recv().ok()));
                if !thread.receive(batch.collect()) {
                    break;
                }
            }
        });
        Self { commands, _connection: connection, receive }
    }

    pub(crate) fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        self.receive.cancel();
    }
}

async fn drive(
    connector: Arc<AgentConnector>,
    mut target: SessionTarget,
    session_id: Option<SessionId>,
    commands: UnboundedReceiver<Command>,
    messages: UnboundedSender<DriverMessage>,
) {
    match connector.launch(&target).await {
        Ok(Some(launched)) => {
            target.set_instance(launched.clone());
            if messages.send(DriverMessage::Launched(launched)).is_err() {
                return;
            }
        }
        Ok(None) => {}
        Err(error) => {
            let _ = messages.send(DriverMessage::ConnectFailed(error));
            return;
        }
    }
    let session = match connector.connect(&target, session_id).await {
        Ok(session) => session,
        Err(error) => {
            let _ = messages.send(DriverMessage::ConnectFailed(error));
            return;
        }
    };
    let (config, mut runtime) = session.into_core();
    if messages.send(DriverMessage::Connected(Box::new(config))).is_ok() {
        forward(&mut runtime, connector.review_override(&target), commands, &messages).await;
    }
    runtime.shutdown().await;
}

async fn forward(
    runtime: &mut SessionRuntime,
    review: impl Future<Output = Result<Option<ReviewOverride>, ConnectError>>,
    mut commands: UnboundedReceiver<Command>,
    messages: &UnboundedSender<DriverMessage>,
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
                DriverMessage::OpenReview
            }
            input = runtime.next() => DriverMessage::Input(input),
            command = commands.recv() => match command {
                Some(command) => {
                    runtime.dispatch(command);
                    continue;
                }
                None => return,
            },
        };
        let closed =
            matches!(message, DriverMessage::Input(Input::Event(foxfire_acp::client::AcpEvent::ConnectionClosed)));
        if messages.send(message).is_err() || closed {
            return;
        }
    }
}
