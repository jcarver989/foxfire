use agent_client_protocol::v2::schema::SessionId;
use foxfire_acp::chat::{
    ChatChannelAnchor, ChatChannelEvent, ChatChannelHistoryFrom, ChatChannelHistoryParams, ChatChannelJoinParams,
    ChatChannelJoinResponse, ChatChannelPage, ChatChannelPostParams, ChatChannelReadParams, ChatMessage,
    ChatMessageKind, Participant,
};
use foxfire_chat::{Ask, ChatChannels, ChatError, Delivery, MAX_BODY_BYTES, MAX_PAGE};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

const SESSION: &str = "session";

#[test]
fn a_message_is_delivered_to_everyone_else() -> Result<(), ChatError> {
    let mut chat = ChatBuilder::new().member("maya").member("theo").member("lee").build()?;
    let (first, delivered) = chat.post("maya", draft("1", "hello"))?;
    assert_eq!(received(&delivered), [(vec!["lee", "theo"], sent(&first))]);

    let anchor = Some(ChatChannelAnchor { item: "reply".to_owned(), excerpt: "hi".to_owned() });
    let after_item = Some("last".to_owned());
    let quoting =
        ChatChannelPostParams { anchor: anchor.clone(), after_item: after_item.clone(), ..draft("1", "why?") };
    let (second, delivered) = chat.post("theo", quoting)?;
    assert_eq!(received(&delivered), [(vec!["lee", "maya"], sent(&second))]);
    assert_eq!((second.author.as_str(), second.body.as_str()), ("theo", "why?"));
    assert_eq!(second.kind, ChatMessageKind::Post { key: "1".to_owned(), anchor, after_item });
    assert!(second.seq > first.seq);
    Ok(())
}

#[test]
fn everyone_is_told_who_joins_and_who_is_present() -> Result<(), ChatError> {
    let mut chat = ChatBuilder::new().member("maya").build()?;
    let (joined, delivered) = chat.join("laptop", &participant("theo"))?;
    assert_eq!((names(&joined), &joined.present), (vec!["maya", "theo"], &people(&["maya", "theo"])));
    let member = |joined: &ChatChannelJoinResponse| ChatChannelEvent::Member { member: joined.members[1].clone() };
    let present = |present| ChatChannelEvent::Presence { present: people(present) };
    let expected = [(vec!["maya"], member(&joined)), (vec!["maya"], present(&["maya", "theo"]))];
    assert_eq!(received(&delivered), expected);

    let renamed = Participant { name: "Theo R".to_owned(), ..participant("theo") };
    let (rejoined, delivered) = chat.join("phone", &renamed)?;
    assert_eq!(rejoined.present, people(&["maya", "theo"]), "two devices are one person");
    assert_eq!(received(&delivered), [(vec!["laptop", "maya"], member(&rejoined))]);
    assert_eq!(rejoined.members[1].participant, renamed);
    assert_eq!(rejoined.members[1].joined_at_ms, joined.members[1].joined_at_ms);

    assert_eq!(chat.leave("laptop"), None);
    let left = Vec::from_iter(chat.leave("phone"));
    assert_eq!(received(&left), [(vec!["maya"], present(&["maya"]))]);
    let (joined, delivered) = chat.join("maya", &participant("maya"))?;
    assert_eq!(names(&joined), ["maya", "Theo R"], "those who left are still members");
    assert_eq!(received(&delivered), [], "joining again changes nothing");
    Ok(())
}

#[test]
fn a_channel_is_read_backwards_and_forwards() -> Result<(), ChatError> {
    let mut chat = ChatBuilder::new().member("maya").messages(5).build()?;
    let posted = chat.join("maya", &participant("maya"))?.0.page.messages;
    let (before, after) = (ChatChannelHistoryFrom::Before, ChatChannelHistoryFrom::After);
    let cases = [
        (before(posted[3].seq), &posted[1..3], true),
        (before(posted[1].seq), &posted[..1], false),
        (after(posted[1].seq), &posted[2..4], true),
        (after(posted[3].seq), &posted[4..], false),
        (after(i64::MAX), &[], false),
    ];
    for (from, messages, more) in cases {
        let page = chat.channels.history("maya", &history(from, 2))?;
        assert_eq!(page, ChatChannelPage { messages: messages.to_vec(), more }, "{from:?}");
    }
    let params = ChatChannelJoinParams { after: Some(posted[2].seq), ..join_params() };
    let (caught_up, _) = chat.channels.join("maya", &participant("maya"), &params)?;
    assert_eq!(caught_up.page, ChatChannelPage { messages: posted[3..].to_vec(), more: false });
    Ok(())
}

