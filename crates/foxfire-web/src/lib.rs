//! Browser host for the shared Foxfire interface.

mod app;
#[cfg(target_arch = "wasm32")]
mod browser;

use app::WebApp;
#[cfg(target_arch = "wasm32")]
pub use browser::mount;
use foxfire_ui::backend::HostedBackend;
use foxfire_ui::platform::ChatPlatform;
use rouge::core::View;
use std::rc::Rc;

/// Builds the browser shell for a supplied backend. Call inside a Rouge window.
pub fn web_view(backend: Rc<dyn HostedBackend>, platform: Rc<dyn ChatPlatform>) -> impl View {
    WebApp::new(backend, platform).view()
}
