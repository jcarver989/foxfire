use super::Sidebar;
use super::listing::{Row, RowKind};
use crate::remotes::{InstanceState, Lifecycle};
use crate::threads::ThreadStatus;
use crate::ui::tokens::{ICON_SIZE, ROW_HEIGHT, Radius, TWO_LINE_ROW_HEIGHT, UNREAD_DOT_SIZE};
use crate::ui::{Icon, IconName, SelectionState, UiStyles, WispState, hint, icon_button, icon_slot, list_row, wisp};
use rouge::core::{
    AnyView, Div, Key, MenuItem as WindowMenuItem, Role, Styled, Theme, View, div, editable, text, view,
};
use rouge::reactive::{Signal, signal};
use rouge::widgets::context_menu;

#[derive(Clone, PartialEq)]
pub(super) enum RowIcon {
    Status(ThreadStatus),
    Instance(Option<InstanceState>),
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum RowAction {
    Rename,
    Archive,
    Stop,
    Suspend,
    Resume,
    StopInstance,
}

pub(super) fn row(sidebar: &Sidebar, row: &Row) -> AnyView {
    let sidebar = *sidebar;
    let selector = row.key.selector();
    let hovered = signal(false);
    let editing = matches!(row.kind, RowKind::Local { editing: true, .. });
    let hover_actions: Vec<AnyView> = row
        .actions
        .iter()
        .filter(|action| action.hover())
        .map(|&action| {
            let (id, label, icon) = action.spec();
            let target = row.clone();
            view! {
                <icon_button test_id={format!("{selector}-{id}")} icon label on_click={move || action.run(&target, &sidebar)}/>
            }
            .into_any()
        })
        .collect();
    let reconnect = row
        .live()
        .filter(|_| matches!(row.icon, RowIcon::Status(status) if status.problem().is_some()))
        .map(|thread| {
            view! {
                <icon_button test_id={format!("{selector}-reconnect")} icon=IconName::RotateCw label="Reconnect"
                             on_click={move || thread.reconnect()}/>
            }
        });
    let title = if editing { rename_editor(&sidebar) } else { title(&selector, row) };
    let unread = row.unread.then(|| {
        let dot = view! { <div size=UNREAD_DOT_SIZE rounded=Radius::Full bg=Role::Accent/> };
        hint(format!("{selector}-unread"), "Unread response", dot)
    });
    let title = view! { <div min_w=0.0 flex items_center gap=4.0>{title}{unread}</div> };
    let icon = match row.icon {
        RowIcon::Status(status) => status_icon(&selector, status),
        RowIcon::Instance(state) => instance_icon(&selector, state),
    };
    let two_line = matches!(row.kind, RowKind::Cloud { .. });
    let (body, trailing) = match &row.kind {
        RowKind::Local { age, .. } => {
            (title.flex_1(), trailing(&selector, age.clone(), hover_actions, row.selected, hovered).child(reconnect))
        }
        RowKind::Cloud { subtitle, expiring, .. } => {
            let color = if *expiring { Role::Warning } else { Role::TextMuted };
            let body = view! {
                <div flex_1 min_w=0.0 py=4.0 flex col>
                    {title}
                    <div test_id={format!("{selector}-subtitle")} min_w=0.0 overflow_hidden>
                        <text caption no_wrap color=color>{subtitle.clone()}</text>
                    </div>
                </div>
            };
            let actions = view! {
                <div flex_none flex items_center>
                    {reconnect}
                    <div flex items_center opacity={move || if hovered.get() { 1.0 } else { 0.0 }}>
                        {..hover_actions}
                    </div>
                </div>
            };
            (body, actions)
        }
    };
    let state = if row.selected { SelectionState::Selected } else { SelectionState::None };
    let opened = row.clone();
    let open = move || {
        if !editing {
            open_row(&opened, &sidebar);
        }
    };
    let entry = view! {
        <list_row test_id={selector.clone()} depth={usize::from(!two_line)} selection=state on_click=open
                  h={if two_line { TWO_LINE_ROW_HEIGHT } else { ROW_HEIGHT }}>
            {icon_slot()}
            {icon_slot().when(two_line, |slot| slot.self_start().mt(8.0)).child(icon)}
            {body}
            {trailing}
        </list_row>
    };
    let list = div().w_full().child(entry).on_hover(move |inside| {
        hovered.set_if_changed(inside);
    });
    if editing {
        return list.into_any();
    }
    let items: Vec<WindowMenuItem> = row
        .actions
        .iter()
        .map(|&action| {
            let target = row.clone();
            WindowMenuItem::callback(action.spec().1, move || action.run(&target, &sidebar))
        })
        .collect();
    context_menu(list, items).w_full().into_any()
}

impl RowAction {
    fn spec(self) -> (&'static str, &'static str, IconName) {
        match self {
            Self::Rename => ("rename", "Rename", IconName::Pencil),
            Self::Archive => ("archive", "Archive", IconName::Archive),
            Self::Stop => ("stop", "Stop thread", IconName::Square),
            Self::Suspend => ("suspend", "Suspend", IconName::Pause),
            Self::Resume => ("resume", "Resume", IconName::Play),
            Self::StopInstance => ("stop-instance", "Stop instance", IconName::Power),
        }
    }

