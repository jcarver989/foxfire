use crate::support::{deliver, offline, receive};
use agent_client_protocol::v2::schema::SessionId;
use foxfire_acp::chat::{
    ChatChannelEvent, ChatChannelEventPayload, ChatChannelHistoryFrom, ChatChannelJoinResponse, ChatChannelPage,
    ChatChannelPostParams, ChatChannelPostResponse, ChatMessage, ChatMessageKind, Participant,
};
use foxfire_acp::client::AcpEvent;
use foxfire_core::command::{AgentCommand, ChatCommand, ChatReply, Command, CommandResult};
use foxfire_core::session_core::{ChatChannel, Earlier, Effect, SessionCore};
use foxfire_core::testing::SessionCoreConfigBuilder;
use std::path::Path;

#[test]
fn a_channel_is_shared_once_someone_else_is_present_or_has_posted() {
    assert!(SessionCore::new(SessionCoreConfigBuilder::new().build()).take_effects().is_empty());
    let mut core = ChatBuilder::new().build();
    assert!(core.chat().is_none());
    happen(&mut core, ChatChannelEvent::Presence { present: ids(&["maya", "you"]) });
    assert!(core.chat().is_some());
    happen(&mut core, ChatChannelEvent::Presence { present: ids(&["you"]) });
    assert!(core.chat().is_none());
    happen(&mut core, ChatChannelEvent::Message { message: post(1, "maya") });
    assert_eq!(core.chat().map(|chat| (chat.others(), chat.unread())), Some((Vec::new(), 1)));
}

#[test]
fn events_handled_before_the_join_reply_are_kept() {
    let mut core = SessionCore::new(SessionCoreConfigBuilder::new().chat(person("you")).build());
    happen(&mut core, ChatChannelEvent::Presence { present: ids(&["maya", "you"]) });
    reply(&mut core, joined(Vec::new(), false));
    assert_eq!(core.chat().map(|chat| chat.others().len()), Some(1));
}

#[test]
fn a_failed_post_is_kept_until_it_is_retried_or_dismissed() {
    let mut core = ChatBuilder::new().build();
    for key in ["retried", "dismissed"] {
        core.post_chat(draft(&core, key));
        reply(&mut core, ChatReply::Posted { key: key.to_owned(), result: Err(offline()) });
    }
    assert!(core.chat().is_some_and(|chat| chat.pending.iter().all(|pending| pending.error.is_some())));

    core.dismiss_chat("dismissed");
    core.take_effects();
    core.post_chat(draft(&core, "retried"));
    assert!(matches!(requested(&mut core).as_slice(), [ChatCommand::Post(post)] if post.key == "retried"));
    assert_eq!(core.chat().map(|chat| chat.pending.len()), Some(1));

    let message = ChatMessage { kind: kind("retried"), ..post(1, "you") };
    reply(&mut core, ChatReply::Posted { key: "retried".to_owned(), result: Ok(ChatChannelPostResponse { message }) });
    assert_eq!(core.chat().map(|chat| (chat.pending.len(), chat.messages.len(), chat.unread())), Some((0, 1, 0)));
}

#[test]
fn earlier_messages_are_loaded_once_at_a_time() {
    let mut core = ChatBuilder::new().messages(vec![post(2, "maya")]).more().build();
    core.load_earlier_chat();
    core.load_earlier_chat();
    let requests = requested(&mut core);
    assert!(
        matches!(requests.as_slice(), [ChatCommand::History(params)] if params.from == ChatChannelHistoryFrom::Before(2))
    );

    reply(&mut core, ChatReply::History(Err(offline())));
    assert_eq!(core.chat().map(|chat| chat.earlier), Some(Earlier::Available));
    reply(&mut core, ChatReply::History(Ok(ChatChannelPage { messages: vec![post(1, "maya")], more: false })));
    let loaded = core.chat().map(|chat| (chat.messages.keys().copied().collect(), chat.earlier));
    assert_eq!(loaded, Some((vec![1, 2], Earlier::None)));
}

