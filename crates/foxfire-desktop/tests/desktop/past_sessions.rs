use crate::support::{Edges, define_agent, replying, texts};
use agent_client_protocol::v2::schema::{AbsolutePath, SessionId};
use foxfire_acp::testing::FakeAgent;
use foxfire_desktop::history::{Listing, SavedSession};
use foxfire_desktop::sessions::SessionKey;
use foxfire_desktop::testing::{DesktopHarnessBuilder, TEST_AGENT};
use rouge::core::testing::TestResult;
use rouge::widgets::tokens::MotionScale;
use serde_json::json;
use std::path::{Path, PathBuf};

#[test]
fn the_composer_bar_lists_recent_sessions_on_demand_and_filters_them_locally() -> TestResult {
    let (mut harness, thread) = DesktopHarnessBuilder::default()
        .now("2026-01-07T12:00:00Z")
        .agent("/alpha", replying("fresh", "hello"))
        .session("auth", "/alpha", "2026-01-07T11:59:30Z", "Refactor auth")
        .session("other", "/alpha", "2026-01-07T11:55:00Z", "Other")
        .session("third", "/alpha", "2026-01-07T09:00:00Z", "Third")
        .session("fourth", "/alpha", "2026-01-05T12:00:00Z", "Fourth")
        .session("parser", "/alpha", "2025-12-01T09:00:00Z", "Fix the parser")
        .session("elsewhere", "/beta", "2026-01-07T11:00:00Z", "Another project")
        .start("/alpha");
    let [composer, bar, toggle] =
        ["composer", "composer-bar", "past-sessions-toggle"].map(|id| harness.bounds(id).expect("painted"));
    assert!(harness.shows("past-sessions-toggle-label", "Past sessions"));
    assert!((bar.top() - composer.bottom() - 4.0).abs() < 0.5, "the bar sits just below the composer");
    assert!((bar.left() - composer.left()).abs() < 0.5 && (bar.right() - composer.right()).abs() < 0.5);
    assert!((bar.right() - toggle.right() - 24.0).abs() < 0.5, "the toggle stays at the right end of the bar");
    assert!(toggle.top() >= bar.top() && toggle.bottom() <= bar.bottom());
    assert!(harness.bounds("past-sessions").is_none());
    assert_eq!(harness.sessions().asked(), [] as [(String, PathBuf); 0]);

    harness.click("past-sessions-toggle")?;
    harness.run_until_painted("past-session-agent:auth");
    let tray = harness.text("past-sessions").expect("the tray is painted");
    let [today, earlier] = ["Today", "Earlier"].map(|group| tray.find(group).expect("the group is labelled"));
    assert!(today < earlier, "{tray}");
    let [newest, older] = ["past-session-agent:auth", "past-session-agent:parser"]
        .map(|id| harness.bounds(id).expect("session is listed"));
    assert!(newest.top() < older.top());
    assert!(harness.shows("past-session-agent:auth-label", "Refactor auth"));
    for (id, age) in [("auth", "now"), ("other", "5m"), ("third", "3h"), ("fourth", "2d")] {
        let row = harness.text(&format!("past-session-agent:{id}")).expect("session is listed");
        assert!(row.ends_with(age), "{id} reads {row:?}");
    }
    assert!(harness.bounds("past-session-agent:elsewhere").is_none());

    harness.type_text("pars");
    assert!(harness.bounds("past-session-agent:parser").is_some());
    assert!(harness.bounds("past-session-agent:auth").is_none());
    assert_eq!(harness.sessions().asked(), [(TEST_AGENT.to_owned(), PathBuf::from("/alpha"))]);

    harness.press("escape");
    harness.type_text("hello");
    assert!(harness.bounds("past-sessions").is_none());
    assert!(harness.shows("composer-text", "hello"));
    harness.press("secondary-o");
    harness.run_until_painted("past-session-agent:auth");
    harness.press("secondary-o");
    assert!(harness.bounds("past-sessions").is_none());

    harness.click("past-sessions-toggle")?;
    harness.click("composer-text")?;
    harness.press("secondary-a");
    harness.send(thread, "hi there");
    assert!(harness.bounds("past-sessions-toggle").is_none());
    assert!(harness.bounds("past-sessions").is_none());
    Ok(())
}

#[test]
fn the_picker_shows_renamed_titles_skips_archived_rows_and_resumes_the_highlight() -> TestResult {
    let (mut harness, fresh) = DesktopHarnessBuilder::default()
        .agent("/alpha", replying("fresh", "unused"))
        .script("/alpha", FakeAgent::default().replay_message("parser", "parsed"))
        .session("auth", "/alpha", "2026-01-07T09:00:00Z", "Refactor auth")
        .session("archived", "/alpha", "2026-01-01T09:00:00Z", "Old auth")
        .session("parser", "/alpha", "2025-12-01T09:00:00Z", "Fix the parser")
        .start("/alpha");
    harness.registry().update(|state| {
        state.rename(SessionKey::new(TEST_AGENT, &SessionId::new("auth")), "Login rewrite");
        state.archive(SessionKey::new(TEST_AGENT, &SessionId::new("archived")));
    });
    harness.click("past-sessions-toggle")?;
    harness.run_until_painted("past-session-agent:parser");
    assert!(harness.shows("past-session-agent:auth-label", "Login rewrite"));
    assert!(harness.bounds("past-session-agent:archived").is_none());

    harness.press("down enter");
    let resumed = harness.connected_thread();
    assert_eq!(resumed.session_id(), Some(SessionId::new("parser")));
    assert_eq!(texts(resumed), ["parsed"]);
    assert!(!harness.threads().threads().contains(&fresh));
    Ok(())
}

