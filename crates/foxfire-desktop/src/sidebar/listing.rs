use super::row::{RowAction, RowIcon};
use crate::registry::{Collapsed, State};
use crate::remotes::{
    EXPIRY_WARNING, Fleet, Instance, InstanceState, Lifecycle, RemoteSpec, short_id, time_left, time_left_label,
};
use crate::sessions::SessionKey;
use crate::threads::{Thread, ThreadManager, ThreadStatus};
use chrono::{DateTime, Utc};
use clankerdiff_core::format_age;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub(super) struct Listing<'a> {
    pub(super) state: &'a State,
    pub(super) threads: ThreadManager,
    pub(super) fleet: Fleet,
    pub(super) renaming: Option<&'a SessionKey>,
    pub(super) now: DateTime<Utc>,
}

#[derive(Clone, PartialEq)]
pub(super) struct Section {
    pub(super) root: PathBuf,
    pub(super) collapsed: bool,
    pub(super) rows: Vec<Row>,
}

#[derive(Clone, PartialEq)]
pub(super) struct RemoteSection {
    pub(super) remote: RemoteSpec,
    pub(super) collapsed: bool,
    pub(super) problem: Option<String>,
    pub(super) rows: Vec<Row>,
}

#[derive(Clone, PartialEq)]
pub(super) struct Row {
    pub(super) key: RowKey,
    pub(super) thread: Option<Thread>,
    pub(super) title: String,
    pub(super) icon: RowIcon,
    pub(super) unread: bool,
    pub(super) selected: bool,
    pub(super) actions: Vec<RowAction>,
    pub(super) kind: RowKind,
}

#[derive(Clone, PartialEq)]
pub(super) enum RowKind {
    Local { session_id: Option<SessionKey>, age: Option<String>, editing: bool },
    Cloud { remote: Box<RemoteSpec>, instance_id: Option<String>, subtitle: String, expiring: bool },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RowKey {
    Live(usize),
    Instance(String),
}

#[derive(Clone)]
pub(crate) struct SwitcherEntry {
    pub(crate) key: RowKey,
    pub(crate) title: String,
    pub(crate) project: String,
    pub(crate) thread: Thread,
}

impl Listing<'_> {
    pub(super) fn roots(&self) -> Vec<&Path> {
        self.state.projects.iter().map(PathBuf::as_path).collect()
    }

    pub(super) fn sections(&self) -> Vec<Section> {
        let live = self.live();
        self.roots()
            .into_iter()
            .map(|root| {
                let collapsed = self.state.collapsed.contains(&Collapsed::Project(root.to_path_buf()));
                let rows = if collapsed {
                    Vec::new()
                } else {
                    live.iter()
                        .filter(|thread| thread.project().as_deref() == Some(root))
                        .map(|thread| self.live_row(*thread))
                        .collect()
                };
                Section { root: root.to_path_buf(), collapsed, rows }
            })
            .collect()
    }

    pub(super) fn remotes(&self) -> Vec<RemoteSection> {
        self.fleet
            .with_inventory(|inventory| {
                inventory
                    .remotes()
                    .iter()
                    .map(|remote| {
                        let collapsed = self.state.collapsed.contains(&Collapsed::Remote(remote.name.clone()));
                        let problem = inventory.problem(&remote.name).map(str::to_owned);
                        (remote.clone(), collapsed, problem)
                    })
                    .collect::<Vec<_>>()
            })
            .into_iter()
            .map(|(remote, collapsed, problem)| {
                let rows = if collapsed { Vec::new() } else { self.cloud_rows(&remote) };
                RemoteSection { remote, collapsed, problem, rows }
            })
            .collect()
    }

    pub(super) fn switcher_entries(&self, live_order: &[Thread]) -> Vec<SwitcherEntry> {
        live_order
            .iter()
            .map(|thread| {
                let target = thread.target();
                let project =
                    target.remote_name().map(str::to_owned).or_else(|| thread.project().as_deref().map(project_name));
                SwitcherEntry {
                    key: RowKey::Live(thread.number()),
                    title: self.state.title(thread.session_key().as_ref(), thread.title()),
                    project: project.unwrap_or_default(),
                    thread: *thread,
                }
            })
            .collect()
    }

