#![doc = include_str!("../README.md")]

pub mod aether;
pub mod config_option_id;
pub mod content;
#[cfg(feature = "client")]
pub mod conversation;
#[cfg(feature = "server")]
pub mod elicitation;

#[cfg(feature = "websocket")]
pub mod websocket;

#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "testing")]
pub mod testing;
