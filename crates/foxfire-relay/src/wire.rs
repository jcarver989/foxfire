use agent_client_protocol::v2::schema::{InitializeResponse, Meta};
use agent_client_protocol::{
    Error, RawJsonRpcError, RawJsonRpcId, RawJsonRpcMessage, RawJsonRpcNotification, RawJsonRpcParams,
    RawJsonRpcResponse, TransportFrame,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

pub(crate) fn params(params: impl Serialize) -> Option<RawJsonRpcParams> {
    RawJsonRpcParams::from_value(serde_json::to_value(params).ok()?).ok().flatten()
}

pub(crate) fn notification(method: &str, params: impl Serialize) -> TransportFrame {
    let notification = RawJsonRpcNotification { method: method.into(), params: self::params(params) };
    TransportFrame::Single(RawJsonRpcMessage::Notification(notification))
}

pub(crate) fn response(id: RawJsonRpcId, result: Result<Value, Box<RawJsonRpcError>>) -> TransportFrame {
    TransportFrame::Single(RawJsonRpcMessage::Response(RawJsonRpcResponse::new(id, result)))
}

pub(crate) fn result(id: RawJsonRpcId, result: impl Serialize) -> TransportFrame {
    response(id, Ok(serde_json::to_value(result).unwrap_or(Value::Null)))
}

pub(crate) fn failure(id: RawJsonRpcId, error: Error) -> TransportFrame {
    response(id, Err(Box::new(error.into())))
}

pub(crate) fn cancel(id: i64) -> TransportFrame {
    notification("$/cancel_request", serde_json::json!({ "requestId": id }))
}

pub(crate) fn readdress(response: RawJsonRpcResponse, id: RawJsonRpcId) -> RawJsonRpcResponse {
    match response {
        RawJsonRpcResponse::Result { result, .. } => RawJsonRpcResponse::Result { id, result },
        RawJsonRpcResponse::Error { error, .. } => RawJsonRpcResponse::Error { id, error },
    }
}

pub(crate) fn field<'a>(params: Option<&'a RawJsonRpcParams>, name: &str) -> Option<&'a Value> {
    match params? {
        RawJsonRpcParams::Object(object) => object.get(name),
        RawJsonRpcParams::Array(_) => None,
    }
}

pub(crate) fn session_of(params: Option<&RawJsonRpcParams>) -> Option<String> {
    field(params, "sessionId").and_then(Value::as_str).map(str::to_owned)
}

pub(crate) fn parse<T: DeserializeOwned>(params: Option<RawJsonRpcParams>) -> Result<T, Error> {
    let params = params.map_or(Value::Null, RawJsonRpcParams::into_value);
    serde_json::from_value(params).map_err(|error| Error::invalid_params().data(error.to_string()))
}

pub(crate) fn advertise(response: &mut InitializeResponse, addition: Meta) {
    let session = response.capabilities.session.get_or_insert_default();
    session.meta = Some(merge(session.meta.take(), addition));
}

pub(crate) fn merge(meta: Option<Meta>, addition: Meta) -> Meta {
    let mut meta = meta.unwrap_or_default();
    for (namespace, value) in addition {
        match (meta.get_mut(&namespace), value) {
            (Some(Value::Object(existing)), Value::Object(added)) => existing.extend(added),
            (_, value) => {
                meta.insert(namespace, value);
            }
        }
    }
    meta
}