#[test]
fn a_page_is_never_longer_than_the_limit() -> Result<(), ChatError> {
    let chat = ChatBuilder::new().member("maya").messages(MAX_PAGE + 1).build()?;
    let page = chat.channels.history("maya", &history(ChatChannelHistoryFrom::After(0), u32::MAX))?;
    assert_eq!((page.messages.len(), page.more), (MAX_PAGE as usize, true));
    Ok(())
}

#[test]
fn a_retried_message_is_stored_and_delivered_once() -> Result<(), ChatError> {
    let mut chat = ChatBuilder::new().member("maya").member("theo").build()?;
    let (first, _) = chat.post("maya", draft("1", "hello"))?;
    assert_eq!(chat.post("maya", draft("1", "hello"))?, (first.clone(), Vec::new()));
    let (other, _) = chat.post("theo", draft("1", "hello"))?;
    assert_ne!(other, first, "keys belong to their author");
    assert_eq!(chat.messages()?, [first, other]);
    Ok(())
}

#[test]
fn a_message_without_a_key_or_text_or_that_is_too_long_is_refused() -> Result<(), ChatError> {
    let mut chat = ChatBuilder::new().member("maya").build()?;
    let long = "x".repeat(MAX_BODY_BYTES + 1);
    let anchor = Some(ChatChannelAnchor { item: "reply".to_owned(), excerpt: long.clone() });
    let quoting = ChatChannelPostParams { anchor, ..draft("1", "why?") };
    assert!(matches!(chat.post("maya", draft(" ", "hello")), Err(ChatError::MissingKey)));
    assert!(matches!(chat.post("maya", draft("1", " \n")), Err(ChatError::Blank)));
    for refused in [draft("1", &long), quoting] {
        assert!(matches!(chat.post("maya", refused), Err(ChatError::TooLong { .. })));
    }
    assert_eq!(chat.messages()?, []);
    Ok(())
}

#[test]
fn a_read_mark_moves_forward_and_reaches_only_its_owners_devices() -> Result<(), ChatError> {
    let mut chat = ChatBuilder::new().member("maya").member("theo").messages(2).build()?;
    let (joined, _) = chat.join("phone", &participant("maya"))?;
    let [first, second] = [joined.page.messages[0].seq, joined.page.messages[1].seq];
    assert_eq!(joined.read, 0);
    for (seq, moved) in [(first, Some(first)), (first, None), (i64::MAX, Some(second)), (first, None)] {
        let delivered = chat.channels.read("maya", &ChatChannelReadParams { session_id: SESSION.into(), seq })?;
        let mark = |seq| (vec!["maya", "phone"], ChatChannelEvent::Read { participant: "maya".to_owned(), seq });
        assert_eq!(received(&delivered), Vec::from_iter(moved.map(mark)));
    }
    assert_eq!(chat.join("maya", &participant("maya"))?.0.read, second);
    assert_eq!(chat.join("theo", &participant("theo"))?.0.read, 0);
    Ok(())
}

#[test]
fn an_ask_joins_its_author_and_keeps_the_messages_it_was_sent_with() -> Result<(), ChatError> {
    let mut chat = ChatBuilder::new().member("maya").messages(2).build()?;
    let posted = chat.messages()?;
    let [first, second] = [posted[0].seq, posted[1].seq];
    let asking = ask("do it", vec![second, first, first, 9999]);
    let delivered = chat.channels.record_ask(asking.clone(), "prompt".into())?;
    let (joined, _) = chat.join("maya", &participant("maya"))?;
    let asked = joined.page.messages[2].clone();
    let member = ChatChannelEvent::Member { member: joined.members[1].clone() };
    assert_eq!(received(&delivered), [(vec!["maya"], member), (vec!["maya"], sent(&asked))]);
    assert_eq!((asked.author.as_str(), asked.body.as_str()), ("theo", "do it"));
    assert_eq!(asked.kind, ChatMessageKind::Ask { message_id: "prompt".into(), sources: vec![first, second] });
    assert_eq!(chat.channels.record_ask(asking, "prompt".into())?, []);

    let long = format!("a{}", "é".repeat(MAX_BODY_BYTES));
    chat.channels.record_ask(ask(&long, vec![asked.seq]), "long".into())?;
    let messages = chat.messages()?;
    assert_eq!(messages.len(), 4);
    let kind = ChatMessageKind::Ask { message_id: "long".into(), sources: Vec::new() };
    assert_eq!(messages[3].kind, kind, "only posts are sources");
    assert_eq!(messages[3].body.len(), MAX_BODY_BYTES - 1, "a long prompt is cut short at a character");
    assert!(long.starts_with(&messages[3].body));
    Ok(())
}