#[test]
fn a_read_mark_only_moves_forward() {
    let mut core = ChatBuilder::new().messages(vec![post(1, "maya"), post(2, "maya")]).build();
    happen(&mut core, ChatChannelEvent::Read { participant: "maya".to_owned(), seq: 2 });
    assert_eq!(core.chat().map(ChatChannel::unread), Some(2));
    happen(&mut core, ChatChannelEvent::Read { participant: "you".to_owned(), seq: 1 });
    assert_eq!(core.chat().map(ChatChannel::unread), Some(1));
    core.mark_chat_read();
    core.mark_chat_read();
    assert!(matches!(requested(&mut core).as_slice(), [ChatCommand::Read(params)] if params.seq == 2));
}

#[test]
fn a_session_that_cannot_be_resumed_does_not_keep_the_previous_ones_channel() {
    let mut core = ChatBuilder::new().messages(vec![post(1, "maya")]).build();
    let session_id = SessionId::new("elsewhere");
    core.resume_session(&session_id, Path::new("/elsewhere"));
    deliver(&mut core, CommandResult::ResumeSession { session_id, result: Err(offline()) });
    assert!(core.chat().is_none());
    core.post_chat(draft(&core, "key"));
    assert!(requested(&mut core).is_empty(), "nothing is posted to a channel that was not joined");
}

struct ChatBuilder {
    messages: Vec<ChatMessage>,
    more: bool,
}

impl ChatBuilder {
    fn new() -> Self {
        Self { messages: Vec::new(), more: false }
    }

    fn messages(mut self, messages: Vec<ChatMessage>) -> Self {
        self.messages = messages;
        self
    }

    fn more(mut self) -> Self {
        self.more = true;
        self
    }

    fn build(self) -> SessionCore {
        let mut core = SessionCore::new(SessionCoreConfigBuilder::new().chat(person("you")).build());
        assert!(matches!(requested(&mut core).as_slice(), [ChatCommand::Join(_)]));
        reply(&mut core, joined(self.messages, self.more));
        core
    }
}

fn joined(messages: Vec<ChatMessage>, more: bool) -> ChatReply {
    let page = ChatChannelPage { messages, more };
    ChatReply::Joined(Ok(ChatChannelJoinResponse { members: Vec::new(), present: ids(&["you"]), read: 0, page }))
}

fn happen(core: &mut SessionCore, event: ChatChannelEvent) {
    let session_id = core.session_id().clone();
    receive(core, AcpEvent::ChatChannelEvent(ChatChannelEventPayload { session_id, event }));
}

fn reply(core: &mut SessionCore, reply: ChatReply) {
    let session_id = core.session_id().clone();
    deliver(core, CommandResult::Chat { session_id, reply });
}

fn requested(core: &mut SessionCore) -> Vec<ChatCommand> {
    let effects = core.take_effects().into_iter();
    let commands = effects.filter_map(|effect| match effect {
        Effect::Dispatch(Command::Agent(AgentCommand::Chat(command))) => Some(command),
        _ => None,
    });
    commands.collect()
}

fn draft(core: &SessionCore, key: &str) -> ChatChannelPostParams {
    let session_id = core.session_id().clone();
    ChatChannelPostParams { session_id, key: key.to_owned(), body: "hello".to_owned(), anchor: None, after_item: None }
}

fn post(seq: i64, author: &str) -> ChatMessage {
    let (author, body) = (author.to_owned(), "hello".to_owned());
    ChatMessage { seq, author, body, sent_at_ms: 0, kind: kind(&seq.to_string()) }
}

fn kind(key: &str) -> ChatMessageKind {
    ChatMessageKind::Post { key: key.to_owned(), anchor: None, after_item: None }
}

fn person(id: &str) -> Participant {
    Participant { id: id.to_owned(), name: id.to_owned() }
}

fn ids(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}
