use crate::history::git_root;
use crate::sessions::SessionKey;
use crate::target::{AgentConnector, SessionTarget};
use agent_client_protocol::v2::schema::SessionId;
use chrono::{DateTime, Utc};
use foxfire_shell::UNTITLED;
use foxfire_ui::thread::Thread as ChatThread;
pub use foxfire_ui::thread::{ThreadEvent, ThreadStatus};
use rouge::core::window_focused;
use rouge::reactive::{Signal, Stored, signal, stored};
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub struct Thread {
    chat: ChatThread,
    target: Signal<SessionTarget>,
    project: Stored<Option<PathBuf>>,
    opened_at: DateTime<Utc>,
}

impl Thread {
    pub(crate) fn connect(
        number: usize,
        target: SessionTarget,
        session_id: Option<SessionId>,
        connector: Arc<AgentConnector>,
        opened_at: DateTime<Utc>,
    ) -> Self {
        let cwd = target.cwd().map(ToOwned::to_owned);
        let project = cwd.as_deref().map(git_root);
        let outlives_connections = target.outlives_connections();
        let target = signal(target);
        let chat = ChatThread::connect(number, cwd, session_id, move |resume| {
            super::driver::open(number, connector.clone(), target.get_untracked(), resume)
        });
        if outlives_connections {
            let focused = window_focused();
            chat.keep_connected(move || focused.get());
        }
        chat.on_event(move |event| {
            if let ThreadEvent::Launched(id) = event {
                target.update(|target| target.set_instance(id));
            }
        });
        Self { chat, target, project: stored(project), opened_at }
    }

    pub fn number(&self) -> usize {
        self.chat.number()
    }

    pub fn chat(&self) -> ChatThread {
        self.chat
    }

    pub fn title(&self) -> String {
        self.chat.title().unwrap_or_else(|| UNTITLED.to_owned())
    }

    pub fn session_key(&self) -> Option<SessionKey> {
        let id = self.session_id()?;
        self.target.with(|target| Some(SessionKey::new(target.label(), &id)))
    }

    pub fn target(&self) -> SessionTarget {
        self.target.get()
    }

    pub fn project(&self) -> Option<PathBuf> {
        self.project.get()
    }

    pub fn opened_at(&self) -> DateTime<Utc> {
        self.opened_at
    }
}

impl Deref for Thread {
    type Target = ChatThread;

    fn deref(&self) -> &Self::Target {
        &self.chat
    }
}

impl PartialEq for Thread {
    fn eq(&self, other: &Self) -> bool {
        self.chat == other.chat
    }
}

impl Eq for Thread {}

impl Hash for Thread {
    fn hash<T: Hasher>(&self, state: &mut T) {
        self.chat.hash(state);
    }
}
