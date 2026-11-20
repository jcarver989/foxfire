use foxfire_ui::backend::{HostedBackend, Project, ThreadId};
use foxfire_ui::memory::MemoryBackend;
use foxfire_ui::platform::ChatPlatform;
use foxfire_web::web_view;
use rouge::core::{
    InputKind, Point, Size,
    testing::{TestError, TestUi},
};
use rouge::persist::MemoryStorage;
use rouge::reactive::Runtime;
use rouge_widgets::testing::component_test;
use std::rc::Rc;
use std::time::Duration;

#[tokio::test]
async fn the_browser_shell_launches_chats_switches_and_stops() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    let backend = MemoryBackend::new(vec![Project { id: "demo".to_owned(), name: "Demo project".to_owned() }])
        .with_reply(vec!["Testing a reply.".to_owned()], Duration::ZERO);
    let ui_backend = backend.clone();
    let mut ui = component_test()
        .size(1120.0, 840.0)
        .build(move || web_view(Rc::new(ui_backend), Rc::new(NoPlatform), Rc::new(MemoryStorage::default())));

    ui.hover_on("project-demo")?;
    ui.run(0.25);
    ui.click_on("launch-demo")?;
    ui.run(1.0);
    assert!(ui.bounds_of("attach-files").is_none());
    ui.click_on("composer-text")?;
    ui.type_text("first prompt");
    assert!(ui.press("enter"));
    ui.run(0.0);
    assert!(ui.text_bounds("Testing a reply.").is_some());

    ui.type_text("/permission");
    assert!(ui.press("enter"));
    ui.run(0.0);
    assert!(ui.bounds_of("thread-demo-1-status-needs-input-speech-bubble").is_some());
    ui.click_on("permission-option-0")?;
    ui.run(0.0);
    assert!(ui.text_bounds("Permission granted.").is_some());

    ui.click_on("composer-text")?;
    ui.type_text("/permission");
    assert!(ui.press("enter"));
    ui.run(0.0);
    assert!(ui.bounds_of("elicitation").is_some());
    assert!(ui.press("escape"));
    ui.run(0.5);
    assert!(ui.bounds_of("elicitation").is_none());

    ui.hover_on("project-demo")?;
    ui.run(0.25);
    ui.click_on("launch-demo")?;
    ui.run(1.0);
    assert!(ui.bounds_of("new-thread-heading").is_some());
    ui.click_on("thread-demo-1")?;
    ui.run(0.0);
    assert!(ui.text_bounds("Permission granted.").is_some());
    assert!(ui.bounds_of("new-thread-heading").is_none());

    ui.click_on("composer-text")?;
    ui.type_text("unsent draft");
    ui.click_on("thread-demo-2")?;
    ui.run(0.0);
    ui.click_on("thread-demo-1")?;
    ui.click_on("refresh-threads")?;
    ui.run(0.0);
    assert!(ui.text_bounds("unsent draft").is_some());

    ui.click_on("thread-demo-2")?;
    ui.run(0.0);
    assert!(backend.stop(ThreadId("demo-2".to_owned())).await.is_ok());
    ui.click_on("refresh-threads")?;
    ui.run(0.0);
    assert!(ui.bounds_of("thread-demo-2").is_none());
    assert!(ui.bounds_of("thread-demo-1").is_some());
    assert!(ui.text_bounds("No thread selected").is_some());

    ui.click_on("thread-demo-1")?;
    ui.click_on("stop-demo-1")?;
    ui.run(0.0);
    assert!(ui.bounds_of("thread-demo-1").is_none());
    assert!(ui.bounds_of("thread-demo-2").is_none());
    assert!(ui.text_bounds("No thread selected").is_some());
    Ok(())
}

#[tokio::test]
async fn the_sidebar_resizes_hides_and_reopens_at_its_previous_width() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    let backend = MemoryBackend::new(vec![Project { id: "demo".to_owned(), name: "Demo project".to_owned() }]);
    let mut ui = component_test()
        .size(1120.0, 840.0)
        .build(move || web_view(Rc::new(backend), Rc::new(NoPlatform), Rc::new(MemoryStorage::default())));

    let initial = ui.rect_of("sidebar")?.size.width;
    let edge = ui.center_of("panel-resize-handle")?;
    ui.drag_on("panel-resize-handle", Point::new(edge.x + 81.0, edge.y))?;
    let wider = ui.rect_of("sidebar")?.size.width;
    assert!(wider > initial);

    ui.click_on("toggle-sidebar")?;
    assert!(ui.bounds_of("sidebar").is_none());
    assert_eq!(ui.rect_of("main")?.size.width, ui.rect_of("hosted")?.size.width);
    ui.click_on("toggle-sidebar")?;
    assert_eq!(ui.rect_of("sidebar")?.size.width, wider);
    ui.run(0.25);
    ui.double_click_on("panel-resize-handle")?;
    assert_eq!(ui.rect_of("sidebar")?.size.width, initial);

    ui.hover_on("project-demo")?;
    ui.run(0.25);
    ui.click_on("launch-demo")?;
    ui.run(1.0);
    ui.click_on("composer-text")?;
    ui.type_text("unsent draft");
    assert!(ui.press("secondary-b"));
    assert!(ui.bounds_of("sidebar").is_none());
    let toggle = ui.rect_of("toggle-sidebar")?;
    let header = ui.rect_of("thread-header")?;
    assert!(header.contains(toggle.center()));
    ui.click_on("toggle-sidebar")?;
    assert!(ui.bounds_of("sidebar").is_some());
    assert!(ui.text_bounds("unsent draft").is_some());
    ui.run(0.25);
    ui.click_on("project-demo-toggle")?;
    assert!(ui.bounds_of("thread-demo-1").is_none());
    ui.press("secondary-b");
    ui.press("secondary-b");
    ui.run(0.25);
    assert!(ui.bounds_of("thread-demo-1").is_none());
    ui.click_on("project-demo-toggle")?;
    assert!(ui.bounds_of("thread-demo-1").is_some());
    Ok(())
}

