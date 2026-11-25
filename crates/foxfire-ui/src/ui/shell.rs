use super::tokens::{HEADER_HEIGHT, ICON_SIZE, PANEL_GUTTER, Pace, ROW_HEIGHT, Space, TWO_LINE_ROW_HEIGHT, UNIT};
use super::{
    FoxfireState, Icon, IconName, ListRow, ResizablePanel, Side, StyledExt, UI_FONT, UiStyles, chord, empty_state,
    foxfire, hint, icon_button, icon_slot, list_row, resizable_panel, section_header, selection, tree_row,
};
use crate::ChatRoom;
use crate::thread::ThreadStatus;
use rouge::core::{
    AnyView, Div, Motion, Role, Styled, View, div, dynamic, motion, presentation, scroll, show, text, view, window_size,
};
use rouge::reactive::{Callback, Memo, effect, signal, untrack};

pub const MIN_MAIN_WIDTH: f32 = 80.0 * UNIT;
pub const TOGGLE_SIDEBAR_KEYS: &str = "secondary-b";
/// The narrowest, usual and widest each panel can be.
const SIDEBAR_WIDTHS: [f32; 3] = [50.0 * UNIT, 68.0 * UNIT, 120.0 * UNIT];
const ROOM_WIDTHS: [f32; 3] = [64.0 * UNIT, 84.0 * UNIT, 120.0 * UNIT];
const SIDEBAR_ENTRY_SLIDE: f32 = Space::X6.px();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToggleSidebar;

/// How wide a host remembers the user leaving a panel.
#[derive(Clone, Copy)]
pub struct PanelWidth {
    saved: Memo<Option<f32>>,
    set: Callback<(Option<f32>,)>,
}

/// Shared window layout. Hosts own the sidebar preferences and supply content and actions.
#[derive(Clone, Copy)]
pub struct ThreadShell {
    collapsed: Memo<bool>,
    set_collapsed: Callback<(bool,)>,
    width: PanelWidth,
    room_width: PanelWidth,
    slide: Motion,
    fade: Motion,
}

impl ThreadShell {
    pub fn new(
        collapsed: Memo<bool>,
        set_collapsed: impl Fn(bool) + 'static,
        width: PanelWidth,
        room_width: PanelWidth,
    ) -> Self {
        Self {
            collapsed,
            set_collapsed: Callback::from(set_collapsed),
            width,
            room_width,
            slide: motion(0.0),
            fade: motion(1.0),
        }
    }

    pub fn view<T: View, U: View>(
        self,
        id: &'static str,
        controls: impl Fn() -> T + 'static,
        sections: impl Fn() -> U + 'static,
        main: impl View,
        overlays: impl View,
    ) -> Div {
        view! {
            <div test_id=id relative size_full flex bg=Role::Canvas text_color=Role::Text font=UI_FONT body>
                <dynamic>{(!self.collapsed.get()).then(|| untrack(|| self.sidebar(controls(), sections())))}</dynamic>
                <div test_id="main" relative flex_1 min_w=0.0 h_full>{main}</div>
                {overlays}
            </div>
        }
        .bind_key(chord(TOGGLE_SIDEBAR_KEYS), ToggleSidebar)
        .on_action(move |_: &ToggleSidebar| self.toggle())
    }

    /// A conversation with its side room beside it.
    pub fn with_room(self, room: ChatRoom, conversation: impl View) -> Div {
        let side = dynamic(move || {
            let panel = || self.room_width.panel(Side::Right, ROOM_WIDTHS, room.panel()).test_id("chat-room-resizable");
            room.is_open().then(|| untrack(panel))
        });
        view! {
            <div size_full flex>
                <div relative flex_1 min_w=0.0 h_full>{conversation}</div>
                {side}
            </div>
        }
    }

    /// A leading control for the thread header, shown only when the sidebar is closed.
    pub fn reopen(self) -> impl View {
        dynamic(move || self.collapsed.get().then(|| self.toggle_button(false)))
    }

    /// The shared placeholder when no thread is selected.
    pub fn no_thread(self) -> impl View {
        self.placeholder(view! {
            <empty_state icon=IconName::MessageSquareText title="No thread selected"
                         detail="Select a thread, or start a new one"/>
        })
    }

