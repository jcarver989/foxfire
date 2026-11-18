use crate::support::{RelayBuilder, SESSION, TestError, prompt, transcript, until};
use agent_client_protocol::Responder;
use agent_client_protocol::v2::schema::{
    PermissionOption, PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome,
};
use foxfire_acp::client::{AcpClient, AcpEvent};
use foxfire_acp::testing::{FakeAgent, Question, run_local};

#[test]
fn every_client_watching_is_asked_and_the_first_answer_wins() -> Result<(), TestError> {
    run_local(async {
        let relay = RelayBuilder::new(asking_agent()).build().await?;
        let mut first = relay.creator().await?;
        let mut second = relay.joiner(false).await?;

        first.handle.request(prompt("go")).await?;
        let (asked_first, first_responder) = permission(&mut first).await?;
        let (asked_second, second_responder) = permission(&mut second).await?;
        assert_eq!((asked_first.title.as_str(), asked_second.title.as_str()), ("Run it?", "Run it?"));

        first_responder.respond(allow())?;
        second_responder.cancellation().cancelled().await;
        let echoed = transcript(&mut second).await?;
        assert!(echoed.last().is_some_and(|echo| echo.contains("allow")), "the agent echoes its answer: {echoed:?}");
        Ok(())
    })
}

#[test]
fn a_question_nobody_answered_waits_for_the_next_client_to_join() -> Result<(), TestError> {
    run_local(async {
        let relay = RelayBuilder::new(asking_agent()).build().await?;
        let mut first = relay.creator().await?;
        first.handle.request(prompt("go")).await?;
        let (_, unanswered) = permission(&mut first).await?;
        first.handle.disconnect().await;
        drop(unanswered);

        let mut second = relay.joiner(false).await?;
        let (asked, responder) = permission(&mut second).await?;
        assert_eq!(asked.title, "Run it?");
        responder.respond(allow())?;
        let echoed = transcript(&mut second).await?;
        assert!(echoed.last().is_some_and(|echo| echo.contains("allow")), "the agent echoes its answer: {echoed:?}");
        Ok(())
    })
}

fn asking_agent() -> FakeAgent {
    let options = vec![PermissionOption::new("allow", "Allow", PermissionOptionKind::AllowOnce)];
    FakeAgent::new(SESSION).prompt_asks("Run it?", Question::Permission(options))
}

fn allow() -> RequestPermissionResponse {
    RequestPermissionResponse::new(RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new("allow")))
}

async fn permission(
    client: &mut AcpClient,
) -> Result<(RequestPermissionRequest, Responder<RequestPermissionResponse>), TestError> {
    until(client, |event| match event {
        AcpEvent::PermissionRequest { params, responder } => Some((*params, responder)),
        _ => None,
    })
    .await
}
