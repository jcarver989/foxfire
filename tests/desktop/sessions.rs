use acp_utils::testing::FakeAgent;
use aether_desktop::agents::{AgentProfile, Agents, CommandLine};
use aether_desktop::sessions::{AcpSessions, SessionSource};
use aether_desktop::target::AgentConnector;
use aether_desktop::testing::FakeConnector;
use agent_client_protocol::v2::schema::{AbsolutePath, SessionInfo};
use std::path::Path;
use std::sync::Arc;

#[tokio::test]
async fn an_agent_lists_only_the_sessions_it_saved_in_its_project() {
    let connector = FakeConnector::default();
    connector.script("/alpha", FakeAgent::default().sessions(vec![saved("here", "/alpha"), saved("there", "/beta")]));

    let listed = sessions(&connector).list(&default_agent("/alpha")).await.unwrap();

    assert_eq!(listed, [saved("here", "/alpha")]);
}

#[tokio::test]
async fn every_page_of_an_agents_sessions_is_listed() {
    let connector = FakeConnector::default();
    let all = vec![saved("one", "/alpha"), saved("two", "/alpha"), saved("three", "/alpha")];
    connector.script("/alpha", FakeAgent::default().sessions(all.clone()).session_page_size(2));

    let listed = sessions(&connector).list(&default_agent("/alpha")).await.unwrap();

    assert_eq!(listed, all);
}

#[tokio::test]
async fn listing_again_reuses_the_agents_connection() {
    let connector = FakeConnector::default();
    connector.script("/alpha", FakeAgent::default().sessions(vec![saved("here", "/alpha")]));
    let (sessions, alpha) = (sessions(&connector), default_agent("/alpha"));
    sessions.list(&alpha).await.unwrap();

    let again = sessions.list(&alpha).await.unwrap();

    assert_eq!(again, [saved("here", "/alpha")], "no second agent was scripted, so the first answered");
    assert_eq!(connector.live_agents("/alpha"), 1);
}

#[tokio::test]
async fn an_agent_whose_connection_died_is_started_again() {
    let connector = FakeConnector::default();
    connector.script("/alpha", FakeAgent::default().sessions(vec![saved("before", "/alpha")]));
    connector.script("/alpha", FakeAgent::default().sessions(vec![saved("after", "/alpha")]));
    let (sessions, alpha) = (sessions(&connector), default_agent("/alpha"));
    sessions.list(&alpha).await.unwrap();
    connector.kill_agents("/alpha");

    let listed = sessions.list(&alpha).await.unwrap();

    assert_eq!(listed, [saved("after", "/alpha")]);
}

#[tokio::test]
async fn an_agent_that_cannot_be_reached_fails_to_list() {
    let connector = FakeConnector::default();

    assert!(sessions(&connector).list(&default_agent("/alpha")).await.is_err());
}

fn sessions(connector: &FakeConnector) -> AcpSessions {
    let remotes = Arc::new(connector.remotes().clone());
    AcpSessions::new(Arc::new(AgentConnector::new(remotes, Arc::new(connector.clone()))))
}

fn default_agent(project: &str) -> AgentProfile {
    Agents::new(Some(CommandLine::new("agent")), None).catalog(Path::new(project)).default_agent().cloned().unwrap()
}

fn saved(session_id: &str, cwd: &str) -> SessionInfo {
    SessionInfo::new(session_id, AbsolutePath::new(cwd))
}
