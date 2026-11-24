use agent_client_protocol::v2::schema::{AgentCapabilities, MessageId, Meta, SessionId};
use agent_client_protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::meta::{from_meta, to_meta};

pub const FOXFIRE_META_NAMESPACE: &str = "foxfire";

pub const FOXFIRE_METHOD_PREFIX: &str = "_foxfire/";

pub trait FoxfireMeta: Serialize + DeserializeOwned + Default {
    #[must_use]
    fn to_meta(&self) -> Meta {
        to_meta(self, Some(FOXFIRE_META_NAMESPACE)).unwrap_or_default()
    }

    #[must_use]
    fn from_meta(meta: Option<&Meta>) -> Self {
        from_meta(meta, Some(FOXFIRE_META_NAMESPACE))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FoxfireCapabilities {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub chat: bool,
}

impl FoxfireMeta for FoxfireCapabilities {}

impl FoxfireCapabilities {
    pub fn of(capabilities: &AgentCapabilities) -> Self {
        Self::from_meta(capabilities.session.as_ref().and_then(|session| session.meta.as_ref()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClientMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub participant: Option<Participant>,
}

impl FoxfireMeta for ClientMeta {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PromptMeta {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<i64>,
}

impl FoxfireMeta for PromptMeta {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelMember {
    #[serde(flatten)]
    pub participant: Participant,
    pub joined_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub seq: i64,
    pub author: String,
    pub body: String,
    pub sent_at_ms: i64,
    #[serde(flatten)]
    pub kind: ChatMessageKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ChatMessageKind {
    Post {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<ChatChannelAnchor>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after_item: Option<String>,
    },
    Ask {
        message_id: MessageId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        sources: Vec<i64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelAnchor {
    pub item: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonRpcRequest)]
#[request(method = "_foxfire/channel_join", response = ChatChannelJoinResponse)]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelJoinParams {
    pub session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelJoinResponse {
    pub members: Vec<ChatChannelMember>,
    pub present: Vec<String>,
    pub read: i64,
    #[serde(flatten)]
    pub page: ChatChannelPage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonRpcRequest)]
#[request(method = "_foxfire/channel_post", response = ChatChannelPostResponse)]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelPostParams {
    pub session_id: SessionId,
    pub key: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<ChatChannelAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_item: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelPostResponse {
    pub message: ChatMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonRpcRequest)]
#[request(method = "_foxfire/channel_history", response = ChatChannelPage)]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelHistoryParams {
    pub session_id: SessionId,
    pub from: ChatChannelHistoryFrom,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatChannelHistoryFrom {
    Before(i64),
    After(i64),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelPage {
    pub messages: Vec<ChatMessage>,
    pub more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonRpcNotification)]
#[notification(method = "_foxfire/channel_read")]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelReadParams {
    pub session_id: SessionId,
    pub seq: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonRpcNotification)]
#[notification(method = "_foxfire/channel_event")]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelEventPayload {
    pub session_id: SessionId,
    pub event: ChatChannelEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ChatChannelEvent {
    Message { message: ChatMessage },
    Member { member: ChatChannelMember },
    Presence { present: Vec<String> },
    Read { participant: String, seq: i64 },
}
