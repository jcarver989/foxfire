#![doc = include_str!("../README.md")]

#[cfg(feature = "protocol")]
pub mod aether;
pub mod artifact_review;
#[cfg(feature = "protocol")]
pub mod chat;
pub mod config_option_id;
#[cfg(feature = "protocol")]
pub mod content;
#[cfg(feature = "client")]
pub mod conversation;
pub mod display_meta;
pub mod mcp_status;
mod meta;
pub mod reasoning;

#[cfg(feature = "websocket")]
pub mod websocket;

#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "testing")]
pub mod testing;

pub use reasoning::ReasoningEffort;
