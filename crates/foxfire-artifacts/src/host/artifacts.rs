use super::error::ArtifactError;
use super::file::Snapshot;
use super::mcp::Submitted;
use super::session::{Recorded, SessionArtifacts};
use crate::store::ArtifactStore;
use agent_client_protocol::v2::ConnectionTo;
use agent_client_protocol::v2::schema::SessionId;
use agent_client_protocol::{Client, Conductor, Error};
use foxfire_acp::artifact::{Artifact, ArtifactContent, ArtifactId, ArtifactPayload};
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};

pub(crate) struct Artifacts<T> {
    sessions: HashMap<SessionId, SessionArtifacts>,
    contents: Contents<T>,
    clients: Clients,
}

#[derive(Clone)]
pub(crate) struct ArtifactsHandle(mpsc::UnboundedSender<Operation>);

pub(crate) struct ArtifactsInbox(mpsc::UnboundedReceiver<Operation>);

enum Operation {
    Submit(SessionId, Snapshot, oneshot::Sender<Result<Submitted, Error>>),
    Fetch(SessionId, ArtifactId, oneshot::Sender<Result<ArtifactContent, Error>>),
    Replay(SessionId),
}

struct Contents<T> {
    store: T,
    refs: HashMap<ArtifactId, usize>,
}

struct Clients(ConnectionTo<Conductor>);

impl<T: ArtifactStore> Artifacts<T> {
    pub(crate) fn new(cx: ConnectionTo<Conductor>, store: T) -> Self {
        Self { sessions: HashMap::new(), contents: Contents { store, refs: HashMap::new() }, clients: Clients(cx) }
    }

    pub(crate) async fn run(mut self, ArtifactsInbox(mut inbox): ArtifactsInbox) -> Result<(), Error> {
        while let Some(op) = inbox.recv().await {
            match op {
                Operation::Submit(session, snapshot, reply) => {
                    let _ = reply.send(self.submit(&session, snapshot).await);
                }
                Operation::Fetch(session, id, reply) => {
                    let _ = reply.send(self.fetch(&session, &id).await.map_err(Error::from));
                }
                Operation::Replay(session) => self.replay(&session)?,
            }
        }
        Ok(())
    }

    async fn submit(&mut self, session: &SessionId, snapshot: Snapshot) -> Result<Submitted, Error> {
        let Snapshot { file, title, content } = snapshot;
        let id = self.contents.hold(content).await?;
        let artifact = match self.sessions.entry(session.clone()).or_default().record(file, title, id) {
            Recorded::Unchanged(current) => {
                let version = current.version;
                self.contents.release(id).await?;
                return Ok(Submitted { version });
            }
            Recorded::Added(current) => current.artifact(),
            Recorded::Replaced(current, replaced) => {
                let artifact = current.artifact();
                self.contents.release(replaced).await?;
                artifact
            }
        };
        self.clients.announce(session, &artifact)?;
        Ok(Submitted { version: artifact.version })
    }

    async fn fetch(&self, session: &SessionId, id: &ArtifactId) -> Result<ArtifactContent, ArtifactError> {
        if !self.sessions.get(session).is_some_and(|shown| shown.shows(id)) {
            return Err(ArtifactError::NotShown);
        }
        Ok(self.contents.store.get(id).await?)
    }

    fn replay(&self, session: &SessionId) -> Result<(), Error> {
        let Some(shown) = self.sessions.get(session) else { return Ok(()) };
        shown.iter().try_for_each(|current| self.clients.announce(session, &current.artifact()))
    }
}

impl ArtifactsHandle {
    pub(crate) fn new() -> (Self, ArtifactsInbox) {
        let (ops, inbox) = mpsc::unbounded_channel();
        (Self(ops), ArtifactsInbox(inbox))
    }

    pub(crate) async fn submit(&self, session: SessionId, snapshot: Snapshot) -> Result<Submitted, Error> {
        let (reply, submitted) = oneshot::channel();
        self.0.send(Operation::Submit(session, snapshot, reply)).map_err(Error::into_internal_error)?;
        submitted.await.map_err(Error::into_internal_error)?
    }

    pub(crate) async fn fetch(&self, session: SessionId, id: ArtifactId) -> Result<ArtifactContent, Error> {
        let (reply, fetched) = oneshot::channel();
        self.0.send(Operation::Fetch(session, id, reply)).map_err(Error::into_internal_error)?;
        fetched.await.map_err(Error::into_internal_error)?
    }

    pub(crate) fn replay(&self, session: SessionId) -> Result<(), Error> {
        self.0.send(Operation::Replay(session)).map_err(Error::into_internal_error)
    }
}

impl<T: ArtifactStore> Contents<T> {
    async fn hold(&mut self, content: ArtifactContent) -> Result<ArtifactId, ArtifactError> {
        let id = self.store.put(content).await?;
        *self.refs.entry(id).or_default() += 1;
        Ok(id)
    }

    async fn release(&mut self, id: ArtifactId) -> Result<(), ArtifactError> {
        let Some(count) = self.refs.get_mut(&id) else { return Ok(()) };
        *count -= 1;
        if *count == 0 {
            self.refs.remove(&id);
            self.store.remove(&id).await?;
        }
        Ok(())
    }
}

impl Clients {
    fn announce(&self, session: &SessionId, artifact: &Artifact) -> Result<(), Error> {
        self.0.send_notification_to(Client, ArtifactPayload { session_id: session.clone(), artifact: artifact.clone() })
    }
}
