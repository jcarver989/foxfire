use crate::support::{choose, replying};
use foxfire_desktop::testing::DesktopHarness;

#[test]
fn renaming_a_thread_retitles_its_row_and_header_until_the_name_is_cleared() {
    let (mut harness, thread) = DesktopHarness::alpha(replying("alpha-session", "hello"));
    harness.submit(thread, "first prompt");
    assert!(harness.shows("thread-row-0-title", "first prompt"));

    choose(&mut harness, "thread-row-0", "menu-item-Rename");
    harness.type_text("Renamed");
    harness.press("enter");

    assert!(harness.bounds("rename-input").is_none(), "Enter ends the edit");
    assert!(harness.shows("thread-row-0-title", "Renamed"));
    assert!(harness.shows("thread-path-current", "Renamed"));

    choose(&mut harness, "thread-row-0", "menu-item-Rename");
    harness.press("backspace enter");

    assert!(harness.shows("thread-row-0-title", "first prompt"));
    assert!(harness.shows("thread-path-current", "first prompt"));
}

#[test]
fn a_rename_keeps_spaces_and_escape_cancels_it() {
    let (mut harness, thread) = DesktopHarness::alpha(replying("alpha-session", "hello"));
    harness.submit(thread, "first prompt");

    choose(&mut harness, "thread-row-0", "menu-item-Rename");
    harness.press("a space b enter");
    assert!(harness.shows("thread-row-0-title", "a b"));

    choose(&mut harness, "thread-row-0", "menu-item-Rename");
    harness.type_text("Never mind");
    harness.press("escape");
    assert!(harness.bounds("rename-input").is_none());
    assert!(harness.shows("thread-row-0-title", "a b"));
}

#[test]
fn archiving_a_live_thread_stops_its_agent_and_removes_its_row() {
    let (mut harness, _) = DesktopHarness::alpha(replying("alpha-session", "hello"));

    choose(&mut harness, "thread-row-0", "menu-item-Archive");

    harness.run_until(|harness| harness.connector().live_agents("/alpha") == 0);
    assert!(harness.bounds("thread-row-0").is_none());
}
