use agent_client_protocol::schema::MaybeUndefined;

use agent_client_protocol::v2::schema as acp;
use foxfire_acp::client::AcpEvent;
use foxfire_acp::content::first_text;
use foxfire_core::command::{AgentCommand, Command, CommandResult, RequestId};
use foxfire_core::elicitation::{ElicitationAnswer, PendingElicitation};
use foxfire_core::session_core::Input;
use foxfire_ui::backend::{
    BackendError, ConnectFailure, HostedBackend, HostedThread, Project, SessionEvent, SessionLink,
};
use foxfire_ui::memory::MemoryBackend;
use rouge::reactive::{Runtime, poll_tasks};
use std::time::Duration;
use tokio::sync::mpsc::error::TryRecvError;

#[tokio::test]
async fn permission_is_delivered_to_the_prompt_origin_and_can_be_answered() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    let (mut origin, mut observer, _, question) = asking().await?;
    question.answer(ElicitationAnswer::Choose("allow".into()));
    assert!(
        pending(&mut observer)
            .into_iter()
            .all(|event| matches!(event, SessionEvent::Input(Input::Event(AcpEvent::SessionUpdate(_)))))
    );
    let outcome = pending(&mut origin);
    assert!(outcome.iter().any(|event| matches!(
        event,
        SessionEvent::Input(Input::Result(CommandResult::Prompt { result: Ok(()), .. }))
    )));
    assert_eq!(messages(outcome), ["Permission granted."]);
    Ok(())
}

#[tokio::test]
async fn another_attachment_can_cancel_a_pending_permission() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    let (mut origin, observer, id, question) = asking().await?;
    send(&observer, AgentCommand::Cancel { session_id: id.clone() })?;
    let outcome = pending(&mut origin);
    assert!(outcome.iter().any(|event| matches!(event, SessionEvent::Input(Input::Result(CommandResult::ElicitationWithdrawn(id))) if id == &question.request_id)));
    assert_eq!(messages(outcome), [] as [String; 0]);

    prompt(&origin, &id, "next prompt")?;
    assert_eq!(
        messages(pending(&mut origin)),
        ["next prompt", "**Demo agent** received: next prompt", "Scripted reply"]
    );
    Ok(())
}

#[tokio::test]
async fn a_new_attachment_replays_only_its_thread_history_once() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    let backend = backend();
    let by_project = backend.projects().await?;
    assert!(matches!(backend.launch("unknown".into()).await, Err(BackendError::UnknownProject(_))));
    let first = backend.launch(by_project[0].id.clone()).await?;
    let second = backend.launch(by_project[0].id.clone()).await?;
    assert_eq!(backend.list_threads().await?, [first.clone(), second.clone()]);
    let mut origin = backend.open(first.id.clone(), None);
    let id = connected(&mut origin)?;
    prompt(&origin, &id, "first prompt")?;
    let live = messages(pending(&mut origin));
    assert_eq!(live, ["first prompt", "**Demo agent** received: first prompt", "Scripted reply"]);
    drop(origin);

    let mut separate = backend.open(second.id.clone(), None);
    assert_ne!(connected(&mut separate)?, id);
    assert!(pending(&mut separate).is_empty());
    let mut mismatched = backend.open(second.id, Some(id.clone()));
    assert!(matches!(mismatched.events.try_recv()?, SessionEvent::ConnectFailed(ConnectFailure::Other(_))));
    assert_eq!(mismatched.events.recv().await.map(|_| ()), None);

    let mut reopened = backend.open(first.id, Some(id.clone()));
    let (reopened_id, replay) = attached(&mut reopened)?;
    assert_eq!((reopened_id, messages(replay)), (id, live));
    assert!(pending(&mut reopened).is_empty());
    Ok(())
}

