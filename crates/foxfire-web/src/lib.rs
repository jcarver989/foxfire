//! Browser host for the shared Foxfire interface.

#[cfg(target_arch = "wasm32")]
mod browser;

#[cfg(target_arch = "wasm32")]
pub use browser::mount;
pub use foxfire_ui::hosted::hosted_view as web_view;
