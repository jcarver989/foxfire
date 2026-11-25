use agent_client_protocol::v2::schema::{Implementation, SessionId};
use foxfire_acp::client::AcpEvent;
use foxfire_acp::testing::{Channel, FakeAgent, run_local};
use foxfire_core::connection::{Connection, ConnectionError, Session};
use foxfire_core::session_core::Input;
use foxfire_ui::backend::{ConnectFailure, SessionEvent, SessionLink};
use foxfire_ui::driver::{OpenedSession, drive};
use rouge::reactive::{Runtime, set_tokio};
use std::future::pending;
use tokio::runtime::Handle;
use tokio::sync::oneshot::{self, error::RecvError};
use tokio::task::{JoinError, JoinHandle, spawn_local};

#[test]
fn a_session_is_handed_over_before_its_review_is_found() -> Result<(), TestError> {
    driven(async {
        let (found, review) = oneshot::channel();
        let (mut link, _agent) = LinkBuilder::default().review(review).open();

        assert_eq!(connected(&mut link).await?, SessionId::new(SESSION));
        assert!(link.events.is_empty());

        let _ = found.send(());
        assert!(matches!(next(&mut link).await?, SessionEvent::OpenReview));
        Ok(())
    })
}

#[test]
fn an_instance_is_reported_even_if_its_session_fails_to_open() -> Result<(), TestError> {
    driven(async {
        let failure = ConnectFailure::Unauthenticated("sign in".to_owned());
        let (mut link, _agent) = LinkBuilder::default().launching("vm-1").failing(failure.clone()).open();

        assert!(matches!(next(&mut link).await?, SessionEvent::Launched(id) if id == "vm-1"));
        assert!(matches!(next(&mut link).await?, SessionEvent::ConnectFailed(reason) if reason == failure));
        assert!(link.events.recv().await.is_none());
        Ok(())
    })
}

#[test]
fn a_link_ends_when_its_agent_disconnects() -> Result<(), TestError> {
    driven(async {
        let (mut link, agent) = LinkBuilder::default().open();
        connected(&mut link).await?;

        agent.abort();

        while !matches!(next(&mut link).await?, SessionEvent::Input(Input::Event(AcpEvent::ConnectionClosed))) {}
        assert!(link.events.recv().await.is_none());
        Ok(())
    })
}

#[test]
fn dropping_a_link_disconnects_its_agent() -> Result<(), TestError> {
    driven(async {
        let (mut link, agent) = LinkBuilder::default().open();
        connected(&mut link).await?;

        drop(link);

        agent.await?;
        Ok(())
    })
}

#[test]
fn dropping_a_link_abandons_a_session_that_is_still_opening() -> Result<(), TestError> {
    driven(async {
        let (opening, abandoned) = oneshot::channel::<()>();
        let link = drive(|_| async move {
            let _opening = opening;
            pending().await
        });

        drop(link);

        assert!(abandoned.await.is_err());
        Ok(())
    })
}

#[derive(Debug, thiserror::Error)]
enum TestError {
    #[error("the link ended early")]
    LinkEnded,
    #[error("expected a connected session")]
    NotConnected,
    #[error(transparent)]
    Agent(#[from] JoinError),
}

const SESSION: &str = "live";

#[derive(Default)]
struct LinkBuilder {
    instance: Option<String>,
    failure: Option<ConnectFailure>,
    review: Option<oneshot::Receiver<()>>,
}

impl LinkBuilder {
    fn launching(mut self, instance: &str) -> Self {
        self.instance = Some(instance.to_owned());
        self
    }

    fn failing(mut self, failure: ConnectFailure) -> Self {
        self.failure = Some(failure);
        self
    }

    /// Finds nowhere else to review the session, once it is told to.
    fn review(mut self, found: oneshot::Receiver<()>) -> Self {
        self.review = Some(found);
        self
    }

    /// A link to a fake agent, and the task the agent runs in.
    fn open(self) -> (SessionLink, JoinHandle<()>) {
        let Self { instance, failure, review } = self;
        let (agent_end, client_end) = Channel::duplex();
        let agent = spawn_local(async move {
            let _ = FakeAgent::new(SESSION).agent().connect_to(agent_end).await;
        });
        let link = drive(|launches| async move {
            if let Some(instance) = instance {
                launches.launched(instance);
            }
            if let Some(failure) = failure {
                return Err(failure);
            }
            let opened = OpenedSession::from(open(client_end).await?);
            Ok(match review {
                Some(found) => opened.reviewed_at(async move {
                    let _: Result<(), RecvError> = found.await;
                    None
                }),
                None => opened,
            })
        });
        (link, agent)
    }
}

async fn open(agent: Channel) -> Result<Session, ConnectionError> {
    Connection::connect_to(Implementation::new("foxfire-test", "1"), agent).await?.open(None, "/work".into()).await
}

fn driven<T>(test: impl Future<Output = T>) -> T {
    run_local(async {
        let _entered = Runtime::new().enter();
        set_tokio(Handle::current());
        test.await
    })
}

async fn next(link: &mut SessionLink) -> Result<SessionEvent, TestError> {
    link.events.recv().await.ok_or(TestError::LinkEnded)
}

async fn connected(link: &mut SessionLink) -> Result<SessionId, TestError> {
    match next(link).await? {
        SessionEvent::Connected(config) => Ok(config.session_response.session_id),
        _ => Err(TestError::NotConnected),
    }
}
