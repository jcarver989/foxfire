mod agent;
mod git_review;

pub use git_review::ReviewOverride;

use super::{build_attachments, local_status};
use crate::command::{Command, CommandResult};
use crate::session_core::Input;
use agent::Requests;
use agent_client_protocol::{RawJsonRpcId, RequestCancellation};
use foxfire_acp::client::{AcpClientHandle, AcpEvent};
use futures::StreamExt;
use git_review::GitReviewRuntime;
use std::collections::VecDeque;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::{AbortHandle, JoinSet};

pub struct SessionRuntime {
    handle: AcpClientHandle,
    events: UnboundedReceiver<AcpEvent>,
    events_closed: bool,
    requests: Requests,
    tasks: JoinSet<CommandResult>,
    preparing: Option<AbortHandle>,
    watching: Option<AbortHandle>,
    git_review: GitReviewRuntime,
    ready: VecDeque<CommandResult>,
}

impl SessionRuntime {
    pub fn new(handle: AcpClientHandle, events: UnboundedReceiver<AcpEvent>) -> Self {
        Self {
            git_review: GitReviewRuntime::new(handle.clone()),
            handle,
            events,
            events_closed: false,
            requests: Requests::new(),
            tasks: JoinSet::new(),
            preparing: None,
            watching: None,
            ready: VecDeque::new(),
        }
    }

    /// Sets the host override for subsequent review opens, or clears it to use the session default.
    pub fn set_review_override(&mut self, source: Option<ReviewOverride>) {
        self.git_review.set_review_override(source);
    }

    pub fn dispatch(&mut self, command: Command) {
        let result = match command {
            Command::Agent(command) => agent::execute(&self.handle, command, &mut self.requests),
            Command::GitReview(command) => self.git_review.dispatch(command, &mut self.tasks),
            Command::PrepareSubmission { attachments, access } => {
                let preparing = self
                    .tasks
                    .spawn_blocking(move || CommandResult::SubmissionPrepared(build_attachments(&attachments, access)));
                if let Some(superseded) = self.preparing.replace(preparing) {
                    superseded.abort();
                }
                None
            }
            Command::ReadWorkspaceStatus { cwd } => {
                self.tasks.spawn_blocking(move || CommandResult::WorkspaceResolved { status: local_status(&cwd), cwd });
                None
            }
        };
        self.ready.extend(result);
    }

    pub async fn next(&mut self) -> Input {
        if let Some(result) = self.ready.pop_front() {
            return Input::Result(result);
        }
        loop {
            tokio::select! {
                event = self.events.recv(), if !self.events_closed => {
                    let Some(event) = event else {
                        self.events_closed = true;
                        return Input::Event(AcpEvent::ConnectionClosed);
                    };
                    if let Some(event) = self.route(event) {
                        return Input::Event(event);
                    }
                }
                state = self.git_review.changed() => match state {
                    Some(state) => return Input::Result(CommandResult::GitReview(state)),
                    None => self.git_review.close(),
                },
                Some(result) = self.requests.next(), if !self.requests.is_empty() => {
                    return Input::Result(result);
                }
                Some(result) = next_task(&mut self.tasks, &mut self.preparing), if !self.tasks.is_empty() => {
                    return Input::Result(result);
                }
            }
        }
    }

    pub fn try_next_event(&mut self) -> Option<AcpEvent> {
        while let Ok(event) = self.events.try_recv() {
            if let Some(event) = self.route(event) {
                return Some(event);
            }
        }
        None
    }

    pub async fn shutdown(&mut self) {
        self.git_review.close();
        self.handle.disconnect().await;
        self.requests.clear();
        self.tasks.shutdown().await;
    }

    fn route(&mut self, event: AcpEvent) -> Option<AcpEvent> {
        match event {
            AcpEvent::GitDiffEvent(params) => {
                self.git_review.forward(&params.session_id, params.event);
                return None;
            }
            AcpEvent::PermissionRequest { ref responder, .. } => {
                self.watch(responder.cancellation(), responder.raw_id());
            }
            AcpEvent::ElicitationRequest { ref responder, .. } => {
                self.watch(responder.cancellation(), responder.raw_id());
            }
            _ => {}
        }
        Some(event)
    }

    fn watch(&mut self, cancellation: RequestCancellation, id: &RawJsonRpcId) {
        let id = id.clone();
        let watching = self.tasks.spawn(async move {
            cancellation.cancelled().await;
            CommandResult::ElicitationWithdrawn(id)
        });
        if let Some(superseded) = self.watching.replace(watching) {
            superseded.abort();
        }
    }
}

/// A superseded submission's attachments may still finish reading after the
/// abort, so only the latest preparation's result is passed on.
async fn next_task(tasks: &mut JoinSet<CommandResult>, preparing: &mut Option<AbortHandle>) -> Option<CommandResult> {
    loop {
        match tasks.join_next_with_id().await? {
            Ok((id, result)) => {
                if preparing.as_ref().is_some_and(|handle| handle.id() == id) {
                    *preparing = None;
                } else if matches!(result, CommandResult::SubmissionPrepared(_)) {
                    continue;
                }
                return Some(result);
            }
            Err(error) if error.is_cancelled() => {}
            Err(error) => return Some(CommandResult::BackgroundFailed(error.to_string())),
        }
    }
}
