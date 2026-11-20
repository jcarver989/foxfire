use super::tokens::{
    BAR_HEIGHT, GRID_MARGIN, GROUP_HEIGHT, HEADER_HEIGHT, ICON_BUTTON_SIZE, ICON_SIZE, MotionScale, PANEL_GUTTER,
    PANEL_INSET, POP_SCALE, Pace, Space,
};
use super::{Hint, Material, MaterialExt, SearchPickerEvent, SelectionState, UiStyles, avatar};
use rouge::core::Chord;
use rouge::core::{
    AnyView, Div, Mask, MenuItem as WindowMenuItem, Point, Pose, Role, Styled, View, div, dynamic, px, show, text,
    view, window_handle,
};
use rouge::reactive::{Memo, Signal, effect, signal, untrack, use_context};
use std::any::Any;
use std::rc::Rc;

pub trait StyledExt: Styled {
    fn top_bar(self) -> Self {
        self.flex_none().h(BAR_HEIGHT).flex().items_center().pl(PANEL_GUTTER)
    }
}

impl<T: Styled> StyledExt for T {}

/// Fades scrolled content out under a bar at the top or bottom edge of a panel.
pub fn scroll_edge(top: bool, solid: f32, reach: f32) -> Div {
    let (from, to) = if top { (0.0, 1.0) } else { (1.0, 0.0) };
    let fade = Mask::linear_gradient(Point::new(0.0, from), Point::new(0.0, to)).stops(solid / (solid + reach), 1.0);
    let edge = div().absolute().left(0.0).right(0.0).h(solid + reach).pass_through().bg(Role::Canvas).mask(fade);
    if top { edge.top(0.0) } else { edge.bottom(0.0) }
}

/// Floating heading aligned to the transcript, with host-specific navigation on either side.
pub fn thread_header(
    gutter: impl Fn() -> f32 + 'static,
    leading: impl View,
    path: impl View,
    agent: impl Fn() -> String + 'static,
    trailing: impl View,
) -> Div {
    view! {
        <div test_id="thread-header" absolute top=0.0 left=0.0 right={move || px(gutter())} h=HEADER_HEIGHT px=GRID_MARGIN
             flex items_center gap=Space::X3 pass_through>
            {scroll_edge(true, PANEL_INSET, HEADER_HEIGHT)}
            <div flex_1 min_w=0.0 flex pass_through>
                <div test_id="thread-heading" min_w=0.0 h=GROUP_HEIGHT px=Space::X3 flex items_center gap=Space::X2
                     material=Material::GlassControl>
                    {leading}{path}
                    <dynamic>{let name = agent(); (!name.is_empty()).then(|| view! {
                        <div test_id="thread-agent" flex_none flex items_center gap=Space::X1>
                            {avatar(name.clone()).avatar_size(ICON_BUTTON_SIZE)}
                            <text small muted no_wrap>{name}</text>
                        </div>
                    })}</dynamic>
                </div>
            </div>
            {trailing}
        </div>
    }
}

/// Shows what `render` makes of `value` while it has one, popping it in and out as a floating panel does.
pub fn popping<T: Clone + PartialEq + 'static, U: View>(
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
pub fn presence<T: View>(
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

pub fn icon_slot() -> Div {
    div().flex_none().size(ICON_SIZE).flex().items_center().justify_center()
}

pub fn with_middots(parts: impl IntoIterator<Item = AnyView>) -> Vec<AnyView> {
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

pub fn hint(id: impl Into<String>, label: &'static str, child: impl View) -> AnyView {
    div().test_id(id).flex_none().flex().items_center().child(child).tooltip(label).into_any()
}

/// How a trigger looks while the panel it opens is showing.
pub fn selection(open: bool) -> SelectionState {
    if open { SelectionState::Selected } else { SelectionState::None }
}

pub fn picker_closed(event: &SearchPickerEvent, on_chose: impl FnOnce(usize)) -> bool {
    match event {
        SearchPickerEvent::Chose(index) => {
            on_chose(*index);
            true
        }
        SearchPickerEvent::Dismissed => true,
        SearchPickerEvent::Query(_) | SearchPickerEvent::Highlighted(_) => false,
    }
}

pub fn dispatch<T: Any>(action: T) {
    window_handle().dispatch_menu_item(WindowMenuItem::action(String::new(), action));
}

/// Resolves a built-in keyboard chord.
pub fn chord(keys: &str) -> Chord {
    Chord::parse(keys).unwrap_or_else(|error| unreachable!("built-in chord {keys}: {error}"))
}
