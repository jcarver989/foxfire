use crate::support::{Edges, replying};
use foxfire_desktop::sidebar::MIN_MAIN_WIDTH;
use foxfire_desktop::testing::{DesktopHarness, DesktopHarnessBuilder};
use foxfire_desktop::threads::{Thread, ThreadStatus};
use foxfire_desktop::ui::tokens::{CONTENT_MAX_WIDTH, GUTTER, SCROLLBAR_WIDTH};
use rouge::core::Point;

#[test]
fn widening_the_sidebar_leaves_room_for_the_thread() {
    let mut harness = DesktopHarnessBuilder::default().window_size(640.0, 480.0).build();
    let min_main = MIN_MAIN_WIDTH;

    harness.drag("panel-resize-handle", Point::new(600.0, 240.0)).unwrap();

    let main = harness.bounds("main").expect("the main region is painted");
    assert!((main.size.width - min_main).abs() < 0.5, "{main:?}");
}

#[test]
fn collapsing_the_sidebar_gives_its_room_to_the_thread_and_its_toggle_to_the_header() {
    let (mut harness, _) = DesktopHarnessBuilder::default()
        .window_size(640.0, 480.0)
        .agent("/alpha", replying("alpha-session", "hello"))
        .start("/alpha");
    // Linux defaults to a larger interface size than macOS, so prove the layout holds at a zoom other than 1.
    harness.press("secondary-=");
    let settings = harness.bounds("open-settings").expect("settings is painted in the sidebar");
    let sidebar = harness.bounds("sidebar").expect("the sidebar is painted");
    let expanded = harness.bounds("toggle-sidebar").expect("the toggle is painted in the sidebar");
    assert!(expanded.left() >= settings.right(), "{settings:?} {expanded:?}");
    assert!(expanded.right() <= sidebar.right(), "{sidebar:?} {expanded:?}");

    harness.click("toggle-sidebar").unwrap();

    assert!(harness.bounds("sidebar").is_none(), "the sidebar is hidden");
    let main = harness.bounds("main").expect("the main region is painted");
    let desktop = harness.bounds("desktop").expect("the desktop is painted");
    assert!((main.size.width - desktop.size.width).abs() < 0.5, "{desktop:?} {main:?}");
    assert!(harness.bounds("toggle-sidebar").is_some(), "the toggle is painted in the thread header");
    assert!(harness.bounds("thread-header").is_some());

    harness.click("toggle-sidebar").unwrap();
    assert!(harness.bounds("project-alpha").is_some(), "the sidebar shows again");

    harness.press("secondary-b");
    assert!(harness.bounds("sidebar").is_none(), "the shortcut hides the sidebar");
    harness.press("secondary-b");
    assert!(harness.bounds("sidebar").is_some(), "the shortcut shows the sidebar again");
}

#[test]
fn the_transcript_scrolls_beneath_the_header_and_the_composer_that_float_over_it() {
    let (mut harness, thread) = long_thread();
    let transcript = harness.bounds("transcript").expect("the transcript is painted");
    let heading = harness.bounds("thread-heading").expect("the heading is painted");
    let composer = harness.bounds("composer").expect("the composer is painted");

    assert!(transcript.top() <= heading.top(), "{transcript:?} {heading:?}");
    assert!(composer.bottom() <= transcript.bottom(), "{transcript:?} {composer:?}");
    assert_eq!(thread.status(), ThreadStatus::Idle);
}

#[test]
fn the_latest_reply_rests_above_the_composer_however_tall_the_composer_grows() {
    let (mut harness, _) = long_thread();
    let rests_above = |harness: &mut DesktopHarness| {
        let reply = harness.bounds("reply-1").expect("the reply is painted");
        let composer = harness.bounds("composer").expect("the composer is painted");
        assert!(reply.bottom() <= composer.top(), "{reply:?} {composer:?}");
        composer
    };
    let short = rests_above(&mut harness);

    harness.type_text(&"a draft long enough to wrap onto several lines ".repeat(12));

    let tall = rests_above(&mut harness);
    assert!(tall.size.height > short.size.height, "{short:?} {tall:?}");
}

