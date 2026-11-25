pub mod agents;
mod app;
pub mod branding;
mod chat;
pub mod cli;
pub mod clock;
mod file_picker;
pub mod history;
mod menus;
pub mod platform;
pub mod registry;
pub mod remotes;
pub mod sessions;
mod settings;
pub mod sidebar;
pub mod target;
mod tasks;
mod temp_dir;
#[cfg(feature = "testing")]
pub mod testing;
mod thread_view;
pub mod threads;
pub mod ui;
pub mod variables;

pub use app::{
    DesktopApp, DesktopConfig, NewCloudThread, NewThread, OpenFile, OpenSettings, OpenThreadSwitcher, SelectNextThread,
    SelectPreviousThread, ShowChat, ShowFiles, ToggleChatRoom, ToggleSidebar,
};
pub use clankerdiff_ui_core::{AppearanceMode, ThemePreference};

// macOS's system allocator corrupts its own bookkeeping when the UI thread follows a new review
// document while short-lived blocking-pool threads prepare syntax, aborting with "pointer being
// freed was not allocated". Declared in the library so the desktop tests run on it too.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
pub use ui::ActiveReviewTheme;

use agents::{Agents, default_agents_file};
use clankerdiff_remote::{CommandRemotes, Remotes};
use cli::Cli;
use clock::SystemClock;
use platform::SystemPlatform;
use registry::{State, WindowSize, default_state_dir, saved_in};
use rouge::persist::{FileStorage, MemoryStorage, Storage};
use rouge::{App, WindowOptions};
use rouge_widgets::bundled_fonts;
use rustls::crypto::aws_lc_rs;
use sessions::AcpSessions;
use std::rc::Rc;
use std::sync::Arc;
use target::{AgentConnector, ProcessTransport};
use tokio::runtime::Builder;

pub fn run(cli: Cli) {
    aws_lc_rs::default_provider().install_default().expect("failed to install the Rustls AWS-LC crypto provider");
    let runtime = Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("failed to start the desktop Tokio runtime");
    let (storage, saved): (Rc<dyn Storage>, State) = if let Some(dir) = default_state_dir() {
        let saved = saved_in(&dir);
        (Rc::new(FileStorage::new(dir)), saved)
    } else {
        tracing::warn!("no home directory, so nothing is remembered between runs");
        (Rc::new(MemoryStorage::default()), State::default())
    };
    let options = window_options(&saved);
    let remotes: Arc<dyn Remotes> = Arc::new(CommandRemotes);
    let connector = Arc::new(AgentConnector::new(remotes.clone(), Arc::new(ProcessTransport)));
    let config = DesktopConfig {
        sessions: Arc::new(AcpSessions::new(connector.clone())),
        connector,
        clock: Arc::new(SystemClock),
        agents: Agents::new(cli.agent, default_agents_file()),
        remotes,
        remotes_file: remotes::default_file(),
        platform: Rc::new(SystemPlatform),
        storage,
        saved,
    };
    let app = App::new()
        .icon(include_bytes!("../../../assets/app-icon-desktop.png").as_slice())
        .fonts(bundled_fonts())
        .tokio(runtime.handle().clone())
        .menus(menus::all())
        .window(options, move || DesktopApp::new(config).view());
    if let Err(error) = app.run() {
        eprintln!("{}: {error}", branding::APP_NAME);
    }
    runtime.shutdown_background();
}

pub const DEFAULT_WINDOW_SIZE: WindowSize = WindowSize { width: 1280.0, height: 840.0 };
pub const MIN_WINDOW_SIZE: WindowSize = WindowSize { width: 640.0, height: 480.0 };

#[must_use]
pub fn window_size(state: &State) -> WindowSize {
    state.window_size.map_or(DEFAULT_WINDOW_SIZE, |size| size.at_least(MIN_WINDOW_SIZE))
}

fn window_options(state: &State) -> WindowOptions {
    let size = window_size(state);
    WindowOptions::default()
        .title(branding::APP_NAME)
        .size(size.width, size.height)
        .min_size(MIN_WINDOW_SIZE.width, MIN_WINDOW_SIZE.height)
}
