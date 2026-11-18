use crate::support::{RelayBuilder, SESSION, TestError, prompt, resume, transcript, until};
use agent_client_protocol::v2::schema::{
    CloseSessionRequest, NewSessionResponse, ResumeSessionRequest, ResumeSessionResponse, SessionConfigKind,
    SessionConfigOption, SessionUpdate, SetSessionConfigOptionRequest, UpdateSessionNotification,
};
use foxfire_acp::client::AcpEvent;
use foxfire_acp::testing::{FakeAgent, idle_notification, message_chunk, model_select, run_local};
use tokio::task::spawn_local;

#[test]
fn a_client_joining_is_replayed_the_session_then_sees_it_live() -> Result<(), TestError> {
    run_local(async {
        let agent = FakeAgent::default().replay_message(SESSION, "from before").prompt_reply("hi");
        let mut relay = RelayBuilder::new(agent).build().await?;
        let mut first = relay.joiner(true).await?;
        assert_eq!(transcript(&mut first).await?, ["from before"]);
        first.handle.request(prompt("hello")).await?;
        assert_eq!(transcript(&mut first).await?, ["hello", "hi"]);

        let mut second = relay.joiner(true).await?;
        assert_eq!(transcript(&mut second).await?, ["from before"]);
        assert_eq!(transcript(&mut second).await?, ["hello", "hi"]);
        let replayed: Vec<_> = [relay.agent.resume.recv().await, relay.agent.resume.recv().await]
            .map(|resumed| resumed.is_some_and(|resumed| resumed.replay_from.is_some()))
            .into();
        assert_eq!(replayed, [true, false], "the relay replays a watched session itself");

        second.handle.request(prompt("again")).await?;
        assert_eq!(transcript(&mut second).await?, ["again", "hi"]);
        assert_eq!(transcript(&mut first).await?, ["again", "hi"]);
        Ok(())
    })
}

#[test]
fn a_client_joining_while_the_session_opens_is_sent_the_agents_replay_too() -> Result<(), TestError> {
    run_local(async {
        let mut relay = RelayBuilder::new(FakeAgent::new(SESSION).hold::<ResumeSessionRequest>()).build().await?;
        let mut first = relay.client().await?;
        let mut second = relay.client().await?;
        let opening = spawn_local(first.handle.request(resume(&relay.cwd(), true)));
        let (_, first_resumed) = relay.agent.held::<ResumeSessionRequest>().recv().await.ok_or(TestError::Closed)?;
        let joining = spawn_local(second.handle.request(resume(&relay.cwd(), true)));
        let (request, second_resumed) =
            relay.agent.held::<ResumeSessionRequest>().recv().await.ok_or(TestError::Closed)?;
        assert!(request.replay_from.is_none());

        let agent = relay.agent.connection.recv().await.ok_or(TestError::Closed)?;
        agent.send_notification(UpdateSessionNotification::new(SESSION, message_chunk("message", "history")))?;
        agent.send_notification(idle_notification(SESSION, None))?;
        first_resumed.respond(ResumeSessionResponse::new())?;
        second_resumed.respond(ResumeSessionResponse::new())?;
        opening.await.map_err(std::io::Error::other)??;
        joining.await.map_err(std::io::Error::other)??;
        assert_eq!(transcript(&mut first).await?, ["history"]);
        assert_eq!(transcript(&mut second).await?, ["history"]);
        Ok(())
    })
}

#[test]
fn a_shared_session_closes_at_the_agent_once_its_last_client_leaves() -> Result<(), TestError> {
    run_local(async {
        let mut relay = RelayBuilder::new(FakeAgent::new(SESSION)).build().await?;
        let first = relay.creator().await?;
        let second = relay.joiner(false).await?;

        first.handle.request(CloseSessionRequest::new(SESSION)).await?;
        assert!(relay.agent.close_session.try_recv().is_err(), "the second client still watches the session");
        second.handle.request(CloseSessionRequest::new(SESSION)).await?;
        let closed = relay.agent.close_session.recv().await.ok_or(TestError::Closed)?;
        assert_eq!(closed.session_id.0.as_ref(), SESSION);
        Ok(())
    })
}

#[test]
fn a_configuration_change_reaches_every_client_watching_the_session() -> Result<(), TestError> {
    run_local(async {
        let models = model_select("fast", &[("fast", "Fast"), ("smart", "Smart")]);
        let created = NewSessionResponse::new(SESSION).config_options(vec![models]);
        let relay = RelayBuilder::new(FakeAgent::default().new_session_response(created)).build().await?;
        let first = relay.creator().await?;
        let mut second = relay.joiner(false).await?;

        first.handle.request(SetSessionConfigOptionRequest::new(SESSION, "model", "smart")).await?;
        let options = until(&mut second, |event| match event {
            AcpEvent::SessionUpdate(notification) => match notification.update {
                SessionUpdate::ConfigOptionUpdate(update) => Some(update.config_options),
                _ => None,
            },
            _ => None,
        })
        .await?;
        assert_eq!(current_values(&options), ["smart"]);
        Ok(())
    })
}

fn current_values(options: &[SessionConfigOption]) -> Vec<String> {
    options
        .iter()
        .filter_map(|option| match &option.kind {
            SessionConfigKind::Select(select) => Some(select.current_value.0.to_string()),
            _ => None,
        })
        .collect()
}
