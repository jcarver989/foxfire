use super::file::MAX_ARTIFACT_BYTES;
use crate::store::StoreError;
use agent_client_protocol::Error;
use foxfire_acp::artifact::ContentError;
use std::io;
use std::path::PathBuf;
use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub(crate) enum ArtifactError {
    #[error("`{0}` is not a file:// URL or an absolute path")]
    Location(String),
    #[error("failed to read {}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("{} cannot be shown: {source}", path.display())]
    Content { path: PathBuf, source: ContentError },
    #[error("{} exceeds the {MAX_ARTIFACT_BYTES}-byte artifact limit", .0.display())]
    TooLarge(PathBuf),
    #[error("the session shows no such artifact")]
    NotShown,
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<ArtifactError> for Error {
    fn from(error: ArtifactError) -> Self {
        match error {
            ArtifactError::Store(_) => Self::internal_error(),
            _ => Self::invalid_params(),
        }
        .data(error.to_string())
    }
}
