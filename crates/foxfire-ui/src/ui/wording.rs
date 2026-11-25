use crate::thread::ThreadStatus;
use rouge::reactive::{provide_context, use_context};

/// What the shared views say about a thread and its agent, which a host can put in its own words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wording {
    /// A thread that has not been prompted.
    pub untitled: &'static str,
    /// Asks for the first prompt, where there is no composer to.
    pub unprompted: &'static str,
    pub connecting: &'static str,
    pub connecting_detail: &'static str,
    pub stopped: &'static str,
    pub stopped_detail: &'static str,
    pub cannot_start: &'static str,
    pub reconnecting: &'static str,
    pub disconnected: &'static str,
    pub disconnected_detail: &'static str,
    pub reconnect: &'static str,
    pub placeholder: &'static str,
    pub files_waiting: &'static str,
    pub files_stopped: &'static str,
    pub files_stopped_detail: &'static str,
    pub room: &'static str,
    /// What an empty room says about who reads it.
    pub room_detail: &'static str,
}

impl Wording {
    pub fn provide(self) {
        provide_context(self);
    }

    #[must_use]
    pub fn current() -> Self {
        use_context().unwrap_or_default()
    }

    /// What is wrong with a thread, if anything.
    #[must_use]
    pub fn problem(&self, status: ThreadStatus) -> Option<&'static str> {
        match status {
            ThreadStatus::Errored => Some(self.cannot_start),
            ThreadStatus::Reconnecting => Some(self.reconnecting),
            ThreadStatus::Disconnected => Some(self.disconnected),
            _ => None,
        }
    }
}

impl Default for Wording {
    fn default() -> Self {
        Self {
            untitled: "New thread",
            unprompted: "Send a prompt to start",
            connecting: "Connecting…",
            connecting_detail: "Starting the agent for this thread",
            stopped: "Stopped",
            stopped_detail: "This thread's agent is not running",
            cannot_start: "The agent could not start",
            reconnecting: "Reconnecting to the agent…",
            disconnected: "The agent connection closed",
            disconnected_detail: "Reconnect to pick up where it left off",
            reconnect: "Reconnect",
            placeholder: "Message the agent…  / for commands, @ for files",
            files_waiting: "Waiting for the agent",
            files_stopped: "Agent not running",
            files_stopped_detail: "Start the thread again to browse its files",
            room: "People",
            room_detail: "Messages here only reach people in this thread.",
        }
    }
}
