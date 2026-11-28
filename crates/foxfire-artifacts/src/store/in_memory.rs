use crate::store::{ArtifactStore, StoreError};
use foxfire_acp::artifact::{ArtifactContent, ArtifactId};
use std::collections::HashMap;
use std::future::{Future, ready};

#[derive(Debug, Default)]
pub struct InMemory(HashMap<ArtifactId, ArtifactContent>);

impl ArtifactStore for InMemory {
    fn put(&mut self, content: ArtifactContent) -> impl Future<Output = Result<ArtifactId, StoreError>> + Send {
        let id = content.id();
        self.0.entry(id).or_insert(content);
        ready(Ok(id))
    }

    fn get(&self, id: &ArtifactId) -> impl Future<Output = Result<ArtifactContent, StoreError>> + Send {
        ready(self.0.get(id).cloned().ok_or(StoreError::Missing(*id)))
    }

    fn remove(&mut self, id: &ArtifactId) -> impl Future<Output = Result<(), StoreError>> + Send {
        self.0.remove(id);
        ready(Ok(()))
    }
}
