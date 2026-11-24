use crate::aether::{ContextClearedParams, GitDiffEventPayload, McpNotification};
use crate::chat::ChatChannelEventPayload;
use agent_client_protocol::Responder;
use agent_client_protocol::v2::schema::{
    AuthStatusUpdateNotification, CreateElicitationRequest, CreateElicitationResponse, ElicitationScope,
    RequestPermissionRequest, RequestPermissionResponse, SessionId, UpdateSessionNotification,
};

/// Events forwarded from the ACP connection to the main event loop.
pub enum AcpEvent {
    SessionUpdate(Box<UpdateSessionNotification>),
    ContextCleared(ContextClearedParams),
    AuthStatusUpdated(AuthStatusUpdateNotification),
    McpNotification(McpNotification),
    GitDiffEvent(GitDiffEventPayload),
    ChatChannelEvent(ChatChannelEventPayload),
    ElicitationRequest { params: Box<CreateElicitationRequest>, responder: Responder<CreateElicitationResponse> },
    PermissionRequest { params: Box<RequestPermissionRequest>, responder: Responder<RequestPermissionResponse> },
    ConnectionClosed,
}

impl AcpEvent {
    pub fn session_id(&self) -> Option<&SessionId> {
        match self {
            Self::SessionUpdate(notification) => Some(&notification.session_id),
            Self::ContextCleared(params) => Some(&params.session_id),
            Self::GitDiffEvent(params) => Some(&params.session_id),
            Self::ChatChannelEvent(params) => Some(&params.session_id),
            Self::PermissionRequest { params, .. } => Some(&params.session_id),
            Self::ElicitationRequest { params, .. } => match params.scope() {
                ElicitationScope::Session(scope) => Some(&scope.session_id),
                _ => None,
            },
            Self::AuthStatusUpdated(_) | Self::McpNotification(_) | Self::ConnectionClosed => None,
        }
    }
}

impl From<UpdateSessionNotification> for AcpEvent {
    fn from(notification: UpdateSessionNotification) -> Self {
        Self::SessionUpdate(Box::new(notification))
    }
}
