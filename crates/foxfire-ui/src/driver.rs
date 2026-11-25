use crate::backend::{ConnectFailure, SessionEvent, SessionLink};
use foxfire_acp::client::AcpEvent;
use foxfire_core::command::Command;
use foxfire_core::connection::Session;
use foxfire_core::runtime::{ReviewOverride, SessionRuntime};
use foxfire_core::session_core::Input;
use rouge::reactive::spawn_tokio;
use std::future::{Future, ready};
use std::pin::Pin;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tracing::Instrument;
use web_time::Instant;

/// A session a host has opened, and where its changes are reviewed.
pub struct OpenedSession {
    session: Session,
    review: Review,
}

/// Tells a thread which instance was launched for it, even if its session then fails to open.
pub struct Launches(UnboundedSender<SessionEvent>);

type Review = Pin<Box<dyn Future<Output = Option<ReviewOverride>> + Send>>;

/// Opens a session and runs it until its agent disconnects or the link is dropped.
pub fn drive<T>(open: impl FnOnce(Launches) -> T) -> SessionLink
where
    T: Future<Output = Result<OpenedSession, ConnectFailure>> + Send + 'static,
{
    let (commands, commands_rx) = unbounded_channel();
    let (messages, events) = unbounded_channel();
    let opening = open(Launches(messages.clone()));
    spawn_tokio(run(opening, commands_rx, messages).in_current_span());
    SessionLink { commands, events }
}

impl OpenedSession {
    /// Reviews the session's changes where `review` says, instead of through its agent.
    #[must_use]
    pub fn reviewed_at(mut self, review: impl Future<Output = Option<ReviewOverride>> + Send + 'static) -> Self {
        self.review = Box::pin(review);
        self
    }
}

impl From<Session> for OpenedSession {
    fn from(session: Session) -> Self {
        Self { session, review: Box::pin(ready(None)) }
    }
}

impl Launches {
    pub fn launched(&self, instance: String) {
        let _ = self.0.send(SessionEvent::Launched(instance));
    }
}

async fn run(
    opening: impl Future<Output = Result<OpenedSession, ConnectFailure>>,
    commands: UnboundedReceiver<Command>,
    messages: UnboundedSender<SessionEvent>,
) {
    let started = Instant::now();
    let opened = tokio::select! {
        () = messages.closed() => return,
        opened = opening => opened,
    };
    let OpenedSession { session, review } = match opened {
        Ok(opened) => opened,
        Err(failure) => {
            let _ = messages.send(SessionEvent::ConnectFailed(failure));
            return;
        }
    };
    let (config, mut runtime) = session.into_core();
    if messages.send(SessionEvent::Connected(Box::new(config))).is_ok() {
        tracing::debug!(target: "foxfire_startup", elapsed_ms = started.elapsed().as_millis(), "Session handed to the UI");
        forward(&mut runtime, review, commands, &messages).await;
    }
    runtime.shutdown().await;
}

async fn forward(
    runtime: &mut SessionRuntime,
    mut review: Review,
    mut commands: UnboundedReceiver<Command>,
    messages: &UnboundedSender<SessionEvent>,
) {
    let mut review_pending = true;
    loop {
        let message = tokio::select! {
            source = &mut review, if review_pending => {
                review_pending = false;
                runtime.set_review_override(source);
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
        let closed = matches!(message, SessionEvent::Input(Input::Event(AcpEvent::ConnectionClosed)));
        if messages.send(message).is_err() || closed {
            return;
        }
    }
}