    fn hover(self) -> bool {
        matches!(self, Self::Archive | Self::Stop | Self::StopInstance)
    }

    fn run(self, row: &Row, sidebar: &Sidebar) {
        let thread = row.live();
        let stop = || {
            if let Some(thread) = thread {
                sidebar.threads.stop(thread);
            }
        };
        match (self, &row.kind) {
            (Self::Rename, RowKind::Local { session_id: Some(session_id), .. }) => {
                sidebar.start_rename(session_id.clone(), &row.title);
            }
            (Self::Archive, RowKind::Local { session_id: Some(session_id), .. }) => {
                sidebar.registry.update(|state| state.archive(session_id.clone()));
                stop();
            }
            (Self::Stop, _) => stop(),
            (
                Self::Suspend | Self::Resume | Self::StopInstance,
                RowKind::Cloud { remote, instance_id: Some(id), .. },
            ) => {
                let change = match self {
                    Self::Suspend => Lifecycle::Suspend,
                    Self::Resume => Lifecycle::Resume,
                    _ => {
                        stop();
                        Lifecycle::Stop
                    }
                };
                sidebar.fleet.change(&remote.name, id, change);
            }
            _ => {}
        }
    }
}

fn open_row(row: &Row, sidebar: &Sidebar) {
    match (&row.thread, &row.kind) {
        (Some(thread), _) => sidebar.threads.open(thread),
        (None, RowKind::Cloud { remote, instance_id: Some(id), .. }) => {
            sidebar.threads.attach(remote, id);
        }
        (None, _) => {}
    }
}

fn title(selector: &str, row: &Row) -> AnyView {
    let muted = matches!(row.icon, RowIcon::Status(ThreadStatus::Dormant) | RowIcon::Instance(_));
    let color = if muted { Role::TextMuted } else { Role::Text };
    view! {
        <div test_id={format!("{selector}-title")} min_w=0.0 overflow_hidden>
            <text body no_wrap color=color>{row.title.clone()}</text>
        </div>
    }
    .into_any()
}

fn rename_editor(sidebar: &Sidebar) -> AnyView {
    let sidebar = *sidebar;
    let Some(value) = sidebar.sources.rename.with_untracked(|rename| rename.as_ref().map(|rename| rename.value)) else {
        return ().into_any();
    };
    let accent = Theme::current().get_untracked().accent;
    let field = editable(value)
        .autofocus()
        .select_all_on_focus()
        .caret_color(accent)
        .selection_color(accent.with_alpha(0.3))
        .w_full()
        .on_blur(move |_| sidebar.commit_rename())
        .on_submit(move |_| sidebar.commit_rename());
    div()
        .test_id("rename-input")
        .w_full()
        .min_w(0.0)
        .px(4.0)
        .rounded(Radius::Inner)
        .border(1.0)
        .border_color(Role::Accent)
        .bg(Role::Canvas)
        .body()
        .text_color(Role::Text)
        .on_key_down(move |event| {
            if event.key == Key::Escape {
                event.stop_propagation();
                sidebar.cancel_rename();
            }
        })
        .child(field)
        .into_any()
}

fn trailing(selector: &str, age: Option<String>, actions: Vec<AnyView>, selected: bool, hovered: Signal<bool>) -> Div {
    let age = age.map(|age| {
        view! {
            <div test_id={format!("{selector}-age-{age}")} opacity={move || if hovered.get() { 0.0 } else { 1.0 }}>
                <text caption muted no_wrap>{age}</text>
            </div>
        }
    });
    let background = if selected { Role::SurfaceSelected } else { Role::SurfaceHover };
    view! {
        <div flex_none relative flex items_center justify_end>
            {age}
            <div absolute right=0.0 top=0.0 bottom=0.0 pl=4.0 flex items_center bg=background
                 opacity={move || if hovered.get() { 1.0 } else { 0.0 }}>
                {..actions}
            </div>
        </div>
    }
}

fn status_icon(row: &str, status: ThreadStatus) -> AnyView {
    let (name, label, shape, icon, color) = match status {
        ThreadStatus::Connecting => ("connecting", "Starting agent", "wisp", None, Role::TextMuted),
        ThreadStatus::Running => ("running", "Agent working", "wisp", None, Role::Info),
        ThreadStatus::Background => {
            ("background", "Background task still running", "dot", Some(IconName::CircleDot), Role::TextMuted)
        }
        ThreadStatus::NeedsInput => {
            ("needs-input", "Waiting for your answer", "speech-bubble", Some(IconName::MessageCircle), Role::Warning)
        }
        ThreadStatus::Errored => (
            "errored",
            "Unable to connect — reconnect to try again",
            "warning-triangle",
            Some(IconName::TriangleAlert),
            Role::Destructive,
        ),
        ThreadStatus::Disconnected => (
            "disconnected",
            "Disconnected — reconnect to continue",
            "warning-triangle",
            Some(IconName::TriangleAlert),
            Role::Destructive,
        ),
        ThreadStatus::Idle | ThreadStatus::Dormant => {
            return tagged(format!("{row}-conversation-icon"), Icon::new(IconName::MessageSquareText, Role::TextMuted));
        }
    };
    let id = format!("{row}-status-{name}");
    let mark = match icon {
        Some(icon) => Icon::new(icon, color).into_any(),
        None if status == ThreadStatus::Running => wisp(ICON_SIZE, color, || WispState::Working).into_any(),
        None => wisp(ICON_SIZE, color, || WispState::Connecting).into_any(),
    };
    hint(id.clone(), label, tagged(format!("{id}-{shape}"), mark))
}

fn instance_icon(row: &str, state: Option<InstanceState>) -> AnyView {
    let muted = Role::TextMuted;
    let (name, label, icon) = match state {
        Some(InstanceState::Suspended) => ("suspended", "Suspended", Some(IconName::Pause)),
        Some(InstanceState::Running) | None => ("unattached", "Running, with no thread on it", Some(IconName::Unplug)),
        Some(_) => ("settling", "Changing state", None),
    };
    let id = format!("{row}-instance-{name}");
    let mark = match icon {
        Some(icon) => Icon::new(icon, muted).into_any(),
        None => wisp(ICON_SIZE, muted, || WispState::Connecting).into_any(),
    };
    hint(id.clone(), label, tagged(format!("{id}-icon"), mark))
}

fn tagged(id: String, child: impl View) -> AnyView {
    div().test_id(id).flex().child(child).into_any()
}
