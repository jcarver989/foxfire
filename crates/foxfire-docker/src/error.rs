use std::io;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("could not run {program}: {source}; is it installed and on PATH?")]
    Spawn {
        program: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("`{command}` failed: {message}")]
    Failed { command: String, message: String },
    #[error("`{0}` is not a valid project name; use lowercase letters, digits and dashes")]
    InvalidName(String),
    #[error("`{0}` is not a container ID")]
    InvalidId(String),
    #[error("{image} does not exist; run `foxfire-docker warm {project}` first")]
    NoImage { image: String, project: String },
    #[error("{id} is not a {project} instance")]
    NotOwned { id: String, project: String },
    #[error("instance {0} does not exist and left no work in the checkout")]
    UnknownInstance(String),
    #[error("instance {0} is not running; Resume it first")]
    NotRunning(String),
    #[error("could not use {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not write the output: {0}")]
    Output(#[from] io::Error),
}

impl Error {
    /// Whether Docker failed because the image or container does not exist.
    pub(crate) fn is_missing(&self) -> bool {
        matches!(self, Self::Failed { message, .. } if message.contains("No such"))
    }
}
