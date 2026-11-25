mod activity;
mod cells;
mod components;
mod person_avatar;
mod side_panel;
mod themes;
pub mod tokens;
mod wording;

pub use activity::{Activity, ActivityMark, ActivityMotion, ActivityState, activity, may_animate};
pub use cells::cells;
pub use components::{
    StyledExt, chord, dispatch, hint, icon_slot, picker_closed, popping, presence, scroll_edge, selection,
    thread_header,
};
pub use person_avatar::{format_names, people_avatars, person_avatar, person_color};
pub use side_panel::{MIN_MAIN_WIDTH, PanelWidth, beside_room};
pub use themes::ThemeSet;
pub use wording::Wording;

pub use clankerdiff_rouge::theme::{
    ActiveReviewTheme, ThemePickerEvent, theme_picker, theme_picker_overlay, theme_picker_view,
};
pub use clankerdiff_ui_core::{ThemeChoice, ThemePair, ThemePreference};
pub use rouge::core::{Theme, appearance};
pub use rouge_widgets::*;

use rouge::reactive::{Scope, Task, spawn};
use std::future::Future;

pub(crate) fn spawn_in<T: 'static>(scope: Scope, future: impl Future<Output = T> + 'static) -> Task<T> {
    scope.run(|| spawn(future))
}
