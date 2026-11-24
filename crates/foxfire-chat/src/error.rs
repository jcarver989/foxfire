use std::fs::TryLockError;
use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ChatError {
    #[error("join the session's channel first")]
    NotJoined,
    #[error("a message needs a key")]
    MissingKey,
    #[error("a message cannot be blank")]
    Blank,
    #[error("a message and its quote can each be at most {limit} bytes")]
    TooLong { limit: usize },
    #[error("the channel is in use elsewhere")]
    InUse,
    #[error("the channel is not stored in a form this version understands: {0}")]
    Unreadable(#[from] serde_json::Error),
    #[error("failed to access stored chat: {0}")]
    Io(#[from] io::Error),
}

impl From<TryLockError> for ChatError {
    fn from(error: TryLockError) -> Self {
        match error {
            TryLockError::WouldBlock => Self::InUse,
            TryLockError::Error(source) => Self::Io(source),
        }
    }
}
