use crate::hub::ClientId;
use crate::wire;
use agent_client_protocol::v2::schema::{ContentBlock, Meta, SessionId};
use agent_client_protocol::{Error, RawJsonRpcParams, RawJsonRpcRequest};
use foxfire_acp::chat::{
    ClientMeta, FOXFIRE_META_NAMESPACE, FoxfireCapabilities, FoxfireMeta, Participant, PromptMeta,
};
use foxfire_chat::{Ask, ChatChannels, ChatError, Delivery};
use serde_json::{Map, Value, json};
use std::path::PathBuf;

pub(crate) struct Caller<'a> {
    pub(crate) client: ClientId,
    pub(crate) participant: Option<&'a Participant>,
    pub(crate) watching: bool,
}

const MAX_IDENTITY_BYTES: usize = 256;

pub(crate) fn open(directory: PathBuf) -> Option<ChatChannels<ClientId>> {
    let path = directory.display().to_string();
    ChatChannels::open(directory)
        .inspect_err(|error| tracing::warn!(%error, path, "serving no chat, as its directory cannot be used"))
        .ok()
}

pub(crate) fn capabilities() -> Meta {
    FoxfireCapabilities { chat: true }.to_meta()
}

pub(crate) fn answer(
    chat_channels: &mut ChatChannels<ClientId>,
    caller: &Caller<'_>,
    method: &str,
    params: Option<RawJsonRpcParams>,
) -> Result<(Value, Vec<Delivery<ClientId>>), Error> {
    match method {
        "_foxfire/channel_join" => {
            let params = wire::parse(params)?;
            let Some(participant) = caller.participant else {
                return Err(Error::invalid_request().data("the client did not say who is using it"));
            };
            if !caller.watching {
                return Err(Error::invalid_request().data("open the session before joining its channel"));
            }
            let joined = chat_channels.join(caller.client, participant, &params);
            joined.map(|(response, deliveries)| (json!(response), deliveries)).map_err(|error| refusal(&error))
        }
        "_foxfire/channel_post" => {
            let posted = chat_channels.post(caller.client, wire::parse(params)?);
            posted.map(|(response, deliveries)| (json!(response), deliveries)).map_err(|error| refusal(&error))
        }
        "_foxfire/channel_history" => {
            let page = chat_channels.history(caller.client, &wire::parse(params)?);
            page.map(|page| (json!(page), Vec::new())).map_err(|error| refusal(&error))
        }
        _ => Err(Error::method_not_found()),
    }
}

pub(crate) fn refusal(error: &ChatError) -> Error {
    let refused = match error {
        ChatError::NotJoined => Error::invalid_request(),
        ChatError::MissingKey | ChatError::Blank | ChatError::TooLong { .. } => Error::invalid_params(),
        ChatError::InUse | ChatError::Unreadable(_) | ChatError::Io(_) => {
            tracing::warn!(%error, "chat failed");
            Error::internal_error()
        }
    };
    refused.data(error.to_string())
}

pub(crate) fn introduced(request: &RawJsonRpcRequest) -> Option<Participant> {
    let meta = wire::field(request.params.as_ref(), "_meta").and_then(Value::as_object);
    let Participant { id, name } = ClientMeta::from_meta(meta).participant?;
    let (id, name) = (id.trim(), name.trim());
    let valid = [id, name].iter().all(|text| !text.is_empty() && text.len() <= MAX_IDENTITY_BYTES);
    valid.then(|| Participant { id: id.to_owned(), name: name.to_owned() })
}

pub(crate) fn take_ask(request: &mut RawJsonRpcRequest, session: String, author: Option<Participant>) -> Option<Ask> {
    let Some(RawJsonRpcParams::Object(params)) = &mut request.params else {
        return None;
    };
    let PromptMeta { sources } = take_prompt_meta(params);
    Some(Ask { session_id: SessionId::new(session), author: author?, body: prompt_text(params), sources })
}

fn take_prompt_meta(params: &mut Map<String, Value>) -> PromptMeta {
    let Some(Value::Object(meta)) = params.get_mut("_meta") else {
        return PromptMeta::default();
    };
    let taken = PromptMeta::from_meta(Some(meta));
    if meta.remove(FOXFIRE_META_NAMESPACE).is_some() && meta.is_empty() {
        params.remove("_meta");
    }
    taken
}

fn prompt_text(params: &Map<String, Value>) -> String {
    let blocks =
        params.get("prompt").cloned().and_then(|prompt| serde_json::from_value::<Vec<ContentBlock>>(prompt).ok());
    let texts = blocks.into_iter().flatten().filter_map(|block| match block {
        ContentBlock::Text(text) => Some(text.text),
        _ => None,
    });
    texts.collect::<Vec<_>>().join("\n")
}
