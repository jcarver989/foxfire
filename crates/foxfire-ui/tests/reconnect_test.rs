use agent_client_protocol::v2::schema::SessionId;
use foxfire_acp::testing::{FakeAgent, run_local};
use foxfire_ui::backend::ConnectFailure;
use foxfire_ui::driver::{OpenedSession, drive};
use foxfire_ui::thread::{Thread, ThreadStatus};
use rouge::reactive::testing::Driver;
use rouge::reactive::{Signal, poll_tasks, set_tokio, signal};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use tokio::runtime::Handle;
use tokio::task::{JoinHandle, yield_now};

mod support;
use support::{fake_agent, open};

#[test]
fn a_dropped_agent_is_reconnected_without_a_click() {
    run_local(async {
        let mut scene = SceneBuilder::default().restored().build();
        scene.until(|thread| thread.status() == ThreadStatus::Idle).await;
        assert!(scene.shows_history());

        scene.drop_agent();
        scene.until(|thread| thread.status() == ThreadStatus::Reconnecting).await;
        assert!(scene.shows_history(), "the transcript stays on screen");
        scene.until(|thread| thread.status() == ThreadStatus::Idle).await;
        assert!(scene.shows_history());
        assert_eq!(scene.resumed.borrow().as_slice(), [None, Some(SessionId::new(SESSION))]);
    });
}

#[test]
fn returning_to_the_window_reconnects_without_waiting() {
    run_local(async {
        let mut scene = SceneBuilder::default().restored().build();
        scene.until(|thread| thread.status() == ThreadStatus::Idle).await;
        scene.drop_agent();
        scene.until(|thread| thread.status() == ThreadStatus::Reconnecting).await;

        scene.focused.set(true);
        assert_eq!(scene.resumed.borrow().len(), 2);
        scene.until(|thread| thread.status() == ThreadStatus::Idle).await;
        assert!(scene.shows_history());
    });
}

#[test]
fn a_failed_attempt_is_followed_by_another() {
    run_local(async {
        let failure = ConnectFailure::Retryable("no network".to_owned());
        let mut scene = SceneBuilder::default().failing(failure).restored().build();
        scene.until(|thread| thread.status() == ThreadStatus::Idle).await;
        scene.drop_agent();
        scene.until(|thread| thread.status() == ThreadStatus::Reconnecting).await;
        scene.until(|thread| thread.status() == ThreadStatus::Idle).await;
        assert_eq!(scene.resumed.borrow().len(), 3);
    });
}

#[test]
fn it_stops_trying_once_the_user_must_sign_in_or_the_session_has_ended() {
    run_local(async {
        for failure in [ConnectFailure::Unauthenticated("sign in".into()), ConnectFailure::Ended("over".into())] {
            let mut scene = SceneBuilder::default().failing(failure.clone()).restored().build();
            scene.until(|thread| thread.status() == ThreadStatus::Idle).await;
            scene.drop_agent();
            scene.until(|thread| thread.status() == ThreadStatus::Errored).await;
            scene.focused.set(true);
            scene.until(|_| true).await;
            assert_eq!((scene.thread.error(), scene.resumed.borrow().len()), (Some(failure), 2));
            assert!(scene.shows_history(), "the transcript stays on screen");
        }
    });
}

#[test]
fn a_first_connection_that_fails_is_left_to_the_user() {
    run_local(async {
        let failure = ConnectFailure::Retryable("no network".to_owned());
        let mut scene = SceneBuilder::unreachable(failure).build();
        scene.until(|thread| thread.status() == ThreadStatus::Errored).await;
        scene.focused.set(true);
        scene.until(|_| true).await;
        assert_eq!(scene.resumed.borrow().len(), 1);
    });
}

const SESSION: &str = "live";

/// What happens on each attempt to connect, in order.
struct SceneBuilder {
    attempts: VecDeque<Result<(), ConnectFailure>>,
}

struct Scene {
    thread: Thread,
    focused: Signal<bool>,
    resumed: Rc<RefCell<Vec<Option<SessionId>>>>,
    agents: Rc<RefCell<Vec<JoinHandle<()>>>>,
    driver: Driver,
}

impl Default for SceneBuilder {
    fn default() -> Self {
        Self { attempts: VecDeque::from([Ok(())]) }
    }
}

impl SceneBuilder {
    fn unreachable(failure: ConnectFailure) -> Self {
        Self { attempts: VecDeque::from([Err(failure)]) }
    }

    fn restored(mut self) -> Self {
        self.attempts.push_back(Ok(()));
        self
    }

    fn failing(mut self, failure: ConnectFailure) -> Self {
        self.attempts.push_back(Err(failure));
        self
    }

    fn build(self) -> Scene {
        let driver = Driver::new();
        set_tokio(Handle::current());
        let (resumed, agents) = (Rc::<RefCell<Vec<_>>>::default(), Rc::<RefCell<Vec<_>>>::default());
        let (asked, serving, attempts) = (resumed.clone(), agents.clone(), RefCell::new(self.attempts));
        let thread = Thread::connect(0, None, None, move |resume| {
            asked.borrow_mut().push(resume.clone());
            let attempt =
                attempts.borrow_mut().pop_front().unwrap_or_else(|| Err(ConnectFailure::Other("spent".into())));
            let (client_end, agent) = fake_agent(FakeAgent::new(SESSION).replay_message(SESSION, "an earlier answer"));
            serving.borrow_mut().push(agent);
            drive(|_| async move {
                attempt?;
                Ok(OpenedSession::from(open(client_end, resume.or(Some(SESSION.into()))).await?))
            })
        });
        let focused = signal(false);
        thread.keep_connected(move || focused.get());
        Scene { thread, focused, resumed, agents, driver }
    }
}

impl Scene {
    fn drop_agent(&self) {
        self.agents.borrow_mut().drain(..).for_each(|agent| agent.abort());
    }

    fn shows_history(&self) -> bool {
        self.thread.with_core(|core| !core.conversation().items().is_empty()).unwrap_or(false)
    }

    async fn until(&mut self, done: impl Fn(Thread) -> bool) {
        loop {
            poll_tasks();
            if done(self.thread) {
                return;
            }
            let _ = self.driver.wakes();
            yield_now().await;
        }
    }
}