    /// Keeps the sidebar toggle in reach while a thread has no conversation header.
    pub fn placeholder(self, content: impl View) -> impl View {
        view! {
            <div size_full flex col>
                <show when={move || self.collapsed.get()}>
                    <div top_bar h=HEADER_HEIGHT pr=PANEL_GUTTER>{self.toggle_button(false)}</div>
                </show>
                <div flex_1 min_h=0.0 flex items_center justify_center>{content}</div>
            </div>
        }
    }

    fn sidebar(self, controls: impl View, sections: impl View) -> AnyView {
        let contents = self.sidebar_contents(controls, sections);
        view! {
            <div h_full flex_none flex translate_x={self.slide} opacity={self.fade}>
                {self.width.panel(Side::Left, SIDEBAR_WIDTHS, contents).test_id("sidebar")}
            </div>
        }
        .into_any()
    }

    fn sidebar_contents(self, controls: impl View, sections: impl View) -> impl View {
        view! {
            <div size_full flex col>
                <div top_bar h=HEADER_HEIGHT pr=PANEL_GUTTER gap=Space::X1>
                    <div flex_1/>{controls}{self.toggle_button(true)}
                </div>
                <scroll test_id="projects" flex_1 min_h=0.0 w_full>
                    <div px=Space::X2 pb=Space::X2 flex col gap=Space::X1>{sections}</div>
                </scroll>
            </div>
        }
    }

    fn toggle_button(self, open: bool) -> Div {
        view! {
            <div flex_none flex items_center>
                <icon_button test_id="toggle-sidebar" icon=IconName::PanelLeft label="Toggle sidebar" selected=open
                             shortcut={chord(TOGGLE_SIDEBAR_KEYS)} on_click={move || self.toggle()}/>
            </div>
        }
    }

    fn toggle(self) {
        let opening = self.collapsed.get_untracked();
        if opening {
            self.slide.set(-SIDEBAR_ENTRY_SLIDE);
            self.fade.set(0.0);
            self.slide.animate_to(0.0, Pace::Enter);
            self.fade.animate_to(1.0, Pace::Enter);
        }
        self.set_collapsed.call(!opening);
    }
}

impl PanelWidth {
    pub fn new(saved: Memo<Option<f32>>, set: impl Fn(Option<f32>) + 'static) -> Self {
        Self { saved, set: Callback::from(set) }
    }

    /// A panel at the remembered width, or the closest that leaves room for the rest of the window.
    fn panel(self, edge: Side, [min, default, max]: [f32; 3], content: impl View) -> ResizablePanel {
        let window = window_size();
        let fit = move |width: f32, available: f32| {
            let max = (available - MIN_MAIN_WIDTH).clamp(min, max);
            (width.clamp(min, max) / UNIT).floor() * UNIT
        };
        let width = signal(fit(self.saved.get_untracked().unwrap_or(default), window.get_untracked().width));
        effect(move || {
            width.set_if_changed(fit(self.saved.get().unwrap_or(default), window.get().width));
        });
        let on_resize = move |requested| {
            let chosen = fit(requested, window.get_untracked().width);
            if self.saved.get_untracked() != Some(chosen) {
                self.set.call(Some(chosen));
            }
            width.set_if_changed(chosen);
        };
        let on_reset = move || self.set.call(None);
        view! {
            <resizable_panel edge size=width min_size=min max_size=max reset_size=default on_resize=on_resize
                             on_reset=on_reset h_full flex col bg=Role::Chrome>
                {content}
            </resizable_panel>
        }
    }
}

/// The sidebar's hosted-project section, shared by desktop remotes and browser projects.
pub fn cloud_section<T: View>(
    expanded: Memo<bool>,
    toggle: impl Fn() + 'static,
    groups: impl Fn() -> T + 'static,
) -> Div {
    view! {
        <div flex col gap=Space::X1 pt=Space::X2>
            <section_header test_id="section-cloud" title="Cloud" icon=IconName::Cloud
                            expanded={move || expanded.get()} on_toggle=toggle/>
            <show when={move || expanded.get()}>
                <div flex col gap=Space::X1>{groups()}</div>
            </show>
        </div>
    }
}

