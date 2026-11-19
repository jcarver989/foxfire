use crate::chat::attachment::{Attachment, chip, chip_icon, thumbnail};
use crate::threads::{Thread, ThreadStatus};
use crate::ui::tokens::{HAIRLINE, Pace, Space};
use crate::ui::{
    ButtonVariant, IconName, Material, MaterialExt, NoticeTone, UiStyles, backdrop, button, disclosure, icon_button,
    notification,
};
use clankerdiff_rouge::{ReviewHandle, diff_view};
use rouge::core::{
    AnyView, Cursor, Div, Fit, ImageSource, Role, Styled, View, WindowHandle, div, image, pct, px, text, view,
};
use rouge::reactive::{Callback, Signal};
use std::sync::Arc;

pub(crate) fn prompt(
    row: usize,
    body: String,
    attachments: &[(usize, Attachment)],
    on_open: Callback<(Arc<[u8]>,)>,
) -> Div {
    let attachments: Vec<AnyView> = attachments
        .iter()
        .map(|(block, attachment)| {
            let id = format!("message-attachment-{row}-{block}");
            match attachment {
                Attachment::Image(bytes) => {
                    let bytes = bytes.clone();
                    let opened = bytes.clone();
                    thumbnail(id, ImageSource::Bytes(bytes), THUMBNAIL_SIZE)
                        .cursor(Cursor::Pointer)
                        .on_click(move |event| {
                            event.stop_propagation();
                            on_open.call(opened.clone());
                        })
                        .into_any()
                }
                Attachment::File { icon, name } => chip(id, chip_icon(*icon), name).into_any(),
            }
        })
        .collect();
    let body = (!body.is_empty()).then(|| view! { <text reading>{body}</text> });
    let files = (!attachments.is_empty()).then(|| view! { <div flex flex_wrap gap=Space::X2>{..attachments}</div> });
    view! {
        <div test_id={format!("prompt-{row}")} px={Space::X4.px() - HAIRLINE} py={Space::X2.px() - HAIRLINE} flex col gap=Space::X2
             material=Material::Surface>
            {body}
            {files}
        </div>
    }
}

pub(crate) fn message_actions(row: usize, text: String, hovered: Signal<bool>, window: WindowHandle) -> Div {
    view! {
        <div pt=Space::X1 flex opacity={move || if hovered.get() { 1.0 } else { 0.0 }} transition=Pace::Move unselectable>
            <icon_button test_id={format!("copy-message-{row}")} icon=IconName::Copy label="Copy"
                         on_click={move || window.copy_text(text.clone())}/>
        </div>
    }
}

pub(crate) fn image_preview(bytes: Arc<[u8]>, on_close: Callback) -> impl View {
    view! {
        <backdrop test_id="image-preview" on_dismiss=on_close>
            <div w=pct(100.0) h=pct(100.0) min_h=0.0 cursor=Cursor::Pointer on_click={move |_| on_close.call()}>
                {image(ImageSource::Bytes(bytes)).fit(Fit::Contain).size_full()}
            </div>
        </backdrop>
    }
}

pub(crate) fn full_diff(handle: &ReviewHandle, on_close: Callback) -> impl View + use<> {
    view! {
        <backdrop test_id="full-diff" on_dismiss=on_close>
            <div test_id="full-diff-panel" w=pct(100.0) h=pct(100.0) min_h=0.0 flex col overflow_hidden material=Material::Glass>
                <div flex justify_end p=Space::X1>
                    <icon_button test_id="close-full-diff" icon=IconName::X label="Close" on_click=on_close/>
                </div>
                <div flex_1 min_h=0.0>{diff_view(*handle)}</div>
            </div>
        </backdrop>
    }
}

pub(crate) fn thinking_block(
    thought: String,
    streaming: bool,
    index: usize,
    expanded: bool,
    on_toggle: Callback,
) -> AnyView {
    let body = expanded.then(|| {
        view! {
            <div test_id={format!("thinking-text-{index}")} pl=Space::X3 border_l=px(2.0) border_color=Role::BorderSubtle>
                <text body muted>{thought}</text>
            </div>
        }
    });
    view! {
        <div flex col gap=Space::X2>
            <disclosure test_id={format!("thinking-{index}")} expanded on_toggle=on_toggle>
                <text body muted>{if streaming { "Thinking…" } else { "Thought" }}</text>
            </disclosure>
            {body}
        </div>
    }
    .into_any()
}

pub(crate) fn notice(text: &str) -> AnyView {
    notification(text.to_owned(), NoticeTone::Neutral, None).into_any()
}

pub(crate) fn error(index: usize, text: &str) -> AnyView {
    notification(text.to_owned(), NoticeTone::Error, None).test_id(format!("transcript-error-{index}")).into_any()
}

pub(crate) fn reconnect_banner(thread: Thread, status: ThreadStatus) -> Option<Div> {
    let problem = status.problem()?;
    let banner = notification(problem.to_owned(), NoticeTone::Warning, None)
        .material(Material::Surface)
        .p(Space::X2.px() - HAIRLINE)
        .child(reconnect_button(thread));
    Some(banner)
}

pub(crate) fn reconnect_button(thread: Thread) -> AnyView {
    view! {
        <button test_id="transcript-reconnect" label="Reconnect" icon=IconName::RotateCw variant=ButtonVariant::Primary
                on_click={move || thread.reconnect()}/>
    }
    .into_any()
}

const THUMBNAIL_SIZE: f32 = Space::X24.px();
