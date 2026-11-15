use crate::support::{Edges, replying};
use acp_utils::testing::FakeAgent;
use aether_desktop::testing::{DesktopHarness, DesktopHarnessBuilder};
use aether_desktop::threads::ThreadStatus;
use agent_client_protocol::v2::schema as acp;

#[test]
fn a_new_thread_centers_the_composer_under_a_heading_with_a_fire_kindling_behind_it() {
    let (mut harness, _) = DesktopHarness::alpha(replying("alpha-session", "hello"));
    let main = harness.bounds("main").expect("the main region is painted");
    let heading = harness.bounds("new-thread-heading").expect("a new thread asks what to build");
    let composer = harness.bounds("composer").expect("the composer is painted");

    assert!(harness.shows("new-thread-heading", "What do you want to build?"));
    assert!(heading.bottom() < composer.top(), "{heading:?} {composer:?}");
    assert!((composer.center().y - main.center().y).abs() < main.size.height * 0.15, "{main:?} {composer:?}");
    assert!(harness.bounds("kindling").is_some(), "a fire kindles behind the composer");
}

#[test]
fn the_first_prompt_docks_the_composer_and_clears_the_heading_and_fire() {
    let (mut harness, thread) = DesktopHarness::alpha(replying("alpha-session", "hello"));

    harness.send(thread, "hi there");

    assert!(harness.bounds("new-thread-heading").is_none());
    assert!(harness.bounds("kindling").is_none());
    let main = harness.bounds("main").expect("the main region is painted");
    let composer = harness.bounds("composer").expect("the composer is painted");
    assert!(composer.bottom() > main.bottom() - main.size.height * 0.25, "{main:?} {composer:?}");
}

#[test]
fn a_connecting_thread_already_asks_what_to_build_and_keeps_its_composer_in_place_once_connected() {
    let (agent, mut requests) = FakeAgent::new("alpha-session").hold::<acp::NewSessionRequest>().capture();
    let mut harness = DesktopHarnessBuilder::default().agent("/alpha", agent).build();
    let thread = harness.new_thread("/alpha");
    let (_, responder) = harness.next(requests.held::<acp::NewSessionRequest>());

    assert_eq!(thread.status(), ThreadStatus::Connecting);
    assert!(harness.shows("new-thread-heading", "What do you want to build?"));
    assert!(harness.shows("new-thread-connecting", "Connecting…"));
    let connecting = harness.bounds("composer").expect("the composer is painted");

    responder.respond(acp::NewSessionResponse::new("alpha-session")).unwrap();
    harness.run_until(|_| thread.status() == ThreadStatus::Idle);

    assert!(harness.bounds("new-thread-connecting").is_none(), "the connecting message clears");
    assert_eq!(harness.bounds("composer"), Some(connecting), "the composer stays where it was");
}

#[test]
fn the_status_row_sits_as_snug_under_a_new_threads_composer_as_under_a_docked_one() {
    let (mut harness, thread) = DesktopHarness::alpha(replying("alpha-session", "hello"));
    let gap = |harness: &mut DesktopHarness| {
        let [composer, status] = ["composer", "status"].map(|id| harness.bounds(id).expect("painted"));
        status.top() - composer.bottom()
    };
    let fresh = gap(&mut harness);

    harness.send(thread, "hi there");

    assert_eq!(fresh, gap(&mut harness));
}
