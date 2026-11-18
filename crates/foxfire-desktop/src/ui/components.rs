use super::tokens::{BAR_HEIGHT, CONTENT_MAX_WIDTH, GUTTER, ICON_SIZE};
use super::{Hint, SearchPickerEvent, SelectionState};
use rouge::core::{
    AnyView, Div, Mask, MenuItem as WindowMenuItem, Point, Role, Styled, View, auto, div, text, view, window_handle,
};
use std::any::Any;

pub(crate) trait StyledExt: Styled {
    fn content_width(self) -> Self {
        self.w_full().max_w(CONTENT_MAX_WIDTH).mx(auto())
    }

    fn top_bar(self) -> Self {
        self.flex_none().h(BAR_HEIGHT).flex().items_center().pl(GUTTER)
    }
}

impl<T: Styled> StyledExt for T {}

/// Fades scrolled content out under a bar at the top or bottom edge of a panel.
pub(crate) fn scroll_edge(top: bool, solid: f32, reach: f32) -> Div {
    let (from, to) = if top { (0.0, 1.0) } else { (1.0, 0.0) };
    let fade = Mask::linear_gradient(Point::new(0.0, from), Point::new(0.0, to)).stops(solid / (solid + reach), 1.0);
    let edge = div().absolute().left(0.0).right(0.0).h(solid + reach).pass_through().bg(Role::Canvas).mask(fade);
    if top { edge.top(0.0) } else { edge.bottom(0.0) }
}

pub(crate) fn icon_slot() -> Div {
    div().flex_none().size(ICON_SIZE).flex().items_center().justify_center()
}

pub(crate) fn with_middots(parts: impl IntoIterator<Item = AnyView>) -> Vec<AnyView> {
    let muted = Role::TextMuted;
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

/// How a trigger looks while the panel it opens is showing.
pub(crate) fn selection(open: bool) -> SelectionState {
    if open { SelectionState::Selected } else { SelectionState::None }
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
