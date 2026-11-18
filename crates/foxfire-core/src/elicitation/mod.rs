mod form;

pub use form::{
    FieldError, FieldInput, FormError, FormField, FormFieldKind, SelectOption, accept_content, form_fields,
};

use agent_client_protocol::v2::schema::{
    CancelledPermissionOutcome, CreateElicitationResponse, ElicitationAcceptAction, ElicitationAction,
    ElicitationContentValue, PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome,
};
use agent_client_protocol::{RawJsonRpcId, Responder};
pub use foxfire_acp::artifact_review::{ArtifactReviewElicitationMeta, ArtifactReviewSubmission};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone)]
pub enum Elicitation {
    Form {
        source: String,
        message: String,
        fields: Vec<FormField>,
    },
    Url {
        source: String,
        message: String,
        url: String,
    },
    ArtifactReview(ArtifactReviewElicitationMeta),
    /// The agent asks before it acts, offering the choices it understands.
    Permission {
        title: String,
        description: Option<String>,
        options: Vec<PermissionChoice>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionChoice {
    pub id: String,
    pub name: String,
    pub kind: PermissionOptionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElicitationId(u64);

#[derive(Debug, Clone, PartialEq)]
pub enum ElicitationAnswer {
    Accept(Option<BTreeMap<String, ElicitationContentValue>>),
    /// Picks one of a [`Elicitation::Permission`]'s choices by its id.
    Choose(String),
    Decline,
    Cancel,
}

impl ElicitationId {
    pub fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

impl Elicitation {
    pub fn question(&self) -> String {
        match self {
            Self::Form { message, .. } | Self::Url { message, .. } => message.clone(),
            Self::ArtifactReview(review) => format!("Review {}", review.title),
            Self::Permission { title, .. } => title.clone(),
        }
    }

    pub fn permission(request: &RequestPermissionRequest) -> Self {
        let options = request
            .options
            .iter()
            .map(|option| PermissionChoice {
                id: option.option_id.to_string(),
                name: option.name.clone(),
                kind: option.kind.clone(),
            })
            .collect();
        Self::Permission { title: request.title.clone(), description: request.description.clone(), options }
    }
}

impl PermissionChoice {
    pub fn allows(&self) -> bool {
        matches!(self.kind, PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways)
    }
}

impl ElicitationAnswer {
    /// Answers an [`Elicitation::ArtifactReview`] with the reviewer's verdict.
    pub fn review(submission: &ArtifactReviewSubmission) -> Self {
        Self::accept_strings(submission.fields())
    }

    pub fn accept_strings<'a, 'b>(content: impl IntoIterator<Item = (&'a str, &'b str)>) -> Self {
        Self::Accept(Some(
            content
                .into_iter()
                .map(|(name, value)| (name.to_string(), ElicitationContentValue::String(value.to_string())))
                .collect(),
        ))
    }

    fn into_action(self) -> ElicitationAction {
        match self {
            Self::Accept(content) => ElicitationAction::Accept(ElicitationAcceptAction::new().content(content)),
            Self::Choose(_) | Self::Decline => ElicitationAction::Decline,
            Self::Cancel => ElicitationAction::Cancel,
        }
    }

    fn into_outcome(self) -> RequestPermissionOutcome {
        match self {
            Self::Choose(option) => RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option)),
            Self::Accept(_) | Self::Decline | Self::Cancel => {
                RequestPermissionOutcome::Cancelled(CancelledPermissionOutcome::new())
            }
        }
    }
}

pub(crate) struct PendingElicitation {
    pub(crate) id: ElicitationId,
    pub(crate) elicitation: Elicitation,
    reply: Option<Reply>,
}

pub(crate) enum Reply {
    Elicitation(Responder<CreateElicitationResponse>),
    Permission(Responder<RequestPermissionResponse>),
}

impl PendingElicitation {
    pub(crate) fn new(id: ElicitationId, elicitation: Elicitation, reply: Reply) -> Self {
        Self { id, elicitation, reply: Some(reply) }
    }

    pub(crate) fn asked_as(&self, request: &RawJsonRpcId) -> bool {
        match &self.reply {
            Some(Reply::Elicitation(responder)) => responder.raw_id() == request,
            Some(Reply::Permission(responder)) => responder.raw_id() == request,
            None => false,
        }
    }

    pub(crate) fn answer(mut self, answer: ElicitationAnswer) {
        self.respond(answer);
    }

    fn respond(&mut self, answer: ElicitationAnswer) {
        match self.reply.take() {
            Some(Reply::Elicitation(responder)) => {
                let _ = responder.respond(CreateElicitationResponse::new(answer.into_action()));
            }
            Some(Reply::Permission(responder)) => {
                let _ = responder.respond(RequestPermissionResponse::new(answer.into_outcome()));
            }
            None => {}
        }
    }
}

impl Drop for PendingElicitation {
    fn drop(&mut self) {
        self.respond(ElicitationAnswer::Cancel);
    }
}
