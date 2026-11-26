//! Foxfire's own shell for the shared conversation views: its mark, words, and layout.

pub mod conversation;
pub mod hosted;
pub mod relay;
pub mod ui;
pub mod workspace;

use foxfire_ui::ui::ActivityMark;

/// What a thread without a title is called.
pub const UNTITLED: &str = "New thread";

/// Draws Foxfire's mark wherever the shared views show an agent's activity.
pub fn install() {
    ActivityMark::provide(ui::foxfire);
}
