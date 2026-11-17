use crate::agents::AgentProfile;
use crate::target::{AgentConnector, ConnectError};
use agent_client_protocol::v2::schema::{SessionId, SessionInfo};
use foxfire_core::connection::Connection;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionKey(String);

pub trait SessionSource: Send + Sync + 'static {
    fn list<'a>(&'a self, agent: &'a AgentProfile) -> ListFuture<'a>;
}

pub type ListFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<SessionInfo>, ConnectError>> + Send + 'a>>;

pub struct AcpSessions {
    connector: Arc<AgentConnector>,
    connections: Mutex<Vec<(AgentProfile, Arc<Connection>)>>,
}

impl SessionKey {
    pub fn new(agent: &str, id: &SessionId) -> Self {
        Self(format!("{agent}:{}", id.0))
    }
}

impl fmt::Display for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AcpSessions {
    pub fn new(connector: Arc<AgentConnector>) -> Self {
        Self { connector, connections: Mutex::new(Vec::new()) }
    }

    fn connections(&self) -> MutexGuard<'_, Vec<(AgentProfile, Arc<Connection>)>> {
        self.connections.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SessionSource for AcpSessions {
    fn list<'a>(&'a self, agent: &'a AgentProfile) -> ListFuture<'a> {
        Box::pin(async move {
            let open = self.connections().iter().find(|(open, _)| open == agent).map(|(_, open)| open.clone());
            if let Some(connection) = open
                && let Ok(sessions) = connection.list_sessions(&agent.workspace).await
            {
                return Ok(sessions);
            }
            let connection = Arc::new(self.connector.connect_agent(agent, &agent.workspace).await?);
            let sessions = connection.list_sessions(&agent.workspace).await?;
            let mut connections = self.connections();
            connections.retain(|(open, _)| open.name != agent.name || open.workspace != agent.workspace);
            connections.push((agent.clone(), connection));
            Ok(sessions)
        })
    }
}
