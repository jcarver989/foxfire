use foxfire_acp::chat::Participant;
use foxfire_acp::testing::{FakeAgent, run_local};
use foxfire_acp::websocket::Endpoint;
use foxfire_relay::Relay;
use foxfire_ui::platform::ChatPlatform;
use foxfire_web::relay_view;
use rouge::core::InputKind;
use rouge::core::testing::{TestError as UiError, TestUi};
use rouge::persist::MemoryStorage;
use rouge::reactive::{Entered, Runtime, set_tokio};
use rouge_widgets::testing::component_test;
use std::io;
use std::rc::Rc;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::task::yield_now;

#[test]
fn a_conversation_is_held_with_a_relay() -> Result<(), TestError> {
    run_local(async {
        let agent = FakeAgent::new("live").prompt_reply("Hello from the relay.");
        let mut browser = Browser::open(agent, InputKind::Pointer).await?;
        browser.ui.hover_on("project-relay")?;
        browser.ui.run(0.25);
        assert!(browser.ui.bounds_of("launch-relay").is_none());
        browser.ui.hover_on("thread-relay")?;
        browser.ui.run(0.25);
        assert!(browser.ui.bounds_of("stop-relay").is_none());
        browser.ui.click_on("thread-relay")?;
        browser.wait_for_session().await;
        browser.ui.run(1.0);
        browser.ui.click_on("composer-text")?;
        browser.ui.type_text("hello?");
        assert!(browser.ui.press("enter"));
        browser.until(|ui| ui.text_bounds("Hello from the relay.").is_some()).await;
        Ok(())
    })
}

#[test]
fn a_touch_relay_keeps_its_draft_when_reopened() -> Result<(), TestError> {
    run_local(async {
        let mut browser = Browser::open(FakeAgent::new("live"), InputKind::Touch).await?;
        assert!(browser.ui.bounds_of("launch-relay").is_none());
        browser.ui.tap_on("thread-relay")?;
        browser.wait_for_session().await;
        let ui = &mut browser.ui;
        ui.run(1.0);
        assert!(ui.bounds_of("end-thread").is_none());
        ui.tap_on("composer-text")?;
        assert!(ui.accepts_text(), "composer at {:?}", ui.rect_of("composer-text")?);
        ui.type_text("unsent relay draft");
        assert!(ui.text_bounds("unsent relay draft").is_some());
        ui.tap_on("conversation-nav-back")?;
        ui.run(0.5);
        assert!(ui.bounds_of("composer").is_none());
        ui.tap_on("thread-relay")?;
        ui.run(0.5);
        assert!(ui.text_bounds("unsent relay draft").is_some());
        Ok(())
    })
}

#[derive(Debug, thiserror::Error)]
enum TestError {
    #[error(transparent)]
    Ui(#[from] UiError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

struct Browser {
    ui: TestUi,
    _entered: Entered,
    _workspace: TempDir,
}

struct NoPlatform;

impl ChatPlatform for NoPlatform {
    fn open(&self, _: &str) {}
}

impl Browser {
    async fn open(agent: FakeAgent, input: InputKind) -> Result<Self, TestError> {
        let workspace = TempDir::new()?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = Endpoint::new(format!("ws://{}", listener.local_addr()?));
        tokio::spawn(Relay::new(workspace.path().to_path_buf()).serve(listener, agent.serve()));
        let entered = Runtime::new().enter();
        set_tokio(Handle::current());
        let you = Participant { id: "you".to_owned(), name: "You".to_owned() };
        let ui = component_test()
            .size(if input == InputKind::Touch { 390.0 } else { 1120.0 }, 840.0)
            .input(input)
            .build(move || relay_view(endpoint, you, Rc::new(NoPlatform), Rc::new(MemoryStorage::default())));
        let mut browser = Self { ui, _entered: entered, _workspace: workspace };
        browser.until(|ui| ui.bounds_of("thread-relay").is_some()).await;
        Ok(browser)
    }

    async fn wait_for_session(&mut self) {
        self.until(|ui| ui.bounds_of("composer-text").is_some() && ui.bounds_of("new-thread-connecting").is_none())
            .await;
    }

    async fn until(&mut self, done: impl Fn(&mut TestUi) -> bool) {
        loop {
            self.ui.run(0.0);
            if done(&mut self.ui) {
                return;
            }
            yield_now().await;
        }
    }
}
