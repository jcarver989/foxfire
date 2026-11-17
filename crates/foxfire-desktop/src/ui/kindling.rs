use super::{FoxfireMotion, Theme};
use rouge::core::{
    Div, Ease, Inputs, Motion, PaintRegion, Rect, Role, ShaderEffect, Styled, Tween, div, switch, window_focused,
};
use rouge::reactive::{effect, memo, on_cleanup, signal, untrack, use_context};

/// A Koch snowflake that blooms in the dark where the composer will be, unfolds onto its outline and sets it
/// burning. Fill the space behind the composer with it; `composer` is the composer's bounds in this
/// decoration's own coordinates.
///
/// `reveal` is the composer's opacity: hidden while the snowflake blooms, fading in as its outline catches.
/// It plays once. Reduced motion, an unfocused window and clipping hold the burning composer still and show it
/// at once, and it stays burning when the clock resumes.
pub fn kindling(composer: impl Fn() -> Rect + 'static, reveal: Motion) -> Div {
    let composer = memo(composer);
    let theme = Theme::current();
    let region = signal(PaintRegion::default());
    let focused = window_focused();
    let motion = use_context::<FoxfireMotion>();
    let reduced = move || motion.is_some_and(FoxfireMotion::reduced);
    let animated = memo(move || focused.get() && !reduced() && region.with(|region| !region.bounds.is_empty()));
    let veiled = focused.get_untracked() && !untrack(reduced);
    reveal.set(if veiled { HIDDEN } else { 1.0 });
    let started = signal(false);
    let kindled = signal(false);
    effect(move || {
        if animated.get() {
            if veiled && !untrack(|| started.get()) {
                reveal.animate_to(1.0, Tween::new(REVEAL_AT + REVEAL_FOR).ease(Ease::Linear));
            }
            started.set(true);
        } else if untrack(|| started.get()) {
            kindled.set(true);
            reveal.set(1.0);
        }
    });
    on_cleanup(move || reveal.set(1.0));
    let fire = move |clock: bool| Fire {
        composer: composer.get(),
        color: theme.get().accent.to_array(),
        canvas: theme.get().canvas.to_array(),
        settled: !clock || untrack(|| kindled.get()),
    };
    div().test_id("kindling").absolute().top(0.0).left(0.0).size_full().pass_through().track_paint_region(region).child(
        switch(
            move || animated.get(),
            move |animated| {
                let art = div().size_full().pass_through().bg(Role::Canvas);
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

/// Matches the composer's rounded glass, so the snowflake and fire follow its outline.
const CORNER_RADIUS: f32 = super::tokens::Radius::Floating.px();
/// The composer fades in over `REVEAL_FOR` from the moment its outline catches, `CATCH_AT` in `kindling.wgsl`.
const REVEAL_AT: f32 = 0.7;
const REVEAL_FOR: f32 = 0.3;
/// Seconds from a kindling's first frame until its composer is fully shown, and so takes clicks.
#[cfg(any(test, feature = "testing"))]
pub(crate) const KINDLED_AFTER: f64 = (REVEAL_AT + REVEAL_FOR) as f64;
/// Opacity is clamped to [0, 1], so one linear ramp up from here holds the composer hidden until `REVEAL_AT`
/// and stays on the compositor's clock with the shader.
const HIDDEN: f32 = -REVEAL_AT / REVEAL_FOR;
