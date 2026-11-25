#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(not(target_arch = "wasm32"))]
mod native;

#[cfg(target_arch = "wasm32")]
use browser::is_retryable;
#[cfg(target_arch = "wasm32")]
pub use browser::{HandshakeError, WebSocketTransport, connect};
#[cfg(not(target_arch = "wasm32"))]
use native::is_retryable;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{HandshakeError, WebSocketError, WebSocketTransport, connect};

use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use thiserror::Error;

/// Where an agent's WebSocket is, and the credentials it asks for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Endpoint {
    pub url: String,
    /// Sent by native clients, as a JSON object when deserialized.
    #[serde(default, deserialize_with = "header_pairs")]
    pub headers: Vec<(String, String)>,
    /// Offered by browsers, which cannot set headers. Native clients leave them out, since a server that selects none
    /// of the protocols it is offered fails the handshake.
    #[serde(default)]
    pub subprotocols: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ConnectError {
    #[error("invalid WebSocket URL `{url}`")]
    InvalidUrl { url: String },
    #[error("invalid WebSocket header `{name}`")]
    InvalidHeader { name: String },
    #[error("another client is already attached to the agent")]
    Occupied,
    #[error("failed to connect to the agent's WebSocket: {0}")]
    Handshake(#[source] Box<HandshakeError>),
}

impl Endpoint {
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into(), headers: Vec::new(), subprotocols: Vec::new() }
    }
}

impl ConnectError {
    /// Whether a later attempt may succeed.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::InvalidUrl { .. } | Self::InvalidHeader { .. } => false,
            Self::Occupied => true,
            Self::Handshake(error) => is_retryable(error),
        }
    }
}

fn header_pairs<'de, T: Deserializer<'de>>(deserializer: T) -> Result<Vec<(String, String)>, T::Error> {
    Ok(BTreeMap::deserialize(deserializer)?.into_iter().collect())
}
