use foxfire_chat::ChatError;
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
    assert!(ui.bounds_of("toggle-chat-room").is_none(), "nobody shares the thread");
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
async fn the_footer_holds_its_status_with_a_readable_label_on_a_phone_and_a_tablet() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    let mut ui = touch_demo(390.0, 780.0);
    ui.tap_on("launch-demo")?;
    ui.run(1.0);
    assert!(ui.bounds_of("status").is_none(), "a thread with nothing sent has no status");

    ui.tap_on("composer-text")?;
    ui.type_text("/permission");
    ui.tap_on("send")?;
    ui.run(0.5);

    for (width, height) in [(390.0, 780.0), (1180.0, 820.0)] {
        ui.set_size(Size::new(width, height));
        ui.run(0.5);
        let (bar, status) = (ui.rect_of("composer-bar")?, ui.rect_of("status")?);
        assert!(status.origin.x >= bar.origin.x && status.max().x <= bar.max().x, "at {width}: {bar:?} {status:?}");
        assert!(status.origin.y >= bar.origin.y && status.max().y <= bar.max().y, "at {width}: {bar:?} {status:?}");
        let (label, mark) = (ui.rect_of("status-label")?, ui.rect_of("activity")?);
        assert!(label.size.width > 2.0 * mark.size.width, "at {width} the label is readable: {label:?}");
    }
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

#[tokio::test]
async fn messages_are_handed_from_the_side_room_to_the_agent_and_back() -> Result<(), ChatTestError> {
    let _entered = Runtime::new().enter();
    let mut ui = shared_thread(&shared_backend(), &MemoryStorage::default())?;
    assert!(ui.bounds_of("thread-presence").is_some());
    ui.click_on("chat-room-input")?;
    ui.type_text("Cap backoff at `30s`");
    assert!(ui.press("enter"));
    ui.run(0.2);
    assert!(ui.bounds_of("chat-post-3").is_some());

    ui.hover_on("chat-post-1")?;
    ui.run(0.2);
    ui.click_on("chat-select-1")?;
    ui.click_on("chat-post-2")?;
    ui.click_on("chat-room-ask-agent")?;
    ui.run(0.1);
    assert!(ui.bounds_of("chat-room-selection-bar").is_none());
    assert!(ui.text_bounds("2 messages from Maya and Theo").is_some());
    assert!(ui.press("enter"));
    ui.run(0.2);
    assert!(ui.bounds_of("composer-chat-sources").is_none());
    assert!(ui.bounds_of("prompt-author-0").is_some());
    assert!(ui.text_bounds("Channel messages").is_some(), "the prompt is not an empty bubble");
    assert!(ui.text_bounds("Moved backoff into the driver.").is_some());

    ui.click_on("close-chat-room")?;
    ui.run(0.1);
    ui.hover_on("transcript-row-1")?;
    ui.run(0.25);
    ui.click_on("discuss-1")?;
    ui.run(0.2);
    assert!(ui.bounds_of("chat-room-anchor-chip").is_some());
    ui.type_text("Does this need a test?");
    assert!(ui.press("enter"));
    ui.run(0.2);
    assert!(ui.bounds_of("chat-room-anchor-chip").is_none());
    ui.click_on("chat-anchor-5")?;
    ui.run(0.2);
    assert!(ui.bounds_of("jump-to-latest").is_some(), "the transcript shows what was quoted");
    ui.hover_on("transcript-row-0")?;
    ui.run(0.25);
    ui.click_on("discuss-0")?;
    ui.run(0.2);
    assert!(ui.press("escape"));
    ui.run(0.1);
    assert!(ui.bounds_of("chat-room-anchor-chip").is_none());
    Ok(())
}

