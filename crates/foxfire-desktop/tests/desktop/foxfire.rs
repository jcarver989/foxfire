use foxfire_desktop::ui::{FoxfireMotion, FoxfireState, foxfire};
use rouge::core::testing::TestUi;
use rouge::core::{Color, Styled, div, show};
use rouge::reactive::{Entered, Runtime, Signal, signal};
use rouge_widgets::testing::component_test;

#[test]
fn waiting_and_reduced_motion_have_no_clock_and_never_change_pixels() {
    for (state, reduced) in [(FoxfireState::Waiting, false), (FoxfireState::Writing, true)] {
        let mut scene = SceneBuilder::default().state(state).reduced(reduced).build();
        let before = scene.ui.render().unwrap().pixels;
        scene.ui.run(3.0);
        assert!(!scene.ui.is_animating());
        assert_eq!(scene.ui.render().unwrap().pixels, before);
    }
}

#[test]
fn clipped_hidden_and_unfocused_foxfires_stop_and_resume_their_clocks() {
    let mut scene = SceneBuilder::default().build();
    assert!(scene.ui.is_animating());
    scene.offset.set(1000.0);
    scene.ui.run(1.0);
    assert!(!scene.ui.is_animating(), "clipped art is still");
    scene.offset.set(0.0);
    scene.ui.advance(0.0);
    assert!(scene.ui.is_animating());
    scene.hidden.set(true);
    scene.ui.run(1.0);
    assert!(!scene.ui.is_animating(), "hidden art is still");
    scene.hidden.set(false);
    scene.ui.advance(0.0);
    assert!(scene.ui.is_animating());
    scene.ui.set_focused(false);
    scene.ui.run(1.0);
    assert!(!scene.ui.is_animating(), "unfocused art is still");
    scene.ui.set_focused(true);
    scene.ui.advance(0.0);
    assert!(scene.ui.is_animating());
    scene.motion.set_reduced(true);
    scene.ui.run(1.0);
    assert!(!scene.ui.is_animating(), "the preference applies to mounted art");
}

#[test]
fn many_running_foxfires_animate_on_the_compositor_alone() {
    let mut scene = SceneBuilder::default().count(32).build();
    scene.ui.run(1.0);
    let before = scene.ui.stats();
    scene.ui.run(1.0);
    let after = scene.ui.stats();
    assert_eq!(before.layouts, after.layouts);
    assert_eq!(before.paints, after.paints);
    assert_eq!(before.patches, after.patches, "no reactive work per frame");
    let stats = scene.ui.draw().unwrap();
    assert_eq!(stats.groups, 0, "cells need no offscreen groups");
    assert_eq!(stats.backdrop_passes, 0);
    assert_eq!(stats.content_blurs, 0);
}

#[test]
fn a_running_foxfire_wakes_the_compositor_at_most_thirty_times_a_second() {
    for state in [
        FoxfireState::Connecting,
        FoxfireState::Thinking,
        FoxfireState::Working,
        FoxfireState::Writing,
        FoxfireState::Compacting,
    ] {
        let mut scene = SceneBuilder::default().state(state).build();
        scene.ui.run(1.0);
        scene.ui.draw().unwrap();
        let frames = scene.ui.compositor_frames(1.0).unwrap();
        assert!((1..=30).contains(&frames), "{state:?} drew {frames} frames in a second");
    }
}

const FOXFIRE_SIZE: f32 = 12.0;

struct Scene {
    ui: TestUi,
    motion: FoxfireMotion,
    offset: Signal<f32>,
    hidden: Signal<bool>,
    _entered: Entered,
}

struct SceneBuilder {
    state: FoxfireState,
    reduced: bool,
    count: usize,
}

impl Default for SceneBuilder {
    fn default() -> Self {
        Self { state: FoxfireState::Thinking, reduced: false, count: 1 }
    }
}

impl SceneBuilder {
    fn state(mut self, state: FoxfireState) -> Self {
        self.state = state;
        self
    }
    fn reduced(mut self, reduced: bool) -> Self {
        self.reduced = reduced;
        self
    }
    fn count(mut self, count: usize) -> Self {
        self.count = count;
        self
    }

    fn build(self) -> Scene {
        let entered = Runtime::new().enter();
        let (ui, (motion, offset, hidden)) =
            component_test().size(800.0, 120.0).refresh_rate(120.0).build_with(move || {
                let state = self.state;
                let motion = FoxfireMotion::install(self.reduced);
                let offset = signal(0.0);
                let hidden = signal(false);
                let art = show(
                    move || !hidden.get(),
                    move || {
                        div().absolute().top(16.0).left(move || offset.get()).flex().gap(8.0).children(
                            (0..self.count).map(move |_| foxfire(FOXFIRE_SIZE, Color::rgb(0x0038_7ccf), move || state)),
                        )
                    },
                );
                (div().size_full().relative().overflow_hidden().p(16.0).child(art), (motion, offset, hidden))
            });
        Scene { ui, motion, offset, hidden, _entered: entered }
    }
}