#[tokio::test]
async fn the_browser_uses_base_interface_size_independently_of_pixel_density() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    for device_scale in [1.0, 2.0] {
        let mut ui = component_test().size(1120.0, 840.0).scale(device_scale).build(|| {
            web_view(Rc::new(MemoryBackend::new(Vec::new())), Rc::new(NoPlatform), Rc::new(MemoryStorage::default()))
        });
        assert!((ui.scale() - device_scale).abs() < f32::EPSILON);
        assert!(ui.press("secondary-="));
        assert!(ui.scale() > device_scale);
        assert!(ui.press("secondary-0"));
        assert!((ui.scale() - device_scale).abs() < f32::EPSILON);
    }
    Ok(())
}

#[tokio::test]
async fn the_phone_shell_opens_threads_and_sends_prompts_with_taps() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    let mut ui = touch_demo(390.0, 780.0);

    assert!(ui.bounds_of("threads-nav-large").is_some());
    assert!(ui.bounds_of("sidebar").is_none());
    ui.tap_on("launch-demo")?;
    ui.run(1.0);
    assert!(ui.bounds_of("conversation-nav-back").is_some());
    assert!(ui.rect_of("send")?.size.height >= 44.0);

    ui.tap_on("composer-text")?;
    assert!(ui.has_focus(), "the composer takes focus on tap");
    ui.type_text("/permission");
    assert!(ui.text_bounds("/permission").is_some(), "the composer receives typed text");
    ui.tap_on("send")?;
    ui.run(0.0);
    assert!(ui.bounds_of("elicitation").is_some());
    ui.tap_on("conversation-nav-back")?;
    ui.run(0.5);
    assert!(ui.bounds_of("composer").is_none());
    assert!(!ui.accepts_text());
    ui.tap_on("thread-demo-1")?;
    ui.run(0.5);
    assert!(ui.bounds_of("elicitation").is_some(), "Back doesn't cancel the current turn");
    ui.tap_on("stop")?;
    ui.run(0.5);
    assert!(ui.bounds_of("elicitation").is_none());

    ui.tap_on("composer-text")?;
    ui.type_text("unsent draft");
    ui.press("enter");
    ui.type_text("second line");
    assert!(ui.text_bounds("unsent draft").is_some(), "Return doesn't send a touch draft");
    ui.swipe(Point::new(8.0, 180.0), Point::new(355.0, 180.0));
    ui.run(0.5);
    assert!(ui.bounds_of("composer").is_none());
    assert!(!ui.accepts_text());
    ui.tap_on("thread-demo-1")?;
    ui.run(0.5);
    assert!(ui.text_bounds("unsent draft").is_some());
    assert!(ui.text_bounds("second line").is_some());
    Ok(())
}

#[tokio::test]
async fn resizing_the_touch_conversation_keeps_the_draft_and_lifts_send_above_the_keyboard() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    let mut ui = touch_demo(390.0, 780.0);
    ui.tap_on("launch-demo")?;
    ui.run(1.0);
    ui.tap_on("composer-text")?;
    ui.type_text("draft across rotation");
    let original = ui.rect_of("send")?;

    ui.set_keyboard_inset(290.0);
    ui.run(1.0);
    let send = ui.rect_of("send")?;
    assert!(send.max().y <= 490.0);
    assert!(original.max().y > send.max().y);
    assert!(ui.rect_of("conversation-surface")?.origin.y >= ui.rect_of("conversation-nav")?.max().y);

    ui.set_keyboard_inset(0.0);
    ui.set_size(Size::new(850.0, 430.0));
    ui.run(1.0);
    assert!(ui.text_bounds("draft across rotation").is_some());
    ui.tap_on("send")?;
    ui.run(1.0);
    assert!(ui.text_bounds("draft across rotation").is_some(), "the sent draft appears in the transcript");
    let transcript = ui.rect_of("transcript-view")?;
    assert!(transcript.origin.y >= ui.rect_of("conversation-nav")?.max().y);
    assert!(transcript.max().y <= ui.rect_of("composer")?.origin.y);
    ui.tap_on("conversation-nav-back")?;
    ui.run(1.0);
    assert!(ui.rect_of("project-demo")?.size.width < 850.0);
    Ok(())
}

