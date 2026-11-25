use super::{ConnectError, Endpoint};
use crate::client::platform::spawn;
use agent_client_protocol::{ConnectTo, Error, Lines, Role};
use futures::channel::{mpsc, oneshot};
use futures::future::{Either, poll_fn, select};
use futures::{SinkExt, StreamExt};
use gloo_net::websocket::futures::WebSocket;
use gloo_net::websocket::{Message, State, WebSocketError};
use std::io;
use std::pin::pin;

/// What a browser says about a handshake that failed, which is never why.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct HandshakeError(String);

/// WebSocket transport for ACP. A local task owns the socket, because it cannot leave the thread it was opened on.
pub struct WebSocketTransport {
    outgoing: mpsc::UnboundedSender<String>,
    incoming: mpsc::UnboundedReceiver<io::Result<String>>,
}

pub async fn connect(endpoint: &Endpoint) -> Result<WebSocketTransport, ConnectError> {
    let (opened_tx, opened) = oneshot::channel();
    let (outgoing, to_socket) = mpsc::unbounded();
    let (from_socket, incoming) = mpsc::unbounded();
    spawn(pump(endpoint.clone(), opened_tx, to_socket, from_socket));
    opened.await.unwrap_or_else(|_| Err(refused("the socket was closed")))?;
    Ok(WebSocketTransport { outgoing, incoming })
}

pub(super) fn is_retryable(_: &HandshakeError) -> bool {
    true
}

impl<T: Role> ConnectTo<T> for WebSocketTransport {
    async fn connect_to(self, peer: impl ConnectTo<T::Counterpart>) -> Result<(), Error> {
        let output = self.outgoing.sink_map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe));
        ConnectTo::<T>::connect_to(Lines::new(output, self.incoming), peer).await
    }
}

async fn pump(
    endpoint: Endpoint,
    opened: oneshot::Sender<Result<(), ConnectError>>,
    mut to_socket: mpsc::UnboundedReceiver<String>,
    from_socket: mpsc::UnboundedSender<io::Result<String>>,
) {
    let protocols: Vec<&str> = endpoint.subprotocols.iter().map(String::as_str).collect();
    let socket = if protocols.is_empty() {
        WebSocket::open(&endpoint.url)
    } else {
        WebSocket::open_with_protocols(&endpoint.url, &protocols)
    };
    let Ok(mut socket) = socket else {
        let _ = opened.send(Err(ConnectError::InvalidUrl { url: endpoint.url }));
        return;
    };
    let ready = poll_fn(|cx| socket.poll_ready_unpin(cx)).await.is_ok()
        && matches!(socket.state(), State::Open)
        && (protocols.is_empty() || protocols.contains(&socket.protocol().as_str()));
    let outcome = if ready { Ok(()) } else { Err(refused("the handshake was refused")) };
    if opened.send(outcome).is_err() || !ready {
        return;
    }
    loop {
        let next = match select(pin!(to_socket.next()), socket.next()).await {
            Either::Left((message, _)) => Either::Left(message),
            Either::Right((message, _)) => Either::Right(message),
        };
        match next {
            Either::Left(Some(text)) => {
                if socket.send(Message::Text(text)).await.is_err() {
                    return;
                }
            }
            Either::Left(None) => {
                let _ = socket.close(None, None);
                return;
            }
            Either::Right(Some(Ok(Message::Text(text)))) => {
                if from_socket.unbounded_send(Ok(text)).is_err() {
                    let _ = socket.close(None, None);
                    return;
                }
            }
            Either::Right(Some(Ok(Message::Bytes(_)))) => {
                let _ = from_socket.unbounded_send(Err(io::Error::other("binary messages are not supported")));
                let _ = socket.close(None, None);
                return;
            }
            Either::Right(Some(Err(WebSocketError::ConnectionClose(_))) | None) => return,
            Either::Right(Some(Err(error))) => {
                let _ = from_socket.unbounded_send(Err(io::Error::other(error.to_string())));
                return;
            }
        }
    }
}

fn refused(reason: &str) -> ConnectError {
    ConnectError::Handshake(Box::new(HandshakeError(reason.to_owned())))
}
