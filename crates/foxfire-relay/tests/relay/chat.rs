use crate::support::{RelayBuilder, SESSION, TestError, participant, prompt, resume, until};
use agent_client_protocol::v2::schema::{
    CloseSessionRequest, ContentBlock, EmbeddedResource, EmbeddedResourceResource, NewSessionRequest, PromptRequest,
    PromptResponse, ResumeSessionRequest,
};
use agent_client_protocol::{Error, ErrorCode, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use foxfire_acp::chat::{
    CHAT_MESSAGES_URI, ChatChannelEvent, ChatChannelHistoryFrom, ChatChannelHistoryParams, ChatChannelJoinParams,
    ChatChannelJoinResponse, ChatChannelPage, ChatChannelPostParams, ChatChannelReadParams, ChatMessage,
    ChatMessageKind, FoxfireCapabilities, FoxfireMeta, Participant, PromptMeta,
};
use foxfire_acp::client::{AcpClient, AcpClientError, AcpEvent};
use foxfire_acp::testing::{FakeAgent, run_local};
use foxfire_core::composer::submission::Submission;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use tempfile::TempDir;
use tokio::task::spawn_local;

#[test]
fn a_message_reaches_everyone_in_the_channel_and_never_the_agent() -> Result<(), TestError> {
    run_local(async {
        let mut relay = RelayBuilder::new(FakeAgent::new(SESSION)).chat().build().await?;
        let mut maya = relay.member("maya").await?;
        let mut theo = relay.member("theo").await?;
        let first = post(&maya, "1", "hello").await?;
        let second = post(&theo, "1", "hi").await?;
        assert_eq!((first.author.as_str(), second.author.as_str()), ("maya", "theo"));
        assert_eq!(next_message(&mut theo).await?, first);
        assert_eq!(next_message(&mut maya).await?, second, "a client is not sent its own message");
        assert_eq!(history(&maya).await?.messages, [first, second]);
        assert!(relay.agent.extensions.try_recv().is_err());
        Ok(())
    })
}

#[test]
fn a_client_that_disconnects_or_closes_the_session_leaves_the_channel() -> Result<(), TestError> {
    run_local(async {
        let relay = RelayBuilder::new(FakeAgent::new(SESSION)).chat().build().await?;
        let mut maya = relay.member("maya").await?;
        let theo = relay.member("theo").await?;
        let lee = relay.member("lee").await?;
        theo.handle.disconnect().await;
        assert_eq!(presence(&mut maya, 3).await?, ["lee", "maya"]);
        lee.handle.request(CloseSessionRequest::new(SESSION)).await?;
        assert_eq!(presence(&mut maya, 1).await?, ["maya"]);
        Ok(())
    })
}

#[test]
fn a_channel_outlives_the_relay() -> Result<(), TestError> {
    run_local(async {
        let data = TempDir::new()?;
        let mut relay = RelayBuilder::new(FakeAgent::new(SESSION)).data_dir(data.path()).build().await?;
        let maya = relay.member("maya").await?;
        let posted = post(&maya, "1", "hello").await?;
        relay.stop_agent().await;
        assert!(relay.stopped().await.is_err(), "the relay stops with its agent");

        let relay = RelayBuilder::new(FakeAgent::new(SESSION)).data_dir(data.path()).build().await?;
        let theo = relay.client_as("theo").await?;
        let joined = join(&theo).await?;
        assert_eq!(joined.page.messages, [posted]);
        assert_eq!(names(&joined), [participant("maya"), participant("theo")]);
        Ok(())
    })
}

#[test]
fn a_read_mark_reaches_its_owners_other_devices() -> Result<(), TestError> {
    run_local(async {
        let relay = RelayBuilder::new(FakeAgent::new(SESSION)).chat().build().await?;
        let laptop = relay.member("maya").await?;
        let mut phone = relay.member("maya").await?;
        let seq = post(&laptop, "1", "hello").await?.seq;
        laptop.handle.notify(ChatChannelReadParams { session_id: SESSION.into(), seq })?;
        next_message(&mut phone).await?;
        assert_eq!(next_event(&mut phone).await?, ChatChannelEvent::Read { participant: "maya".to_owned(), seq });
        Ok(())
    })
}

#[test]
fn a_prompt_is_recorded_with_its_author_and_sources() -> Result<(), TestError> {
    run_local(async {
        let agent = FakeAgent::new(SESSION).prompt_reply("done");
        let mut relay = RelayBuilder::new(agent).chat().build().await?;
        let mut maya = relay.member("maya").await?;
        let mut theo = relay.member("theo").await?;
        let source = post(&theo, "1", "cap it at 30s").await?;
        assert_eq!(next_message(&mut maya).await?, source);

        let mut meta = PromptMeta { sources: vec![source.seq] }.to_meta();
        meta.insert("other".to_owned(), json!(true));
        let prompted = maya.handle.request(prompt("do it").meta(meta)).await?;
        let asked = next_message(&mut theo).await?;
        assert_eq!((asked.author.as_str(), asked.body.as_str()), ("maya", "do it"));
        let kind = ChatMessageKind::Ask { message_id: prompted.message_id, sources: vec![source.seq] };
        assert_eq!(asked.kind, kind);
        assert_eq!(next_message(&mut maya).await?, asked);
        let received = relay.agent.prompt.recv().await.ok_or(TestError::Closed)?;
        assert_eq!(received.meta.map(Value::Object), Some(json!({ "other": true })));

        maya.handle.request(prompt("again").meta(PromptMeta { sources: vec![source.seq] }.to_meta())).await?;
        let received = relay.agent.prompt.recv().await.ok_or(TestError::Closed)?;
        assert_eq!(received.meta, None);
        Ok(())
    })
}

#[test]
fn a_prompt_accepted_after_its_author_left_is_still_theirs() -> Result<(), TestError> {
    run_local(async {
        let agent = FakeAgent::new(SESSION).hold::<PromptRequest>();
        let mut relay = RelayBuilder::new(agent).chat().build().await?;
        let maya = relay.client_as("maya").await?;
        let mut theo = relay.member("theo").await?;
        let prompting = spawn_local(maya.handle.request(prompt("do it")));
        let (_, accepted) = relay.agent.held::<PromptRequest>().recv().await.ok_or(TestError::Closed)?;
        maya.handle.disconnect().await;
        assert!(prompting.await.map_err(std::io::Error::other)?.is_err());

        accepted.respond(PromptResponse::new("prompt"))?;
        let asked = next_message(&mut theo).await?;
        assert_eq!(asked.author, "maya");
        assert_eq!(asked.kind, ChatMessageKind::Ask { message_id: "prompt".into(), sources: Vec::new() });
        assert_eq!(names(&join(&theo).await?), [participant("theo"), participant("maya")], "asking joins the channel");
        Ok(())
    })
}

#[test]
fn a_prompt_accepted_after_the_relay_moved_on_is_kept_in_its_own_channel() -> Result<(), TestError> {
    run_local(async {
        let agent = FakeAgent::new(SESSION).hold::<PromptRequest>();
        let mut relay = RelayBuilder::new(agent).chat().build().await?;
        let maya = relay.client_as("maya").await?;
        let prompting = spawn_local(maya.handle.request(prompt("do it")));
        let (_, accepted) = relay.agent.held::<PromptRequest>().recv().await.ok_or(TestError::Closed)?;
        maya.handle.request(ResumeSessionRequest::new("elsewhere", relay.cwd())).await?;
        accepted.respond(PromptResponse::new("prompt"))?;
        prompting.await.map_err(std::io::Error::other)??;

        maya.handle.request(resume(&relay.cwd(), false)).await?;
        let messages = join(&maya).await?.page.messages;
        assert_eq!(messages.iter().map(|message| message.body.as_str()).collect::<Vec<_>>(), ["do it"]);
        Ok(())
    })
}

#[test]
fn a_prompt_the_agent_refuses_is_not_recorded() -> Result<(), TestError> {
    run_local(async {
        let relay = RelayBuilder::new(FakeAgent::new(SESSION)).chat().build().await?;
        let maya = relay.member("maya").await?;
        relay.agent.turns.refuse(Error::invalid_params());
        assert!(maya.handle.request(prompt("do it")).await.is_err());
        assert_eq!(join(&maya).await?.page.messages, []);
        Ok(())
    })
}

#[test]
fn a_client_that_names_no_one_can_prompt_but_not_chat() -> Result<(), TestError> {
    run_local(async {
        let agent = FakeAgent::new(SESSION).prompt_reply("hi");
        let relay = RelayBuilder::new(agent).chat().build().await?;
        let anonymous = relay.creator().await?;
        assert!(join(&anonymous).await.is_err());
        let blank = relay.client_as(" ").await?;
        assert!(join(&blank).await.is_err());
        anonymous.handle.request(prompt("hello")).await?;

        let maya = relay.client_as("maya").await?;
        assert_eq!(join(&maya).await?.page.messages, []);
        Ok(())
    })
}

#[test]
fn a_channel_is_open_only_to_clients_that_joined_it() -> Result<(), TestError> {
    run_local(async {
        let relay = RelayBuilder::new(FakeAgent::new(SESSION)).chat().build().await?;
        let maya = relay.client_as("maya").await?;
        assert_eq!(refusal(post(&maya, "1", "hello").await), Some(ErrorCode::INVALID_REQUEST));
        let elsewhere = ChatChannelJoinParams { session_id: "elsewhere".into(), after: None };
        assert!(maya.handle.request(elsewhere.clone()).await.is_err(), "the client has not opened the session");
        join(&maya).await?;
        assert_eq!(refusal(post(&maya, "1", " ").await), Some(ErrorCode::INVALID_PARAMS));
        post(&maya, "1", "hello").await?;

        maya.handle.request(ResumeSessionRequest::new("elsewhere", relay.cwd())).await?;
        assert!(post(&maya, "2", "hello").await.is_err(), "the client has moved to another session");
        let moved = maya.handle.request(elsewhere).await?;
        assert_eq!(moved.page.messages, [], "each session has its own channel");
        Ok(())
    })
}

#[test]
fn a_channel_that_cannot_be_read_does_not_stop_the_session() -> Result<(), TestError> {
    run_local(async {
        let data = TempDir::new()?;
        fs::create_dir(data.path().join("chat"))?;
        fs::write(data.path().join("chat/shared.jsonl"), "not an event\n")?;
        let agent = FakeAgent::new(SESSION).prompt_reply("hi");
        let relay = RelayBuilder::new(agent).data_dir(data.path()).build().await?;
        let maya = relay.client_as("maya").await?;
        assert_eq!(refusal(join(&maya).await), Some(ErrorCode::INTERNAL_ERROR));
        maya.handle.request(prompt("hello")).await?;
        Ok(())
    })
}

#[test]
fn a_client_that_rejoins_the_session_stays_in_the_channel() -> Result<(), TestError> {
    run_local(async {
        let relay = RelayBuilder::new(FakeAgent::new(SESSION)).chat().build().await?;
        let maya = relay.member("maya").await?;
        maya.handle.request(NewSessionRequest::new(relay.cwd())).await?;
        maya.handle.request(resume(&relay.cwd(), true)).await?;
        post(&maya, "1", "still here").await?;
        Ok(())
    })
}

#[test]
fn a_relay_without_a_usable_directory_offers_no_chat() -> Result<(), TestError> {
    run_local(async {
        let data = TempDir::new()?;
        let file = data.path().join("file");
        fs::write(&file, "in the way")?;
        for builder in
            [RelayBuilder::new(FakeAgent::new(SESSION)), RelayBuilder::new(FakeAgent::new(SESSION)).data_dir(&file)]
        {
            let mut relay = builder.build().await?;
            let maya = relay.client_as("maya").await?;
            assert!(!offers_chat(&maya));
            assert!(join(&maya).await.is_err());
            maya.handle.notify(ChatChannelReadParams { session_id: SESSION.into(), seq: 1 })?;
            let sources = PromptMeta { sources: vec![1] }.to_meta();
            maya.handle.request(prompt("hello").meta(sources)).await?;
            let received = relay.agent.prompt.recv().await.ok_or(TestError::Closed)?;
            assert_eq!(received.meta, None);
            assert!(relay.agent.extensions.try_recv().is_err(), "the agent is not asked instead");
        }
        Ok(())
    })
}

#[test]
fn unknown_foxfire_methods_never_reach_the_agent() -> Result<(), TestError> {
    run_local(async {
        let mut relay = RelayBuilder::new(FakeAgent::new(SESSION)).chat().build().await?;
        let maya = relay.client_as("maya").await?;
        assert!(offers_chat(&maya));
        maya.handle.notify(UnknownNotification {})?;
        assert!(maya.handle.request(UnknownRequest {}).await.is_err());
        maya.handle.request(resume(&relay.cwd(), false)).await?;
        assert!(relay.agent.extensions.try_recv().is_err());
        Ok(())
    })
}

#[test]
fn people_share_posts_reads_and_the_messages_a_prompt_is_sent_with() -> Result<(), TestError> {
    run_local(async {
        let mut relay = RelayBuilder::new(FakeAgent::new(SESSION).prompt_reply("done")).chat().build().await?;
        let mut maya = relay.session_as("maya").await?;
        let mut theo = relay.session_as("theo").await?;
        tokio::join!(
            maya.run_until(|core| core.chat().is_some_and(|chat| chat.others() == [participant("theo")])),
            theo.run_until(|core| core.chat().is_some_and(|chat| chat.others() == [participant("maya")])),
        );

        maya.core.post_chat(ChatChannelPostParams {
            session_id: SESSION.into(),
            key: "1".to_owned(),
            body: "cap it at 30s".to_owned(),
            anchor: None,
            after_item: None,
        });
        maya.run_until(|core| core.chat().is_some_and(|chat| chat.messages.len() == 1 && chat.pending.is_empty()))
            .await;
        theo.run_until(|core| core.chat().is_some_and(|chat| chat.unread() == 1)).await;
        theo.core.mark_chat_read();

        theo.core.submit(Submission { sources: vec![1], ..Submission::default() });
        theo.run_until(|core| core.chat().is_some_and(|chat| chat.unread() == 0 && chat.messages.len() == 2)).await;
        maya.run_until(|core| core.chat().is_some_and(|chat| chat.messages.len() == 2)).await;
        let received = relay.agent.prompt.recv().await.ok_or(TestError::Closed)?;
        assert_eq!(received.meta, None);
        let quoted = "[Channel messages referenced by the user]\nMAYA: cap it at 30s";
        assert!(matches!(
            received.prompt.as_slice(),
            [ContentBlock::Resource(EmbeddedResource {
                resource: EmbeddedResourceResource::TextResourceContents(resource), ..
            })] if resource.uri == CHAT_MESSAGES_URI && resource.text == quoted
        ));
        let asked = maya.core.chat().and_then(|chat| chat.messages.get(&2));
        assert!(
            asked.is_some_and(|ask| ask.author == "theo"
                && matches!(&ask.kind, ChatMessageKind::Ask { sources, .. } if sources == &[1]))
        );
        maya.shutdown().await;
        theo.shutdown().await;
        Ok(())
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "_foxfire/unknown", response = UnknownResponse)]
struct UnknownRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
struct UnknownResponse {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcNotification)]
#[notification(method = "_foxfire/unknown")]
struct UnknownNotification {}

async fn join(client: &AcpClient) -> Result<ChatChannelJoinResponse, TestError> {
    Ok(client.handle.request(ChatChannelJoinParams { session_id: SESSION.into(), after: None }).await?)
}

async fn post(client: &AcpClient, key: &str, body: &str) -> Result<ChatMessage, TestError> {
    let session_id = SESSION.into();
    let params = ChatChannelPostParams {
        session_id,
        key: key.to_owned(),
        body: body.to_owned(),
        anchor: None,
        after_item: None,
    };
    Ok(client.handle.request(params).await?.message)
}

async fn history(client: &AcpClient) -> Result<ChatChannelPage, TestError> {
    let from = ChatChannelHistoryFrom::After(0);
    Ok(client.handle.request(ChatChannelHistoryParams { session_id: SESSION.into(), from, limit: None }).await?)
}

async fn next_event(client: &mut AcpClient) -> Result<ChatChannelEvent, TestError> {
    until(client, |event| match event {
        AcpEvent::ChatChannelEvent(payload) => Some(payload.event),
        _ => None,
    })
    .await
}

async fn next_message(client: &mut AcpClient) -> Result<ChatMessage, TestError> {
    loop {
        if let ChatChannelEvent::Message { message } = next_event(client).await? {
            return Ok(message);
        }
    }
}

async fn presence(client: &mut AcpClient, changes: usize) -> Result<Vec<String>, TestError> {
    let mut latest = Vec::new();
    for _ in 0..changes {
        latest = until(client, |event| match event {
            AcpEvent::ChatChannelEvent(payload) => match payload.event {
                ChatChannelEvent::Presence { present } => Some(present),
                _ => None,
            },
            _ => None,
        })
        .await?;
    }
    Ok(latest)
}

fn refusal<T>(result: Result<T, TestError>) -> Option<ErrorCode> {
    match result {
        Err(TestError::Acp(AcpClientError::Protocol(error)) | TestError::Protocol(error)) => Some(error.code),
        _ => None,
    }
}

fn names(joined: &ChatChannelJoinResponse) -> Vec<Participant> {
    joined.members.iter().map(|member| member.participant.clone()).collect()
}

fn offers_chat(client: &AcpClient) -> bool {
    FoxfireCapabilities::of(&client.initialize_response.capabilities).chat
}
