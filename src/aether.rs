//! The Aether extension types that clients see through this crate's API. Clients read Aether features only through
//! here and [`crate::session_model::SessionModel::capabilities`], never from `acp_utils::aether` directly.

pub use acp_utils::aether::{
    AetherCapabilities, McpServerStatus, McpServerStatusEntry, PromptSearchResult, SessionPreviewResponse,
    SessionPreviewRole, WorkspaceEntry, WorkspaceMoveTarget,
};
