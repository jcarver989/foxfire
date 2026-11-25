//! How Foxfire arranges the parts of a conversation.

use crate::ui::kindling;
use foxfire_ui::ChatPane;
use foxfire_ui::thread::ThreadStatus;
use foxfire_ui::ui::tokens::{BAR_HEIGHT, CONTROL_HEIGHT_LARGE, ICON_SIZE_SMALL, ROW_HEIGHT, Space, UNIT};
use foxfire_ui::ui::{ActivityState, UiStyles, Wording, activity, column_grid, reading_cell, scroll_edge};
use rouge::core::{Div, Motion, Rect, Role, Styled, TextAlign, View, auto, div, presentation, px, show, text, view};
use rouge::reactive::memo;

/// The composer and what surrounds it, with optional host-specific controls. A fresh thread centers it under a
/// heading.
pub fn dock(pane: &ChatPane, reveal: Motion, actions: impl View, tray: impl View) -> Div {
    let thread = pane.thread();
    let fresh = memo(move || thread.is_fresh());
    let connecting = memo(move || fresh.get() && thread.status() == ThreadStatus::Connecting);
    let spacer = move || view! { <show when={move || fresh.get()}><div flex_1/></show> };
    let (reading, gutter) = (pane.reading(), pane.gutter());
    let controls_height = if presentation().get_untracked().is_touch() { CONTROL_HEIGHT_LARGE } else { ROW_HEIGHT };
    view! {
        <div absolute left=0.0 right={move || px(gutter.get())} bottom=0.0
             top={move || if fresh.get() { px(0.0) } else { auto() }}
             pb=Space::X3 flex col pass_through track_bounds={pane.dock()}>
            <show when={move || !fresh.get()}>{scroll_edge(false, BAR_HEIGHT, BAR_HEIGHT)}</show>
            {spacer()}
            <column_grid flex_none pass_through>
                <reading_cell span=reading flex col>
                    <show when={move || fresh.get()}>
                        <div test_id="new-thread-heading" flex justify_center mb=HEADING_GAP>
                            <text display align=TextAlign::Center>"What do you want to build?"</text>
                        </div>
                    </show>
                    {pane.reconnect_banner()}
                    {pane.elicitation()}
                    <div track_bounds={pane.composer_bounds()} opacity=reveal>{pane.composer().view()}</div>
                    <div flex col opacity=reveal>
                        <div test_id="composer-bar" min_h=controls_height mt=Space::X1 px=Space::X1
                             flex flex_wrap items_center gap=Space::X1>
                            {pane.options()}
                            {pane.status()}
                            <show when={move || connecting.get()}>{connecting_message()}</show>
                            {actions}
                        </div>
                        {tray}
                    </div>
                </reading_cell>
            </column_grid>
            {spacer()}
        </div>
    }
}

/// The fire a fresh thread kindles behind its composer.
pub fn fire(pane: &ChatPane, reveal: Motion) -> Div {
    let (stage, composer) = (pane.stage(), pane.composer_bounds());
    kindling(
        move || {
            let (stage, composer) = (stage.get(), composer.get());
            Rect::new(
                composer.origin.x - stage.origin.x,
                composer.origin.y - stage.origin.y + COMPOSER_MARGIN,
                composer.size.width,
                (composer.size.height - COMPOSER_MARGIN).max(0.0),
            )
        },
        reveal,
    )
}

fn connecting_message() -> impl View {
    let muted = Role::TextMuted;
    view! {
        <div test_id="new-thread-connecting" flex_none px=Space::X2 flex items_center gap=Space::X2>
            {activity(ICON_SIZE_SMALL, muted, || ActivityState::Connecting)}
            <text body muted>{Wording::current().connecting}</text>
        </div>
    }
}

/// Leaves room for the kindling effect above the composer.
const HEADING_GAP: f32 = 14.0 * UNIT;
const COMPOSER_MARGIN: f32 = Space::X2.px();
