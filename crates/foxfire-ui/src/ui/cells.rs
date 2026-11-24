use super::foxfire::{ROUNDNESS, may_animate};
use rouge::core::access::Role;
use rouge::core::{
    Corners, Div, Ease, Fill, IntoProp, Keyframes, PaintRegion, Prop, Styled, component, div, switch, view,
};
use rouge::reactive::{memo, signal, stored};

#[component]
pub fn cells<T>(
    share: impl Fn() -> f32 + 'static,
    tint: impl IntoProp<Fill, T>,
    live: impl Fn() -> bool + 'static,
) -> Div {
    let tint = stored(tint.into_prop());
    let fill = move || tint.with(Prop::get);
    let share = memo(move || share().clamp(0.0, 1.0));
    let region = signal(PaintRegion::default());
    let animated = may_animate(region);
    let breathing = memo(move || live() && animated.get());
    let cells = (0..CELLS).map(move |index| {
        let lit = move || lit(share.get(), index);
        let newest = memo(move || lit() > 0.0 && (index + 1 == CELLS || self::lit(share.get(), index + 1) <= 0.0));
        view! {
            <div flex_none size=CELL opacity={move || opacity(lit())}>
                <switch on={move || breathing.get() && newest.get()} let:breathes>
                    <div size_full rounded={Corners::all(CELL * ROUNDNESS)} bg=fill when:animate=(breathes, breath())/>
                </switch>
            </div>
        }
    });
    view! {
        <div role=Role::ProgressIndicator value_range=(0.0, 1.0) numeric_value={move || f64::from(share.get())}
             flex_none flex items_center gap=GAP track_paint_region=region>
            {..cells}
        </div>
    }
}

fn lit(share: f32, index: u8) -> f32 {
    let lit = share * f32::from(CELLS) - f32::from(index);
    if lit < 1e-3 { 0.0 } else { lit.min(1.0) }
}

fn opacity(lit: f32) -> f32 {
    if lit > 0.0 { lit.max(FAINTEST) } else { UNLIT }
}

fn breath() -> Keyframes {
    Keyframes::new()
        .at(0.0, |pose| pose.opacity(1.0))
        .at(0.5, |pose| pose.opacity(EXHALED))
        .at(1.0, |pose| pose.opacity(1.0))
        .duration(1.6)
        .ease(Ease::InOut)
        .repeat()
}

const CELLS: u8 = 10;
const CELL: f32 = 6.0;
const GAP: f32 = 2.0;
const FAINTEST: f32 = 0.45;
const UNLIT: f32 = 0.2;
const EXHALED: f32 = 0.4;
