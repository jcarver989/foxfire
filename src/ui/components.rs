use super::tokens::{BAR_HEIGHT, CONTENT_MAX_WIDTH, ICON_SIZE, RADIUS_CONTROL};
use super::{Hint, SearchPickerEvent, Theme, UI_FONT};
use rouge::core::{
    AnyView, Div, Mask, MenuItem as WindowMenuItem, Point, Styled, View, auto, div, editable, px, text, view,
    window_handle,
};
use rouge::reactive::Signal;
use std::any::Any;

pub(crate) trait StyledExt: Styled {
    fn content_width(self) -> Self {
        self.w_full().max_w(CONTENT_MAX_WIDTH).mx(auto())
    }

    fn top_bar(self) -> Self {
        self.flex_none().h(BAR_HEIGHT).flex().items_center().pl(16.0)
    }
}

impl<T: Styled> StyledExt for T {}

/// Fades scrolled content out under a bar at the top or bottom edge of a panel.
pub(crate) fn scroll_edge(top: bool, solid: f32, reach: f32) -> Div {
    let (from, to) = if top { (0.0, 1.0) } else { (1.0, 0.0) };
    let fade = Mask::linear_gradient(Point::new(0.0, from), Point::new(0.0, to)).stops(solid / (solid + reach), 1.0);
    let edge =
        div().absolute().left(0.0).right(0.0).h(solid + reach).pass_through().bg(Theme::current().canvas()).mask(fade);
    if top { edge.top(0.0) } else { edge.bottom(0.0) }
}

pub(crate) fn icon_slot() -> Div {
    div().flex_none().size(ICON_SIZE).flex().items_center().justify_center()
}

pub(crate) fn with_middots(parts: impl IntoIterator<Item = AnyView>) -> Vec<AnyView> {
    let muted = Theme::current().text_muted();
    let mut separated = Vec::new();
    for part in parts {
        if !separated.is_empty() {
            separated.push(view! { <div flex_none><text color=muted>"·"</text></div> }.into_any());
        }
        separated.push(part);
    }
    separated
}

pub(crate) fn hint(id: impl Into<String>, label: &'static str, child: impl View) -> AnyView {
    div().test_id(id).flex_none().flex().items_center().child(child).tooltip(label).into_any()
}

pub(crate) fn text_field(id: impl Into<String>, value: Signal<String>, on_submit: impl Fn(&str) + 'static) -> Div {
    let theme = Theme::current();
    view! {
        <div min_w=0.0 px=8.0 py=4.0 rounded=RADIUS_CONTROL border=px(1.0) border_color=theme.border() bg=theme.canvas()>
            <editable value test_id=id font=UI_FONT text_size=13.0 color=theme.text() w_full on_submit=on_submit/>
        </div>
    }
}

pub(crate) fn picker_closed(event: &SearchPickerEvent, on_chose: impl FnOnce(usize)) -> bool {
    match event {
        SearchPickerEvent::Chose(index) => {
            on_chose(*index);
            true
        }
        SearchPickerEvent::Dismissed => true,
        SearchPickerEvent::Query(_) | SearchPickerEvent::Highlighted(_) => false,
    }
}

pub(crate) fn dispatch<T: Any>(action: T) {
    window_handle().dispatch_menu_item(WindowMenuItem::action(String::new(), action));
}