#[tokio::test]
async fn ending_a_thread_requires_confirmation_and_only_ends_that_thread() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    let mut ui = touch_demo(390.0, 780.0);
    ui.tap_on("launch-demo")?;
    ui.run(1.0);
    ui.tap_on("composer-text")?;
    assert!(ui.accepts_text());
    ui.type_text("first thread draft");
    assert!(ui.text_bounds("first thread draft").is_some());
    ui.tap_on("conversation-nav-back")?;
    ui.run(1.0);
    ui.tap_on("launch-demo")?;
    ui.run(1.0);
    assert!(ui.text_bounds("first thread draft").is_none());

    ui.tap_on("end-thread")?;
    ui.run(1.0);
    ui.tap_on("end-thread-confirmation-cancel")?;
    ui.run(1.0);
    assert!(ui.bounds_of("end-thread").is_some());
    ui.tap_on("end-thread")?;
    ui.run(1.0);
    ui.tap_on("end-thread-confirmation-action-0")?;
    ui.run(1.0);
    assert!(ui.bounds_of("conversation-screen").is_none());
    assert!(ui.bounds_of("thread-demo-2").is_none());
    assert!(ui.bounds_of("thread-demo-1").is_some());
    ui.tap_on("thread-demo-1")?;
    ui.run(1.0);
    assert!(ui.bounds_of("conversation-screen").is_some());
    assert!(ui.text_bounds("first thread draft").is_some());
    Ok(())
}

#[tokio::test]
async fn swiping_a_thread_row_offers_end_thread_behind_the_same_confirmation() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    let mut ui = touch_demo(390.0, 780.0);
    ui.tap_on("launch-demo")?;
    ui.run(1.0);
    ui.tap_on("conversation-nav-back")?;
    ui.run(1.0);

    let row = ui.rect_of("thread-demo-1")?;
    let (y, right) = (row.center().y, row.max().x);
    ui.swipe_over(Point::new(right - 20.0, y), Point::new(right - 200.0, y), 0.5);
    ui.run(2.0);
    ui.tap_on("end-demo-1")?;
    ui.run(1.0);
    ui.tap_on("end-thread-confirmation-cancel")?;
    ui.run(1.0);
    assert!(ui.bounds_of("thread-demo-1").is_some(), "dismissing the alert keeps the thread");

    ui.swipe_over(Point::new(right - 20.0, y), Point::new(right - 200.0, y), 0.5);
    ui.run(2.0);
    ui.tap_on("end-demo-1")?;
    ui.run(1.0);
    ui.tap_on("end-thread-confirmation-action-0")?;
    ui.run(1.0);
    assert!(ui.bounds_of("thread-demo-1").is_none());
    assert!(ui.bounds_of("threads-screen").is_some());
    Ok(())
}

#[tokio::test]
async fn a_regular_touch_width_keeps_the_stack_and_a_readable_list() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    let mut ui = touch_demo(820.0, 1180.0);
    assert!(ui.bounds_of("sidebar").is_none());
    assert!(ui.bounds_of("threads-nav-large").is_some());
    assert!(ui.rect_of("project-demo")?.size.width <= 600.0);

    ui.tap_on("launch-demo")?;
    ui.run(1.0);
    assert!(ui.bounds_of("conversation-screen").is_some());
    assert!(ui.rect_of("send")?.size.height >= 44.0);
    ui.tap_on("composer-text")?;
    ui.type_text("tablet draft");
    ui.set_size(Size::new(1180.0, 820.0));
    ui.run(1.0);
    assert!(ui.bounds_of("conversation-screen").is_some(), "rotation keeps the conversation open");
    assert!(ui.text_bounds("tablet draft").is_some());
    ui.tap_on("conversation-nav-back")?;
    ui.run(1.0);
    assert!(ui.rect_of("project-demo")?.size.width <= 600.0);
    Ok(())
}

#[tokio::test]
async fn malformed_and_missing_thread_routes_can_return_to_threads() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    let mut ui = touch_demo(390.0, 780.0);
    for path in ["/thread/invalid!", "/thread/bWlzc2luZw"] {
        ui.set_route(path);
        ui.run(1.0);
        assert!(ui.text_bounds("Thread unavailable").is_some());
        assert!(ui.bounds_of("composer").is_none());
        ui.tap_on("conversation-nav-back")?;
        ui.run(1.0);
        assert!(ui.bounds_of("launch-demo").is_some());
    }
    Ok(())
}

fn touch_demo(width: f32, height: f32) -> TestUi {
    let backend = MemoryBackend::new(vec![Project { id: "demo".to_owned(), name: "Demo project".to_owned() }]);
    component_test()
        .size(width, height)
        .input(InputKind::Touch)
        .build(move || web_view(Rc::new(backend), Rc::new(NoPlatform), Rc::new(MemoryStorage::default())))
}

struct NoPlatform;

impl ChatPlatform for NoPlatform {
    fn open(&self, _: &str) {}
}
