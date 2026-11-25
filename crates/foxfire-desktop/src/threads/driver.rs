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
use std::time::Instant;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tracing::{Instrument, Span, field};

pub(super) fn open(
    thread: usize,
    connector: Arc<AgentConnector>,
    target: SessionTarget,
    resume: Option<SessionId>,
) -> SessionLink {
    let (commands, commands_rx) = unbounded_channel();
    let (messages, events) = unbounded_channel();
    let span = tracing::debug_span!(target: "foxfire_startup", "thread_startup", thread, agent = target.label(), instance = field::Empty);
    if let Some(instance) = target.instance() {
        span.record("instance", instance);
    }
    spawn_tokio(drive(connector, target, resume, commands_rx, messages).instrument(span));
    SessionLink { commands, events }
}

async fn drive(
    connector: Arc<AgentConnector>,
    mut target: SessionTarget,
    session_id: Option<SessionId>,
    commands: UnboundedReceiver<Command>,
    messages: UnboundedSender<SessionEvent>,
) {
    let started = Instant::now();
    match connector.launch(&target).await {
        Ok(Some(launched)) => {
            Span::current().record("instance", launched.as_str());
            tracing::debug!(target: "foxfire_startup", elapsed_ms = started.elapsed().as_millis(), "Launch command finished");
            target.set_instance(launched.clone());
            if messages.send(SessionEvent::Launched(launched)).is_err() {
                return;
            }
        }
        Ok(None) => {}
        Err(error) => {
            let _ = messages.send(SessionEvent::ConnectFailed(error.into()));
            return;
        }
    }
    let session = match connector.connect(&target, session_id).await {
        Ok(session) => session,
        Err(error) => {
            let _ = messages.send(SessionEvent::ConnectFailed(error.into()));
            return;
        }
    };
    let (config, mut runtime) = session.into_core();
    if messages.send(SessionEvent::Connected(Box::new(config))).is_ok() {
        tracing::debug!(target: "foxfire_startup", elapsed_ms = started.elapsed().as_millis(), "Session handed to the UI");
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
