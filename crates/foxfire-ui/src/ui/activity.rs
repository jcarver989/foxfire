use super::tokens::{Pace, Radius};
use rouge::core::{AnyView, Div, Fill, IntoProp, PaintRegion, Prop, Styled, View, div, view, window_focused};
use rouge::reactive::{Memo, Signal, expect_context, memo, provide_context, signal, stored, use_context};
use std::rc::Rc;

/// What an agent is doing, for a host's mark to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActivityState {
    Connecting,
    Thinking,
    Compacting,
    Working,
    Writing,
    Waiting,
}

/// A shared, reactive preference for activity indicators. Reduced motion removes their clocks.
#[derive(Clone, Copy)]
pub struct ActivityMotion(Signal<bool>);

/// Draws a host's mark wherever an agent's activity is shown.
#[derive(Clone)]
pub struct ActivityMark(Rc<dyn Fn(Activity) -> AnyView>);

/// One place an agent's activity is shown.
pub struct Activity {
    pub size: f32,
    pub tint: Rc<dyn Fn() -> Fill>,
    pub state: Memo<ActivityState>,
}

/// An agent's activity, drawn with the host's mark. Pair it with a label rather than relying on its color or motion.
pub fn activity<T>(size: f32, tint: impl IntoProp<Fill, T>, state: impl Fn() -> ActivityState + 'static) -> Div {
    let tint = stored(tint.into_prop());
    let activity = Activity { size, tint: Rc::new(move || tint.with(Prop::get)), state: memo(state) };
    let mark = match use_context::<ActivityMark>() {
        Some(ActivityMark(draw)) => draw(activity),
        None => dot(activity),
    };
    view! { <div flex_none relative size=size pass_through>{mark}</div> }
}

/// Whether something that moves should: its window has focus, it is in view and motion is not reduced.
pub fn may_animate(region: Signal<PaintRegion>) -> Memo<bool> {
    let focused = window_focused();
    let motion = use_context::<ActivityMotion>();
    memo(move || {
        focused.get() && !motion.is_some_and(ActivityMotion::reduced) && region.with(|region| !region.bounds.is_empty())
    })
}

impl ActivityState {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Connecting => "Connecting",
            Self::Thinking => "Thinking",
            Self::Compacting => "Compacting the conversation",
            Self::Working => "Working",
            Self::Writing => "Writing",
            Self::Waiting => "Waiting for you",
        }
    }
}

impl ActivityMotion {
    #[must_use]
    pub fn install(reduced: bool) -> Self {
        let motion = Self(signal(reduced));
        provide_context(motion);
        motion
    }

    #[must_use]
    pub fn current() -> Self {
        expect_context()
    }

    pub fn set_reduced(self, reduced: bool) {
        self.0.set_if_changed(reduced);
    }

    #[must_use]
    pub fn reduced(self) -> bool {
        self.0.get()
    }
}

impl ActivityMark {
    /// Draws every activity in this scope with `mark`.
    pub fn provide<T: View>(mark: impl Fn(Activity) -> T + 'static) {
        provide_context(Self(Rc::new(move |activity| mark(activity).into_any())));
    }
}

/// What a host without a mark shows.
fn dot(activity: Activity) -> AnyView {
    let Activity { size, tint, state } = activity;
    let region = signal(PaintRegion::default());
    let allowed = may_animate(region);
    let active = memo(move || allowed.get() && state.get() != ActivityState::Waiting);
    let lit = move || if active.get() { 1.0 } else { STILL };
    view! {
        <div size_full flex items_center justify_center track_paint_region=region>
            <div size={size / 2.0} rounded=Radius::Full bg={move || tint()} opacity=lit transition=Pace::Move/>
        </div>
    }
    .into_any()
}

const STILL: f32 = 0.6;