#[tokio::test]
async fn stop_removes_the_thread_and_closes_its_active_attachments() -> Result<(), TestError> {
    let _entered = Runtime::new().enter();
    let backend = backend();
    let thread = backend.launch("demo".into()).await?;
    let mut first = backend.open(thread.id.clone(), None);
    let mut second = backend.open(thread.id.clone(), None);
    assert_eq!(connected(&mut first)?, connected(&mut second)?);
    backend.stop(thread.id.clone()).await?;
    assert_eq!(backend.list_threads().await?, [] as [HostedThread; 0]);
    poll_tasks();
    for link in [&mut first, &mut second] {
        assert!(matches!(link.events.try_recv()?, SessionEvent::Input(Input::Event(AcpEvent::ConnectionClosed))));
        assert!(matches!(link.events.try_recv(), Err(TryRecvError::Disconnected)));
        assert!(link.commands.is_closed());
    }
    let mut reopen = backend.open(thread.id, None);
    assert!(matches!(reopen.events.try_recv()?, SessionEvent::ConnectFailed(ConnectFailure::Ended(_))));
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum TestError {
    #[error(transparent)]
    Backend(#[from] BackendError),
    #[error(transparent)]
    Receive(#[from] TryRecvError),
    #[error("expected a connected session")]
    NotConnected,
    #[error("command channel closed")]
    CommandClosed,
    #[error("no pending permission request")]
    MissingPermission,
}

async fn asking() -> Result<(SessionLink, SessionLink, acp::SessionId, PendingElicitation), TestError> {
    let backend = backend();
    let thread = backend.launch("demo".into()).await?;
    let mut origin = backend.open(thread.id.clone(), None);
    let id = connected(&mut origin)?;
    let mut observer = backend.open(thread.id, None);
    assert_eq!(connected(&mut observer)?, id);
    prompt(&origin, &id, "/permission")?;
    let question = pending(&mut origin)
        .into_iter()
        .find_map(|event| match event {
            SessionEvent::Input(Input::Elicitation(question)) => Some(question),
            _ => None,
        })
        .ok_or(TestError::MissingPermission)?;
    Ok((origin, observer, id, question))
}

fn backend() -> MemoryBackend {
    MemoryBackend::new(vec![Project { id: "demo".into(), name: "Demo project".into() }])
        .with_reply(vec!["Scripted reply".into()], Duration::ZERO)
}

fn connected(link: &mut SessionLink) -> Result<acp::SessionId, TestError> {
    attached(link).map(|(id, _)| id)
}

/// The session a link attached to, and what was replayed from it.
fn attached(link: &mut SessionLink) -> Result<(acp::SessionId, Vec<SessionEvent>), TestError> {
    match link.events.try_recv()? {
        SessionEvent::Connected(config, replay) => {
            Ok((config.session_response.session_id, replay.into_iter().map(SessionEvent::Input).collect()))
        }
        _ => Err(TestError::NotConnected),
    }
}

fn prompt(link: &SessionLink, id: &acp::SessionId, text: &str) -> Result<(), TestError> {
    send(
        link,
        AgentCommand::Prompt {
            request_id: RequestId::next(),
            session_id: id.clone(),
            content: vec![text.into()],
            sources: Vec::new(),
        },
    )
}

fn send(link: &SessionLink, command: AgentCommand) -> Result<(), TestError> {
    link.commands.send(Command::Agent(command)).map_err(|_| TestError::CommandClosed)
}

fn pending(link: &mut SessionLink) -> Vec<SessionEvent> {
    poll_tasks();
    std::iter::from_fn(|| link.events.try_recv().ok()).collect()
}

fn messages(events: Vec<SessionEvent>) -> Vec<String> {
    events
        .into_iter()
        .filter_map(|event| match event {
            SessionEvent::Input(Input::Event(AcpEvent::SessionUpdate(update))) => match update.update {
                acp::SessionUpdate::UserMessage(message) => match message.content {
                    MaybeUndefined::Value(content) => first_text(&content).map(str::to_owned),
                    _ => None,
                },
                acp::SessionUpdate::AgentMessageChunk(chunk) => first_text(&[chunk.content]).map(str::to_owned),
                _ => None,
            },
            _ => None,
        })
        .collect()
}
