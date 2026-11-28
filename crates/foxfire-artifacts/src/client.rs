#[cfg(target_arch = "wasm32")]
use crate::store::InMemory;
#[cfg(not(target_arch = "wasm32"))]
use crate::store::TmpDir;
use crate::store::{ArtifactStore, StoreError};
use agent_client_protocol::v2::schema::SessionId;
use foxfire_acp::artifact::{ArtifactContent, ArtifactFetchParams, ArtifactFetchResponse, ArtifactId};
use foxfire_acp::client::{AcpClientError, AcpClientHandle};
#[cfg(not(target_arch = "wasm32"))]
use futures::future::BoxFuture;
#[cfg(target_arch = "wasm32")]
use futures::future::LocalBoxFuture;
use std::collections::VecDeque;
use std::future::pending;
use thiserror::Error;

pub struct ArtifactLoader {
    state: Option<State>,
    waiting: VecDeque<(SessionId, ArtifactId)>,
}

#[derive(Debug)]
pub struct Loaded {
    pub id: ArtifactId,
    pub result: Result<ArtifactContent, LoadError>,
}

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("failed to fetch the artifact: {0}")]
    Fetch(#[from] AcpClientError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("the session sent content other than artifact {0}")]
    Mismatch(ArtifactId),
    #[error("there is nowhere to keep artifacts: {0}")]
    Unavailable(String),
}

#[cfg(not(target_arch = "wasm32"))]
type ClientStore = TmpDir;
#[cfg(target_arch = "wasm32")]
type ClientStore = InMemory;

enum State {
    Unavailable(String),
    Idle(ReadThrough<ClientStore>),
    Loading(Loading),
}

struct ReadThrough<T> {
    store: T,
    handle: AcpClientHandle,
}

#[cfg(not(target_arch = "wasm32"))]
type Loading = BoxFuture<'static, (ReadThrough<ClientStore>, Loaded)>;
#[cfg(target_arch = "wasm32")]
type Loading = LocalBoxFuture<'static, (ReadThrough<ClientStore>, Loaded)>;

impl ArtifactLoader {
    pub fn new(handle: AcpClientHandle) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let store = TmpDir::new();
        #[cfg(target_arch = "wasm32")]
        let store = Ok::<_, StoreError>(InMemory::default());
        let state = match store {
            Ok(store) => State::Idle(ReadThrough { store, handle }),
            Err(error) => {
                tracing::warn!(%error, "artifacts cannot be shown");
                State::Unavailable(error.to_string())
            }
        };
        Self { state: Some(state), waiting: VecDeque::new() }
    }

    pub fn load(&mut self, session_id: SessionId, id: ArtifactId) -> Option<Loaded> {
        if let Some(State::Unavailable(reason)) = &self.state {
            return Some(Loaded { id, result: Err(LoadError::Unavailable(reason.clone())) });
        }
        self.waiting.push_back((session_id, id));
        self.start();
        None
    }

    pub fn is_loading(&self) -> bool {
        matches!(self.state, Some(State::Loading(_)))
    }

    pub async fn finished(&mut self) -> Loaded {
        let Some(State::Loading(loading)) = self.state.as_mut() else { return pending().await };
        let (source, loaded) = loading.await;
        self.state = Some(State::Idle(source));
        self.start();
        loaded
    }

    fn start(&mut self) {
        if !matches!(self.state, Some(State::Idle(_))) {
            return;
        }
        let Some((session_id, id)) = self.waiting.pop_front() else { return };
        let Some(State::Idle(mut source)) = self.state.take() else { return };
        self.state = Some(State::Loading(Box::pin(async move {
            let result = source.load(session_id, id).await;
            (source, Loaded { id, result })
        })));
    }
}

impl<T: ArtifactStore> ReadThrough<T> {
    async fn load(&mut self, session_id: SessionId, id: ArtifactId) -> Result<ArtifactContent, LoadError> {
        match self.store.get(&id).await {
            Err(StoreError::Missing(_)) => {}
            kept => return Ok(kept?),
        }
        let ArtifactFetchResponse { content } = self.handle.request(ArtifactFetchParams { session_id, id }).await?;
        if content.id() != id {
            return Err(LoadError::Mismatch(id));
        }
        self.store.put(content.clone()).await?;
        Ok(content)
    }
}
