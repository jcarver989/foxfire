use crate::agents::AgentProfile;
use crate::sessions::{SessionKey, SessionSource};
use crate::tasks::spawn_in;
use agent_client_protocol::v2::schema::{SessionId, SessionInfo};
use chrono::{DateTime, Utc};
use futures::future::join_all;
use rouge::reactive::{Scope, Signal, Stored, signal, spawn_tokio, stored};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const UNTITLED: &str = "New thread";

/// The sessions agents have saved, listed only when something asks for an agent's sessions.
#[derive(Clone, Copy)]
pub struct History {
    source: Stored<Arc<dyn SessionSource>>,
    entries: Signal<Vec<Entry>>, // Appended only: request batches keep entry indices.
    scope: Scope,
}

struct Entry {
    agent: AgentProfile,
    state: Listing,
    rows: Vec<SavedSession>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SavedSession {
    pub id: SessionId,
    pub agent: AgentProfile,
    pub updated_at: Option<DateTime<Utc>>,
    pub title: String,
}

/// How asking one agent for its sessions went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listing {
    Listing,
    Listed,
    Failed(String),
}

impl SavedSession {
    pub fn key(&self) -> SessionKey {
        SessionKey::new(&self.agent.name, &self.id)
    }
}

impl History {
    pub fn new(source: Arc<dyn SessionSource>) -> Self {
        Self { source: stored(source), entries: signal(Vec::new()), scope: Scope::new() }
    }

    /// Every session listed so far, newest first.
    pub fn sessions(&self) -> Vec<SavedSession> {
        let mut rows: Vec<_> =
            self.entries.with(|entries| entries.iter().flat_map(|entry| entry.rows.clone()).collect());
        sort_sessions(&mut rows);
        rows
    }

    pub(crate) fn with_listing<T>(
        &self,
        agent: &AgentProfile,
        read: impl FnOnce(Option<&Listing>, &[SavedSession]) -> T,
    ) -> T {
        self.entries.with(|entries| match entries.iter().find(|entry| entry.agent == *agent) {
            Some(entry) => read(Some(&entry.state), &entry.rows),
            None => read(None, &[]),
        })
    }

    /// How the last request for `agent` went, or `None` if it was never asked.
    pub fn listing(&self, agent: &AgentProfile) -> Option<Listing> {
        self.with_listing(agent, |state, _| state.cloned())
    }

    pub(crate) fn restorable(&self, key: &SessionKey) -> (bool, Option<SavedSession>) {
        self.entries.with(|entries| {
            let mut rows = entries.iter().flat_map(|entry| &entry.rows).peekable();
            (rows.peek().is_some(), rows.find(|session| session.key() == *key).cloned())
        })
    }

    /// Asks only profiles not already listing. Failed requests leave their old rows intact.
    pub fn list(&self, agents: &[AgentProfile]) {
        let entries = self.entries;
        let mut asking = Vec::new();
        entries.update(|entries| {
            for agent in agents {
                let index = entries.iter().position(|entry| entry.agent == *agent).unwrap_or(entries.len());
                match entries.get_mut(index) {
                    Some(entry) if entry.state == Listing::Listing => continue,
                    Some(entry) => entry.state = Listing::Listing,
                    None => entries.push(Entry { agent: agent.clone(), state: Listing::Listing, rows: Vec::new() }),
                }
                asking.push(index);
            }
        });
        if asking.is_empty() {
            return;
        }
        let requested: Vec<_> =
            entries.with_untracked(|entries| asking.iter().map(|&index| entries[index].agent.clone()).collect());
        let source = self.source.get();
        spawn_in(self.scope, async move {
            let answers = spawn_tokio(async move { join_all(requested.iter().map(|agent| source.list(agent))).await })
                .await
                .map_or_else(
                    |error| vec![Err(error.to_string()); asking.len()],
                    |answers| answers.into_iter().map(|answer| answer.map_err(|error| error.to_string())).collect(),
                );
            if entries.is_alive() {
                entries.update(|entries| record(entries, asking.into_iter().zip(answers)));
            }
        });
    }

    /// Re-asks every previously requested profile.
    pub fn refresh(&self) {
        let agents: Vec<_> =
            self.entries.with_untracked(|entries| entries.iter().map(|entry| entry.agent.clone()).collect());
        self.list(&agents);
    }
}

fn record(entries: &mut [Entry], answers: impl Iterator<Item = (usize, Result<Vec<SessionInfo>, String>)>) {
    let mut listed = Vec::new();
    for (index, answer) in answers {
        let entry = &mut entries[index];
        entry.state = match answer {
            Ok(rows) => {
                entry.rows.clear();
                listed.push((index, rows));
                Listing::Listed
            }
            Err(error) => {
                tracing::warn!(%error, agent = entry.agent.name, "could not list an agent's sessions");
                Listing::Failed(error)
            }
        };
    }
    // Existing rows win duplicate keys, then new rows in request order.
    let mut seen: HashSet<_> = entries.iter().flat_map(|entry| entry.rows.iter().map(SavedSession::key)).collect();
    for (index, rows) in listed {
        let entry = &mut entries[index];
        entry.rows.extend(rows.into_iter().filter_map(|info| {
            seen.insert(SessionKey::new(&entry.agent.name, &info.session_id))
                .then(|| SavedSession::new(info, entry.agent.clone()))
        }));
        sort_sessions(&mut entry.rows);
    }
}

pub fn git_root(dir: &Path) -> PathBuf {
    dir.ancestors().find(|ancestor| ancestor.join(".git").exists()).unwrap_or(dir).to_path_buf()
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

fn sort_sessions(sessions: &mut [SavedSession]) {
    sessions.sort_by(|a, b| {
        b.updated_at.cmp(&a.updated_at).then_with(|| b.id.0.cmp(&a.id.0)).then_with(|| a.agent.name.cmp(&b.agent.name))
    });
}
