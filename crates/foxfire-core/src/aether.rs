//! The Aether extension types that clients see through this crate's API. Clients read Aether features only through
//! here and [`crate::session_model::SessionModel::capabilities`], never from `foxfire_acp::aether` directly.

pub use foxfire_acp::aether::{
    AetherCapabilities, McpServerStatus, McpServerStatusEntry, PromptSearchResult, SessionPreviewResponse,
    SessionPreviewRole, WorkspaceEntry, WorkspaceMoveTarget,
};
