use crate::support::{Edges, asked, asking_name, replying};
use foxfire_desktop::sidebar::MIN_MAIN_WIDTH;
use foxfire_desktop::testing::{DesktopHarness, DesktopHarnessBuilder};
use foxfire_desktop::threads::{Thread, ThreadStatus};
use foxfire_desktop::ui::tokens::{
    BASE_SIZE, CONTENT_MAX_WIDTH, GRID_COLUMNS, GRID_GUTTER, GRID_MARGIN, SCROLLBAR_WIDTH, UNIT,
};
use rouge::core::Point;
use rouge::core::testing::TestError;
use rouge::widgets::tokens::MotionScale;

#[test]
fn widening_the_sidebar_leaves_room_for_the_thread() {
    let mut harness = DesktopHarnessBuilder::default().window_size(640.0, 480.0).build();
    let min_main = MIN_MAIN_WIDTH;

    harness.drag("panel-resize-handle", Point::new(600.0, 240.0)).unwrap();

    let main = harness.bounds("main").expect("the main region is painted");
    assert!((main.size.width - min_main).abs() < 0.5, "{main:?}");
}

#[test]
fn a_dragged_sidebar_edge_settles_on_the_unit() -> Result<(), TestError> {
    for x in [301.0, 302.0, 303.0] {
        let mut harness = at_base_size(DesktopHarnessBuilder::default().window_size(1000.0, 480.0).build());

        harness.drag("panel-resize-handle", Point::new(x, 240.0))?;

        let sidebar = harness.bounds("sidebar").expect("the sidebar is painted");
        assert_eq!(sidebar.size.width % UNIT, 0.0, "dragged to {x}: {sidebar:?}");
        assert!((sidebar.size.width - x).abs() <= UNIT, "the edge stays under the pointer at {x}: {sidebar:?}");
    }
    Ok(())
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
        assert!((jump.right() - composer.right()).abs() < 0.5, "{jump:?} should end where {composer:?} does");
        assert!(jump.left() > viewport.center().x, "the button leaves the middle of the text clear");
        assert!(jump.right() < viewport.right(), "the button leaves a gap before the scrollbar");
        assert!(jump.bottom() < composer.top(), "the button clears the composer");

        harness.click("jump-to-latest").unwrap();
        assert!(harness.bounds("jump-to-latest").is_none(), "the right-aligned button still jumps to latest");
    }
}

#[test]
fn the_reading_column_spans_whole_columns_and_lines_the_reply_up_with_the_composer() {
    let (mut harness, _) = long_thread();
    for (width, height) in [(900.0, 600.0), (1280.0, 800.0), (1600.0, 900.0), (2400.0, 1200.0)] {
        harness.resize(width, height);
        let main = harness.bounds("main").unwrap();
        let reply = harness.bounds("reply-1").unwrap();
        let composer = harness.bounds("composer").unwrap();
        let column = (main.size.width - SCROLLBAR_WIDTH) / f32::from(GRID_COLUMNS);
        let before = (composer.left() - main.left() - GRID_MARGIN.px()) / column;
        let spanned = (composer.size.width + GRID_GUTTER.px()) / column;

        assert!((before - before.round()).abs() < 0.01, "at {width} the column starts {before} columns in");
        assert!((spanned - spanned.round()).abs() < 0.01, "at {width} the column spans {spanned} columns");
        assert!((before * 2.0 + spanned - f32::from(GRID_COLUMNS)).abs() < 0.01, "at {width} the column is centred");
        assert!(composer.size.width <= CONTENT_MAX_WIDTH, "at {width} lines stay readable: {composer:?}");
        assert!((reply.left() - composer.left()).abs() < 0.5, "at {width}: {reply:?} {composer:?}");
        assert!((reply.right() - composer.right()).abs() < 0.5, "at {width}: {reply:?} {composer:?}");
    }
}

#[test]
fn the_header_controls_sit_on_the_outer_columns_of_the_grid_the_transcript_is_laid_out_on() {
    let (mut harness, _) = long_thread();
    for (width, height) in [(900.0, 600.0), (1600.0, 900.0)] {
        harness.resize(width, height);
        let main = harness.bounds("main").unwrap();
        let track = harness.bounds("scroll-track-vertical").unwrap();
        let heading = harness.bounds("thread-heading").unwrap();
        let switch = harness.bounds("view-switch").unwrap();

        assert!((heading.left() - main.left() - GRID_MARGIN.px()).abs() < 0.5, "at {width}: {heading:?}");
        assert!((track.left() - switch.right() - GRID_MARGIN.px()).abs() < 0.5, "at {width}: {switch:?} {track:?}");
    }
}

