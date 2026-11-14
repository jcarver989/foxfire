use super::Theme;
use super::tokens::RADIUS_FLOATING;
use rouge::core::{Div, Glass, Styled};

pub(crate) trait LiquidGlassExt {
    /// Flat frosted glass lit from overhead: the top edge catches the light, the sides fade and the
    /// bottom stays dark.
    #[must_use]
    fn liquid_glass(self) -> Self;

    /// [`liquid_glass`](LiquidGlassExt::liquid_glass) whose top edge brightens while `focused`.
    #[must_use]
    fn focusable_glass(self, focused: impl Fn() -> bool + 'static) -> Self;
}

impl LiquidGlassExt for Div {
    fn liquid_glass(self) -> Self {
        self.focusable_glass(|| false)
    }

    fn focusable_glass(self, focused: impl Fn() -> bool + 'static) -> Self {
        let theme = Theme::current();
        self.rounded(RADIUS_FLOATING).backdrop(move || {
            let highlight = if focused() { FOCUSED_HIGHLIGHT } else { HIGHLIGHT };
            OVERHEAD_GLASS.tint(theme.elevated().get().with_alpha(TINT)).highlight(highlight)
        })
    }
}

const OVERHEAD_GLASS: Glass = Glass::frosted(10.0).edge_width(1.5).shadow(0.2).rim_light(0.0, 0.0);

const TINT: f32 = 0.5;

const HIGHLIGHT: f32 = 0.17;

const FOCUSED_HIGHLIGHT: f32 = 0.28;