/// A collapsible project or remote heading with host-provided actions.
pub fn sidebar_group(
    id: String,
    label: String,
    icon: (String, IconName),
    expanded: bool,
    toggle: impl Fn() + 'static,
    controls: impl View,
) -> Div {
    let muted = Role::TextMuted;
    let hovered = signal(false);
    let touch = presentation().get_untracked().is_touch();
    let leading = view! { <div test_id={icon.0} flex>{Icon::new(icon.1, muted)}</div> }.into_any();
    let toggle = Callback::from(toggle);
    let row = view! {
        <tree_row test_id=id label expanded=expanded leading=leading label_color=muted on_click=toggle on_toggle=toggle>
            <div flex items_center opacity={move || if touch || hovered.get() { 1.0 } else { 0.0 }} transition=Pace::Move>
                {controls}
            </div>
        </tree_row>
    };
    div()
        .on_hover(move |inside| {
            hovered.set_if_changed(inside);
        })
        .child(row)
}

/// One- or two-line sidebar thread row. Hosts can add trailing and hover actions.
pub fn sidebar_thread_row(
    id: String,
    selected: impl Fn() -> bool + 'static,
    title: impl View,
    icon: impl View,
    subtitle: Option<(String, Role)>,
    open: impl Fn() + 'static,
) -> ListRow {
    let two_line = subtitle.is_some();
    let title = view! { <div min_w=0.0 flex items_center gap=Space::X1>{title}</div> };
    let body = if let Some((subtitle, color)) = subtitle {
        view! {
            <div flex_1 min_w=0.0 py=Space::X1 flex col>
                {title}
                <div test_id={format!("{id}-subtitle")} min_w=0.0 overflow_hidden>
                    <text caption no_wrap color=color>{subtitle}</text>
                </div>
            </div>
        }
    } else {
        title.flex_1()
    };
    list_row()
        .test_id(id)
        .depth(usize::from(!two_line))
        .selection(move || selection(selected()))
        .on_click(open)
        .h(if two_line { TWO_LINE_ROW_HEIGHT } else { ROW_HEIGHT })
        .child(icon_slot())
        .child(icon_slot().when(two_line, |slot| slot.self_start().mt(Space::X2)).child(icon))
        .child(body)
}

/// A thread status mark for sidebar rows.
pub fn thread_status_icon(row: &str, status: ThreadStatus) -> AnyView {
    let (name, label, shape, icon, color) = match status {
        ThreadStatus::Connecting => ("connecting", "Starting agent", "foxfire", None, Role::TextMuted),
        ThreadStatus::Reconnecting => ("reconnecting", "Reconnecting", "foxfire", None, Role::TextMuted),
        ThreadStatus::Running => ("running", "Agent working", "foxfire", None, Role::Info),
        ThreadStatus::Background => {
            ("background", "Background task still running", "dot", Some(IconName::CircleDot), Role::TextMuted)
        }
        ThreadStatus::NeedsInput => {
            ("needs-input", "Waiting for your answer", "speech-bubble", Some(IconName::MessageCircle), Role::Warning)
        }
        ThreadStatus::Errored => {
            ("errored", "Unable to connect", "warning-triangle", Some(IconName::TriangleAlert), Role::Destructive)
        }
        ThreadStatus::Disconnected => {
            ("disconnected", "Disconnected", "warning-triangle", Some(IconName::TriangleAlert), Role::Destructive)
        }
        ThreadStatus::Idle | ThreadStatus::Dormant => {
            return tagged(format!("{row}-conversation-icon"), Icon::new(IconName::MessageSquareText, Role::TextMuted));
        }
    };
    let id = format!("{row}-status-{name}");
    let mark = match icon {
        Some(icon) => Icon::new(icon, color).into_any(),
        None if status == ThreadStatus::Running => foxfire(ICON_SIZE, color, || FoxfireState::Working).into_any(),
        None => foxfire(ICON_SIZE, color, || FoxfireState::Connecting).into_any(),
    };
    hint(id.clone(), label, tagged(format!("{id}-{shape}"), mark))
}

fn tagged(id: String, child: impl View) -> AnyView {
    div().test_id(id).flex().child(child).into_any()
}
