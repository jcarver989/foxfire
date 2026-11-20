use crate::support::{replying, texts};
use agent_client_protocol::v2::schema::SessionId;
use foxfire_acp::testing::FakeAgent;
use foxfire_desktop::testing::DesktopHarnessBuilder;
use foxfire_desktop::threads::{Thread, ThreadStatus};
use rouge::core::testing::TestResult;

#[test]
fn concurrent_threads_each_receive_only_their_own_updates() {
    let resumed = FakeAgent::default().replay_message("gamma-session", "earlier in gamma").prompt_reply("from gamma");
    let (mut harness, alpha) = DesktopHarnessBuilder::default()
        .agent("/alpha", replying("alpha-session", "from alpha"))
        .agent("/beta", replying("beta-session", "from beta"))
        .agent("/gamma", resumed)
        .start("/alpha");
    let beta = harness.start_thread("/beta");
    let gamma = harness.resume("gamma-session", "/gamma");
    let threads = [alpha, beta, gamma];
    harness.run_until(|_| gamma.status() == ThreadStatus::Idle);

    for (thread, prompt) in threads.iter().zip(["to alpha", "to beta", "to gamma"]) {
        thread.send(prompt.into());
    }
    harness.run_until(|_| {
        threads
            .iter()
            .zip([2, 2, 3])
            .all(|(thread, len)| texts(*thread).len() == len && thread.status() == ThreadStatus::Idle)
    });

    assert_eq!(alpha.session_id(), Some(SessionId::new("alpha-session")));
    assert_eq!(texts(alpha), ["to alpha", "from alpha"]);
    assert_eq!(beta.session_id(), Some(SessionId::new("beta-session")));
    assert_eq!(texts(beta), ["to beta", "from beta"]);
    assert_eq!(gamma.session_id(), Some(SessionId::new("gamma-session")));
    assert_eq!(texts(gamma), ["earlier in gamma", "to gamma", "from gamma"]);
}

#[test]
fn a_stopped_threads_session_can_be_picked_up_again_from_a_new_thread() -> TestResult {
    let (mut harness, thread) = DesktopHarnessBuilder::default()
        .session("alpha-session", "/alpha", "2026-01-01T00:00:00Z", "hi")
        .agent("/alpha", replying("alpha-session", "hello"))
        .agent("/alpha", replying("fresh-session", "unused"))
        .agent("/alpha", FakeAgent::default().replay_message("alpha-session", "replayed answer"))
        .start("/alpha");

    harness.hover("thread-row-0")?;
    harness.click("thread-row-0-stop")?;
    harness.run_until(|harness| harness.connector().live_agents("/alpha") == 0);

    assert_eq!(harness.threads().threads(), Vec::<Thread>::new());
    assert!(harness.selected_thread().is_none());
    assert_eq!(thread.status(), ThreadStatus::Dormant);
    assert!(harness.bounds("thread-row-0").is_none());

    let fresh = harness.start_thread("/alpha");
    harness.click("past-sessions-toggle")?;
    harness.run_until_painted("past-session-agent:alpha-session");
    harness.click("past-session-agent:alpha-session")?;
    let resumed = harness.connected_thread();

    assert_eq!(resumed.session_id(), Some(SessionId::new("alpha-session")));
    assert!(!harness.threads().threads().contains(&fresh));
    assert_eq!(texts(resumed), ["replayed answer"]);
    harness.run_until(|harness| harness.connector().live_agents("/alpha") == 1);
    assert_eq!(resumed.status(), ThreadStatus::Idle);
    Ok(())
}

#[test]
fn stopping_one_thread_leaves_the_others_running() {
    let (mut harness, alpha) = DesktopHarnessBuilder::default()
        .agent("/alpha", replying("alpha-session", "from alpha"))
        .agent("/beta", replying("beta-session", "from beta"))
        .start("/alpha");
    harness.start_thread("/beta");

    harness.hover("thread-row-1").unwrap();
    harness.click("thread-row-1-stop").unwrap();
    harness.run_until(|harness| harness.connector().live_agents("/beta") == 0);
    harness.submit(alpha, "still there?");

    assert_eq!(texts(alpha), ["still there?", "from alpha"]);
    assert_eq!(harness.connector().live_agents("/alpha"), 1);
}

#[test]
fn a_thread_whose_agent_cannot_start_is_errored_and_gives_a_prompt_back() {
    let mut harness = DesktopHarnessBuilder::default().project("/nowhere").build();

    let thread = harness.new_thread("/nowhere");
    harness.run_until(|_| thread.status() != ThreadStatus::Connecting);

    let (status, error) = (thread.status(), thread.error());
    assert_eq!(status, ThreadStatus::Errored);
    assert!(error.as_deref().is_some_and(|error| error.contains("no fake agent")), "{error:?}");
    assert!(harness.bounds("thread-row-0-status-errored-warning-triangle").is_some());
    assert!(harness.bounds("thread-row-0-conversation-icon").is_none());
    harness.type_text("hello?");
    harness.press("enter");
    assert!(harness.shows("composer-text", "hello?"), "a prompt the agent never received is kept");
}

#[test]
fn resuming_a_session_that_is_already_open_selects_its_thread() {
    let mut harness = DesktopHarnessBuilder::default()
        .agent("/alpha", FakeAgent::default().replay_message("saved-session", "an earlier answer"))
        .agent("/beta", replying("beta-session", "hello"))
        .build();
    let resumed = harness.resume("saved-session", "/alpha");
    harness.new_thread("/beta");

    let again = harness.resume("saved-session", "/alpha");

    assert_eq!(again, resumed);
    assert_eq!(harness.threads().threads().len(), 2);
    assert_eq!(harness.selected_thread(), Some(resumed));
}

#[test]
fn a_thread_whose_agent_died_reconnects_from_its_transcript_and_replays_its_session_without_duplicates() {
    let (mut harness, thread) = DesktopHarnessBuilder::default()
        .agent("/alpha", replying("alpha-session", "hello"))
        .agent("/alpha", FakeAgent::default().replay_message("alpha-session", "replayed answer"))
        .start("/alpha");
    harness.submit(thread, "hi");
    assert!(harness.bounds("transcript-reconnect").is_none(), "a connected thread offers no reconnect");

    harness.connector().kill_agents("/alpha");
    harness.run_until(|_| thread.status() == ThreadStatus::Disconnected);
    assert!(harness.bounds("thread-row-0-status-disconnected").is_some());
    harness.click("transcript-reconnect").unwrap();
    harness.run_until(|_| thread.status() == ThreadStatus::Idle);

    assert_eq!(texts(thread), ["replayed answer"]);
    assert_eq!(harness.selected_thread(), Some(thread));
    assert_eq!(harness.connector().live_agents("/alpha"), 1);
    assert!(harness.bounds("transcript-reconnect").is_none());
}

#[test]
fn a_thread_whose_agent_could_not_start_reconnects_from_the_sidebar_once_it_can() {
    let mut harness = DesktopHarnessBuilder::default().build();
    let thread = harness.new_thread("/alpha");
    harness.run_until(|_| thread.status() == ThreadStatus::Errored);

    harness.connector().script("/alpha", replying("alpha-session", "hello"));
    harness.click("thread-row-0-reconnect").unwrap();
    harness.run_until(|_| thread.status() == ThreadStatus::Idle);

    assert_eq!(thread.session_id(), Some(SessionId::new("alpha-session")));
}
