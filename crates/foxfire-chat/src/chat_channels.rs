use crate::chat_channel::{ChatChannel, Delivery};
use crate::error::ChatError;
use agent_client_protocol::v2::schema::{MessageId, SessionId};
use foxfire_acp::chat::{
    ChatChannelHistoryFrom, ChatChannelHistoryParams, ChatChannelJoinParams, ChatChannelJoinResponse, ChatChannelPage,
    ChatChannelPostParams, ChatChannelPostResponse, ChatChannelReadParams, ChatMessageKind, Participant,
};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs::create_dir_all;
use std::hash::Hash;
use std::path::PathBuf;

pub const MAX_BODY_BYTES: usize = 16 * 1024;
pub const MAX_PAGE: u32 = 200;
const PAGE: u32 = 100;

pub struct ChatChannels<T> {
    directory: Option<PathBuf>,
    open: HashMap<SessionId, ChatChannel<T>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub session_id: SessionId,
    pub author: Participant,
    pub body: String,
    pub sources: Vec<i64>,
}

impl<T: Copy + Eq + Hash> ChatChannels<T> {
    pub fn open(directory: PathBuf) -> Result<Self, ChatError> {
        create_dir_all(&directory)?;
        Ok(Self { directory: Some(directory), open: HashMap::new() })
    }

    /// Keeps each channel only while someone is in it.
    pub fn in_memory() -> Self {
        Self { directory: None, open: HashMap::new() }
    }

    pub fn join(
        &mut self,
        client: T,
        participant: &Participant,
        params: &ChatChannelJoinParams,
    ) -> Result<(ChatChannelJoinResponse, Vec<Delivery<T>>), ChatError> {
        let channel = self.channel(&params.session_id)?;
        let deliveries = channel.join(client, participant)?;
        let response = ChatChannelJoinResponse {
            members: channel.members(),
            present: channel.present(),
            read: channel.read(&participant.id),
            page: channel.page(params.after.map(ChatChannelHistoryFrom::After), PAGE as usize),
        };
        Ok((response, deliveries))
    }

    pub fn leave(&mut self, client: T, session: &SessionId) -> Option<Delivery<T>> {
        let left = self.open.get_mut(session)?.leave(client);
        self.close_unused();
        left
    }

    pub fn keep_only(&mut self, session: &SessionId) {
        self.open.retain(|open, _| open == session);
    }

    pub fn post(
        &mut self,
        client: T,
        params: ChatChannelPostParams,
    ) -> Result<(ChatChannelPostResponse, Vec<Delivery<T>>), ChatError> {
        let ChatChannelPostParams { session_id, key, body, anchor, after_item } = params;
        let (channel, author) = self.joined(client, &session_id)?;
        let excerpt = anchor.as_ref().map_or(0, |anchor| anchor.excerpt.len());
        if key.trim().is_empty() {
            return Err(ChatError::MissingKey);
        }
        if body.trim().is_empty() {
            return Err(ChatError::Blank);
        }
        if body.len().max(excerpt) > MAX_BODY_BYTES {
            return Err(ChatError::TooLong { limit: MAX_BODY_BYTES });
        }
        let kind = ChatMessageKind::Post { key, anchor, after_item };
        let (message, deliveries) = channel.post(client, &author, body, kind)?;
        Ok((ChatChannelPostResponse { message }, deliveries))
    }

    pub fn record_ask(&mut self, ask: Ask, message_id: MessageId) -> Result<Vec<Delivery<T>>, ChatError> {
        let Ask { session_id, author, mut body, mut sources } = ask;
        let channel = self.channel(&session_id)?;
        body.truncate(body.floor_char_boundary(MAX_BODY_BYTES));
        sources.retain(|seq| channel.is_post(*seq));
        sources.sort_unstable();
        sources.dedup();
        let recorded = channel.record_ask(&author, body, ChatMessageKind::Ask { message_id, sources });
        self.close_unused();
        recorded
    }

    pub fn history(&self, client: T, params: &ChatChannelHistoryParams) -> Result<ChatChannelPage, ChatError> {
        let channel = self.open.get(&params.session_id).filter(|channel| channel.participant(client).is_some());
        let limit = params.limit.unwrap_or(PAGE).clamp(1, MAX_PAGE);
        Ok(channel.ok_or(ChatError::NotJoined)?.page(Some(params.from), limit as usize))
    }

    pub fn read(&mut self, client: T, params: &ChatChannelReadParams) -> Result<Vec<Delivery<T>>, ChatError> {
        let (channel, participant) = self.joined(client, &params.session_id)?;
        channel.mark_read(participant, params.seq)
    }

    fn channel(&mut self, session: &SessionId) -> Result<&mut ChatChannel<T>, ChatError> {
        match self.open.entry(session.clone()) {
            Entry::Occupied(open) => Ok(open.into_mut()),
            Entry::Vacant(closed) => {
                let path = self.directory.as_ref().map(|directory| directory.join(file_name(&session.to_string())));
                Ok(closed.insert(ChatChannel::open(session.clone(), path.as_deref())?))
            }
        }
    }

    fn joined(&mut self, client: T, session: &SessionId) -> Result<(&mut ChatChannel<T>, String), ChatError> {
        let channel = self.open.get_mut(session).ok_or(ChatError::NotJoined)?;
        let participant = channel.participant(client).ok_or(ChatError::NotJoined)?.to_owned();
        Ok((channel, participant))
    }

    fn close_unused(&mut self) {
        self.open.retain(|_, channel| !channel.is_unused());
    }
}

fn file_name(session: &str) -> String {
    let escaped = session.bytes().map(|byte| match byte {
        b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' => char::from(byte).to_string(),
        _ => format!("%{byte:02x}"),
    });
    escaped.collect::<String>() + ".jsonl"
}
