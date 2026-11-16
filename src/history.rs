use crate::agents::{AgentProfile, Catalogs};
use crate::registry::Registry;
use crate::sessions::{SessionKey, SessionSource};
use crate::target::ConnectError;
use crate::tasks::spawn_in;
use agent_client_protocol::v2::schema::{SessionId, SessionInfo};
use chrono::{DateTime, Utc};
use futures::future::join_all;
use rouge::reactive::{Scope, Signal, Stored, effect, memo, signal, spawn_tokio, stored, untrack};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const UNTITLED: &str = "New thread";

#[derive(Clone, Copy)]
pub struct History {
    source: Stored<Arc<dyn SessionSource>>,
    registry: Registry,
    agents: Catalogs,
    sessions: Signal<Vec<SavedSession>>,
    listed: Signal<bool>,
    refreshing: Stored<Refresh>,
    scope: Scope,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SavedSession {
    pub id: SessionId,
    pub agent: AgentProfile,
    pub updated_at: Option<DateTime<Utc>>,
    pub title: String,
}

impl SavedSession {
    pub fn key(&self) -> SessionKey {
        SessionKey::new(&self.agent.name, &self.id)
    }
}

impl History {
    pub fn new(source: Arc<dyn SessionSource>, registry: Registry, agents: Catalogs) -> Self {
        let history = Self {
            source: stored(source),
            registry,
            agents,
            sessions: signal(Vec::new()),
            listed: signal(false),
            refreshing: stored(Refresh::Idle),
            scope: Scope::new(),
        };
        let roots = memo(move || registry.with(|state| state.projects.clone()));
        effect(move || {
            roots.with(|_| ());
            agents.revision();
            untrack(|| history.refresh());
        });
        history
    }

    pub fn sessions(&self) -> Vec<SavedSession> {
        self.sessions.get()
    }

    pub fn with_sessions<T>(&self, read: impl FnOnce(&[SavedSession]) -> T) -> T {
        self.sessions.with(|sessions| read(sessions))
    }

    pub fn has_listed(&self) -> bool {
        self.listed.get()
    }

    pub fn refresh(&self) {
        let busy = self.refreshing.update(|refreshing| match refreshing {
            Refresh::Idle => {
                *refreshing = Refresh::Running { again: false };
                false
            }
            Refresh::Running { again } => {
                *again = true;
                true
            }
        });
        if busy {
            return;
        }
        let history = *self;
        let (source, agents) = (self.source.get(), self.listed_agents());
        spawn_in(self.scope, async move {
            let listed = spawn_tokio(async move {
                let listings = join_all(agents.iter().map(|agent| source.list(agent))).await;
                saved_sessions(agents, listings)
            })
            .await;
            if !history.sessions.is_alive() {
                return;
            }
            if let Ok(sessions) = listed {
                history.sessions.set_if_changed(sessions);
                history.listed.set_if_changed(true);
            }
            let again = history.refreshing.update(|refreshing| {
                let again = matches!(refreshing, Refresh::Running { again: true });
                *refreshing = Refresh::Idle;
                again
            });
            if again {
                history.refresh();
            }
        });
    }

    fn listed_agents(&self) -> Vec<AgentProfile> {
        let roots = self.registry.with_untracked(|state| state.projects.clone());
        roots.iter().flat_map(|root| untrack(|| self.agents.catalog(root)).agents().to_vec()).collect()
    }
}

pub fn git_root(dir: &Path) -> PathBuf {
    dir.ancestors().find(|ancestor| ancestor.join(".git").exists()).unwrap_or(dir).to_path_buf()
}

enum Refresh {
    Idle,
    Running { again: bool },
}

impl SavedSession {
    fn new(info: SessionInfo, agent: AgentProfile) -> Self {
        Self {
            id: info.session_id,
            agent,
            updated_at: info
                .updated_at
                .and_then(|time| DateTime::parse_from_rfc3339(&time).ok())
                .map(|time| time.to_utc()),
            title: info.title.filter(|title| !title.trim().is_empty()).unwrap_or_else(|| UNTITLED.to_owned()),
        }
    }
}

fn saved_sessions(
    agents: Vec<AgentProfile>,
    listings: Vec<Result<Vec<SessionInfo>, ConnectError>>,
) -> Vec<SavedSession> {
    let mut seen = HashSet::new();
    let mut sessions: Vec<SavedSession> = agents
        .into_iter()
        .zip(listings)
        .flat_map(|(agent, listed)| {
            let listed = listed.inspect_err(|error| tracing::warn!(%error, agent = agent.name, "leaving out an agent"));
            listed.into_iter().flatten().map(move |info| SavedSession::new(info, agent.clone()))
        })
        .filter(|session| seen.insert(session.key()))
        .collect();
    sessions.sort_by(|a, b| {
        b.updated_at.cmp(&a.updated_at).then_with(|| b.id.0.cmp(&a.id.0)).then_with(|| a.agent.name.cmp(&b.agent.name))
    });
    sessions
}
