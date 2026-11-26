use super::SessionCore;
use crate::command::{AgentCommand, ChatCommand, ChatReply};
use agent_client_protocol::v2::schema::{
    ContentBlock, EmbeddedResource, EmbeddedResourceResource, MessageId, TextResourceContents,
};
use foxfire_acp::chat::{
    CHAT_MESSAGES_URI, ChatChannelEvent, ChatChannelHistoryFrom, ChatChannelHistoryParams, ChatChannelJoinParams,
    ChatChannelPage, ChatChannelPostParams, ChatChannelReadParams, ChatMessage, ChatMessageKind, Participant,
};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPost {
    pub post: ChatChannelPostParams,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Earlier {
    #[default]
    None,
    Available,
    Loading,
}

/// One session's chat channel, as its client sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatChannel {
    pub me: String,
    pub messages: BTreeMap<i64, ChatMessage>,
    pub pending: Vec<PendingPost>,
    pub earlier: Earlier,
    people: HashMap<String, Participant>,
    present: Option<Vec<String>>,
    read: i64,
}

impl ChatChannel {
    pub(super) fn new(me: &Participant) -> Self {
        Self { me: me.id.clone(), ..Self::default() }
    }

    pub fn person(&self, id: &str) -> Participant {
        let unknown = || Participant { id: id.to_owned(), name: id.to_owned() };
        self.people.get(id).cloned().unwrap_or_else(unknown)
    }

    pub fn present(&self) -> Vec<Participant> {
        self.present.iter().flatten().map(|id| self.person(id)).collect()
    }

    pub fn others(&self) -> Vec<Participant> {
        self.present().into_iter().filter(|person| person.id != self.me).collect()
    }

    pub fn unread(&self) -> usize {
        let newer = self.messages.range(self.read + 1..).map(|(_, message)| message);
        newer.filter(|message| is_post(message) && message.author != self.me).count()
    }

    pub fn ask(&self, prompt: &MessageId) -> Option<&ChatMessage> {
        self.messages
            .values()
            .find(|message| matches!(&message.kind, ChatMessageKind::Ask { message_id, .. } if message_id == prompt))
    }

    pub(super) fn is_shared(&self) -> bool {
        self.present.as_ref().is_some_and(|present| present.len() > 1)
            || !self.pending.is_empty()
            || self.messages.values().any(is_post)
    }

    /// What the agent is given when a prompt is sent with these messages.
    pub(super) fn quote(&self, sources: &[i64]) -> Option<ContentBlock> {
        let quoted = sources.iter().filter_map(|seq| self.messages.get(seq));
        let lines: Vec<String> =
            quoted.map(|message| format!("{}: {}", self.person(&message.author).name, message.body)).collect();
        let text = format!("[Channel messages referenced by the user]\n{}", lines.join("\n"));
        let resource = TextResourceContents::new(text, CHAT_MESSAGES_URI).mime_type("text/plain");
        let resource = EmbeddedResource::new(EmbeddedResourceResource::TextResourceContents(resource));
        (!lines.is_empty()).then_some(ContentBlock::Resource(resource))
    }

    pub(super) fn apply(&mut self, event: ChatChannelEvent) {
        match event {
            ChatChannelEvent::Message { message } => self.insert(message),
            ChatChannelEvent::Member { member } => {
                self.people.insert(member.participant.id.clone(), member.participant);
            }
            ChatChannelEvent::Presence { present } => self.present = Some(present),
            ChatChannelEvent::Read { participant, seq } if participant == self.me => self.read = self.read.max(seq),
            ChatChannelEvent::Read { .. } => {}
        }
    }

    pub(super) fn on_reply(&mut self, reply: ChatReply) {
        match reply {
            ChatReply::Joined(Ok(joined)) => {
                // Events sent after the join can be handled before its reply, so they win.
                self.present.get_or_insert(joined.present);
                for member in joined.members {
                    self.people.entry(member.participant.id.clone()).or_insert(member.participant);
                }
                self.read = self.read.max(joined.read);
                self.add_page(joined.page);
            }
            ChatReply::Joined(Err(error)) => tracing::warn!(%error, "failed to join the chat channel"),
            ChatReply::Posted { result: Ok(posted), .. } => self.insert(posted.message),
            ChatReply::Posted { key, result: Err(error) } => {
                if let Some(pending) = self.pending.iter_mut().find(|pending| pending.post.key == key) {
                    pending.error = Some(error.to_string());
                }
            }
            ChatReply::History(Ok(page)) => self.add_page(page),
            ChatReply::History(Err(_)) => self.earlier = Earlier::Available,
        }
    }

    fn dismiss(&mut self, key: &str) {
        self.pending.retain(|pending| pending.post.key != key || pending.error.is_none());
    }

    fn add_page(&mut self, page: ChatChannelPage) {
        self.earlier = if page.more { Earlier::Available } else { Earlier::None };
        page.messages.into_iter().for_each(|message| self.insert(message));
    }

    fn insert(&mut self, message: ChatMessage) {
        if let ChatMessageKind::Post { key, .. } = &message.kind
            && message.author == self.me
        {
            self.pending.retain(|pending| pending.post.key != *key);
        }
        self.messages.insert(message.seq, message);
    }
}

impl SessionCore {
    /// The session's chat channel, once someone else is in it or has posted to it.
    pub fn chat(&self) -> Option<&ChatChannel> {
        self.chat.as_ref().filter(|chat| chat.is_shared())
    }

    /// Enqueues a post or retry. Returns false until the channel has been joined.
    pub fn post_chat(&mut self, post: ChatChannelPostParams) -> bool {
        let Some(chat) = self.chat.as_mut().filter(|chat| chat.present.is_some()) else {
            return false;
        };
        chat.dismiss(&post.key);
        chat.pending.push(PendingPost { post: post.clone(), error: None });
        self.request_chat(ChatCommand::Post(post));
        true
    }

    pub fn dismiss_chat(&mut self, key: &str) {
        if let Some(chat) = &mut self.chat {
            chat.dismiss(key);
        }
    }

    pub fn load_earlier_chat(&mut self) {
        let session_id = self.session_id().clone();
        if let Some(chat) = self.chat.as_mut().filter(|chat| chat.earlier == Earlier::Available)
            && let Some(oldest) = chat.messages.first_key_value().map(|(seq, _)| *seq)
        {
            chat.earlier = Earlier::Loading;
            let from = ChatChannelHistoryFrom::Before(oldest);
            self.request_chat(ChatCommand::History(ChatChannelHistoryParams { session_id, from, limit: None }));
        }
    }

    pub fn mark_chat_read(&mut self) {
        let session_id = self.session_id().clone();
        if let Some(chat) = &mut self.chat
            && let Some(seq) = chat.messages.last_key_value().map(|(seq, _)| *seq).filter(|seq| *seq > chat.read)
        {
            chat.read = seq;
            self.request_chat(ChatCommand::Read(ChatChannelReadParams { session_id, seq }));
        }
    }

    pub(super) fn leave_chat(&mut self) {
        self.chat = self.me.as_ref().map(ChatChannel::new);
    }

    pub(super) fn join_chat(&mut self) {
        self.leave_chat();
        if self.chat.is_some() {
            let session_id = self.session_id().clone();
            self.request_chat(ChatCommand::Join(ChatChannelJoinParams { session_id, after: None }));
        }
    }

    fn request_chat(&mut self, command: ChatCommand) {
        self.dispatch(AgentCommand::Chat(command));
    }
}

fn is_post(message: &ChatMessage) -> bool {
    matches!(message.kind, ChatMessageKind::Post { .. })
}
