use super::Sidebar;
use super::listing::{Row, RowKind};
use crate::remotes::{InstanceState, Lifecycle};
use crate::threads::ThreadStatus;
use crate::ui::tokens::{ICON_SIZE, Radius, UNREAD_DOT_SIZE};
use crate::ui::{
    FoxfireState, Icon, IconName, UiStyles, foxfire, hint, icon_button, inline_edit, people_avatars, row_action,
    sidebar_thread_row, thread_status_icon,
};
use rouge::core::{AnyView, MenuItem as WindowMenuItem, Role, Styled, View, div, text, view};
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
    let editing = matches!(row.kind, RowKind::Local { editing: true, .. });
    let reconnect = row
        .thread
        .filter(|_| matches!(row.icon, RowIcon::Status(status) if status.problem().is_some()))
        .map(|thread| {
            view! {
                <icon_button test_id={format!("{selector}-reconnect")} icon=IconName::RotateCw label="Reconnect"
                             on_click={move || thread.reconnect()}/>
            }
        });
    let title = if editing { rename_editor(&sidebar, &row.title) } else { title(&selector, row) };
    let unread = row.unread.then(|| {
        let dot = view! { <div size=UNREAD_DOT_SIZE rounded=Radius::Full bg=Role::Accent/> };
        hint(format!("{selector}-unread"), "Unread response", dot)
    });
    let title: Vec<_> = std::iter::once(title).chain(unread).collect();
    let icon = match row.icon {
        RowIcon::Status(status) => thread_status_icon(&selector, status),
        RowIcon::Instance(state) => instance_icon(&selector, state),
    };
    let (subtitle, age) = match &row.kind {
        RowKind::Local { age, .. } => (None, age.clone()),
        RowKind::Cloud { subtitle, expiring, .. } => {
            let color = if *expiring { Role::Warning } else { Role::TextMuted };
            (Some((subtitle.clone(), color)), None)
        }
    };
    let selected = row.selected;
    let opened = row.clone();
    let open = move || {
        if !editing {
            open_row(&opened, &sidebar);
        }
    };
    let entry = sidebar_thread_row(selector.clone(), move || selected, title, icon, subtitle, open).child(reconnect);
    let entry = match age {
        _ if !row.present.is_empty() => {
            entry.trailing(div().test_id(format!("{selector}-people")).child(people_avatars(&row.present)))
        }
        Some(age) => entry.trailing(age_label(&selector, age)),
        None => entry,
    };
    let entry = row.actions.iter().filter(|action| action.hover()).fold(entry, |entry, &action| {
        let (id, label, icon) = action.spec();
        let target = row.clone();
        let run = move || action.run(&target, &sidebar);
        entry.action(row_action(label, run).icon(icon).icon_only().test_id(format!("{selector}-{id}")))
    });
    let list = div().w_full().child(entry);
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
        let thread = row.thread;
        let stop = || {
            if let Some(thread) = thread {
                sidebar.threads.stop(thread);
            }
        };
        match (self, &row.kind) {
            (Self::Rename, RowKind::Local { session_id: Some(session_id), .. }) => {
                sidebar.start_rename(session_id.clone());
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
        (Some(thread), _) => sidebar.threads.select(*thread),
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

fn rename_editor(sidebar: &Sidebar, title: &str) -> AnyView {
    let (sidebar, title) = (*sidebar, title.to_owned());
    view! {
        <inline_edit test_id="rename-input" value=title begin_editing
                     on_commit={move |title: String| sidebar.commit_rename(&title)}
                     on_cancel={move || sidebar.cancel_rename()}/>
    }
    .into_any()
}

fn age_label(selector: &str, age: String) -> AnyView {
    view! { <div test_id={format!("{selector}-age-{age}")}><text caption muted no_wrap>{age}</text></div> }.into_any()
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
        None => foxfire(ICON_SIZE, muted, || FoxfireState::Connecting).into_any(),
    };
    hint(id.clone(), label, tagged(format!("{id}-icon"), mark))
}

fn tagged(id: String, child: impl View) -> AnyView {
    div().test_id(id).flex().child(child).into_any()
}
