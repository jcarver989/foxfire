use crate::support::{RelayBuilder, SESSION, TestError, live_session, prompt, resume, transcript, until};
use agent_client_protocol::Error;
use agent_client_protocol::v2::schema::{
    CloseSessionRequest, CloseSessionResponse, NewSessionRequest, NewSessionResponse, ResumeSessionRequest,
    ResumeSessionResponse, SessionConfigKind, SessionConfigOption, SessionUpdate, SetSessionConfigOptionRequest,
    StopReason, UpdateSessionNotification,
};
use foxfire_acp::aether::RemoteServerInfo;
use foxfire_acp::artifact::{Artifact, ArtifactContent, ArtifactFetchParams, ArtifactFetchResponse};
use foxfire_acp::client::{AcpClient, AcpEvent};
use foxfire_acp::testing::{FakeAgent, idle_notification, message_chunk, model_select, run_local};
use serde_json::json;
use std::fs;
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
fn a_client_joining_is_shown_the_artifacts_submitted_before_and_can_fetch_them() -> Result<(), TestError> {
    run_local(async {
        let mut relay = RelayBuilder::new(FakeAgent::new(SESSION).mcp_acp()).build().await?;
        let mut first = relay.creator().await?;
        let tools = relay.agent.opened_tools().await.ok_or(TestError::Closed)?;
        let page = relay.cwd().join("pricing.html");
        fs::write(&page, "<h1>Pricing</h1>")?;
        tools.call("submit_artifact", json!({ "url": page })).await?;
        let submitted = artifact(&mut first).await?;

        let mut second = relay.joiner(true).await?;
        assert_eq!(artifact(&mut second).await?, submitted);
        let fetch = ArtifactFetchParams { session_id: SESSION.into(), id: submitted.id };
        let ArtifactFetchResponse { content } = second.handle.request(fetch).await?;
        assert_eq!(content, ArtifactContent::Html("<h1>Pricing</h1>".to_owned()));
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

        let agent = relay.agent.connection().await.ok_or(TestError::Closed)?;
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

#[test]
fn a_client_connecting_is_told_of_the_live_session() -> Result<(), TestError> {
    run_local(async {
        let relay = RelayBuilder::new(FakeAgent::new(SESSION)).build().await?;
        let first = relay.creator().await?;
        assert_eq!(live_session(&first), None);

        let second = relay.client().await?;
        let advertised = RemoteServerInfo::from_meta(second.initialize_response.meta.as_ref());
        assert_eq!(advertised, Some(RemoteServerInfo { cwd: relay.cwd(), session_id: Some(SESSION.into()) }));
        Ok(())
    })
}

#[test]
fn a_client_creating_a_session_is_joined_to_the_live_one() -> Result<(), TestError> {
    run_local(async {
        let agent = FakeAgent::new(SESSION).prompt_reply("hi");
        let mut relay = RelayBuilder::new(agent).build().await?;
        let mut first = relay.client().await?;
        let mut second = relay.client().await?;
        first.handle.request(NewSessionRequest::new(relay.cwd())).await?;
        first.handle.request(prompt("hello")).await?;
        assert_eq!(transcript(&mut first).await?, ["hello", "hi"]);

        let joined = second.handle.request(NewSessionRequest::new(relay.cwd())).await?;
        assert_eq!(joined.session_id, SESSION.into());
        assert_eq!(transcript(&mut second).await?, ["hello", "hi"]);
        assert!(relay.agent.new_session.recv().await.is_some());
        assert!(relay.agent.new_session.try_recv().is_err(), "the agent is asked for one session");

        first.handle.request(prompt("again")).await?;
        assert_eq!(transcript(&mut first).await?, ["again", "hi"]);
        assert_eq!(transcript(&mut second).await?, ["again", "hi"]);
        Ok(())
    })
}

#[test]
fn clients_creating_a_session_at_once_share_it() -> Result<(), TestError> {
    run_local(async {
        let agent = FakeAgent::new(SESSION).hold::<NewSessionRequest>();
        let mut relay = RelayBuilder::new(agent).build().await?;
        let first = relay.client().await?;
        let second = relay.client().await?;
        let creating = spawn_local(first.handle.request(NewSessionRequest::new(relay.cwd())));
        let (_, responder) = relay.agent.held::<NewSessionRequest>().recv().await.ok_or(TestError::Closed)?;
        let joining = spawn_local(second.handle.request(NewSessionRequest::new(relay.cwd())));
        relay.settle(&second).await?;

        responder.respond(NewSessionResponse::new(SESSION))?;
        let created = creating.await.map_err(std::io::Error::other)??;
        let joined = joining.await.map_err(std::io::Error::other)??;
        assert_eq!((created.session_id, joined.session_id), (SESSION.into(), SESSION.into()));
        assert!(relay.agent.held::<NewSessionRequest>().try_recv().is_err(), "the agent is asked for one session");
        Ok(())
    })
}

#[test]
fn a_client_waiting_on_a_session_that_fails_to_open_creates_its_own() -> Result<(), TestError> {
    run_local(async {
        let agent = FakeAgent::new(SESSION).hold::<NewSessionRequest>();
        let mut relay = RelayBuilder::new(agent).build().await?;
        let first = relay.client().await?;
        let second = relay.client().await?;
        let failing = spawn_local(first.handle.request(NewSessionRequest::new(relay.cwd())));
        let (_, refused) = relay.agent.held::<NewSessionRequest>().recv().await.ok_or(TestError::Closed)?;
        let waiting = spawn_local(second.handle.request(NewSessionRequest::new(relay.cwd())));
        relay.settle(&second).await?;

        refused.respond_with_error(Error::internal_error())?;
        assert!(failing.await.map_err(std::io::Error::other)?.is_err());
        let (_, responder) = relay.agent.held::<NewSessionRequest>().recv().await.ok_or(TestError::Closed)?;
        responder.respond(NewSessionResponse::new(SESSION))?;
        assert_eq!(waiting.await.map_err(std::io::Error::other)??.session_id, SESSION.into());
        Ok(())
    })
}

#[test]
fn a_client_that_gives_up_waiting_for_a_session_never_reaches_the_agent() -> Result<(), TestError> {
    run_local(async {
        let agent = FakeAgent::new(SESSION).hold::<NewSessionRequest>();
        let mut relay = RelayBuilder::new(agent).build().await?;
        let first = relay.client().await?;
        let second = relay.client().await?;
        let creating = spawn_local(first.handle.request(NewSessionRequest::new(relay.cwd())));
        let (_, responder) = relay.agent.held::<NewSessionRequest>().recv().await.ok_or(TestError::Closed)?;
        drop(second.handle.request(NewSessionRequest::new(relay.cwd())));
        relay.settle(&second).await?;

        responder.respond(NewSessionResponse::new(SESSION))?;
        creating.await.map_err(std::io::Error::other)??;
        first.handle.request(CloseSessionRequest::new(SESSION)).await?;
        assert!(relay.agent.close_session.recv().await.is_some(), "only the first client watches the session");
        assert!(relay.agent.resume.try_recv().is_err());
        Ok(())
    })
}

#[test]
fn the_live_session_outlives_its_last_client() -> Result<(), TestError> {
    run_local(async {
        let agent = FakeAgent::new(SESSION).replay_message(SESSION, "from before");
        let mut relay = RelayBuilder::new(agent).build().await?;
        let first = relay.creator().await?;
        first.handle.request(CloseSessionRequest::new(SESSION)).await?;
        first.handle.disconnect().await;

        let mut second = relay.client().await?;
        assert_eq!(live_session(&second), Some(SESSION.into()));
        let joined = second.handle.request(NewSessionRequest::new(relay.cwd())).await?;
        assert_eq!(joined.session_id, SESSION.into());
        assert_eq!(transcript(&mut second).await?, ["from before"]);
        assert!(relay.agent.new_session.recv().await.is_some());
        assert!(relay.agent.new_session.try_recv().is_err(), "the agent is asked for one session");
        Ok(())
    })
}

#[test]
fn a_client_joining_while_the_session_closes_waits_to_resume_it() -> Result<(), TestError> {
    run_local(async {
        let agent = FakeAgent::new(SESSION).hold::<CloseSessionRequest>();
        let mut relay = RelayBuilder::new(agent).build().await?;
        let first = relay.creator().await?;
        let second = relay.client().await?;
        let closing = spawn_local(first.handle.request(CloseSessionRequest::new(SESSION)));
        let (_, responder) = relay.agent.held::<CloseSessionRequest>().recv().await.ok_or(TestError::Closed)?;
        let joining = spawn_local(second.handle.request(resume(&relay.cwd(), true)));
        relay.settle(&second).await?;
        first.handle.request(SetSessionConfigOptionRequest::new(SESSION, "model", "smart")).await?;
        assert!(relay.agent.resume.try_recv().is_err(), "the agent would close the session the client had joined");

        responder.respond(CloseSessionResponse::new())?;
        closing.await.map_err(std::io::Error::other)??;
        joining.await.map_err(std::io::Error::other)??;
        assert!(relay.agent.resume.recv().await.is_some_and(|resumed| resumed.replay_from.is_some()));
        Ok(())
    })
}

#[test]
fn a_client_that_leaves_while_its_session_opens_does_not_keep_it_open() -> Result<(), TestError> {
    run_local(async {
        let agent = FakeAgent::new(SESSION).hold::<NewSessionRequest>();
        let mut relay = RelayBuilder::new(agent).build().await?;
        let first = relay.client().await?;
        let second = relay.client().await?;
        let creating = spawn_local(first.handle.request(NewSessionRequest::new(relay.cwd())));
        let (_, responder) = relay.agent.held::<NewSessionRequest>().recv().await.ok_or(TestError::Closed)?;
        let joining = spawn_local(second.handle.request(NewSessionRequest::new(relay.cwd())));
        relay.settle(&second).await?;
        first.handle.disconnect().await;
        assert!(creating.await.map_err(std::io::Error::other)?.is_err());

        responder.respond(NewSessionResponse::new(SESSION))?;
        assert_eq!(joining.await.map_err(std::io::Error::other)??.session_id, SESSION.into());
        while second.handle.request(ResumeSessionRequest::new("other", relay.cwd())).await.is_err() {}
        assert_eq!(live_session(&relay.client().await?), Some("other".into()));
        Ok(())
    })
}

#[test]
fn a_session_others_watch_cannot_be_swapped_for_another() -> Result<(), TestError> {
    run_local(async {
        let mut relay = RelayBuilder::new(FakeAgent::new(SESSION)).build().await?;
        let first = relay.creator().await?;
        let second = relay.joiner(false).await?;
        assert!(relay.agent.resume.recv().await.is_some_and(|resumed| resumed.session_id == SESSION.into()));

        let swapped = second.handle.request(ResumeSessionRequest::new("other", relay.cwd())).await;
        assert!(swapped.is_err_and(|error| error.to_string().contains(SESSION)), "the error names the live session");
        assert!(relay.agent.resume.try_recv().is_err());
        assert_eq!(live_session(&relay.client().await?), Some(SESSION.into()));

        first.handle.request(CloseSessionRequest::new(SESSION)).await?;
        second.handle.request(ResumeSessionRequest::new("other", relay.cwd())).await?;
        assert_eq!(live_session(&relay.client().await?), Some("other".into()));
        Ok(())
    })
}

#[test]
fn a_session_that_cannot_be_found_stops_being_live() -> Result<(), TestError> {
    run_local(async {
        let agent = FakeAgent::new(SESSION).hold::<ResumeSessionRequest>();
        let mut relay = RelayBuilder::new(agent).saved_session("gone", "2026-01-01T00:00:00Z").build().await?;
        let client = relay.client().await?;
        assert_eq!(live_session(&client), Some("gone".into()));
        let joining = spawn_local(client.handle.request(NewSessionRequest::new(relay.cwd())));
        let (request, responder) = relay.agent.held::<ResumeSessionRequest>().recv().await.ok_or(TestError::Closed)?;
        assert_eq!(request.session_id, "gone".into());
        responder.respond_with_error(Error::invalid_params())?;
        assert!(joining.await.map_err(std::io::Error::other)?.is_err());

        assert_eq!(live_session(&relay.client().await?), None);
        let created = client.handle.request(NewSessionRequest::new(relay.cwd())).await?;
        assert_eq!(created.session_id, SESSION.into());
        Ok(())
    })
}

#[test]
fn a_prompt_sent_during_another_clients_turn_reaches_the_agent() -> Result<(), TestError> {
    run_local(async {
        let mut relay = RelayBuilder::new(FakeAgent::new(SESSION)).build().await?;
        let mut first = relay.creator().await?;
        let mut second = relay.joiner(false).await?;
        first.handle.request(prompt("hello")).await?;
        assert!(relay.agent.prompt.recv().await.is_some());

        let steering = spawn_local(second.handle.request(prompt("stop")));
        assert!(relay.agent.prompt.recv().await.is_some_and(|steered| steered.prompt == prompt("stop").prompt));
        relay.agent.turns.insert_waiting();
        steering.await.map_err(std::io::Error::other)??;
        relay.agent.turns.reply("stopping");
        relay.agent.turns.end(StopReason::EndTurn);
        assert_eq!(transcript(&mut first).await?, ["hello", "stop", "stopping"]);
        assert_eq!(transcript(&mut second).await?, ["hello", "stop", "stopping"]);
        Ok(())
    })
}

#[test]
fn a_restarted_relay_adopts_the_most_recent_session() -> Result<(), TestError> {
    run_local(async {
        let agent = FakeAgent::new(SESSION).session_page_size(1).replay_message("newest", "from before");
        let mut relay = RelayBuilder::new(agent)
            .saved_session("older", "2026-01-01T00:00:00Z")
            .saved_session("newest", "2026-03-01T00:00:00Z")
            .saved_session("oldest", "2025-01-01T00:00:00Z")
            .build()
            .await?;
        let mut client = relay.client().await?;
        assert_eq!(live_session(&client), Some("newest".into()));
        assert!(relay.agent.resume.try_recv().is_err(), "the session is resumed once a client joins it");

        let joined = client.handle.request(NewSessionRequest::new(relay.cwd())).await?;
        assert_eq!(joined.session_id, "newest".into());
        assert_eq!(transcript(&mut client).await?, ["from before"]);
        assert!(relay.agent.new_session.try_recv().is_err());
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

async fn artifact(client: &mut AcpClient) -> Result<Artifact, TestError> {
    until(client, |event| match event {
        AcpEvent::Artifact(payload) => Some(payload.artifact),
        _ => None,
    })
    .await
}
