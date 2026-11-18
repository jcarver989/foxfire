use crate::support::{choose, replying};
use agent_client_protocol::v2::schema::SessionId;
use foxfire_acp::testing::FakeAgent;
use foxfire_desktop::registry::STATE_FILE;
use foxfire_desktop::testing::{DesktopHarness, DesktopHarnessBuilder, TEST_AGENT};
use foxfire_desktop::threads::ThreadStatus;
use foxfire_desktop::ui::DEFAULT_INTERFACE_SIZE;
use foxfire_desktop::ui::tokens::BASE_SIZE;
use rouge::core::Point;
use rouge::core::testing::TestResult;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn the_next_launch_restores_what_the_user_changed() {
    let state = tempfile::tempdir().unwrap();
    let mut harness = launch(state.path());
    harness.click("add-project").unwrap();
    harness.platform().answer_path_prompt(Some(vec![PathBuf::from("/picked")]));
    harness.click("project-beta").unwrap();
    harness.start_thread("/alpha");
    harness.click("past-sessions-toggle").unwrap();
    harness.run_until_painted("past-session-agent:saved");
    harness.click("past-session-agent:saved").unwrap();
    let resumed = harness.connected_thread();
    harness.run_until(|_| resumed.status() == ThreadStatus::Idle);
    choose(&mut harness, "thread-row-1", "menu-item-Rename");
    harness.type_text("Renamed");
    harness.press("enter");
    harness.press("secondary-= secondary-=");
    let (handle, sidebar) = (harness.bounds("panel-resize-handle").unwrap(), harness.bounds("sidebar").unwrap());
    harness.drag("panel-resize-handle", Point::new(handle.center().x + 400.0 - sidebar.size.width, 300.0)).unwrap();
    harness.resize(900.0, 700.0);
    harness.quit();

    let mut relaunched = launch(state.path());
    let threads = relaunched.threads();
    relaunched.run_until(|_| threads.selected().is_some_and(|thread| thread.status() == ThreadStatus::Idle));

    let thread = relaunched.selected_thread().expect("the last selected session is selected again");
    assert_eq!(thread.session_id(), Some(SessionId::new("saved")));
    let asked: Vec<String> = relaunched.sessions().asked().into_iter().map(|(agent, _)| agent).collect();
    assert!(asked.iter().all(|agent| agent == TEST_AGENT), "only the restored session's agent is asked: {asked:?}");
    assert!(relaunched.bounds("project-picked").is_some(), "an added project is listed");
    assert!(relaunched.bounds("project-beta-folder").is_some(), "a collapsed project is still listed");
    assert!(relaunched.shows("thread-row-0-title", "Renamed"), "a renamed thread keeps its name");
    assert_eq!(relaunched.interface_size(), DEFAULT_INTERFACE_SIZE + 2.0);
    let sidebar = relaunched.bounds("sidebar").expect("the sidebar is painted");
    let main = relaunched.bounds("main").expect("the main region is painted");
    assert!((sidebar.size.width - 400.0).abs() < 0.5, "the sidebar keeps its width: {sidebar:?}");
    let zoom = relaunched.interface_size() / BASE_SIZE;
    let (width, height) = (sidebar.size.width + main.size.width, main.size.height);
    assert!(
        (width * zoom - 900.0).abs() < 0.5 && (height * zoom - 700.0).abs() < 0.5,
        "the window keeps its size: {width}x{height} at {zoom}x"
    );
}

#[test]
fn a_last_selected_session_missing_from_history_is_not_resumed() -> TestResult {
    let state = tempfile::tempdir()?;
    fs::write(state.path().join(STATE_FILE), r#"{ "last_selected": "agent:deleted" }"#)?;
    let mut relaunched = launch(state.path());
    let history = relaunched.history();
    relaunched.run_until(|_| !history.sessions().is_empty());
    assert!(relaunched.selected_thread().is_none());
    Ok(())
}

fn launch(state: &Path) -> DesktopHarness {
    DesktopHarnessBuilder::default()
        .state_dir(state)
        .agent("/alpha", replying("fresh-session", "unused"))
        .script("/alpha", FakeAgent::default().replay_message("saved", "an earlier answer"))
        .script("/alpha", FakeAgent::default().replay_message("saved", "an earlier answer"))
        .project("/beta")
        .session("saved", "/alpha", "2026-01-01T00:00:00Z", "saved prompt")
        .session("hidden", "/beta", "2026-01-01T00:00:00Z", "beta prompt")
        .build()
}
