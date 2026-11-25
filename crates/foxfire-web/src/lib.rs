//! Browser host for the shared Foxfire interface.

#[cfg(target_arch = "wasm32")]
mod browser;
mod relay;

#[cfg(target_arch = "wasm32")]
pub use browser::{connect, mount};
pub use foxfire_shell::hosted::hosted_view as web_view;
pub use relay::relay_view;
