use crate::platform::ChatPlatform;
use crate::thread::{Thread, ThreadStatus};
use crate::ui::tokens::{BAR_HEIGHT, CONTROL_HEIGHT_LARGE, ICON_SIZE_SMALL, ROW_HEIGHT, Space, UNIT};
use crate::ui::{FoxfireState, UiStyles, column_grid, foxfire, kindling, popping, reading_cell, scroll_edge};
use crate::{Composer, ElicitationCard, SessionOptions, StatusLine, Transcript, reconnect_banner};
use agent_client_protocol::v2::schema::ToolCallLocation;
use rouge::core::{Div, Motion, Rect, Role, Styled, TextAlign, View, auto, div, presentation, px, show, text, view};
use rouge::reactive::{Memo, Signal, memo, signal};
use std::rc::Rc;

/// The conversation surface shared by the desktop and hosted shells.
#[derive(Clone, Copy)]
pub struct ChatPane {
    thread: Thread,
    stage: Signal<Rect>,
    dock: Signal<Rect>,
    composer_bounds: Signal<Rect>,
    transcript: Transcript,
    composer: Composer,
    options: SessionOptions,
    status: StatusLine,
    elicitation: ElicitationCard,
}

impl ChatPane {
    /// Creates a pane with host callbacks for workspace locations and artifact review.
    pub fn new(
        thread: Thread,
        platform: Rc<dyn ChatPlatform>,
        open_location: impl Fn(ToolCallLocation) + 'static,
        open_review: impl Fn() + 'static,
    ) -> Self {
        let (dock, stage) = (signal(Rect::ZERO), signal(Rect::ZERO));
        let composer = Composer::new(thread, platform.clone());
        let transcript = Transcript::new(thread, platform.clone(), dock, stage, open_location, move |typed| {
            composer.type_text(&typed);
        });
        Self {
            thread,
            stage,
            dock,
            composer_bounds: signal(Rect::ZERO),
            transcript,
            options: SessionOptions::new(thread),
            status: StatusLine::new(thread),
            elicitation: ElicitationCard::new(thread, platform, open_review),
            composer,
        }
    }

    pub fn stage(self) -> Signal<Rect> {
        self.stage
    }

    pub fn gutter(self) -> Memo<f32> {
        self.transcript.gutter()
    }

    pub fn composer(self) -> Composer {
        self.composer
    }

    pub fn transcript(self) -> impl View {
        self.transcript.view()
    }

    pub(crate) fn transcript_below_header(self) -> impl View {
        self.transcript.below_header().view()
    }

    /// Renders the shared composer dock, with optional host-specific controls.
    pub fn dock(self, reveal: Motion, actions: impl View, tray: impl View) -> Div {
        let thread = self.thread;
        let unreachable = memo(move || {
            let status = thread.status();
            let has_items = thread.with_core(|core| !core.conversation().items().is_empty()).unwrap_or(false);
            (status.problem().is_some() && has_items).then_some(status)
        });
        let fresh = memo(move || thread.is_fresh());
        let connecting = memo(move || fresh.get() && thread.status() == ThreadStatus::Connecting);
        let spacer = move || view! { <show when={move || fresh.get()}><div flex_1/></show> };
        let (reading, gutter) = (self.transcript.reading(), self.transcript.gutter());
        let controls_height = if presentation().get_untracked().is_touch() { CONTROL_HEIGHT_LARGE } else { ROW_HEIGHT };
        view! {
            <div absolute left=0.0 right={move || px(gutter.get())} bottom=0.0
                 top={move || if fresh.get() { px(0.0) } else { auto() }}
                 pb=Space::X3 flex col pass_through track_bounds={self.dock}>
                <show when={move || !fresh.get()}>{scroll_edge(false, BAR_HEIGHT, BAR_HEIGHT)}</show>
                {spacer()}
                <column_grid flex_none pass_through>
                    <reading_cell span=reading flex col>
                        <show when={move || fresh.get()}>
                            <div test_id="new-thread-heading" flex justify_center mb=HEADING_GAP>
                                <text display align=TextAlign::Center>"What do you want to build?"</text>
                            </div>
                        </show>
                        {popping(unreachable, move |status| reconnect_banner(thread, status).map(|banner| banner.mt(Space::X2)))}
                        {self.elicitation.view()}
                        <div track_bounds={self.composer_bounds} opacity=reveal>{self.composer.view()}</div>
                        <div flex col opacity=reveal>
                            <div test_id="composer-bar" h=controls_height mt=Space::X1 px=Space::X2 flex items_center gap=Space::X4>
                                {self.options.view()}
                                {self.status.view()}
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

    pub fn kindling(self, reveal: Motion) -> Div {
        let (stage, composer) = (self.stage, self.composer_bounds);
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
}

fn connecting_message() -> impl View {
    let muted = Role::TextMuted;
    view! {
        <div test_id="new-thread-connecting" flex_none flex items_center gap=Space::X2>
            {foxfire(ICON_SIZE_SMALL, muted, || FoxfireState::Connecting)}
            <text body muted>"Connecting…"</text>
        </div>
    }
}

/// Leaves room for the kindling effect above the composer.
const HEADING_GAP: f32 = 14.0 * UNIT;
const COMPOSER_MARGIN: f32 = Space::X2.px();
