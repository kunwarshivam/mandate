//! The wire form of one call between the runtime and the model-gateway service (DEC-2265 item 5):
//! a [`CallRequest`] one way, and its [`CallRecord`] or [`RequestError`] back, each as canonical
//! JSON. Decoding is the exact inverse of encoding: anything else reads as a [`WireError`].
//!
//! The same exchange carries a research artifact's [`PutRequest`] (DEC-2279 item 1): the
//! workspace and the bytes one way, and the reference the service computed from them, or a
//! [`PutRefusal`], back. A request frame is a put when it has a `put` member, and a call otherwise
//! ([`any_request_from`]).

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_time::UtcNanos;

use crate::call::{CallRequest, RequestError};
use crate::guard::GuardRule;
use crate::kinds::{KeyOwner, Outcome, Purpose};
use crate::money::{Usage, Usd6};
use crate::record::{CallRecord, model_value};
use crate::registry::{ModelRef, int, object};
use crate::tools::{ToolCall, Turn};

/// The wire form's version, a member of every frame.
pub const WIRE_VERSION: u64 = 1;

/// Why a frame does not decode: the first member that is missing, unlisted, of the wrong kind, out
/// of range, or not in canonical form.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    #[error("the frame is not canonical JSON")]
    NotCanonical,
    #[error("the frame's member `{0}` does not read")]
    Member(&'static str),
}

/// What the gateway answers one request with.
pub type Reply = Result<CallRecord, RequestError>;

/// A research artifact the runtime asks the service to store (DEC-2279 item 1). It carries no
/// reference: the service computes one from the bytes it receives (item 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutRequest {
    pub workspace_id: String,
    pub bytes: Vec<u8>,
}

/// Why the service stored nothing for a put (DEC-2279 items 1, 2 and 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutRefusal {
    /// The put names a workspace other than the service's.
    ForeignWorkspace,
    /// Storing it would take the research store past the service's cap.
    StoreFull,
    /// The store did not take it: a write failed, or the object under its reference no longer
    /// re-hashes.
    StoreUnavailable,
}

impl PutRefusal {
    pub const ALL: [Self; 3] = [
        Self::ForeignWorkspace,
        Self::StoreFull,
        Self::StoreUnavailable,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::ForeignWorkspace => "foreign_workspace",
            Self::StoreFull => "store_full",
            Self::StoreUnavailable => "store_unavailable",
        }
    }
}

/// What the service answers a put with: the SHA-256 of the bytes it stored, or why it stored none.
pub type PutReply = Result<Digest, PutRefusal>;

/// A request frame of either kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Call(Box<CallRequest>),
    Put(PutRequest),
}

/// `put`'s frame body.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn put_bytes(put: &PutRequest) -> Vec<u8> {
    let _ = put;
    todo!()
}

/// The put a frame body holds.
///
/// # Errors
/// [`WireError`] for any body [`put_bytes`] would not write.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn put_from(bytes: &[u8]) -> Result<PutRequest, WireError> {
    let _ = bytes;
    todo!()
}

/// The request a frame body holds, a put when it has a `put` member and a call otherwise.
///
/// # Errors
/// [`WireError`] for any body neither [`put_bytes`] nor [`request_bytes`] would write.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn any_request_from(bytes: &[u8]) -> Result<Request, WireError> {
    let _ = bytes;
    todo!()
}

/// `reply`'s frame body.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn put_reply_bytes(reply: &PutReply) -> Vec<u8> {
    let _ = reply;
    todo!()
}

/// The put reply a frame body holds.
///
/// # Errors
/// [`WireError`] for any body [`put_reply_bytes`] would not write.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn put_reply_from(bytes: &[u8]) -> Result<PutReply, WireError> {
    let _ = bytes;
    todo!()
}

/// `request`'s frame body.
pub fn request_bytes(request: &CallRequest) -> Vec<u8> {
    let inputs = request
        .inputs
        .iter()
        .map(|(name, value)| Value::Array(vec![text(name), text(value)]))
        .collect();
    let turns = request
        .turns
        .iter()
        .map(|turn| {
            object([
                ("response", text(&hex(&turn.response))),
                ("results", texts(&turn.results)),
            ])
        })
        .collect();
    to_canonical(&object([
        ("agent_id", optional(request.agent_id.as_deref())),
        ("as_of", text(&request.as_of.to_string())),
        ("call_id", text(&request.call_id)),
        ("deadline", text(&request.deadline.to_string())),
        ("inputs", Value::Array(inputs)),
        ("key_owner", text(request.key_owner.code())),
        ("model", model_value(&request.model)),
        ("observations", texts(&request.observations)),
        ("purpose", text(request.purpose.code())),
        ("run_kind", optional(request.run_kind.as_deref())),
        (
            "spend_ceiling",
            request
                .spend_ceiling
                .map_or(Value::Null, |ceiling| text(&ceiling.to_string())),
        ),
        ("tools", texts(&request.tools)),
        ("turns", Value::Array(turns)),
        ("wire_version", int(WIRE_VERSION)),
        ("workspace_id", text(&request.workspace_id)),
    ]))
}

