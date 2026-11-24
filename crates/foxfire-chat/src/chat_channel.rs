use crate::error::ChatError;
use crate::log::Log;
use agent_client_protocol::v2::schema::SessionId;
use chrono::Utc;
use foxfire_acp::chat::{
    ChatChannelEvent, ChatChannelEventPayload, ChatChannelHistoryFrom, ChatChannelMember, ChatChannelPage, ChatMessage,
    ChatMessageKind, Participant,
};
use std::collections::{BTreeSet, HashMap};
use std::hash::Hash;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery<T> {
    pub to: Vec<T>,
    pub payload: ChatChannelEventPayload,
}

pub(crate) struct ChatChannel<T> {
    session: SessionId,
    log: Log,
    members: Vec<Member>,
    messages: Vec<ChatMessage>,
    connections: HashMap<T, String>,
}

struct Member {
    member: ChatChannelMember,
    read: i64,
}

impl<T: Copy + Eq + Hash> ChatChannel<T> {
    pub(crate) fn open(session: SessionId, path: &Path) -> Result<Self, ChatError> {
        let (log, events) = Log::open(path)?;
        let mut channel = Self { session, log, members: Vec::new(), messages: Vec::new(), connections: HashMap::new() };
        for event in events {
            channel.apply(event);
        }
        Ok(channel)
    }

    pub(crate) fn participant(&self, client: T) -> Option<&str> {
        self.connections.get(&client).map(String::as_str)
    }

    pub(crate) fn is_unused(&self) -> bool {
        self.connections.is_empty()
    }

    pub(crate) fn members(&self) -> Vec<ChatChannelMember> {
        self.members.iter().map(|known| known.member.clone()).collect()
    }

    pub(crate) fn read(&self, participant: &str) -> i64 {
        self.member(participant).map_or(0, |known| known.read)
    }

    pub(crate) fn present(&self) -> Vec<String> {
        let present: BTreeSet<_> = self.connections.values().collect();
        present.into_iter().cloned().collect()
    }

    pub(crate) fn page(&self, from: Option<ChatChannelHistoryFrom>, limit: usize) -> ChatChannelPage {
        let (before, after) = match from {
            Some(ChatChannelHistoryFrom::After(seq)) => (i64::MAX, Some(seq)),
            Some(ChatChannelHistoryFrom::Before(seq)) => (seq, None),
            None => (i64::MAX, None),
        };
        let start = self.messages.partition_point(|message| after.is_some_and(|after| message.seq <= after));
        let end = self.messages.partition_point(|message| message.seq < before);
        let found = &self.messages[start..end];
        let length = found.len().min(limit);
        let skipped = if after.is_some() { 0 } else { found.len() - length };
        ChatChannelPage { messages: found[skipped..skipped + length].to_vec(), more: length < found.len() }
    }

    pub(crate) fn is_post(&self, seq: i64) -> bool {
        let found = self.messages.binary_search_by_key(&seq, |message| message.seq);
        found.is_ok_and(|index| matches!(self.messages[index].kind, ChatMessageKind::Post { .. }))
    }

    pub(crate) fn join(&mut self, client: T, participant: &Participant) -> Result<Vec<Delivery<T>>, ChatError> {
        let before = self.present();
        let mut deliveries = self.commit(self.introduction(participant).into_iter().collect(), Some(client))?;
        self.connections.insert(client, participant.id.clone());
        deliveries.extend(self.presence_since(&before, Some(client)));
        Ok(deliveries)
    }

    pub(crate) fn leave(&mut self, client: T) -> Option<Delivery<T>> {
        let before = self.present();
        self.connections.remove(&client);
        self.presence_since(&before, None)
    }

    pub(crate) fn post(
        &mut self,
        client: T,
        author: &str,
        body: String,
        kind: ChatMessageKind,
    ) -> Result<(ChatMessage, Vec<Delivery<T>>), ChatError> {
        if let Some(posted) = self.find(author, &kind) {
            return Ok((posted.clone(), Vec::new()));
        }
        let message = self.next_message(author, body, kind);
        let deliveries = self.commit(vec![ChatChannelEvent::Message { message: message.clone() }], Some(client))?;
        Ok((message, deliveries))
    }

