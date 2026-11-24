use super::tokens::{HEADER_HEIGHT, ICON_SIZE, PANEL_GUTTER, Pace, ROW_HEIGHT, Space, TWO_LINE_ROW_HEIGHT, UNIT};
use super::{
    FoxfireState, Icon, IconName, ListRow, Side, StyledExt, UI_FONT, UiStyles, chord, empty_state, foxfire, hint,
    icon_button, icon_slot, list_row, resizable_panel, section_header, selection, tree_row,
};
use crate::thread::ThreadStatus;
use rouge::core::{
    AnyView, Div, Motion, Role, Styled, View, div, dynamic, motion, presentation, scroll, show, text, view, window_size,
};
use rouge::reactive::{Callback, Memo, effect, signal, untrack};

pub const MIN_MAIN_WIDTH: f32 = 80.0 * UNIT;
pub const TOGGLE_SIDEBAR_KEYS: &str = "secondary-b";
const DEFAULT_SIDEBAR_WIDTH: f32 = 68.0 * UNIT;
const MIN_SIDEBAR_WIDTH: f32 = 50.0 * UNIT;
const MAX_SIDEBAR_WIDTH: f32 = 120.0 * UNIT;
const SIDEBAR_ENTRY_SLIDE: f32 = Space::X6.px();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToggleSidebar;

/// Shared window layout. Hosts own the sidebar preferences and supply content and actions.
#[derive(Clone, Copy)]
pub struct ThreadShell {
    collapsed: Memo<bool>,
    width: Memo<Option<f32>>,
    set_collapsed: Callback<(bool,)>,
    set_width: Callback<(Option<f32>,)>,
    slide: Motion,
    fade: Motion,
}

impl ThreadShell {
    pub fn new(
        collapsed: Memo<bool>,
        width: Memo<Option<f32>>,
        set_collapsed: impl Fn(bool) + 'static,
        set_width: impl Fn(Option<f32>) + 'static,
    ) -> Self {
        Self {
            collapsed,
            width,
            set_collapsed: Callback::from(set_collapsed),
            set_width: Callback::from(set_width),
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
        let window = window_size();
        let fit = move |width: f32, available: f32| {
            let max = (available - MIN_MAIN_WIDTH).clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
            (width.clamp(MIN_SIDEBAR_WIDTH, max) / UNIT).floor() * UNIT
        };
        let width =
            signal(fit(self.width.get_untracked().unwrap_or(DEFAULT_SIDEBAR_WIDTH), window.get_untracked().width));
        effect(move || {
            width.set_if_changed(fit(self.width.get().unwrap_or(DEFAULT_SIDEBAR_WIDTH), window.get().width));
        });
        let on_resize = move |requested| {
            let chosen = fit(requested, window.get_untracked().width);
            if self.width.get_untracked() != Some(chosen) {
                self.set_width.call(Some(chosen));
            }
            width.set_if_changed(chosen);
        };
        let on_reset = move || self.set_width.call(None);
        view! {
            <div h_full flex_none flex translate_x={self.slide} opacity={self.fade}>
                <resizable_panel test_id="sidebar" edge=Side::Left size=width min_size=MIN_SIDEBAR_WIDTH
                                 max_size=MAX_SIDEBAR_WIDTH reset_size=DEFAULT_SIDEBAR_WIDTH
                                 on_resize=on_resize on_reset=on_reset h_full flex col bg=Role::Chrome>
                    {contents}
                </resizable_panel>
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