/// The request a frame body holds.
///
/// # Errors
/// [`WireError`] for any body [`request_bytes`] would not write.
pub fn request_from(bytes: &[u8]) -> Result<CallRequest, WireError> {
    let frame = canonical(bytes)?;
    let m = Members(&frame);
    let request = CallRequest {
        call_id: m.text("call_id")?,
        workspace_id: m.text("workspace_id")?,
        agent_id: m.optional_text("agent_id")?,
        purpose: purpose(&m.text("purpose")?).ok_or(WireError::Member("purpose"))?,
        model: model(m.get("model")?)?,
        inputs: m
            .array("inputs")?
            .iter()
            .map(|pair| match pair {
                Value::Array(pair) => match pair.as_slice() {
                    [Value::Str(name), Value::Str(value)] => Ok((name.clone(), value.clone())),
                    _ => Err(WireError::Member("inputs")),
                },
                _ => Err(WireError::Member("inputs")),
            })
            .collect::<Result<_, _>>()?,
        as_of: m.time("as_of")?,
        deadline: m.time("deadline")?,
        observations: m.texts("observations")?,
        key_owner: key_owner(&m.text("key_owner")?).ok_or(WireError::Member("key_owner"))?,
        run_kind: m.optional_text("run_kind")?,
        turns: m
            .array("turns")?
            .iter()
            .map(|turn| {
                let turn = Members(turn);
                Ok(Turn {
                    response: unhex(&turn.text("response")?).ok_or(WireError::Member("turns"))?,
                    results: turn.texts("results")?,
                })
            })
            .collect::<Result<_, WireError>>()?,
        tools: m.texts("tools")?,
        spend_ceiling: m
            .optional_text("spend_ceiling")?
            .map(|ceiling| Usd6::parse(&ceiling).map_err(|_| WireError::Member("spend_ceiling")))
            .transpose()?,
    };
    exact(request_bytes(&request) == bytes, request)
}

/// `reply`'s frame body.
pub fn reply_bytes(reply: &Reply) -> Vec<u8> {
    let (name, value) = match reply {
        Ok(record) => ("record", record_value(record)),
        Err(refusal) => ("refused", text(refusal.code())),
    };
    to_canonical(&object([
        (name, value),
        ("wire_version", int(WIRE_VERSION)),
    ]))
}

/// The reply a frame body holds.
///
/// # Errors
/// [`WireError`] for any body [`reply_bytes`] would not write.
pub fn reply_from(bytes: &[u8]) -> Result<Reply, WireError> {
    let frame = canonical(bytes)?;
    let reply = match (frame.get("record"), frame.get("refused")) {
        (Some(record), None) => Ok(record_from(record)?),
        (None, Some(Value::Str(code))) => Err(refusal(code).ok_or(WireError::Member("refused"))?),
        _ => return Err(WireError::Member("record")),
    };
    exact(reply_bytes(&reply) == bytes, reply)
}

