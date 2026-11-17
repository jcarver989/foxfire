use crate::support::{replying, texts, write_agents_file};
use agent_client_protocol::v2::schema::{AbsolutePath, SessionId};
use foxfire_acp::testing::FakeAgent;
use foxfire_desktop::history::{UNTITLED, git_root};
use foxfire_desktop::testing::{DesktopHarness, DesktopHarnessBuilder, TEST_AGENT};
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

    harness.click("history-row-claude:claude-session").unwrap();
    let thread = harness.connected_thread();

    assert_eq!(thread.target().label(), "claude");
    assert_eq!(thread.session_id(), Some(SessionId::new("claude-session")));
    assert_eq!(texts(thread), ["from claude"]);
    assert_eq!(harness.next(&mut requests.resume).cwd, AbsolutePath::new(&root));
}

#[test]
fn independent_agents_listing_the_same_session_id_each_keep_their_own_row() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path().to_path_buf();
    write_agents_file(&root, &json!({ "agents": { "claude": { "type": "stdio", "command": "claude-code-acp" } } }));
    let mut harness = DesktopHarnessBuilder::default()
        .agent_session("claude", "shared", &root, "2026-01-01T00:00:00Z", "claude prompt")
        .agent_session(TEST_AGENT, "shared", &root, "2026-01-01T00:00:00Z", "agent prompt")
        .project(&root)
        .build();

    harness.run_until_painted("history-row-claude:shared");
    assert!(harness.shows("history-row-claude:shared-title", "claude prompt"));
    assert!(harness.shows("history-row-agent:shared-title", "agent prompt"));
}

#[test]
fn a_session_its_agent_lists_twice_shows_once() {
    let harness = DesktopHarnessBuilder::default()
        .session("shared", "/alpha", "2026-01-01T00:00:00Z", "prompt")
        .session("shared", "/alpha", "2026-01-01T00:00:00Z", "prompt")
        .project("/alpha")
        .build();

    assert_eq!(listed(&harness), [("/alpha".into(), "shared".to_owned(), "prompt".to_owned())]);
}

#[test]
fn opening_a_session_whose_id_a_live_thread_of_another_agent_shares_resumes_it_with_its_own_agent() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path().to_path_buf();
    write_agents_file(&root, &json!({ "agents": { "claude": { "type": "stdio", "command": "claude-code-acp" } } }));
    let mut harness = DesktopHarnessBuilder::default()
        .agent_session("claude", "shared", &root, "2026-01-01T00:00:00Z", "claude prompt")
        .agent(&root, FakeAgent::new("shared"))
        .script(&root, FakeAgent::default().replay_message("shared", "from claude"))
        .build();
    let live = harness.start_thread(&root);

    harness.click("history-row-claude:shared").unwrap();
    let resumed = harness.connected_thread();

    assert_ne!(resumed, live);
    assert_eq!(resumed.target().label(), "claude");
    assert_eq!(texts(resumed), ["from claude"]);
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
