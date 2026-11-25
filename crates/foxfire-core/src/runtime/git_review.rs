use super::agent::Requests;
use super::host;
use crate::command::{CommandResult, GitReviewCommand, Reply, ReviewSource};
use agent_client_protocol::v2::schema::SessionId;
#[cfg(not(target_arch = "wasm32"))]
use clankerdiff_client::ConnectionHeader;
use clankerdiff_client::protocol::client::ClientCommand;
use clankerdiff_client::protocol::server::ServerMessage;
use clankerdiff_client::{
    ClientError, ClientMessageTransport, ClientOptions, ClientState, ClientSubscription, ConnectionState, DiffClient,
    DiffReviewEvent, LookupReply, MaybeSend,
};
#[cfg(not(target_arch = "wasm32"))]
use clankerdiff_remote::Invocation;
use foxfire_acp::aether::{GitDiffClosePayload, GitDiffCommandPayload};
use foxfire_acp::client::AcpClientHandle;
#[cfg(not(target_arch = "wasm32"))]
use std::any::Any;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;

/// A host-configured review source that takes precedence over the session's default.
#[derive(Debug, Clone)]
pub enum ReviewOverride {
    #[cfg(not(target_arch = "wasm32"))]
    Stdio(Invocation),
    #[cfg(not(target_arch = "wasm32"))]
    WebSocket {
        url: String,
        headers: Vec<ConnectionHeader>,
    },
    Unavailable(String),
}

pub(super) struct GitReviewRuntime {
    handle: AcpClientHandle,
    review: Review,
    source_override: Option<ReviewOverride>,
}

enum Review {
    Closed,
    Opening(Opening),
    Open(Box<ReviewConnection>),
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) type Opening = Pin<Box<dyn Future<Output = Result<ReviewConnection, ClientError>> + Send>>;
#[cfg(target_arch = "wasm32")]
pub(super) type Opening = Pin<Box<dyn Future<Output = Result<ReviewConnection, ClientError>>>>;

pub(super) struct ReviewConnection {
    client: DiffClient,
    subscription: ClientSubscription,
    server: ReviewServer,
}

pub(super) enum ReviewServer {
    Agent {
        session_id: SessionId,
        messages: mpsc::UnboundedSender<ServerMessage>,
    },
    /// Something the host opened, which lasts as long as the review does.
    #[cfg(not(target_arch = "wasm32"))]
    Host {
        _opened: Box<dyn Any + Send>,
    },
}

impl GitReviewRuntime {
    pub(super) fn new(handle: AcpClientHandle) -> Self {
        Self { handle, review: Review::Closed, source_override: None }
    }

    pub(super) fn set_review_override(&mut self, source: Option<ReviewOverride>) {
        self.source_override = source;
    }

    pub(super) fn dispatch(&mut self, command: GitReviewCommand, requests: &mut Requests) -> Option<CommandResult> {
        match command {
            GitReviewCommand::Open(source) => self.open(source),
            GitReviewCommand::Event(event) => self.on_event(event, requests),
            GitReviewCommand::Close => {
                self.close();
                None
            }
        }
    }

    pub(super) async fn changed(&mut self) -> Option<Arc<ClientState>> {
        loop {
            match &mut self.review {
                Review::Closed => return std::future::pending().await,
                Review::Opening(opening) => match opening.as_mut().await {
                    Ok(connection) => self.review = Review::Open(Box::new(connection)),
                    Err(error) => {
                        self.review = Review::Closed;
                        return Some(failed(error));
                    }
                },
                Review::Open(connection) => return connection.subscription.changed().await.ok(),
            }
        }
    }

    pub(super) fn close(&mut self) {
        if let Review::Open(connection) = std::mem::replace(&mut self.review, Review::Closed)
            && let ReviewServer::Agent { session_id, .. } = connection.server
        {
            let _ = self.handle.notify(GitDiffClosePayload { session_id: session_id.to_string() });
        }
    }

    pub(super) fn forward(&mut self, session_id: &SessionId, message: ServerMessage) {
        if let Review::Open(connection) = &self.review
            && let ReviewServer::Agent { session_id: reviewed, messages } = &connection.server
            && reviewed == session_id
        {
            let _ = messages.send(message);
        }
    }

    fn open(&mut self, source: ReviewSource) -> Option<CommandResult> {
        self.close();
        self.review = match (self.source_override.clone(), source) {
            (Some(ReviewOverride::Unavailable(reason)), _) | (None, ReviewSource::Unavailable(reason)) => {
                return Some(CommandResult::GitReview(failed(ClientError::Transport(reason))));
            }
            (None, ReviewSource::Agent { session_id }) => {
                let (messages, receiver) = mpsc::unbounded_channel();
                let transport = AcpClientTransport {
                    handle: self.handle.clone(),
                    session_id: session_id.clone(),
                    messages: receiver,
                };
                let client = DiffClient::spawn(transport, ClientOptions::default());
                Review::Open(Box::new(ReviewConnection::new(client, ReviewServer::Agent { session_id, messages })))
            }
            (None, ReviewSource::Local(path)) => Review::Opening(host::open_local_review(path)),
            #[cfg(not(target_arch = "wasm32"))]
            (Some(source), _) => Review::Opening(host::open_review(source)),
        };
        None
    }

    fn on_event(&mut self, event: DiffReviewEvent, requests: &mut Requests) -> Option<CommandResult> {
        match (&self.review, event) {
            (Review::Open(connection), DiffReviewEvent::Lookup(lookup)) => {
                let client = connection.client.clone();
                requests.push(Box::pin(async move {
                    CommandResult::Reply(Reply::GitReviewLookup(client.lookup(lookup).await))
                }));
                None
            }
            (Review::Open(connection), event) => match connection.client.dispatch(event) {
                Ok(Some(reply)) => {
                    requests.push(Box::pin(async move {
                        let result = reply.await.unwrap_or(Err(ClientError::Disconnected));
                        CommandResult::Reply(Reply::GitReviewAction(result))
                    }));
                    None
                }
                Ok(None) => None,
                Err(error) => Some(CommandResult::Reply(Reply::GitReviewAction(Err(error)))),
            },
            (_, DiffReviewEvent::Lookup(lookup)) => {
                let reply = LookupReply::new(lookup, Err(ClientError::Disconnected.to_string()));
                Some(CommandResult::Reply(Reply::GitReviewLookup(reply)))
            }
            (_, _) => Some(CommandResult::Reply(Reply::GitReviewAction(Err(ClientError::Disconnected)))),
        }
    }
}

impl ReviewConnection {
    pub(super) fn new(client: DiffClient, server: ReviewServer) -> Self {
        Self { subscription: client.subscribe(), client, server }
    }
}

struct AcpClientTransport {
    handle: AcpClientHandle,
    session_id: SessionId,
    messages: mpsc::UnboundedReceiver<ServerMessage>,
}

impl ClientMessageTransport for AcpClientTransport {
    fn send(&mut self, command: ClientCommand) -> impl Future<Output = Result<(), ClientError>> + MaybeSend {
        let result = self
            .handle
            .notify(GitDiffCommandPayload { session_id: self.session_id.to_string(), command })
            .map_err(|_| ClientError::Disconnected);
        std::future::ready(result)
    }

    async fn recv(&mut self) -> Result<ServerMessage, ClientError> {
        self.messages.recv().await.ok_or(ClientError::Disconnected)
    }

    async fn close(&mut self) {}
}

fn failed(error: ClientError) -> Arc<ClientState> {
    Arc::new(ClientState { connection: ConnectionState::Failed(error), ..ClientState::default() })
}
