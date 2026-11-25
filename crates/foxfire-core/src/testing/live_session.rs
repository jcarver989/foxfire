use super::{FakeGit, git_diff_agent};
use crate::connection::{Connection, ConnectionError, Session};
use crate::runtime::SessionRuntime;
use crate::session_core::{Effect, SessionCore};
use agent_client_protocol::v2::schema::{Implementation, SessionId};
use foxfire_acp::testing::{Channel, FakeAgent};
use std::path::PathBuf;

/// A `SessionCore` driven by a real `SessionRuntime`, the way a frontend's
/// event loop drives them.
pub struct LiveSession {
    pub core: SessionCore,
    pub runtime: SessionRuntime,
}

impl LiveSession {
    pub async fn connect(
        agent: FakeAgent,
        working_dir: impl Into<PathBuf>,
        session_id: Option<SessionId>,
    ) -> Result<Self, ConnectionError> {
        let connection = Connection::connect_to(Implementation::new("foxfire-test", "1"), agent.serve()).await?;
        Ok(Self::start(connection.open(session_id, working_dir.into()).await?))
    }

    pub async fn reviewing(git: FakeGit) -> Result<Self, ConnectionError> {
        let (agent_end, client_end) = Channel::duplex();
        tokio::task::spawn_local(git_diff_agent(FakeAgent::new("reviewing"), git).connect_to(agent_end));
        let connection = Connection::connect_to(Implementation::new("foxfire-test", "1"), client_end).await?;
        Ok(Self::start(connection.open(None, "/repo".into()).await?))
    }

    pub fn start(session: Session) -> Self {
        let (config, runtime) = session.into_core();
        Self { core: SessionCore::new(config), runtime }
    }

    pub async fn run_until(&mut self, done: impl Fn(&SessionCore) -> bool) {
        loop {
            for effect in self.core.take_effects() {
                if let Effect::Dispatch(command) = effect {
                    self.runtime.dispatch(command);
                }
            }
            if done(&self.core) {
                return;
            }
            let input = self.runtime.next().await;
            self.core.on_input(input);
        }
    }

    pub async fn shutdown(mut self) {
        self.runtime.shutdown().await;
    }
}
