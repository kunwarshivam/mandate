//! The OpenRouter adapter (DEC-997 item 2): one pinned model slug, one upstream provider with
//! fallbacks off, zero data retention, and no capability beyond the tool declarations a turn
//! offers, and a reply read for the reported model and provider that the gateway compares with the
//! pin (INF-1, INF-2, INF-5).
//!
//! A tool goes by its wire name ([`tools::wire_name`]: `.` becomes `__`), since OpenRouter's
//! upstreams take only `[a-zA-Z0-9_-]` in a function name (DEC-2204). The transcript's tool calls
//! carry ids the gateway synthesizes, never the provider's, and their arguments as canonical JSON.

use serde_json::{Map, Number, Value as Json, json};

use crate::money::Usage;
use crate::provider::{HttpRequest, Prompt, ProviderAdapter, Reply};
use crate::registry::{Endpoint, PinnedContent, Reported};
use crate::tools::{self, WireCall};

/// The chat-completions path; the host is the endpoint's.
pub const PATH: &str = "/api/v1/chat/completions";
/// Request members that would give the model a capability, which no request carries (INF-5).
/// `transforms` is sent, empty, so OpenRouter applies no prompt compression by default.
pub const CAPABILITY_MEMBERS: [&str; 3] = ["functions", "function_call", "plugins"];
/// The members that offer tools: present exactly when a turn offers some, holding exactly the
/// offered declarations, with `tool_choice` `auto` (inference spec §6, DEC-2203).
pub const TOOL_MEMBERS: [&str; 2] = ["tools", "tool_choice"];

#[derive(Debug, Clone, Copy, Default)]
pub struct OpenRouter;

impl ProviderAdapter for OpenRouter {
    fn request(
        &self,
        content: &PinnedContent,
        endpoint: &Endpoint,
        prompt: &Prompt,
    ) -> HttpRequest {
        let upstream = endpoint.route.upstream.as_str();
        let mut body = Map::new();
        body.insert("model".to_owned(), json!(content.identity.model_name));
        body.insert("messages".to_owned(), Json::Array(messages(prompt)));
        if !prompt.tools.is_empty() {
            let offered: Vec<Json> = prompt
                .tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tools::wire_name(&tool.name),
                            "description": tool.description,
                            "parameters": tools::wire_schema(&tool.arguments),
                        },
                    })
                })
                .collect();
            body.insert("tools".to_owned(), Json::Array(offered));
            body.insert("tool_choice".to_owned(), json!("auto"));
        }
        body.insert("temperature".to_owned(), json!(0));
        if let Some(top_p) = content.top_p
            && let Ok(number) = top_p.text().parse::<Number>()
        {
            body.insert("top_p".to_owned(), Json::Number(number));
        }
        if let Some(seed) = content.seed {
            body.insert("seed".to_owned(), json!(seed));
        }
        match content.reasoning {
            Some(crate::registry::Reasoning::Off) => {
                body.insert("reasoning".to_owned(), json!({"enabled": false}));
            }
            Some(crate::registry::Reasoning::On(effort)) => {
                body.insert("reasoning".to_owned(), json!({"effort": effort.code()}));
            }
            None => {}
        }
        body.insert("max_tokens".to_owned(), json!(content.max_output_tokens));
        body.insert("response_format".to_owned(), json!({"type": "json_object"}));
        body.insert("stream".to_owned(), json!(false));
        body.insert("transforms".to_owned(), json!([]));
        let mut provider = Map::new();
        provider.insert("order".to_owned(), json!([upstream]));
        provider.insert("only".to_owned(), json!([upstream]));
        provider.insert("allow_fallbacks".to_owned(), json!(false));
        provider.insert("require_parameters".to_owned(), json!(true));
        provider.insert("data_collection".to_owned(), json!("deny"));
        provider.insert("zdr".to_owned(), json!(true));
        if let Some(quantization) = &content.identity.quantization {
            provider.insert("quantizations".to_owned(), json!([quantization]));
        }
        body.insert("provider".to_owned(), Json::Object(provider));
        HttpRequest {
            host: endpoint.host.clone(),
            path: PATH.to_owned(),
            body: serde_json::to_vec(&Json::Object(body)).unwrap_or_default(),
        }
    }

    fn reply(&self, body: &[u8]) -> Option<Reply> {
        let value: Json = serde_json::from_slice(body).ok()?;
        let object = value.as_object()?;
        let choice = object.get("choices")?.as_array()?.first()?;
        let message = choice.get("message")?;
        let text = |v: Option<&Json>| v.and_then(Json::as_str).map(str::to_owned);
        let count = |path: &[&str]| -> Option<u64> {
            let mut at = object.get("usage")?;
            for name in path {
                at = at.get(*name)?;
            }
            at.as_u64()
        };
        let tool_calls = match message.get("tool_calls") {
            None | Some(Json::Null) => Vec::new(),
            Some(Json::Array(calls)) if calls.is_empty() => Vec::new(),
            Some(Json::Array(calls)) => calls.iter().map(wire_call).collect(),
            Some(_) => vec![WireCall::default()],
        };
        let refused = choice.get("finish_reason").and_then(Json::as_str) == Some("content_filter")
            || message.get("refusal").is_some_and(|r| !r.is_null());
        Some(Reply {
            request_id: text(object.get("id")),
            content: text(message.get("content")),
            tool_calls,
            refused,
            usage: Usage {
                input: count(&["prompt_tokens"])?,
                output: count(&["completion_tokens"])?,
                cached: count(&["prompt_tokens_details", "cached_tokens"]).unwrap_or(0),
            },
        })
    }

    fn reported(&self, body: &[u8]) -> Reported {
        let value: Json = serde_json::from_slice(body).unwrap_or(Json::Null);
        let text = |name: &str| value.get(name).and_then(Json::as_str).map(str::to_owned);
        Reported {
            model: text("model"),
            provider: text("provider"),
        }
    }
}

/// The messages in the stable order (inference spec §3.1): the system text, the user text, then each
/// earlier turn's tool calls and their results. The tools themselves travel in `tools`.
fn messages(prompt: &Prompt) -> Vec<Json> {
    let mut out = vec![
        json!({"role": "system", "content": prompt.system}),
        json!({"role": "user", "content": prompt.user}),
    ];
    for (turn, earlier) in (1..).zip(&prompt.transcript) {
        let calls: Vec<Json> = earlier
            .calls
            .iter()
            .map(|call| {
                json!({
                    "id": tools::call_id(turn, call.index),
                    "type": "function",
                    "function": {
                        "name": tools::wire_name(&call.name),
                        "arguments": tools::canonical_text(&call.arguments),
                    },
                })
            })
            .collect();
        out.push(json!({"role": "assistant", "content": Json::Null, "tool_calls": calls}));
        for (call, result) in earlier.calls.iter().zip(&earlier.results) {
            out.push(json!({
                "role": "tool",
                "tool_call_id": tools::call_id(turn, call.index),
                "content": result,
            }));
        }
    }
    out
}

/// One reply tool call: its function's name and its arguments' JSON text, each `None` when missing
/// or of the wrong type.
fn wire_call(call: &Json) -> WireCall {
    let function = call.get("function");
    let text = |name: &str| {
        function
            .and_then(|f| f.get(name))
            .and_then(Json::as_str)
            .map(str::to_owned)
    };
    WireCall {
        name: text("name"),
        arguments: text("arguments"),
    }
}
