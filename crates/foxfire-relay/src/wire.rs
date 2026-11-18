use agent_client_protocol::{
    RawJsonRpcError, RawJsonRpcId, RawJsonRpcMessage, RawJsonRpcNotification, RawJsonRpcParams, RawJsonRpcResponse,
    TransportFrame,
};
use serde::Serialize;
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