#[test]
fn switching_agents_lists_and_resumes_the_selected_agents_session_even_when_its_id_is_shared() -> TestResult {
    let project = tempfile::tempdir()?;
    let root = project.path();
    define_agent(root, "claude", &json!({ "type": "stdio", "command": "claude-code-acp" }));
    let (claude, mut requests) = FakeAgent::default().replay_message("shared", "from claude").capture();
    let (mut harness, live) = DesktopHarnessBuilder::default()
        .agent(root, FakeAgent::new("shared"))
        .script(root, replying("fresh", "unused"))
        .script(root, claude)
        .session("shared", root, "2026-01-07T09:00:00Z", "From the default agent")
        .agent_session("claude", "shared", root, "2026-01-07T10:00:00Z", "From claude")
        .start(root);
    harness.start_thread(root);
    harness.click("past-sessions-toggle")?;
    harness.run_until_painted("past-session-agent:shared");
    assert!(harness.bounds("past-session-claude:shared").is_none());

    harness.click("past-sessions-agent-claude")?;
    harness.run_until_painted("past-session-claude:shared");
    assert!(harness.bounds("past-session-agent:shared").is_none());
    assert_eq!(harness.sessions().asked(), [TEST_AGENT, "claude"].map(|name| (name.to_owned(), root.to_path_buf())));

    harness.click("past-session-claude:shared")?;
    let resumed = harness.connected_thread();
    assert_ne!(resumed, live);
    assert_eq!(resumed.target().label(), "claude");
    assert_eq!(resumed.session_id(), Some(SessionId::new("shared")));
    assert_eq!(texts(resumed), ["from claude"]);
    assert_eq!(harness.next(&mut requests.resume).cwd, AbsolutePath::new(root));
    Ok(())
}

#[test]
fn placeholder_rows_stand_in_for_the_sessions_until_the_agent_has_answered() -> TestResult {
    let (mut harness, _) =
        DesktopHarnessBuilder::default().agent("/alpha", replying("fresh", "unused")).start("/alpha");
    let agent = harness.threads().default_agent(Path::new("/alpha")).expect("configured agent");
    let history = harness.history();
    harness.sessions().hold();

    harness.click("past-sessions-toggle")?;
    assert!(harness.bounds("past-sessions-loading").is_some());

    harness.sessions().release();
    harness.run_until(|_| history.listing(&agent) == Some(Listing::Listed));
    assert!(harness.bounds("past-sessions-loading").is_none());
    Ok(())
}

#[test]
fn the_tray_follows_reduced_motion_chosen_after_the_thread_was_built() -> TestResult {
    let (mut harness, _) = DesktopHarnessBuilder::default()
        .motion(MotionScale::Full)
        .agent("/alpha", replying("fresh", "unused"))
        .start("/alpha");
    harness.registry().update(|state| state.reduced_motion = true);
    harness.advance();
    harness.ui().run(2.0);

    for shown in [true, false] {
        harness.click("past-sessions-toggle")?;
        harness.ui().draw()?;

        assert_eq!(harness.bounds("past-sessions").is_some(), shown);
        assert_eq!(harness.ui().compositor_frames(1.0)?, 0, "the tray moved while {shown}");
    }
    Ok(())
}

#[test]
fn the_tray_distinguishes_an_empty_listing_from_failure() -> TestResult {
    for failed in [false, true] {
        let builder = DesktopHarnessBuilder::default().agent("/alpha", replying("fresh", "unused"));
        let builder =
            if failed { builder.session("saved", "/alpha", "2026-01-01T00:00:00Z", "saved") } else { builder };
        let (mut harness, thread) = builder.start("/alpha");
        if failed {
            harness.sessions().fail(TEST_AGENT);
        }
        let agent = harness.threads().default_agent(Path::new("/alpha")).expect("configured agent");
        let history = harness.history();
        harness.click("past-sessions-toggle")?;
        harness.run_until(|_| matches!(history.listing(&agent), Some(Listing::Listed | Listing::Failed(_))));
        if failed {
            assert!(matches!(history.listing(&agent), Some(Listing::Failed(_))));
            assert!(harness.shows("past-sessions-status-label", "Couldn't list agent's sessions"));
        } else {
            assert!(harness.shows("past-sessions", "No past sessions"));
        }
        assert_eq!(history.sessions(), Vec::<SavedSession>::new());
        assert!(thread.is_fresh());
    }
    Ok(())
}
