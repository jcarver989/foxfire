//! Browser host for the shared Foxfire interface.

#[cfg(target_arch = "wasm32")]
mod browser;

#[cfg(target_arch = "wasm32")]
pub use browser::connect;
pub use foxfire_shell::hosted::hosted_view as web_view;
pub use foxfire_shell::relay::relay_view;