#[tokio::test]
async fn the_side_room_follows_new_messages_and_marks_only_what_is_shown_read() -> Result<(), ChatTestError> {
    let _entered = Runtime::new().enter();
    let backend = shared_backend();
    let thread = ThreadId("demo-1".to_owned());
    let mut ui = shared_thread(&backend, &MemoryStorage::default())?;
    ui.hover_on("chat-post-1")?;
    ui.run(0.2);
    ui.click_on("chat-select-1")?;
    ui.click_on("chat-room-ask-agent")?;
    ui.run(0.1);
    assert!(ui.press("enter"));
    for number in 0..60 {
        backend.post(&thread, &format!("filler {number}"))?;
    }
    ui.run(0.5);
    let room = ui.rect_of("chat-room-messages")?;
    let newest = ui.rect_of("chat-post-63")?;
    assert!(newest.max().y <= room.max().y, "{newest:?} is below {room:?}");

    ui.click_on("prompt-author-0")?;
    ui.run(0.5);
    let source = ui.rect_of("chat-post-1")?;
    assert!(source.origin.y >= room.origin.y, "{source:?} is above {room:?}");

    backend.post(&thread, "unseen")?;
    ui.run(0.5);
    ui.click_on("close-chat-room")?;
    ui.run(0.1);
    assert!(ui.bounds_of("chat-room-unread").is_some());
    ui.click_on("toggle-chat-room")?;
    ui.run(0.5);
    assert!(ui.bounds_of("chat-post-64").is_some());
    ui.click_on("close-chat-room")?;
    ui.run(0.1);
    assert!(ui.bounds_of("chat-room-unread").is_none());
    Ok(())
}

#[tokio::test]
async fn the_side_room_keeps_its_width() -> Result<(), ChatTestError> {
    let _entered = Runtime::new().enter();
    let storage = MemoryStorage::default();
    let mut ui = shared_thread(&shared_backend(), &storage)?;
    let initial = ui.rect_of("chat-room-resizable")?;
    let edge = Point::new(initial.origin.x, initial.center().y);
    ui.drag(edge, Point::new(edge.x - 40.0, edge.y));
    ui.run(1.0);
    let wider = ui.rect_of("chat-room-resizable")?.size.width;
    assert!(wider > initial.size.width);
    drop(ui);
    let mut restored = shared_thread(&shared_backend(), &storage)?;
    assert_eq!(restored.rect_of("chat-room-resizable")?.size.width, wider);
    Ok(())
}

#[tokio::test]
async fn the_phone_shell_opens_the_side_room_as_a_sheet() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    let mut ui = demo(&shared_backend(), &MemoryStorage::default(), InputKind::Touch, (390.0, 844.0));
    ui.tap_on("launch-demo")?;
    ui.run(1.0);
    assert!(ui.bounds_of("chat-room-sheet").is_none(), "the room never covers a phone unasked");
    ui.tap_on("toggle-chat-room")?;
    ui.run(0.5);
    assert!(ui.bounds_of("chat-room-sheet").is_some());
    assert!(ui.bounds_of("chat-post-1").is_some());
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum ChatTestError {
    #[error(transparent)]
    Ui(#[from] TestError),
    #[error(transparent)]
    Chat(#[from] ChatError),
}

fn shared_backend() -> MemoryBackend {
    MemoryBackend::new(vec![Project { id: "demo".to_owned(), name: "Demo project".to_owned() }])
        .with_reply(vec!["Moved backoff into the driver.".to_owned()], Duration::ZERO)
        .with_chat()
}

fn shared_thread(backend: &MemoryBackend, storage: &MemoryStorage) -> Result<TestUi, TestError> {
    let mut ui = demo(backend, storage, InputKind::Pointer, (1360.0, 840.0));
    ui.hover_on("project-demo")?;
    ui.run(0.25);
    ui.click_on("launch-demo")?;
    ui.run(1.0);
    Ok(ui)
}

fn touch_demo(width: f32, height: f32) -> TestUi {
    let backend = MemoryBackend::new(vec![Project { id: "demo".to_owned(), name: "Demo project".to_owned() }]);
    demo(&backend, &MemoryStorage::default(), InputKind::Touch, (width, height))
}

fn demo(backend: &MemoryBackend, storage: &MemoryStorage, input: InputKind, (width, height): (f32, f32)) -> TestUi {
    let (backend, storage) = (backend.clone(), storage.clone());
    component_test()
        .size(width, height)
        .input(input)
        .build(move || web_view(Rc::new(backend), Rc::new(NoPlatform), Rc::new(storage)))
}

struct NoPlatform;

impl ChatPlatform for NoPlatform {
    fn open(&self, _: &str) {}
}
