use super::tokens::{BAR_HEIGHT, ICON_SIZE, MotionScale, PANEL_GUTTER, POP_SCALE, Pace};
use super::{Hint, SearchPickerEvent, SelectionState};
use rouge::core::{
    AnyView, Div, Mask, MenuItem as WindowMenuItem, Point, Pose, Role, Styled, View, div, dynamic, show, text, view,
    window_handle,
};
use rouge::reactive::{Memo, Signal, effect, signal, untrack, use_context};
use std::any::Any;
use std::rc::Rc;

pub(crate) trait StyledExt: Styled {
    fn top_bar(self) -> Self {
        self.flex_none().h(BAR_HEIGHT).flex().items_center().pl(PANEL_GUTTER)
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

/// Shows what `render` makes of `value` while it has one, popping it in and out as a floating panel does.
pub(crate) fn popping<T: Clone + PartialEq + 'static, U: View>(
    value: Memo<Option<T>>,
    render: impl Fn(T) -> U + 'static,
) -> impl View {
    let last = signal(value.get_untracked());
    effect(move || {
        if let Some(value) = value.get() {
            last.set_if_changed(Some(value));
        }
    });
    let render = Rc::new(render);
    presence(
        move || value.with(Option::is_some),
        move || {
            let render = render.clone();
            dynamic(move || last.get().map(|value| untrack(|| render(value))))
        },
    )
}

/// Shows `children` while `when` holds, popping them in and out as a floating panel does. The pace is read again
/// whenever the motion setting changes, so panels built before it changed follow it too.
pub(crate) fn presence<T: View>(
    when: impl Fn() -> bool + Clone + 'static,
    children: impl Fn() -> T + Clone + 'static,
) -> impl View {
    let scale = use_context::<Signal<MotionScale>>();
    dynamic(move || {
        if let Some(scale) = scale {
            scale.get();
        }
        show(when.clone(), children.clone()).enter(POPPED, Pace::Enter).exit(POPPED, Pace::Exit)
    })
}

const POPPED: Pose = Pose::new().opacity(0.0).scale(POP_SCALE);

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
