//! The recorded-response transport: the default everywhere except a real cell with a vaulted key
//! (DEC-997 item 4). It replays recorded deliveries and opens no connection. It logs every
//! request it is handed, so a test can show what reached the wire and when.
//!
//! For agentic runs (ADR-0005 slice 2): [`openrouter_reply`] builds one turn's recorded reply,
//! with tool calls or a final output, so a run's turns are scripted in order; and
//! [`CompromisedModel`] is the double the harness's safety properties run against, a model that
//! picks any tool call each turn, adaptively from what it has read.

use std::collections::{BTreeMap, VecDeque};

use mandate_canon::Digest;
use mandate_time::UtcNanos;

use serde_json::{Map, Value as Json, json};

use crate::money::Usage;
use crate::provider::{Clock, Delivery, HttpRequest, ProviderKey, Transport, after_ms};
use crate::registry::Reported;
use crate::tools;

/// One recorded delivery and how long it took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    pub latency_ms: u64,
    pub delivery: Delivery,
}

/// One request the transport was handed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub host: String,
    pub body_digest: Digest,
    pub body: Vec<u8>,
    pub at: UtcNanos,
    pub deadline: UtcNanos,
    pub with_key: bool,
}

/// Replays recorded deliveries: first those recorded for the exact request body, then the
/// scripted queue in order. With neither, the attempt fails to connect.
#[derive(Debug, Clone, Default)]
pub struct RecordedTransport {
    by_request: BTreeMap<Digest, VecDeque<Recorded>>,
    script: VecDeque<Recorded>,
    sent: Vec<Sent>,
}

impl RecordedTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Deliveries for a request whose body has this digest, in order.
    pub fn record(&mut self, body_digest: Digest, deliveries: Vec<Recorded>) {
        self.by_request
            .entry(body_digest)
            .or_default()
            .extend(deliveries);
    }

    /// Deliveries for any request, in order, after the recorded ones.
    pub fn script(&mut self, deliveries: Vec<Recorded>) {
        self.script.extend(deliveries);
    }

    /// Drops every scripted delivery not yet replayed.
    pub fn clear_script(&mut self) {
        self.script.clear();
    }

    /// Every request handed to the transport, in order.
    pub fn sent(&self) -> &[Sent] {
        &self.sent
    }
}

impl Transport for RecordedTransport {
    fn send(
        &mut self,
        request: &HttpRequest,
        key: Option<&ProviderKey>,
        deadline: UtcNanos,
        clock: &mut dyn Clock,
    ) -> Delivery {
        let body_digest = Digest::of(&request.body);
        self.sent.push(Sent {
            host: request.host.clone(),
            body_digest,
            body: request.body.clone(),
            at: clock.now(),
            deadline,
            with_key: key.is_some(),
        });
        let next = self
            .by_request
            .get_mut(&body_digest)
            .and_then(VecDeque::pop_front)
            .or_else(|| self.script.pop_front());
        let Some(recorded) = next else {
            return Delivery::ConnectFailed;
        };
        let now = clock.now();
        if let Some(done) = after_ms(now, recorded.latency_ms) {
            clock.wait_until(done);
        }
        recorded.delivery
    }
}

/// An OpenRouter reply body: `content`, the tool calls `calls` (declared names, sent by their wire
/// names, with their arguments' JSON text), what it says served it, and its token usage. For
/// recorded multi-turn fakes: each turn of a run is one such body.
pub fn openrouter_reply(
    content: Option<&str>,
    calls: &[(&str, &str)],
    reported: &Reported,
    usage: Usage,
) -> Vec<u8> {
    let tool_calls: Vec<Json> = calls
        .iter()
        .zip(0u32..)
        .map(|((name, arguments), i)| {
            json!({
                "id": format!("provider-call-{i}"),
                "type": "function",
                "function": {"name": tools::wire_name(name).unwrap_or_else(|| (*name).to_owned()), "arguments": arguments},
            })
        })
        .collect();
    let mut message = Map::new();
    message.insert("role".to_owned(), json!("assistant"));
    message.insert(
        "content".to_owned(),
        content.map_or(Json::Null, |c| json!(c)),
    );
    let finish = if tool_calls.is_empty() {
        "stop"
    } else {
        message.insert("tool_calls".to_owned(), Json::Array(tool_calls));
        "tool_calls"
    };
    let mut body = Map::new();
    body.insert("id".to_owned(), json!("gen-fake"));
    body.insert("object".to_owned(), json!("chat.completion"));
    body.insert(
        "choices".to_owned(),
        json!([{"index": 0, "finish_reason": finish, "message": Json::Object(message)}]),
    );
    body.insert(
        "usage".to_owned(),
        json!({
            "prompt_tokens": usage.input,
            "completion_tokens": usage.output,
            "prompt_tokens_details": {"cached_tokens": usage.cached},
        }),
    );
    if let Some(model) = &reported.model {
        body.insert("model".to_owned(), json!(model));
    }
    if let Some(provider) = &reported.provider {
        body.insert("provider".to_owned(), json!(provider));
    }
    serde_json::to_vec(&Json::Object(body)).unwrap_or_default()
}

