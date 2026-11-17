use aether_desktop::ui::{WispMotion, kindling};
use rouge::core::testing::TestUi;
use rouge::core::{Motion, Rect, Styled, div, motion};
use rouge::reactive::{Entered, Runtime};
use rouge_widgets::testing::component_test;

#[test]
fn reduced_motion_shows_the_burning_composer_without_a_clock() {
    let mut scene = SceneBuilder::default().reduced(true).build();
    scene.ui.advance(0.0);
    let before = scene.ui.render().unwrap().pixels;

    scene.ui.run(3.0);

    assert!(!scene.ui.is_animating());
    assert_eq!(scene.ui.render().unwrap().pixels, before);
    assert_eq!(scene.reveal.get(), 1.0, "the composer is shown at once");
}

#[test]
fn turning_on_reduced_motion_stills_a_burning_fire_and_shows_the_composer() {
    let mut scene = SceneBuilder::default().build();
    scene.ui.run(0.2);
    assert!(scene.ui.is_animating());

    scene.motion.set_reduced(true);
    scene.ui.run(1.0);

    assert!(!scene.ui.is_animating(), "the preference applies to mounted art");
    assert_eq!(scene.reveal.get(), 1.0);
}

#[test]
fn the_composer_is_hidden_while_the_fire_kindles_and_then_shown() {
    let mut scene = SceneBuilder::default().build();
    scene.ui.run(0.02);
    assert!(scene.reveal.get() < 1.0, "hidden while the fire kindles: {}", scene.reveal.get());

    scene.ui.run(5.0);
    assert_eq!(scene.reveal.get(), 1.0);
}

#[test]
fn unfocusing_the_window_mid_bloom_shows_the_composer_at_once() {
    let mut scene = SceneBuilder::default().build();
    scene.ui.run(0.2);

    scene.ui.set_focused(false);
    scene.ui.run(0.1);

    assert_eq!(scene.reveal.get(), 1.0);
}

#[test]
fn an_unfocused_window_holds_the_fire_still_and_refocusing_keeps_the_composer_shown() {
    let mut scene = SceneBuilder::default().build();
    scene.ui.run(5.0);

    scene.ui.set_focused(false);
    scene.ui.run(1.0);
    assert!(!scene.ui.is_animating(), "an unfocused window is still");

    scene.ui.set_focused(true);
    scene.ui.run(0.1);
    assert!(scene.ui.is_animating());
    assert_eq!(scene.reveal.get(), 1.0, "the composer stays shown");
}

struct Scene {
    ui: TestUi,
    reveal: Motion,
    motion: WispMotion,
    _entered: Entered,
}

#[derive(Default)]
struct SceneBuilder {
    reduced: bool,
}

impl SceneBuilder {
    fn reduced(mut self, reduced: bool) -> Self {
        self.reduced = reduced;
        self
    }

    fn build(self) -> Scene {
        let entered = Runtime::new().enter();
        let (ui, (reveal, wisp_motion)) = component_test().size(900.0, 400.0).build_with(move || {
            let wisp_motion = WispMotion::install(self.reduced);
            let composer = Rect::new(150.0, 160.0, 600.0, 80.0);
            let reveal = motion(1.0);
            (div().size_full().relative().child(kindling(move || composer, reveal)), (reveal, wisp_motion))
        });
        Scene { ui, reveal, motion: wisp_motion, _entered: entered }
    }
}