#[test]
fn the_scrollbar_has_its_own_full_height_gutter_as_the_window_and_composer_resize() {
    let (mut harness, _) = long_thread();
    for (width, height) in [(900.0, 600.0), (640.0, 480.0), (1200.0, 800.0)] {
        harness.resize(width, height);
        let transcript = harness.bounds("transcript").unwrap();
        let rows = harness.bounds("transcript-view").unwrap();
        let gutter = harness.bounds("transcript-scrollbar").unwrap();
        let track = harness.bounds("scroll-track-vertical").unwrap();
        let thumb = harness.bounds("scroll-thumb-vertical").unwrap();

        assert_eq!(gutter.size.width, SCROLLBAR_WIDTH, "the scrollbar reserves space beside the rows");
        assert_eq!(gutter.right(), transcript.right());
        assert_eq!(rows.right(), gutter.left(), "the scrollbar never covers the rows");
        assert_eq!(gutter.top(), transcript.top());
        assert_eq!(gutter.bottom(), transcript.bottom());
        assert_eq!(track, gutter, "the track is anchored to its gutter");
        assert!(thumb.left() >= track.left() && thumb.right() <= track.right(), "{thumb:?} {track:?}");
        assert!(thumb.top() >= track.top() && thumb.bottom() <= track.bottom(), "{thumb:?} {track:?}");
    }

    harness.type_text(&"a draft long enough to wrap onto several lines ".repeat(12));
    let composer = harness.bounds("composer").unwrap();
    let track = harness.bounds("scroll-track-vertical").unwrap();
    let header = harness.bounds("view-switch").unwrap();
    assert!(composer.right() <= track.left(), "the composer leaves the gutter clear");
    assert!(header.right() <= track.left(), "the header controls leave the gutter clear");

    harness.click_at(Point::new(track.center().x, track.top() + 1.0));
    assert!(harness.bounds("jump-to-latest").is_some(), "the track is clickable beside the header");
    let prompt = harness.bounds("prompt-0").unwrap();
    let heading = harness.bounds("thread-heading").unwrap();
    assert!(prompt.top() >= heading.bottom(), "clicking the top reveals the first prompt");

    harness.click_at(Point::new(track.center().x, track.bottom() - 1.0));
    assert!(harness.bounds("jump-to-latest").is_none(), "the track is clickable beside the composer");
    let thumb = harness.bounds("scroll-thumb-vertical").unwrap();
    assert_eq!(thumb.bottom(), track.bottom());
}

#[test]
fn the_jump_to_latest_button_sits_at_the_right_edge_of_the_content_above_the_composer() {
    let (mut harness, _) = long_thread();
    for (width, height) in [(640.0, 480.0), (900.0, 600.0), (1600.0, 900.0)] {
        harness.resize(width, height);
        harness.scroll("transcript-view", Point::new(0.0, 200.0)).unwrap();

        let jump = harness.bounds("jump-to-latest").expect("scrolling up shows the jump button");
        let viewport = harness.bounds("transcript-view").unwrap();
        let composer = harness.bounds("composer").unwrap();
        let content_width = (viewport.size.width - 2.0 * GUTTER).min(CONTENT_MAX_WIDTH);
        let content_right = viewport.center().x + content_width / 2.0;
        assert!((jump.right() - content_right).abs() < 0.5, "{jump:?} should align with {content_right}");
        assert!(jump.left() > viewport.center().x, "the button leaves the middle of the text clear");
        assert!(jump.right() < viewport.right(), "the button leaves a gap before the scrollbar");
        assert!(jump.bottom() < composer.top(), "the button clears the composer");

        harness.click("jump-to-latest").unwrap();
        assert!(harness.bounds("jump-to-latest").is_none(), "the right-aligned button still jumps to latest");
    }
}

#[test]
fn the_first_prompt_clears_the_header_when_the_transcript_is_scrolled_to_its_start() {
    let (mut harness, _) = long_thread();

    harness.scroll("transcript-view", Point::new(0.0, 100_000.0)).unwrap();

    let heading = harness.bounds("thread-heading").expect("the heading is painted");
    let prompt = harness.bounds("prompt-0").expect("the prompt is painted");
    assert!(prompt.top() >= heading.bottom(), "{heading:?} {prompt:?}");
}

fn long_thread() -> (DesktopHarness, Thread) {
    let (mut harness, thread) = DesktopHarness::alpha(replying("alpha-session", &"A long reply.\n\n".repeat(80)));
    harness.send(thread, "go");
    (harness, thread)
}