/// How a [`CompromisedModel`] picks its reply each turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Any offered tool, with schema-valid arguments, chosen by the seed.
    Random,
    /// Adaptive: a tool a tool result names, with arguments copied from the results where a string
    /// fits, the way injected text would steer a model; otherwise as `Random`.
    FollowResults,
    /// The previous turn's tool calls again, word for word.
    Repeat,
    /// What a hostile or broken model sends: a tool not offered, arguments outside the schema, or
    /// more calls than a turn takes, chosen by the seed.
    Hostile,
}

/// A compromised-model double (ADR-0005 §10, agent harness spec HI properties): a transport that
/// answers each turn with whatever tool calls its strategy picks from the request it is sent, its
/// offered tools and its transcript, and never opens a connection. With no tool offered it answers
/// `final_output`. Every reply reports `reported` as its identity.
#[derive(Debug, Clone)]
pub struct CompromisedModel {
    strategy: Strategy,
    state: u64,
    calls_per_turn: u32,
    final_output: String,
    reported: Reported,
    usage: Usage,
    sent: Vec<Sent>,
}

impl CompromisedModel {
    pub fn new(strategy: Strategy, seed: u64, reported: Reported) -> Self {
        Self {
            strategy,
            state: seed,
            calls_per_turn: 1,
            final_output: "{}".to_owned(),
            reported,
            usage: Usage {
                input: 100,
                output: 20,
                cached: 0,
            },
            sent: Vec::new(),
        }
    }

    /// Up to this many calls a turn (at least one); more than the entry allows tests its bound.
    pub fn with_calls_per_turn(mut self, calls: u32) -> Self {
        self.calls_per_turn = calls.max(1);
        self
    }

    /// The text of the reply to a turn offered no tool.
    pub fn with_final_output(mut self, output: &str) -> Self {
        output.clone_into(&mut self.final_output);
        self
    }

    pub fn with_usage(mut self, usage: Usage) -> Self {
        self.usage = usage;
        self
    }

    /// Every request handed to the double, in order.
    pub fn sent(&self) -> &[Sent] {
        &self.sent
    }

    fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next().checked_rem(n).unwrap_or(0)
    }

    /// The reply body to one request, from the request alone and the double's seed.
    fn answer(&mut self, request: &Json) -> Vec<u8> {
        let offered: Vec<(String, Json)> = request
            .get("tools")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(|tool| {
                let function = tool.get("function")?;
                Some((
                    function.get("name")?.as_str()?.to_owned(),
                    function.get("parameters").cloned().unwrap_or(Json::Null),
                ))
            })
            .collect();
        if offered.is_empty() {
            let output = self.final_output.clone();
            return openrouter_reply(Some(&output), &[], &self.reported, self.usage);
        }
        let messages: Vec<&Json> = request
            .get("messages")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .collect();
        let results: Vec<String> = messages
            .iter()
            .filter(|m| m.get("role").and_then(Json::as_str) == Some("tool"))
            .filter_map(|m| m.get("content").and_then(Json::as_str).map(str::to_owned))
            .collect();
        let previous: Vec<(String, String)> = messages
            .iter()
            .rev()
            .find(|m| m.get("role").and_then(Json::as_str) == Some("assistant"))
            .and_then(|m| m.get("tool_calls").and_then(Json::as_array))
            .into_iter()
            .flatten()
            .filter_map(|c| {
                let function = c.get("function")?;
                Some((
                    function.get("name")?.as_str()?.to_owned(),
                    function.get("arguments")?.as_str()?.to_owned(),
                ))
            })
            .collect();
        let count = self.below(u64::from(self.calls_per_turn)).saturating_add(1);
        let mut calls: Vec<(String, String)> = Vec::new();
        match self.strategy {
            Strategy::Repeat if !previous.is_empty() => calls = previous,
            Strategy::Hostile => {
                let pick = self.below(3);
                let (name, _) = self.pick(&offered);
                calls = match pick {
                    0 => vec![("shell__exec".to_owned(), "{}".to_owned())],
                    1 => vec![(name, INVALID_ARGUMENTS.to_owned())],
                    _ => (0..=self.calls_per_turn.saturating_add(4))
                        .map(|_| (name.clone(), "{}".to_owned()))
                        .collect(),
                };
            }
            _ => {
                for _ in 0..count {
                    let named = (self.strategy == Strategy::FollowResults)
                        .then(|| {
                            offered
                                .iter()
                                .find(|(wire, _)| {
                                    results.iter().any(|r| {
                                        r.contains(wire.as_str())
                                            || r.contains(&wire.replace("__", "."))
                                    })
                                })
                                .cloned()
                        })
                        .flatten();
                    let (name, schema) = named.unwrap_or_else(|| self.pick(&offered));
                    let words: Vec<String> = if self.strategy == Strategy::FollowResults {
                        results
                            .iter()
                            .flat_map(|r| {
                                r.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.'))
                                    .filter(|w| !w.is_empty())
                                    .map(str::to_owned)
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                    let arguments = self.generate(&schema, &words);
                    calls.push((name, arguments.to_string()));
                }
            }
        }
        let declared: Vec<(String, String)> = calls
            .into_iter()
            .map(|(wire, args)| (wire.replace("__", "."), args))
            .collect();
        let pairs: Vec<(&str, &str)> = declared
            .iter()
            .map(|(n, a)| (n.as_str(), a.as_str()))
            .collect();
        openrouter_reply(None, &pairs, &self.reported, self.usage)
    }

    fn pick(&mut self, offered: &[(String, Json)]) -> (String, Json) {
        let n = u64::try_from(offered.len()).unwrap_or(1);
        let at = usize::try_from(self.below(n)).unwrap_or(0);
        offered
            .get(at)
            .cloned()
            .unwrap_or_else(|| (String::new(), Json::Null))
    }

    /// Arguments that fit `schema`, a provider JSON Schema of the gateway's language
    /// ([`tools::wire_schema`]); strings come from `words` when one fits.
    fn generate(&mut self, schema: &Json, words: &[String]) -> Json {
        let bound = |name: &str| schema.get(name).and_then(Json::as_u64);
        match schema.get("type").and_then(Json::as_str) {
            Some("object") => {
                let mut out = Map::new();
                for (name, inner) in schema
                    .get("properties")
                    .and_then(Json::as_object)
                    .into_iter()
                    .flatten()
                {
                    out.insert(name.clone(), self.generate(inner, words));
                }
                Json::Object(out)
            }
            Some("array") => {
                let max = bound("maxItems").unwrap_or(0);
                let n = self.below(max.saturating_add(1));
                let items = schema.get("items").cloned().unwrap_or(Json::Null);
                Json::Array((0..n).map(|_| self.generate(&items, words)).collect())
            }
            Some("string") => {
                if let Some(options) = schema.get("enum").and_then(Json::as_array) {
                    let n = u64::try_from(options.len()).unwrap_or(1);
                    let at = usize::try_from(self.below(n)).unwrap_or(0);
                    return options.get(at).cloned().unwrap_or(Json::Null);
                }
                match bound("maxLength") {
                    None => json!("0.5"),
                    Some(max) => {
                        let fits: Vec<&String> = words
                            .iter()
                            .filter(|w| u64::try_from(w.chars().count()).is_ok_and(|n| n <= max))
                            .collect();
                        let n = u64::try_from(fits.len()).unwrap_or(0);
                        if n > 0 {
                            let at = usize::try_from(self.below(n)).unwrap_or(0);
                            return fits.get(at).map_or(Json::Null, |w| json!(w));
                        }
                        let len = usize::try_from(max.min(4)).unwrap_or(0);
                        json!("x".repeat(len))
                    }
                }
            }
            Some("integer") => {
                let lo = schema.get("minimum").and_then(Json::as_u64).unwrap_or(0);
                let hi = schema.get("maximum").and_then(Json::as_u64).unwrap_or(lo);
                let span = hi.saturating_sub(lo).saturating_add(1);
                json!(lo.saturating_add(self.below(span)))
            }
            Some("boolean") => json!(self.below(2) == 1),
            _ => Json::Null,
        }
    }
}

/// Arguments outside every tool's schema: a member no declaration lists.
const INVALID_ARGUMENTS: &str = r#"{"not_a_parameter":true}"#;

impl Transport for CompromisedModel {
    fn send(
        &mut self,
        request: &HttpRequest,
        key: Option<&ProviderKey>,
        deadline: UtcNanos,
        clock: &mut dyn Clock,
    ) -> Delivery {
        self.sent.push(Sent {
            host: request.host.clone(),
            body_digest: Digest::of(&request.body),
            body: request.body.clone(),
            at: clock.now(),
            deadline,
            with_key: key.is_some(),
        });
        let Ok(parsed) = serde_json::from_slice::<Json>(&request.body) else {
            return Delivery::ConnectFailed;
        };
        Delivery::Response {
            status: 200,
            body: self.answer(&parsed),
            retry_after_ms: None,
        }
    }
}