    fn cloud_rows(&self, remote: &RemoteSpec) -> Vec<Row> {
        let listed = self.fleet.instances(&remote.name);
        let on_remote: Vec<Thread> = self
            .live()
            .into_iter()
            .filter(|thread| thread.target().remote_name() == Some(remote.name.as_str()))
            .collect();
        let threads = on_remote.iter().map(|thread| {
            let target = thread.target();
            let instance_id = target.instance();
            let instance = listed.iter().find(|instance| Some(instance.id.as_str()) == instance_id);
            let key = RowKey::Live(thread.number());
            let title = self.state.title(thread.session_key().as_ref(), thread.title());
            self.cloud_row(key, Some(*thread), title, remote, instance_id, instance)
        });
        let unattached = listed
            .iter()
            .filter(|_| !remote.is_static())
            .filter(|instance| on_remote.iter().all(|thread| thread.target().instance() != Some(instance.id.as_str())))
            .map(|instance| {
                let (key, title) = (RowKey::Instance(instance.id.clone()), short_id(&instance.id).to_owned());
                self.cloud_row(key, None, title, remote, Some(&instance.id), Some(instance))
            });
        threads.collect::<Vec<_>>().into_iter().chain(unattached).collect()
    }

    fn cloud_row(
        &self,
        key: RowKey,
        thread: Option<Thread>,
        title: String,
        remote: &RemoteSpec,
        instance_id: Option<&str>,
        instance: Option<&Instance>,
    ) -> Row {
        let status = thread.map(|thread| thread.status());
        let state = instance.map(|instance| instance.state);
        let stop_instance = instance_id.is_some() && remote.can_change(Lifecycle::Stop);
        let mut actions: Vec<RowAction> =
            thread.filter(|_| !stop_instance).map(|_| RowAction::Stop).into_iter().collect();
        match state {
            Some(InstanceState::Running) if remote.can_change(Lifecycle::Suspend) => actions.push(RowAction::Suspend),
            Some(InstanceState::Suspended) if remote.can_change(Lifecycle::Resume) => actions.push(RowAction::Resume),
            _ => {}
        }
        actions.extend(stop_instance.then_some(RowAction::StopInstance));
        let left = instance.and_then(|instance| time_left(instance, SystemTime::from(self.now)));
        let state_label = match (state, left) {
            (_, Some(left)) => Some(format!("{} left", time_left_label(left))),
            (Some(state), None) => Some(state.label().to_owned()),
            (None, None) => None,
        };
        let subtitle = match (instance_id, thread) {
            (None, _) if status == Some(ThreadStatus::Connecting) => "Launching…".to_owned(),
            (None, _) => "Not launched".to_owned(),
            (Some(_), _) if remote.is_static() => remote.name.clone(),
            (Some(_), None) => state_label.unwrap_or_default(),
            (Some(id), Some(_)) => {
                std::iter::once(short_id(id).to_owned()).chain(state_label).collect::<Vec<_>>().join(" · ")
            }
        };
        let (unread, selected) = self.attention(thread);
        Row {
            key,
            thread,
            title,
            icon: status.map_or(RowIcon::Instance(state), RowIcon::Status),
            unread,
            selected,
            actions,
            kind: RowKind::Cloud {
                remote: Box::new(remote.clone()),
                instance_id: instance_id.map(str::to_owned),
                subtitle,
                expiring: left.is_some_and(|left| left < EXPIRY_WARNING),
            },
        }
    }

    fn live_row(&self, thread: Thread) -> Row {
        let session_id = thread.session_key();
        let mut actions = Vec::new();
        if session_id.is_some() {
            actions.extend([RowAction::Rename, RowAction::Archive]);
        }
        actions.push(RowAction::Stop);
        let editing = session_id.is_some() && self.renaming == session_id.as_ref();
        let (unread, selected) = self.attention(Some(thread));
        Row {
            key: RowKey::Live(thread.number()),
            title: self.state.title(session_id.as_ref(), thread.title()),
            icon: RowIcon::Status(thread.status()),
            unread,
            selected,
            actions,
            kind: RowKind::Local { session_id, age: Some(relative_time(thread.opened_at(), self.now)), editing },
            thread: Some(thread),
        }
    }

    fn attention(&self, thread: Option<Thread>) -> (bool, bool) {
        thread
            .map_or((false, false), |thread| (self.threads.is_unread(thread), self.threads.selected() == Some(thread)))
    }

    fn live(&self) -> Vec<Thread> {
        self.threads.threads()
    }
}

impl RowKey {
    pub(crate) fn selector(&self) -> String {
        match self {
            Self::Live(number) => format!("thread-row-{number}"),
            Self::Instance(id) => format!("instance-row-{id}"),
        }
    }
}

pub(crate) fn project_name(root: &Path) -> String {
    root.file_name().map_or_else(|| root.display().to_string(), |name| name.to_string_lossy().into_owned())
}

fn relative_time(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    format_age(u64::try_from(now.signed_duration_since(then).num_seconds()).unwrap_or(0))
}
