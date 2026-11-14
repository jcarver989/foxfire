use super::{Theme, WispMotion};
use rouge::core::{Div, Inputs, PaintRegion, Rect, ShaderEffect, Styled, div, switch, window_focused};
use rouge::reactive::{effect, memo, signal, untrack, use_context};

/// Runes that wake one by one in the dark around the composer, then crumble into its edge and leave it
/// burning. Fill the space behind the composer with it; `composer` is the composer's bounds in this
/// decoration's own coordinates.
///
/// The runes play once. Reduced motion, an unfocused window and clipping hold the burning composer still,
/// and it stays burning when the clock resumes.
pub fn kindling(composer: impl Fn() -> Rect + 'static) -> Div {
    let composer = memo(composer);
    let theme = Theme::current();
    let region = signal(PaintRegion::default());
    let focused = window_focused();
    let motion = use_context::<WispMotion>();
    let animated = memo(move || {
        focused.get() && !motion.is_some_and(WispMotion::reduced) && region.with(|region| !region.bounds.is_empty())
    });
    let started = signal(false);
    let kindled = signal(false);
    effect(move || {
        if animated.get() {
            started.set(true);
        } else if untrack(|| started.get()) {
            kindled.set(true);
        }
    });
    let fire = move |clock: bool| Fire {
        composer: composer.get(),
        color: theme.accent().get().to_array(),
        canvas: theme.canvas().get().to_array(),
        settled: !clock || untrack(|| kindled.get()),
    };
    div().test_id("kindling").absolute().top(0.0).left(0.0).size_full().pass_through().track_paint_region(region).child(
        switch(
            move || animated.get(),
            move |animated| {
                let art = div().size_full().pass_through().bg(theme.canvas());
                if animated { art.effect(move || Burning(fire(true))) } else { art.effect(move || Still(fire(false))) }
            },
        ),
    )
}

#[derive(Clone, Copy)]
struct Fire {
    composer: Rect,
    color: [f32; 4],
    canvas: [f32; 4],
    settled: bool,
}

#[derive(Clone, Copy)]
struct Burning(Fire);

#[derive(Clone, Copy)]
struct Still(Fire);

impl ShaderEffect for Burning {
    const WGSL: &'static str = include_str!("kindling.wgsl");
    const INPUTS: Inputs = Inputs::CONTENT;
    const CLOCK: Option<usize> = Some(0);

    fn uniforms(&self, values: &mut [f32; 16]) {
        self.0.uniforms(values);
    }
}

impl ShaderEffect for Still {
    const WGSL: &'static str = include_str!("kindling.wgsl");
    const INPUTS: Inputs = Inputs::CONTENT;

    fn uniforms(&self, values: &mut [f32; 16]) {
        self.0.uniforms(values);
    }
}

impl Fire {
    fn uniforms(self, values: &mut [f32; 16]) {
        let Self { composer, color, canvas, settled } = self;
        values[..15].copy_from_slice(&[
            0.0,
            if settled { 1.0 } else { 0.0 },
            composer.origin.x,
            composer.origin.y,
            composer.size.width,
            composer.size.height,
            CORNER_RADIUS,
            0.0,
            color[0],
            color[1],
            color[2],
            0.0,
            canvas[0],
            canvas[1],
            canvas[2],
        ]);
    }
}

/// Matches the composer's rounded glass, so the runes and fire follow its outline.
const CORNER_RADIUS: f32 = super::tokens::RADIUS_FLOATING;
