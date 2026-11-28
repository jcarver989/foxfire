use crate::store::{ArtifactStore, StoreError};
use bytes::Bytes;
use foxfire_acp::artifact::{ArtifactContent, ArtifactId, ArtifactKind};
use std::collections::HashMap;
use std::env::temp_dir;
use std::io::ErrorKind;
use std::path::PathBuf;
use tempfile::TempDir;
use tokio::fs;

#[derive(Debug)]
pub struct TmpDir {
    dir: TempDir,
    kinds: HashMap<ArtifactId, ArtifactKind>,
}

impl TmpDir {
    pub fn new() -> Result<Self, StoreError> {
        let dir =
            TempDir::with_prefix("foxfire-artifacts-").map_err(|source| StoreError::Io { path: temp_dir(), source })?;
        Ok(Self { dir, kinds: HashMap::new() })
    }

    fn path(&self, id: &ArtifactId) -> PathBuf {
        self.dir.path().join(id.to_string())
    }
}

impl ArtifactStore for TmpDir {
    async fn put(&mut self, content: ArtifactContent) -> Result<ArtifactId, StoreError> {
        let id = content.id();
        if !self.kinds.contains_key(&id) {
            let path = self.path(&id);
            fs::write(&path, content.bytes()).await.map_err(|source| StoreError::Io { path, source })?;
            self.kinds.insert(id, content.kind());
        }
        Ok(id)
    }

    async fn get(&self, id: &ArtifactId) -> Result<ArtifactContent, StoreError> {
        let kind = *self.kinds.get(id).ok_or(StoreError::Missing(*id))?;
        let path = self.path(id);
        let bytes = fs::read(&path).await.map_err(|source| StoreError::Io { path, source })?;
        Ok(ArtifactContent::decode(kind, Bytes::from(bytes))?)
    }

    async fn remove(&mut self, id: &ArtifactId) -> Result<(), StoreError> {
        if self.kinds.remove(id).is_none() {
            return Ok(());
        }
        let path = self.path(id);
        match fs::remove_file(&path).await {
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            removed => removed.map_err(|source| StoreError::Io { path, source }),
        }
    }
}