#[test]
fn a_channel_is_open_only_to_clients_that_joined_it() -> Result<(), ChatError> {
    let mut chat = ChatBuilder::new().member("maya").messages(1).build()?;
    let read = ChatChannelReadParams { session_id: SESSION.into(), seq: 1 };
    let from = history(ChatChannelHistoryFrom::After(0), 10);
    assert!(matches!(chat.post("theo", draft("1", "hello")), Err(ChatError::NotJoined)));
    assert!(matches!(chat.channels.history("theo", &from), Err(ChatError::NotJoined)));
    assert!(matches!(chat.channels.read("theo", &read), Err(ChatError::NotJoined)));

    let elsewhere = ChatChannelJoinParams { session_id: "elsewhere".into(), after: None };
    let (joined, _) = chat.channels.join("maya", &participant("maya"), &elsewhere)?;
    assert_eq!(joined.page.messages, [], "each session has its own channel");
    chat.channels.keep_only(&"elsewhere".into());
    assert!(matches!(chat.channels.history("maya", &from), Err(ChatError::NotJoined)));
    Ok(())
}

#[test]
fn a_reopened_channel_drops_only_a_half_written_event() -> Result<(), ChatError> {
    let mut chat = ChatBuilder::new().member("maya").member("theo").messages(2).build()?;
    let messages = chat.messages()?;
    let read = ChatChannelReadParams { session_id: SESSION.into(), seq: messages[1].seq };
    chat.channels.read("maya", &read)?;
    for client in ["maya", "theo"] {
        chat.leave(client);
    }
    OpenOptions::new().append(true).open(chat.log())?.write_all(br#"{"type":"message","message":{"se"#)?;

    let (joined, _) = chat.join("maya", &participant("maya"))?;
    assert_eq!(joined.page.messages, messages);
    assert_eq!((names(&joined), joined.read), (vec!["maya", "theo"], messages[1].seq));
    assert_eq!(joined.present, people(&["maya"]));
    let (next, _) = chat.post("maya", draft("next", "hi"))?;
    chat.leave("maya");
    assert_eq!(chat.join("maya", &participant("maya"))?.0.page.messages, [messages, vec![next]].concat());
    Ok(())
}

#[test]
fn a_channel_is_used_from_one_place_at_a_time() -> Result<(), ChatError> {
    let mut chat = ChatBuilder::new().member("maya").build()?;
    let mut other = ChatChannels::open(chat.directory.path().to_path_buf())?;
    let refused = other.join("theo", &participant("theo"), &join_params());
    assert!(matches!(refused, Err(ChatError::InUse)));
    chat.leave("maya");
    other.record_ask(ask("do it", Vec::new()), "prompt".into())?;
    let (joined, _) = chat.join("maya", &participant("maya"))?;
    assert_eq!(names(&joined), ["maya", "theo"]);
    Ok(())
}

#[test]
fn a_channel_that_cannot_be_read_is_left_as_it_is() -> Result<(), ChatError> {
    let mut chat = ChatBuilder::new().build()?;
    fs::write(chat.log(), "{\"type\":\"unknown\"}\n")?;
    assert!(matches!(chat.join("maya", &participant("maya")), Err(ChatError::Unreadable(_))));
    assert_eq!(fs::read_to_string(chat.log())?, "{\"type\":\"unknown\"}\n");
    Ok(())
}

#[test]
fn a_session_id_cannot_name_a_file_elsewhere() -> Result<(), ChatError> {
    let mut chat = ChatBuilder::new().build()?;
    let params = ChatChannelJoinParams { session_id: "../Up".into(), after: None };
    chat.channels.join("maya", &participant("maya"), &params)?;
    assert!(chat.directory.path().join("%2e%2e%2f%55p.jsonl").is_file());
    Ok(())
}

#[test]
fn a_directory_that_cannot_be_created_is_refused() -> Result<(), ChatError> {
    let directory = TempDir::new()?;
    let file = directory.path().join("file");
    fs::write(&file, "in the way")?;
    assert!(matches!(ChatChannels::<&str>::open(file.join("chat")), Err(ChatError::Io(_))));
    Ok(())
}

struct ChatBuilder {
    members: Vec<&'static str>,
    messages: u32,
}

struct TestChat {
    channels: ChatChannels<&'static str>,
    directory: TempDir,
}

impl ChatBuilder {
    fn new() -> Self {
        Self { members: Vec::new(), messages: 0 }
    }

    fn member(mut self, name: &'static str) -> Self {
        self.members.push(name);
        self
    }

    fn messages(mut self, count: u32) -> Self {
        self.messages = count;
        self
    }

    fn build(self) -> Result<TestChat, ChatError> {
        let directory = TempDir::new()?;
        let mut chat = TestChat { channels: ChatChannels::open(directory.path().to_path_buf())?, directory };
        for name in &self.members {
            chat.join(name, &participant(name))?;
        }
        for number in 1..=self.messages {
            chat.post(self.members[0], draft(&number.to_string(), "hello"))?;
        }
        Ok(chat)
    }
}

type Deliveries = Vec<Delivery<&'static str>>;

impl TestChat {
    fn join(
        &mut self,
        client: &'static str,
        participant: &Participant,
    ) -> Result<(ChatChannelJoinResponse, Deliveries), ChatError> {
        self.channels.join(client, participant, &join_params())
    }

    fn leave(&mut self, client: &'static str) -> Option<Delivery<&'static str>> {
        self.channels.leave(client, &SESSION.into())
    }

    fn post(
        &mut self,
        client: &'static str,
        params: ChatChannelPostParams,
    ) -> Result<(ChatMessage, Deliveries), ChatError> {
        let (response, deliveries) = self.channels.post(client, params)?;
        Ok((response.message, deliveries))
    }

    fn messages(&self) -> Result<Vec<ChatMessage>, ChatError> {
        Ok(self.channels.history("maya", &history(ChatChannelHistoryFrom::After(0), MAX_PAGE))?.messages)
    }

    fn log(&self) -> PathBuf {
        self.directory.path().join("session.jsonl")
    }
}

fn participant(id: &str) -> Participant {
    Participant { id: id.to_owned(), name: id.to_owned() }
}

fn join_params() -> ChatChannelJoinParams {
    ChatChannelJoinParams { session_id: SESSION.into(), after: None }
}

fn draft(key: &str, body: &str) -> ChatChannelPostParams {
    let session_id = SESSION.into();
    ChatChannelPostParams { session_id, key: key.to_owned(), body: body.to_owned(), anchor: None, after_item: None }
}

fn ask(body: &str, sources: Vec<i64>) -> Ask {
    Ask { session_id: SessionId::new(SESSION), author: participant("theo"), body: body.to_owned(), sources }
}

fn history(from: ChatChannelHistoryFrom, limit: u32) -> ChatChannelHistoryParams {
    ChatChannelHistoryParams { session_id: SESSION.into(), from, limit: Some(limit) }
}

fn received(deliveries: &[Delivery<&'static str>]) -> Vec<(Vec<&'static str>, ChatChannelEvent)> {
    let received = deliveries.iter().map(|delivery| {
        let mut to = delivery.to.clone();
        to.sort_unstable();
        (to, delivery.payload.event.clone())
    });
    received.collect()
}

fn sent(message: &ChatMessage) -> ChatChannelEvent {
    ChatChannelEvent::Message { message: message.clone() }
}

fn people(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|&id| id.to_owned()).collect()
}

fn names(joined: &ChatChannelJoinResponse) -> Vec<&str> {
    joined.members.iter().map(|member| member.participant.name.as_str()).collect()
}
