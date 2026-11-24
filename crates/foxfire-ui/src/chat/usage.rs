use crate::ui::tokens::{Space, UNIT};
use crate::ui::{UiStyles, cells, list_row, selection};
use agent_client_protocol::v2::schema as acp;
use foxfire_core::format::{cost, token_count};
use rouge::core::{Fill, Role, Styled, View, div, dynamic, text, view};
use rouge::reactive::{Memo, memo, signal};
use rouge::widgets::{Placement, popover, tooltip};

pub(crate) fn usage_meter(usage: Memo<Option<acp::UsageUpdate>>, working: Memo<bool>) -> impl View {
    let open = signal(false);
    let share = memo(move || usage.with(|usage| usage.as_ref().map_or(0.0, share)));
    let level = memo(move || level(share.get()));
    let counts = memo(move || usage.with(|usage| usage.as_ref().map_or_default(counts)));
    let left = memo(move || usage.with(|usage| left(usage.as_ref()?)));
    let spent = memo(move || usage.with(|usage| usage.as_ref()?.cost.value().map(cost)));
    let trigger = view! {
        <tooltip label="Context used" content={move || view! { <text small primary no_wrap>{move || counts.get()}</text> }}>
            <list_row test_id="context-meter" selection={move || selection(open.get())} on_click={move || open.set(true)}>
                <cells share={move || share.get()} tint={move || Fill::Role(level.get())} live={move || working.get()}
                       test_id="context-cells" access_label="Context used"/>
                <dynamic>{spent.get().map(|spent| view! {
                    <div test_id="session-cost" flex_none><text body_strong primary no_wrap>{spent}</text></div>
                })}</dynamic>
            </list_row>
        </tooltip>
    };
    view! {
        <popover open=open trigger=trigger placement=Placement::Above>
            <div test_id="usage-panel" w=PANEL_WIDTH p=Space::X3 flex col gap=Space::X1 body>
                {figure("usage-context", "Context", move || Some(counts.get()))}
                {figure("usage-left", "Left", move || left.get())}
                {figure("usage-cost", "Session cost", move || spent.get())}
            </div>
        </popover>
    }
}

fn figure(id: &'static str, label: &'static str, value: impl Fn() -> Option<String> + 'static) -> impl View {
    view! {
        <dynamic>{value().map(|value| view! {
            <div test_id=id flex items_center gap=Space::X3>
                <div flex_1><text secondary no_wrap>{label}</text></div>
                <text primary no_wrap>{value}</text>
            </div>
        })}</dynamic>
    }
}

#[allow(clippy::cast_precision_loss, reason = "The cells only need subpixel precision; labels use integer counts.")]
fn share(usage: &acp::UsageUpdate) -> f32 {
    if usage.size == 0 { 0.0 } else { usage.used.min(usage.size) as f32 / usage.size as f32 }
}

fn level(share: f32) -> Role {
    if share >= FULL {
        Role::Destructive
    } else if share >= NEARLY_FULL {
        Role::Warning
    } else {
        Role::Accent
    }
}

fn counts(usage: &acp::UsageUpdate) -> String {
    if usage.size == 0 {
        format!("{} tokens", token_count(usage.used))
    } else {
        format!("{} / {} tokens", token_count(usage.used), token_count(usage.size))
    }
}

fn left(usage: &acp::UsageUpdate) -> Option<String> {
    (usage.size > 0).then(|| format!("{} tokens", token_count(usage.size.saturating_sub(usage.used))))
}

const PANEL_WIDTH: f32 = 64.0 * UNIT;
const NEARLY_FULL: f32 = 0.75;
const FULL: f32 = 0.9;