    pub(crate) fn record_ask(
        &mut self,
        author: &Participant,
        body: String,
        kind: ChatMessageKind,
    ) -> Result<Vec<Delivery<T>>, ChatError> {
        let mut events = Vec::from_iter(self.introduction(author));
        if self.find(&author.id, &kind).is_none() {
            events.push(ChatChannelEvent::Message { message: self.next_message(&author.id, body, kind) });
        }
        self.commit(events, None)
    }

    pub(crate) fn mark_read(&mut self, participant: String, seq: i64) -> Result<Vec<Delivery<T>>, ChatError> {
        let seq = seq.min(self.newest());
        if seq <= self.read(&participant) {
            return Ok(Vec::new());
        }
        self.commit(vec![ChatChannelEvent::Read { participant, seq }], None)
    }

    fn member(&self, participant: &str) -> Option<&Member> {
        self.members.iter().find(|known| known.member.participant.id == participant)
    }

    fn newest(&self) -> i64 {
        self.messages.last().map_or(0, |message| message.seq)
    }

    fn find(&self, author: &str, kind: &ChatMessageKind) -> Option<&ChatMessage> {
        self.messages.iter().find(|message| message.author == author && same_message(&message.kind, kind))
    }

    fn next_message(&self, author: &str, body: String, kind: ChatMessageKind) -> ChatMessage {
        let sent_at_ms = Utc::now().timestamp_millis();
        ChatMessage { seq: self.newest() + 1, author: author.to_owned(), body, sent_at_ms, kind }
    }

    fn introduction(&self, participant: &Participant) -> Option<ChatChannelEvent> {
        let known = self.member(&participant.id).map(|known| &known.member);
        if known.is_some_and(|known| known.participant == *participant) {
            return None;
        }
        let joined_at_ms = known.map_or_else(|| Utc::now().timestamp_millis(), |known| known.joined_at_ms);
        Some(ChatChannelEvent::Member { member: ChatChannelMember { participant: participant.clone(), joined_at_ms } })
    }

    fn commit(&mut self, events: Vec<ChatChannelEvent>, except: Option<T>) -> Result<Vec<Delivery<T>>, ChatError> {
        self.log.append(&events)?;
        for event in &events {
            self.apply(event.clone());
        }
        Ok(events.into_iter().map(|event| self.deliver(event, except)).collect())
    }

    fn apply(&mut self, event: ChatChannelEvent) {
        match event {
            ChatChannelEvent::Message { message } => self.messages.push(message),
            ChatChannelEvent::Member { member } => {
                let known = self.members.iter_mut().find(|known| known.member.participant.id == member.participant.id);
                match known {
                    Some(known) => known.member = member,
                    None => self.members.push(Member { member, read: 0 }),
                }
            }
            ChatChannelEvent::Read { participant, seq } => {
                let known = self.members.iter_mut().find(|known| known.member.participant.id == participant);
                if let Some(known) = known {
                    known.read = seq;
                }
            }
            ChatChannelEvent::Presence { .. } => {}
        }
    }

    fn presence_since(&self, before: &[String], except: Option<T>) -> Option<Delivery<T>> {
        let present = self.present();
        (present != before).then(|| self.deliver(ChatChannelEvent::Presence { present }, except))
    }

    fn deliver(&self, event: ChatChannelEvent, except: Option<T>) -> Delivery<T> {
        let reaches = |participant: &String| match &event {
            ChatChannelEvent::Read { participant: reader, .. } => participant == reader,
            _ => true,
        };
        let others = self.connections.iter().filter(|(client, _)| Some(**client) != except);
        let to = others.filter(|(_, participant)| reaches(participant)).map(|(client, _)| *client).collect();
        Delivery { to, payload: ChatChannelEventPayload { session_id: self.session.clone(), event } }
    }
}

fn same_message(kind: &ChatMessageKind, other: &ChatMessageKind) -> bool {
    match (kind, other) {
        (ChatMessageKind::Post { key, .. }, ChatMessageKind::Post { key: other, .. }) => key == other,
        (ChatMessageKind::Ask { message_id, .. }, ChatMessageKind::Ask { message_id: other, .. }) => {
            message_id == other
        }
        _ => false,
    }
}
