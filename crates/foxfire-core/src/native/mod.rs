mod attachments;
pub mod fs;
mod git;

pub use attachments::{build_attachments, classify_paths};

use crate::path::home_relative_path;
use crate::session_model::WorkspaceStatus;
use std::path::Path;

/// What the client can tell about a working directory on this machine by itself.
pub fn local_status(cwd: &Path) -> WorkspaceStatus {
    WorkspaceStatus { display_dir: home_relative_path(cwd), git_ref: git::current_ref(cwd) }
}
