pub use rouge_widgets::tokens::*;

pub const UNREAD_DOT_SIZE: f32 = 2.0 * UNIT;

pub const TWO_LINE_ROW_HEIGHT: f32 = 11.0 * UNIT;

pub const CONTENT_MAX_WIDTH: f32 = 720.0;

pub const COMPOSER_POPUP_HEIGHT: f32 = 288.0;

pub const PANEL_INSET: f32 = Space::X2.px();

/// Distance from a panel's edge to its content, so bar buttons, section actions and row labels line up.
pub const GUTTER: f32 = Space::X4.px();

/// Height of the band across the top of the window; its controls are centred so they sit `PANEL_INSET` from both edges.
pub const HEADER_HEIGHT: f32 = BAR_HEIGHT + PANEL_INSET;
