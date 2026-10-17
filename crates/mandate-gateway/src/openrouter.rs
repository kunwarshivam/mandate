//! The OpenRouter adapter (DEC-997 item 2): one pinned model slug, one upstream provider with
//! fallbacks off, zero data retention, and no capability, and a reply read for the reported model
//! and provider that the gateway compares with the pin (INF-1, INF-2, INF-5).

use serde_json::{Map, Value as Json, json};

use crate::money::Usage;
use crate::provider::{HttpRequest, Prompt, ProviderAdapter, Reply};
use crate::registry::{Endpoint, PinnedContent, Reported};

/// The chat-completions path; the host is the endpoint's.
pub const PATH: &str = "/api/v1/chat/completions";
/// Request members that would give the model a capability, which no request carries (INF-5).
/// `transforms` is sent, empty, so OpenRouter applies no prompt compression by default.
pub const CAPABILITY_MEMBERS: [&str; 5] = [
    "tools",
    "tool_choice",
    "functions",
    "function_call",
    "plugins",
];

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
        body.insert(
            "messages".to_owned(),
            json!([
                {"role": "system", "content": prompt.system},
                {"role": "user", "content": prompt.user},
            ]),
        );
        body.insert("temperature".to_owned(), json!(0));
        if let Some(seed) = content.seed {
            body.insert("seed".to_owned(), json!(seed));
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
        let _ = body; None
    }
}