#[test]
fn a_long_prompt_ends_with_the_reading_column_and_stays_out_of_its_first_column() {
    let (mut harness, thread) = DesktopHarness::alpha(replying("alpha-session", "ok"));
    harness.send(thread, &"a prompt long enough to wrap ".repeat(40));
    for (width, height) in [(900.0, 600.0), (1600.0, 900.0)] {
        harness.resize(width, height);
        let main = harness.bounds("main").unwrap();
        let gutter = harness.bounds("scroll-track-vertical").map_or(0.0, |track| track.size.width);
        let column = (main.size.width - gutter) / f32::from(GRID_COLUMNS);
        let prompt = harness.bounds("prompt-0").unwrap();
        let composer = harness.bounds("composer").unwrap();

        assert!(prompt.left() >= composer.left() + column - 0.5, "at {width}: {prompt:?} {composer:?}");
        assert!(prompt.left() < composer.left() + 2.0 * column, "at {width}: {prompt:?} {composer:?}");
        assert!((prompt.right() - composer.right()).abs() < 0.5, "at {width}: {prompt:?} {composer:?}");
    }
}

#[test]
fn the_sidebar_slides_back_in_unless_motion_is_reduced() -> Result<(), TestError> {
    for reduced in [false, true] {
        let mut harness = DesktopHarnessBuilder::default().motion(MotionScale::Full).build();
        harness.registry().update(|state| state.reduced_motion = reduced);
        harness.press("secondary-b");
        harness.ui().run(1.0);

        harness.press("secondary-b");
        harness.ui().draw()?;

        assert!(harness.bounds("sidebar").is_some());
        let frames = harness.ui().compositor_frames(1.0)?;
        assert_eq!(frames == 0, reduced, "the sidebar returned over {frames} frames with reduced motion {reduced}");
    }
    Ok(())
}

#[test]
fn a_shortcut_draws_the_column_grid_over_the_thread_and_leaves_the_scrollbar_clear() {
    let (mut harness, _) = long_thread();
    assert!(harness.bounds("grid-overlay").is_none());

    harness.press("secondary-alt-g");

    let overlay = harness.bounds("grid-overlay").expect("the shortcut shows the grid");
    let main = harness.bounds("main").unwrap();
    let track = harness.bounds("scroll-track-vertical").unwrap();
    assert_eq!((overlay.left(), overlay.right()), (main.left(), track.left()));

    harness.press("secondary-alt-g");
    assert!(harness.bounds("grid-overlay").is_none());
}

#[test]
fn the_first_prompt_clears_the_header_when_the_transcript_is_scrolled_to_its_start() {
    let (mut harness, _) = long_thread();

    harness.scroll("transcript-view", Point::new(0.0, 100_000.0)).unwrap();

    let heading = harness.bounds("thread-heading").expect("the heading is painted");
    let prompt = harness.bounds("prompt-0").expect("the prompt is painted");
    assert!(prompt.top() >= heading.bottom(), "{heading:?} {prompt:?}");
}

#[test]
fn every_named_part_of_the_window_is_a_whole_number_of_units_tall() {
    for scene in ["a question", "a long thread", "settings"] {
        let mut harness = at_base_size(showing(scene));
        let astray: Vec<String> = harness
            .ui()
            .named_bounds()
            .into_iter()
            .filter(|(name, bounds)| {
                !FLOWS_WITH_TEXT.iter().any(|prefix| name.starts_with(prefix)) && bounds.size.height % UNIT != 0.0
            })
            .map(|(name, bounds)| format!("{name}: {} tall", bounds.size.height))
            .collect();
        assert!(astray.is_empty(), "off the unit in {scene}: {astray:#?}");
    }
}

fn long_thread() -> (DesktopHarness, Thread) {
    let (mut harness, thread) = DesktopHarness::alpha(replying("alpha-session", &"A long reply.\n\n".repeat(80)));
    harness.send(thread, "go");
    (harness, thread)
}

fn showing(scene: &str) -> DesktopHarness {
    match scene {
        "a question" => asked(asking_name()).0,
        "a long thread" => long_thread().0,
        _ => {
            let mut harness = DesktopHarness::alpha(replying("alpha-session", "hello")).0;
            harness.press("secondary-,");
            harness
        }
    }
}

/// Linux starts at a larger interface size than macOS; the unit is only whole pixels at the size the UI is drawn at.
fn at_base_size(mut harness: DesktopHarness) -> DesktopHarness {
    while harness.interface_size() > BASE_SIZE {
        harness.press("secondary--");
    }
    harness
}

/// A reply's height follows its Markdown, whose block rhythm is not on the unit.
const FLOWS_WITH_TEXT: [&str; 2] = ["reply-", "transcript-row-"];
