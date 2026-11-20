use crate::ui::tokens::{ICON_SIZE_SMALL, Space};
use crate::ui::{Icon, IconName, Material, MaterialExt, UiStyles};
use foxfire_core::session_core::QueuedPrompt;
use rouge::core::{AnyView, Div, Role, Styled, View, div, text, view};

pub(crate) fn queued_prompts(prompts: &[QueuedPrompt]) -> Div {
    let muted = Role::TextMuted;
    let shown = if prompts.len() > MAX_QUEUED_ROWS + 1 { MAX_QUEUED_ROWS } else { prompts.len() };
    let rows: Vec<AnyView> = prompts[..shown]
        .iter()
        .map(|prompt| {
            let first = prompt.submission.text.lines().next().unwrap_or_default().to_owned();
            view! {
                <div flex items_center gap=Space::X2 min_w=0.0>
                    {Icon::new(IconName::ChevronRight, muted).size(ICON_SIZE_SMALL)}
                    <div flex_none><text small muted>"Queued"</text></div>
                    <div flex_1 min_w=0.0 overflow_hidden><text small secondary no_wrap>{first}</text></div>
                </div>
            }
            .into_any()
        })
        .collect();
    let more = (shown < prompts.len()).then(|| {
        view! { <div><text small muted>{format!("{} more queued", prompts.len() - shown)}</text></div> }
    });
    view! {
        <div test_id="queued-prompts" px=Space::X3 py=Space::X2 flex col gap=Space::X1 material=Material::Glass>
            {..rows}
            {more}
        </div>
    }
}

const MAX_QUEUED_ROWS: usize = 3;
