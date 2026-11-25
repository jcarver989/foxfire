mod cells;
mod components;
mod foxfire;
mod kindling;
mod settings;
mod shell;
pub mod tokens;

pub use cells::cells;
pub use components::{
    StyledExt, chord, dispatch, hint, icon_slot, picker_closed, popping, presence, scroll_edge, selection,
    thread_header,
};
pub use foxfire::{FoxfireMotion, FoxfireState, foxfire};
#[cfg(any(test, feature = "testing"))]
pub use kindling::KINDLED_AFTER;
pub use kindling::kindling;
pub use settings::{SettingsField, SettingsModel, settings, settings_button};
pub use shell::{
    MIN_MAIN_WIDTH, PanelWidth, TOGGLE_SIDEBAR_KEYS, ThreadShell, ToggleSidebar, cloud_section, sidebar_group,
    sidebar_thread_row, thread_status_icon,
};

pub use clankerdiff_rouge::theme::{
    ActiveReviewTheme, ThemePickerEvent, theme_picker, theme_picker_overlay, theme_picker_view,
};
pub use clankerdiff_ui_core::ThemePreference;
pub use rouge::core::{Theme, appearance};
pub use rouge_widgets::*;

use rouge::reactive::{Scope, Task, spawn};
use std::future::Future;

pub(crate) fn spawn_in<T: 'static>(scope: Scope, future: impl Future<Output = T> + 'static) -> Task<T> {
    scope.run(|| spawn(future))
}
