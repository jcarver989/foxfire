use agent_client_protocol::v2::schema::SessionId;
use clankerdiff_ui_core::ThemePreference;
use rouge::persist::Storage;
use rouge::reactive::{Signal, effect, signal, spawn};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// The key everything the desktop app remembers is saved under, in `state.json` for file storage.
pub const STATE_KEY: &str = "state";
pub const STATE_FILE: &str = "state.json";

/// What the app remembers between launches, saved to the window's storage as it changes.
#[derive(Clone, Copy)]
pub struct Registry {
    state: Signal<State>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub projects: Vec<PathBuf>,
    pub collapsed: HashSet<Collapsed>,
    pub last_agents: HashMap<PathBuf, String>,
    pub archived: HashSet<SessionId>,
    pub titles: HashMap<SessionId, String>,
    pub window_size: Option<WindowSize>,
    pub last_selected: Option<SessionId>,
    pub interface_size: Option<f32>,
    pub sidebar_width: Option<f32>,
    pub sidebar_collapsed: bool,
    pub theme: ThemePreference,
    pub reduced_motion: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Collapsed {
    Project(PathBuf),
    Section(String),
    Remote(String),
}

pub fn default_state_dir() -> Option<PathBuf> {
    utils::path::aether_home().map(|home| home.join("desktop"))
}

/// The state saved as `json`. Unreadable state is forgotten.
pub fn saved(json: Option<&str>) -> State {
    json.and_then(|json| serde_json::from_str(json).ok()).unwrap_or_default()
}

/// The state saved to file storage in `dir`, read before the window opens.
pub fn saved_in(dir: &Path) -> State {
    saved(fs::read_to_string(dir.join(STATE_FILE)).ok().as_deref())
}

impl Registry {
    /// Starts from `saved` and writes every change to `storage`.
    ///
    /// The state is read up front rather than through `rouge::persist::persisted`, whose load lands after the
    /// app starts and would overwrite what it records on launch, like the last selected thread.
    pub fn restore(saved: State, storage: Rc<dyn Storage>) -> Self {
        let state = signal(saved);
        let first = Cell::new(true);
        effect(move || {
            let json = state.with(serde_json::to_string_pretty);
            if first.replace(false) {
                return;
            }
            match json {
                Ok(json) => drop(spawn(storage.write(STATE_KEY, json))),
                Err(error) => tracing::warn!(%error, "failed to serialize the desktop state"),
            }
        });
        Self { state }
    }

    pub fn with<T>(&self, read: impl FnOnce(&State) -> T) -> T {
        self.state.with(read)
    }

    pub fn with_untracked<T>(&self, read: impl FnOnce(&State) -> T) -> T {
        self.state.with_untracked(read)
    }

    pub fn get(&self) -> State {
        self.state.get()
    }

    pub fn update(&self, change: impl FnOnce(&mut State)) {
        let mut next = self.state.get_untracked();
        change(&mut next);
        self.state.set_if_changed(next);
    }
}

impl State {
    pub fn add_project(&mut self, root: PathBuf) {
        if !self.projects.contains(&root) {
            self.projects.push(root);
        }
    }

    pub fn archive(&mut self, session_id: SessionId) {
        self.archived.insert(session_id);
    }

    pub fn toggle_collapsed(&mut self, group: Collapsed) {
        if !self.collapsed.remove(&group) {
            self.collapsed.insert(group);
        }
    }

    pub fn title(&self, session_id: Option<&SessionId>, derived: String) -> String {
        session_id.and_then(|id| self.titles.get(id)).cloned().unwrap_or(derived)
    }

    pub fn rename(&mut self, session_id: SessionId, title: &str) {
        match title.trim() {
            "" => self.titles.remove(&session_id),
            title => self.titles.insert(session_id, title.to_owned()),
        };
    }
}

impl WindowSize {
    #[must_use]
    pub fn at_least(self, minimum: Self) -> Self {
        Self { width: self.width.max(minimum.width), height: self.height.max(minimum.height) }
    }
}
