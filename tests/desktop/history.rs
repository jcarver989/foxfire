use crate::support::{replying, texts, write_agents_file};
use acp_utils::testing::FakeAgent;
use aether_desktop::history::{UNTITLED, git_root};
use aether_desktop::testing::{DesktopHarness, DesktopHarnessBuilder, TEST_AGENT};
use agent_client_protocol::v2::schema::{AbsolutePath, SessionId};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn every_listed_projects_saved_sessions_are_listed_newest_first_and_untitled_ones_read_new_thread() {
    let harness = DesktopHarnessBuilder::default()
        .session("first", "/alpha", "2026-01-01T00:00:00Z", "first prompt")
        .session("elsewhere", "/beta", "2026-02-01T00:00:00Z", "beta prompt")
        .session("latest", "/alpha", "2026-03-01T00:00:00Z", None)
        .project("/alpha")
        .project("/beta")
        .build();

    assert_eq!(
        listed(&harness),
        [("/alpha", "latest", UNTITLED), ("/beta", "elsewhere", "beta prompt"), ("/alpha", "first", "first prompt")]
            .map(|(root, id, title)| (PathBuf::from(root), id.to_owned(), title.to_owned()))
    );
}

#[test]
fn a_finished_turn_refreshes_history() {
    let (mut harness, thread) = DesktopHarness::alpha(replying("alpha-session", "done"));
    let history = harness.history();
    harness.save_session("saved-by-the-agent", "/alpha", "2026-01-01T00:00:00Z", Some("hi"));
    assert!(history.sessions().is_empty(), "history has not been re-read yet");

    thread.send("hi".into());
    harness.run_until(|_| !history.sessions().is_empty());

    assert_eq!(listed(&harness), [("/alpha".into(), "saved-by-the-agent".to_owned(), "hi".to_owned())]);
}

#[test]
fn a_saved_session_resumes_with_the_agent_that_listed_it_in_its_project() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path().to_path_buf();
    write_agents_file(&root, &json!({ "agents": { "claude": { "type": "stdio", "command": "claude-code-acp" } } }));
    let (agent, mut requests) = FakeAgent::default().replay_message("claude-session", "from claude").capture();
    let mut harness = DesktopHarnessBuilder::default()
        .agent_session("claude", "claude-session", &root, "2026-01-01T00:00:00Z", "claude prompt")
        .agent(&root, agent)
        .build();

    harness.click("history-row-claude-session").unwrap();
    let thread = harness.connected_thread();

    assert_eq!(thread.target().label(), "claude");
    assert_eq!(thread.session_id(), Some(SessionId::new("claude-session")));
    assert_eq!(texts(thread), ["from claude"]);
    assert_eq!(harness.next(&mut requests.resume).cwd, AbsolutePath::new(&root));
}

#[test]
fn a_session_two_agents_list_shows_once_and_resumes_with_the_projects_default_agent() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path().to_path_buf();
    write_agents_file(&root, &json!({ "agents": { "aether-opus": { "type": "stdio", "command": "aether" } } }));
    let harness = DesktopHarnessBuilder::default()
        .agent_session("aether-opus", "shared", &root, "2026-01-01T00:00:00Z", "prompt")
        .agent_session(TEST_AGENT, "shared", &root, "2026-01-01T00:00:00Z", "prompt")
        .project(&root)
        .build();

    let sessions = harness.history().sessions();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].agent.name, TEST_AGENT);
}

#[test]
fn a_directory_inside_a_checkout_belongs_to_the_checkouts_root() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::create_dir_all(repo.join("crates/core")).unwrap();

    assert_eq!(git_root(&repo.join("crates/core")), repo);
    assert_eq!(git_root(&repo), repo);
}

#[test]
fn a_directory_outside_any_checkout_is_its_own_project() {
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("plain");
    fs::create_dir_all(&plain).unwrap();

    assert_eq!(git_root(&plain), plain);
    assert_eq!(git_root(Path::new("/no/such/dir")), Path::new("/no/such/dir"));
}

fn listed(harness: &DesktopHarness) -> Vec<(PathBuf, String, String)> {
    let sessions = harness.history().sessions().into_iter();
    sessions.map(|session| (session.agent.workspace, session.id.0.to_string(), session.title)).collect()
}
