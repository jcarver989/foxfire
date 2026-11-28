mod in_memory;
#[cfg(not(target_arch = "wasm32"))]
mod tmp_dir;

use foxfire_acp::artifact::{ArtifactContent, ArtifactId, ContentError};
use std::future::Future;
use std::io;
use std::path::PathBuf;
use thiserror::Error;

pub use in_memory::InMemory;
#[cfg(not(target_arch = "wasm32"))]
pub use tmp_dir::TmpDir;

pub trait ArtifactStore: Send + Sync + 'static {
    fn put(&mut self, content: ArtifactContent) -> impl Future<Output = Result<ArtifactId, StoreError>> + Send;

    fn get(&self, id: &ArtifactId) -> impl Future<Output = Result<ArtifactContent, StoreError>> + Send;

    fn remove(&mut self, id: &ArtifactId) -> impl Future<Output = Result<(), StoreError>> + Send;
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("no artifact is kept under {0}")]
    Missing(ArtifactId),
    #[error("failed to use {}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error(transparent)]
    Content(#[from] ContentError),
}
