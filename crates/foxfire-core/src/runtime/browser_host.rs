use super::Task;
use super::git_review::Opening;
use crate::command::CommandResult;
use crate::composer::attachment::{AttachmentOutcome, PromptAttachment};
use crate::session_model::{WorkspaceAccess, initial_workspace_status};
use clankerdiff_client::ClientError;
use std::future::ready;
use std::path::PathBuf;

/// Names the files an agent can read for itself. A browser has none of its own.
pub(super) fn prepare_submission(attachments: Vec<PromptAttachment>, access: WorkspaceAccess) -> Task {
    let mut outcome = AttachmentOutcome::default();
    for attachment in attachments {
        match attachment.link(access) {
            Some(link) => outcome.blocks.push(link),
            None => outcome.warnings.push(format!("Skipped {}: a browser cannot read it", attachment.display_name)),
        }
    }
    Box::pin(ready(CommandResult::SubmissionPrepared(outcome)))
}

pub(super) fn resolve_workspace(cwd: PathBuf) -> Task {
    Box::pin(ready(CommandResult::WorkspaceResolved { status: initial_workspace_status(&cwd), cwd }))
}

pub(super) fn open_local_review(_: PathBuf) -> Opening {
    Box::pin(ready(Err(ClientError::Transport("a browser cannot read a local workspace".to_owned()))))
}
