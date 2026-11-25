use crate::target::{AgentConnector, SessionTarget};
use agent_client_protocol::v2::schema::SessionId;
use foxfire_core::runtime::ReviewOverride;
use foxfire_ui::backend::SessionLink;
use foxfire_ui::driver::{OpenedSession, drive};
use std::sync::Arc;
use std::time::Instant;
use tracing::{Span, field};

pub(super) fn open(
    thread: usize,
    connector: Arc<AgentConnector>,
    mut target: SessionTarget,
    resume: Option<SessionId>,
) -> SessionLink {
    let span = tracing::debug_span!(target: "foxfire_startup", "thread_startup", thread, agent = target.label(), instance = field::Empty);
    if let Some(instance) = target.instance() {
        span.record("instance", instance);
    }
    let _entered = span.entered();
    drive(|launches| async move {
        let started = Instant::now();
        if let Some(launched) = connector.launch(&target).await? {
            Span::current().record("instance", launched.as_str());
            tracing::debug!(target: "foxfire_startup", elapsed_ms = started.elapsed().as_millis(), "Launch command finished");
            target.set_instance(launched.clone());
            launches.launched(launched);
        }
        let session = connector.connect(&target, resume).await?;
        Ok(OpenedSession::from(session).reviewed_at(async move {
            let source = connector.review_override(&target).await;
            source.unwrap_or_else(|error| Some(ReviewOverride::Unavailable(error.to_string())))
        }))
    })
}