fn record_value(record: &CallRecord) -> Value {
    let digest = |d: Option<Digest>| d.map_or(Value::Null, |d| text(&reference(d)));
    let blob = |b: Option<&Vec<u8>>| b.map_or(Value::Null, |b| text(&hex(b)));
    object([
        ("agent_id", optional(record.agent_id.as_deref())),
        ("attempts", int(u64::from(record.attempts))),
        ("cache_hit", Value::Bool(record.cache_hit)),
        ("call_id", text(&record.call_id)),
        ("completed_at", text(&record.completed_at.to_string())),
        ("cost", text(&record.cost.to_string())),
        ("deadline", text(&record.deadline.to_string())),
        ("endpoint", text(&record.endpoint)),
        (
            "guard_rule",
            record
                .guard_rule
                .map_or(Value::Null, |rule| text(rule.code())),
        ),
        ("model", model_value(&record.model)),
        ("observations", texts(&record.observations)),
        ("outcome", text(record.outcome.code())),
        (
            "output",
            record
                .output
                .clone()
                .map_or(Value::Null, |output| Value::Array(vec![output])),
        ),
        ("price_table_ref", text(&reference(record.price_table_ref))),
        ("prompt", blob(record.prompt.as_ref())),
        ("prompt_ref", digest(record.prompt_ref)),
        (
            "provider_request_id",
            optional(record.provider_request_id.as_deref()),
        ),
        ("purpose", text(record.purpose.code())),
        (
            "reported_identity",
            optional(record.reported_identity.as_deref()),
        ),
        ("request_digest", digest(record.request_digest)),
        ("reservation", optional(record.reservation.as_deref())),
        ("response", blob(record.response.as_ref())),
        ("response_ref", digest(record.response_ref)),
        (
            "seed",
            record
                .seed
                .map_or(Value::Null, |seed| text(&seed.to_string())),
        ),
        (
            "tokens",
            object([
                ("cached", text(&record.tokens.cached.to_string())),
                ("input", text(&record.tokens.input.to_string())),
                ("output", text(&record.tokens.output.to_string())),
            ]),
        ),
        (
            "tool_calls",
            Value::Array(
                record
                    .tool_calls
                    .iter()
                    .map(|call| {
                        object([
                            ("arguments", call.arguments.clone()),
                            ("index", int(u64::from(call.index))),
                            ("name", text(&call.name)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

fn record_from(value: &Value) -> Result<CallRecord, WireError> {
    let m = Members(value);
    let digest = |name: &'static str| {
        m.optional_text(name)?
            .map(|r| from_reference(&r).ok_or(WireError::Member(name)))
            .transpose()
    };
    let blob = |name: &'static str| {
        m.optional_text(name)?
            .map(|b| unhex(&b).ok_or(WireError::Member(name)))
            .transpose()
    };
    let number = |of: &Members<'_>, name: &'static str| {
        of.text(name)?
            .parse::<u64>()
            .map_err(|_| WireError::Member(name))
    };
    let tokens = Members(m.get("tokens")?);
    Ok(CallRecord {
        call_id: m.text("call_id")?,
        agent_id: m.optional_text("agent_id")?,
        purpose: purpose(&m.text("purpose")?).ok_or(WireError::Member("purpose"))?,
        model: model(m.get("model")?)?,
        endpoint: m.text("endpoint")?,
        request_digest: digest("request_digest")?,
        guard_rule: m
            .optional_text("guard_rule")?
            .map(|code| guard_rule(&code).ok_or(WireError::Member("guard_rule")))
            .transpose()?,
        seed: m
            .optional_text("seed")?
            .map(|seed| seed.parse::<u64>().map_err(|_| WireError::Member("seed")))
            .transpose()?,
        prompt_ref: digest("prompt_ref")?,
        response_ref: digest("response_ref")?,
        reported_identity: m.optional_text("reported_identity")?,
        provider_request_id: m.optional_text("provider_request_id")?,
        outcome: Outcome::ALL
            .into_iter()
            .find(|outcome| outcome.code() == m.text("outcome").unwrap_or_default())
            .ok_or(WireError::Member("outcome"))?,
        attempts: m
            .get("attempts")?
            .as_int()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or(WireError::Member("attempts"))?,
        tokens: Usage {
            input: number(&tokens, "input")?,
            output: number(&tokens, "output")?,
            cached: number(&tokens, "cached")?,
        },
        cost: Usd6::parse(&m.text("cost")?).map_err(|_| WireError::Member("cost"))?,
        price_table_ref: from_reference(&m.text("price_table_ref")?)
            .ok_or(WireError::Member("price_table_ref"))?,
        cache_hit: match m.get("cache_hit")? {
            Value::Bool(hit) => *hit,
            _ => return Err(WireError::Member("cache_hit")),
        },
        deadline: m.time("deadline")?,
        completed_at: m.time("completed_at")?,
        reservation: m.optional_text("reservation")?,
        observations: m.texts("observations")?,
        output: match m.get("output")? {
            Value::Null => None,
            Value::Array(output) => match output.as_slice() {
                [output] => Some(output.clone()),
                _ => return Err(WireError::Member("output")),
            },
            _ => return Err(WireError::Member("output")),
        },
        tool_calls: m
            .array("tool_calls")?
            .iter()
            .map(|call| {
                let call = Members(call);
                Ok(ToolCall {
                    index: call
                        .get("index")?
                        .as_int()
                        .and_then(|n| u32::try_from(n).ok())
                        .ok_or(WireError::Member("tool_calls"))?,
                    name: call.text("name")?,
                    arguments: call.get("arguments")?.clone(),
                })
            })
            .collect::<Result<_, WireError>>()?,
        prompt: blob("prompt")?,
        response: blob("response")?,
    })
}

/// A frame body's value, refused unless the body is its canonical form.
fn canonical(bytes: &[u8]) -> Result<Value, WireError> {
    let value = parse(bytes).map_err(|_| WireError::NotCanonical)?;
    if to_canonical(&value) == bytes {
        Ok(value)
    } else {
        Err(WireError::NotCanonical)
    }
}

/// `decoded` when re-encoding it gives the frame back: a frame of another version, or with any
/// member the encoder would not have written, is not.
fn exact<T>(same: bool, decoded: T) -> Result<T, WireError> {
    if same {
        Ok(decoded)
    } else {
        Err(WireError::NotCanonical)
    }
}

/// One frame object's members, read strictly.
struct Members<'a>(&'a Value);

impl Members<'_> {
    fn get(&self, name: &'static str) -> Result<&Value, WireError> {
        self.0.get(name).ok_or(WireError::Member(name))
    }

    fn text(&self, name: &'static str) -> Result<String, WireError> {
        match self.get(name)? {
            Value::Str(text) => Ok(text.clone()),
            _ => Err(WireError::Member(name)),
        }
    }

    fn optional_text(&self, name: &'static str) -> Result<Option<String>, WireError> {
        match self.get(name)? {
            Value::Null => Ok(None),
            Value::Str(text) => Ok(Some(text.clone())),
            _ => Err(WireError::Member(name)),
        }
    }

    fn array(&self, name: &'static str) -> Result<&[Value], WireError> {
        match self.get(name)? {
            Value::Array(items) => Ok(items),
            _ => Err(WireError::Member(name)),
        }
    }

    fn texts(&self, name: &'static str) -> Result<Vec<String>, WireError> {
        self.array(name)?
            .iter()
            .map(|item| match item {
                Value::Str(text) => Ok(text.clone()),
                _ => Err(WireError::Member(name)),
            })
            .collect()
    }

    fn time(&self, name: &'static str) -> Result<UtcNanos, WireError> {
        UtcNanos::parse_rfc3339(&self.text(name)?).map_err(|_| WireError::Member(name))
    }
}

fn model(value: &Value) -> Result<ModelRef, WireError> {
    let m = Members(value);
    Ok(ModelRef {
        model_id: m.text("model_id")?,
        model_version: m.text("model_version")?,
        content_hash: from_reference(&m.text("content_hash")?).ok_or(WireError::Member("model"))?,
    })
}

fn purpose(code: &str) -> Option<Purpose> {
    [
        Purpose::Research,
        Purpose::LlmSignal,
        Purpose::FastSignal,
        Purpose::Compiler,
    ]
    .into_iter()
    .find(|purpose| purpose.code() == code)
}

fn key_owner(code: &str) -> Option<KeyOwner> {
    [KeyOwner::Platform, KeyOwner::Customer]
        .into_iter()
        .find(|owner| owner.code() == code)
}

fn guard_rule(code: &str) -> Option<GuardRule> {
    [
        GuardRule::Credential,
        GuardRule::AccountIdentifier,
        GuardRule::PersonalData,
        GuardRule::ForeignWorkspace,
        GuardRule::InputContract,
    ]
    .into_iter()
    .find(|rule| rule.code() == code)
}

fn refusal(code: &str) -> Option<RequestError> {
    RequestError::ALL
        .into_iter()
        .find(|refusal| refusal.code() == code)
}

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

fn texts(values: &[String]) -> Value {
    Value::Array(values.iter().map(|value| text(value)).collect())
}

fn optional(value: Option<&str>) -> Value {
    value.map_or(Value::Null, text)
}

fn reference(digest: Digest) -> String {
    format!("sha256:{}", digest.to_hex())
}

fn from_reference(text: &str) -> Option<Digest> {
    Digest::from_hex(text.strip_prefix("sha256:")?)
}

const HEX: &[u8; 16] = b"0123456789abcdef";

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .flat_map(|byte| {
            [
                HEX.get(usize::from(byte >> 4)),
                HEX.get(usize::from(byte & 0x0f)),
            ]
        })
        .flatten()
        .map(|digit| char::from(*digit))
        .collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let digit = |c: u8| {
        HEX.iter()
            .position(|h| *h == c)
            .and_then(|d| u8::try_from(d).ok())
    };
    text.as_bytes()
        .chunks(2)
        .map(|pair| match pair {
            [high, low] => Some(digit(*high)?.checked_mul(16)?.checked_add(digit(*low)?)?),
            _ => None,
        })
        .collect()
}
