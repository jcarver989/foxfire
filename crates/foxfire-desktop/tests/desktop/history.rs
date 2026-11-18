use crate::support::replying;
use foxfire_desktop::agents::AgentProfile;
use foxfire_desktop::history::{Listing, SavedSession, UNTITLED, git_root};
use foxfire_desktop::testing::{DesktopHarness, DesktopHarnessBuilder, TEST_AGENT};
use rouge::core::testing::TestResult;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn history_lists_requested_projects_only_and_orders_distinct_sessions_newest_first() {
    let mut harness = DesktopHarnessBuilder::default()
        .session("first", "/alpha", "2026-01-01T00:00:00Z", "first prompt")
        .session("elsewhere", "/beta", "2026-02-01T00:00:00Z", "beta prompt")
        .session("latest", "/alpha", "2026-03-01T00:00:00Z", None)
        .session("latest", "/alpha", "2026-03-01T00:00:00Z", "a duplicate")
        .session("latest", "/beta", "2026-03-01T00:00:00Z", "same key, other workspace")
        .project("/alpha")
        .project("/beta")
        .build();
    let history = harness.history();
    assert!(harness.sessions().asked().is_empty(), "no agent is asked at launch");
    assert_eq!(history.sessions(), Vec::<SavedSession>::new());

    let alpha = agent(&harness, "/alpha");
    list(&mut harness, &[alpha]);
    assert_eq!(harness.sessions().asked(), [(TEST_AGENT.to_owned(), PathBuf::from("/alpha"))]);
    assert_eq!(listed(&harness), expected([("/alpha", "latest", UNTITLED), ("/alpha", "first", "first prompt")]));

    let agents = [agent(&harness, "/alpha"), agent(&harness, "/beta")];
    list(&mut harness, &agents);
    assert_eq!(
        listed(&harness),
        expected([
            ("/alpha", "latest", UNTITLED),
            ("/beta", "elsewhere", "beta prompt"),
            ("/alpha", "first", "first prompt")
        ])
    );
}

#[test]
fn a_finished_turn_only_refreshes_previously_requested_profiles() {
    let (mut harness, thread) = DesktopHarness::alpha(replying("alpha-session", "done").prompt_reply("again"));
    harness.submit(thread, "before listing");
    assert_eq!(harness.sessions().asked(), [] as [(String, PathBuf); 0]);

    let alpha = agent(&harness, "/alpha");
    list(&mut harness, std::slice::from_ref(&alpha));
    let history = harness.history();
    harness.save_session("saved-by-the-agent", "/alpha", "2026-01-01T00:00:00Z", Some("hi"));
    assert!(history.sessions().is_empty(), "history has not been re-read yet");

    thread.send("after listing".into());
    harness.run_until(|_| !history.sessions().is_empty());
    let saved = history.sessions();
    assert_eq!(listed(&harness), expected([("/alpha", "saved-by-the-agent", "hi")]));

    harness.sessions().fail(TEST_AGENT);
    history.refresh();
    harness.run_until(|_| matches!(history.listing(&alpha), Some(Listing::Failed(_))));
    assert_eq!(history.sessions(), saved, "a failed refresh keeps the earlier rows");
}

#[test]
fn git_roots_are_the_nearest_checkout_or_the_original_directory() -> TestResult {
    let dir = tempfile::tempdir()?;
    let repo = dir.path().join("repo");
    let plain = dir.path().join("plain");
    fs::create_dir_all(repo.join(".git"))?;
    fs::create_dir_all(repo.join("crates/core"))?;
    fs::create_dir_all(&plain)?;
    assert_eq!(git_root(&repo.join("crates/core")), repo);
    assert_eq!(git_root(&repo), repo);
    assert_eq!(git_root(&plain), plain);
    assert_eq!(git_root(Path::new("/no/such/dir")), Path::new("/no/such/dir"));
    Ok(())
}

fn agent(harness: &DesktopHarness, root: &str) -> AgentProfile {
    harness.threads().default_agent(Path::new(root)).expect("the project has an agent")
}

fn list(harness: &mut DesktopHarness, agents: &[AgentProfile]) {
    let history = harness.history();
    history.list(agents);
    harness.run_until(|_| agents.iter().all(|agent| history.listing(agent) == Some(Listing::Listed)));
}

fn listed(harness: &DesktopHarness) -> Vec<(PathBuf, String, String)> {
    harness
        .history()
        .sessions()
        .into_iter()
        .map(|session| (session.agent.workspace, session.id.0.to_string(), session.title))
        .collect()
}

fn expected<const N: usize>(rows: [(&str, &str, &str); N]) -> [(PathBuf, String, String); N] {
    rows.map(|(root, id, title)| (root.into(), id.into(), title.into()))
}
