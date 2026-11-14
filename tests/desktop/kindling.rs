use aether_desktop::ui::{WispMotion, kindling};
use rouge::core::testing::TestUi;
use rouge::core::{Rect, Styled, div};
use rouge::reactive::{Entered, Runtime};
use rouge_widgets::testing::component_test;

#[test]
fn runes_wake_out_of_the_dark_then_fall_and_leave_the_composer_burning() {
    let mut scene = SceneBuilder::default().build();
    scene.ui.run(0.1);
    let dark = scene.lit();
    scene.ui.run(2.4);
    let waking = scene.lit();
    scene.ui.run(4.0);
    let burning = scene.lit();

    assert!(dark < waking, "runes light up as they wake: {dark} then {waking}");
    assert!(burning > dark, "the composer is left burning: {burning}");
    let before = scene.ui.render().unwrap().pixels;
    scene.ui.run(0.5);
    assert_ne!(before, scene.ui.render().unwrap().pixels, "the fire keeps moving");
    assert!(scene.ui.is_animating());
}

#[test]
fn reduced_motion_shows_the_burning_composer_without_a_clock() {
    let mut scene = SceneBuilder::default().reduced(true).build();
    scene.ui.advance(0.0);
    let before = scene.ui.render().unwrap().pixels;

    scene.ui.run(3.0);

    assert!(!scene.ui.is_animating());
    assert_eq!(scene.ui.render().unwrap().pixels, before);
    assert!(scene.lit() > SceneBuilder::default().build().lit(), "the settled fire is visible at once");
}

#[test]
fn an_unfocused_window_holds_the_fire_still_and_refocusing_does_not_replay_the_runes() {
    let mut scene = SceneBuilder::default().build();
    scene.ui.run(0.1);
    let dark = scene.lit();
    scene.ui.run(7.0);

    scene.ui.set_focused(false);
    scene.ui.run(1.0);
    assert!(!scene.ui.is_animating(), "an unfocused window is still");
    let held = scene.lit();

    scene.ui.set_focused(true);
    scene.ui.run(0.1);
    assert!(scene.ui.is_animating());
    let resumed = scene.lit();
    assert!(resumed * 2 > held && resumed > dark * 4, "still burning rather than dark again: {dark} {held} {resumed}");
}

struct Scene {
    ui: TestUi,
    _entered: Entered,
}

impl Scene {
    /// How much light the decoration gives off, summed over every pixel.
    fn lit(&mut self) -> u64 {
        let image = self.ui.render().unwrap();
        image
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel[..3].iter().map(|channel| u64::from(*channel)).sum::<u64>())
            .sum()
    }
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
        let (ui, ()) = component_test().size(900.0, 400.0).build_with(move || {
            let _ = WispMotion::install(self.reduced);
            let composer = Rect::new(150.0, 160.0, 600.0, 80.0);
            (div().size_full().relative().child(kindling(move || composer)), ())
        });
        Scene { ui, _entered: entered }
    }
}
