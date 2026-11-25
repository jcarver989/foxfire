use foxfire_desktop::ui::{ActivityMotion, cells};
use rouge::core::access::Role;
use rouge::core::testing::{TestError, TestUi};
use rouge::core::{Color, Styled, div, view};
use rouge::reactive::{Entered, Runtime, Signal, signal};
use rouge_widgets::testing::component_test;

#[test]
fn a_cell_lights_for_every_fifth() -> Result<(), TestError> {
    for (share, lit) in [(0.0, 0), (0.2, 1), (0.4, 2), (0.8, 4), (1.0, 5), (1.7, 5)] {
        let shades = SceneBuilder::default().share(share).build().shades()?;

        let (on, off) = shades.split_at(lit);
        assert!(on.iter().all(|shade| *shade == WHITE), "{share} fully lights {lit} cells: {shades:?}");
        assert!(off.iter().all(|shade| *shade < WHITE / 2), "{share} leaves the rest unlit: {shades:?}");
    }
    Ok(())
}

#[test]
fn the_newest_cell_is_lit_in_part() -> Result<(), TestError> {
    let shades = SceneBuilder::default().share(0.47).build().shades()?;

    assert_eq!(shades[1], WHITE);
    assert!(shades[2] < shades[1] && shades[2] > shades[3], "{shades:?}");
    Ok(())
}

#[test]
fn a_little_use_still_shows() -> Result<(), TestError> {
    let shades = SceneBuilder::default().share(0.005).build().shades()?;

    assert!(shades[0] > shades[1], "{shades:?}");
    Ok(())
}

#[test]
fn the_cells_report_their_share() -> Result<(), TestError> {
    let mut scene = SceneBuilder::default().share(0.5).build();
    assert_eq!(scene.ui.access_node(Role::ProgressIndicator, "Cells")?.numeric_value, Some(0.5));

    scene.share.set(0.75);

    assert_eq!(scene.ui.access_node(Role::ProgressIndicator, "Cells")?.numeric_value, Some(0.75));
    Ok(())
}

#[test]
fn the_newest_cell_breathes_only_while_live_seen_and_allowed_to_move() {
    let mut scene = SceneBuilder::default().share(0.47).live(true).build();
    assert!(scene.ui.is_animating());

    scene.motion.set_reduced(true);
    scene.ui.run(1.0);
    assert!(!scene.ui.is_animating(), "reduced motion holds it still");
    scene.motion.set_reduced(false);
    scene.ui.advance(0.0);
    assert!(scene.ui.is_animating());

    scene.ui.set_focused(false);
    scene.ui.run(1.0);
    assert!(!scene.ui.is_animating(), "an unfocused window is still");
    scene.ui.set_focused(true);
    scene.ui.advance(0.0);
    assert!(scene.ui.is_animating());

    scene.live.set(false);
    scene.ui.run(1.0);
    assert!(!scene.ui.is_animating(), "nothing is happening");
}

#[test]
fn breathing_runs_on_the_compositor_alone() {
    let mut scene = SceneBuilder::default().share(0.47).live(true).build();
    scene.ui.run(1.0);
    let before = scene.ui.stats();

    scene.ui.run(1.0);

    let after = scene.ui.stats();
    assert_eq!((before.layouts, before.paints, before.patches), (after.layouts, after.paints, after.patches));
}

#[derive(Default)]
struct SceneBuilder {
    share: f32,
    live: bool,
}

struct Scene {
    share: Signal<f32>,
    live: Signal<bool>,
    motion: ActivityMotion,
    ui: TestUi,
    _runtime: Entered,
}

impl SceneBuilder {
    fn share(mut self, share: f32) -> Self {
        self.share = share;
        self
    }

    fn live(mut self, live: bool) -> Self {
        self.live = live;
        self
    }

    fn build(self) -> Scene {
        let runtime = Runtime::new().enter();
        let (share, live) = (signal(self.share), signal(self.live));
        let motion = signal(None);
        let ui = component_test().size(240.0, 40.0).build(move || {
            motion.set(Some(ActivityMotion::install(false)));
            view! {
                <div size_full flex bg=Color::BLACK>
                    <cells share={move || share.get()} tint=Color::WHITE live={move || live.get()}
                           test_id="cells" access_label="Cells"/>
                </div>
            }
        });
        let motion = motion.get_untracked().expect("the scene installs its motion preference");
        Scene { share, live, motion, ui, _runtime: runtime }
    }
}

impl Scene {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "The cells sit inside the small scene.")]
    fn shades(&mut self) -> Result<Vec<u8>, TestError> {
        let image = self.ui.render()?;
        let cells = self.ui.bounds_of("cells").ok_or_else(|| TestError::NoTestId("cells".to_owned()))?;
        let pitch = cells.size.width / f32::from(CELLS);
        Ok((0..CELLS)
            .map(|index| {
                let x = cells.origin.x + pitch * (f32::from(index) + 0.5);
                let y = cells.origin.y + cells.size.height / 2.0;
                image.pixel(x as u32, y as u32)[0]
            })
            .collect())
    }
}

const CELLS: u8 = 5;
const WHITE: u8 = 255;
