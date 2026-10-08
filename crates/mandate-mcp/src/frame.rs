//! JSON-RPC 2.0 framing, and the two shapes an answer comes in: one JSON body, or a stream of
//! server-sent events of which one carries the response.

use serde::{Deserialize, Deserializer};
use serde_json::Value;
use serde_json::value::RawValue;

use crate::error::{McpError, ServerText};

/// The body of a request with `id`, or of a notification when `id` is `None`.
pub(crate) fn request_body(id: Option<u64>, method: &str, params: &Value) -> String {
    let mut message = serde_json::json!({ "jsonrpc": "2.0", "method": method, "params": params });
    if let (Some(id), Some(fields)) = (id, message.as_object_mut()) {
        fields.insert("id".to_owned(), Value::from(id));
    }
    message.to_string()
}

/// Every member but `jsonrpc` is read through [`present`], so an explicit `null` counts as
/// present: `"error": null` beside a result is two answers, not one.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Message {
    jsonrpc: String,
    #[serde(default, deserialize_with = "present")]
    id: Option<Box<RawValue>>,
    #[serde(default, deserialize_with = "present")]
    method: Option<Box<RawValue>>,
    #[serde(default, deserialize_with = "present")]
    params: Option<Box<RawValue>>,
    #[serde(default, deserialize_with = "present")]
    result: Option<Box<RawValue>>,
    #[serde(default, deserialize_with = "present")]
    error: Option<Box<RawValue>>,
}

/// A member that is in the message, `null` included.
fn present<'de, D: Deserializer<'de>>(member: D) -> Result<Option<Box<RawValue>>, D::Error> {
    Box::<RawValue>::deserialize(member).map(Some)
}

#[derive(Deserialize)]
struct ErrorCode {
    code: i64,
}

/// The answer to request `id` when the body is one JSON message.
pub(crate) fn parse_json(body: &[u8], id: u64) -> Result<ServerText, McpError> {
    let message: Message = serde_json::from_slice(body).map_err(|_| McpError::Malformed)?;
    response(message, id)
}

/// The answer to request `id` in an event stream. Server requests and notifications before it are
/// skipped; anything else that is not exactly that response is [`McpError::Malformed`].
pub(crate) fn parse_sse(body: &[u8], id: u64) -> Result<ServerText, McpError> {
    let text = std::str::from_utf8(body).map_err(|_| McpError::Malformed)?;
    let mut data: Option<String> = None;
    for line in text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
    {
        if line.is_empty() {
            if let Some(event) = data.take().filter(|event| !event.is_empty()) {
                let message: Message =
                    serde_json::from_str(&event).map_err(|_| McpError::Malformed)?;
                if message.method.is_none() {
                    return response(message, id);
                }
            }
            continue;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        if field == "data" {
            let value = value.strip_prefix(' ').unwrap_or(value);
            match data.as_mut() {
                Some(joined) => {
                    joined.push('\n');
                    joined.push_str(value);
                }
                None => data = Some(value.to_owned()),
            }
        }
    }
    Err(McpError::NoResponse)
}

fn response(message: Message, id: u64) -> Result<ServerText, McpError> {
    let answers_this = message.id.is_some_and(|got| got.get() == id.to_string());
    if message.jsonrpc != "2.0" || !answers_this || message.method.is_some() {
        return Err(McpError::Malformed);
    }
    match (message.result, message.error, message.params) {
        (Some(result), None, None) if result.get() != "null" => Ok(ServerText::new(result)),
        (None, Some(error), None) => {
            let ErrorCode { code } =
                serde_json::from_str(error.get()).map_err(|_| McpError::Malformed)?;
            Err(McpError::Rpc {
                code,
                detail: ServerText::new(error),
            })
        }
        _ => Err(McpError::Malformed),
    }
}
