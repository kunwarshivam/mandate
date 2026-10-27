//! The records journal spec v0.46 §9.19 closes for the model gateway (DEC-998, E15-8's code half,
//! DEC-2201): the agent stream's `ModelInvocationRecorded`, with rules 146 to 152, and the meter
//! stream `meter:{workspace_id}`'s `StreamOpened`, `MeterReserved`, and `MeterSettled`, with rules
//! 154 to 157, and the same on the deployment meter stream `dmeter:{deployment_id}` (journal spec
//! v0.53, DEC-2207), whose `StreamOpened` names its deployment. Rules 153 and 158 are stream rules, [`stream_rule`]'s, which
//! [`crate::check_stream`] runs with the stream's earlier records. Every rule only refuses a draft.
//!
//! The control stream's `ModelInvocationRecorded` is the compiler's, [`crate::workspace`]'s; rules
//! 72 to 75 are shared with it there.

use std::collections::BTreeSet;

use mandate_canon::Value;

use crate::schema::Ty;
use crate::workspace::{REFUSALS, bound_model, invocation_rules};
use crate::{Draft, Invalid, InvalidReason, StreamId, StreamType};

const INVOCATION: &str = "ModelInvocationRecorded";
const OPENED: &str = "StreamOpened";
const RESERVED: &str = "MeterReserved";
const SETTLED: &str = "MeterSettled";

/// The outcomes of a call that reached for a provider and found its deadline passed, or the vault
/// without its key, before the first attempt left: the only ones that may send nothing (rules 152
/// and 157).
/// The purposes of a call with no agent: the compiler's and an evaluation's (rule 156, DEC-2216).
const NO_AGENT_PURPOSES: [&str; 2] = ["compiler", "evaluation"];
const SENT_NOTHING: [&str; 2] = ["deadline_exceeded", "credential_invalid"];

/// The agent harness spec §6.5 check reasons only a thesis's verdict may carry (rule 151);
/// `forbidden_language` fits either kind, `unknown_thesis` only an invalidation.
const THESIS_REFUSALS: [&str; 4] = [
    "unknown_instrument",
    "unknown_evidence",
    "duplicate_instrument",
    "revisions_not_enabled",
];

/// Whether §9.19 governs `event_type` on `stream`: every record of the meter stream, and the
/// agent stream's `ModelInvocationRecorded`.
pub(crate) fn governs(stream: &StreamId, event_type: &str) -> bool {
    match stream.stream_type() {
        StreamType::Meter | StreamType::DeploymentMeter => true,
        StreamType::Agent => event_type == INVOCATION,
        _ => false,
    }
}

/// The §9.19 schema of `event_type` at `schema_version` on `stream`, or `None` when this module
/// registers none there.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    match (event_type, schema_version, stream) {
        (INVOCATION, 1, StreamType::Agent) => Some(&AGENT_INVOCATION),
        (OPENED, 1, StreamType::Meter) => Some(&METER_STREAM_OPENED),
        (OPENED, 1, StreamType::DeploymentMeter) => Some(&DEPLOYMENT_METER_STREAM_OPENED),
        (RESERVED, 1, StreamType::Meter | StreamType::DeploymentMeter) => Some(&METER_RESERVED),
        (SETTLED, 1, StreamType::Meter | StreamType::DeploymentMeter) => Some(&METER_SETTLED),
        _ => None,
    }
}

/// Rules 146 to 152 and 155 to 157 on a payload its schema has normalized, in rule order (rules 72
/// to 75, which rule 146's record shares with the control stream's, between 146 and 147): the first
/// that fails is reported. `config_refs` and `actor` are the envelope's.
pub(crate) fn rules(
    event_type: &str,
    stream: StreamType,
    payload: &Value,
    config_refs: Option<&Value>,
    actor: Option<&Value>,
) -> Result<(), Invalid> {
    let p = View(payload);
    let writer = actor
        .and_then(|a| a.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    match (event_type, stream) {
        (INVOCATION, StreamType::Agent) => {
            bound_model(payload, config_refs)?;
            let outcome = p.text("outcome");
            let refusal = REFUSALS.contains(&outcome);
            let cache_hit = p.flag("cache_hit");
            invocation_rules(payload, SENT_NOTHING.contains(&outcome))?;
            ensure(writer == "agent", "actor.kind")?;
            guarded(p, outcome)?;
            ensure(
                p.is_null("reservation") == (refusal || cache_hit),
                "payload.reservation",
            )?;
            ascending(&p.texts("observations"), "payload.observations")?;
            verdicts(p, outcome)?;
            if !refusal && !cache_hit && p.int("attempts") == 0 {
                sent_nothing(p)?;
            }
            Ok(())
        }
        (RESERVED, StreamType::Meter | StreamType::DeploymentMeter) => {
            ensure(writer == "system", "actor.kind")?;
            ensure(
                p.is_null("agent_id") == NO_AGENT_PURPOSES.contains(&p.text("purpose")),
                "payload.agent_id",
            )?;
            ensure(
                !p.text("max_cost_usd").starts_with('-'),
                "payload.max_cost_usd",
            )
        }
        (SETTLED, StreamType::Meter | StreamType::DeploymentMeter) => {
            ensure(writer == "system", "actor.kind")?;
            ensure(!p.text("cost_usd").starts_with('-'), "payload.cost_usd")?;
            if p.int("attempts") == 0 {
                ensure(
                    SENT_NOTHING.contains(&p.text("outcome")),
                    "payload.attempts",
                )?;
                ensure(p.zero_tokens(), "payload.tokens")?;
                ensure(p.text("cost_usd") == "0", "payload.cost_usd")?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Rule 154 (`stream_mismatch`): a meter stream's `StreamOpened` names its own stream, the
/// workspace's or the deployment's.
pub(crate) fn subject(event_type: &str, stream: &StreamId, payload: &Value) -> Result<(), Invalid> {
    if event_type != OPENED {
        return Ok(());
    }
    let named = match stream.stream_type() {
        StreamType::Meter => format!("meter:{}", View(payload).text("workspace_id")),
        StreamType::DeploymentMeter => format!("dmeter:{}", View(payload).text("deployment_id")),
        _ => return Ok(()),
    };
    if named == stream.as_str() {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::StreamMismatch, "stream_id"))
    }
}

/// Rule 148: a refused prompt, and the canonical request that holds it, are never stored, and the
/// record names the guard's rule instead; every other outcome stores both and names no rule. Each
/// is reported at itself, the first that fails in that order.
fn guarded(p: View<'_>, outcome: &str) -> Result<(), Invalid> {
    let refused = outcome == "input_rejected";
    ensure(p.is_null("prompt_ref") == refused, "payload.prompt_ref")?;
    ensure(
        p.is_null("request_digest") == refused,
        "payload.request_digest",
    )?;
    ensure(p.is_null("guard_rule") != refused, "payload.guard_rule")
}

/// Rule 151: verdicts only for a research call that answered, one per candidate in order, the
/// theses first, each with a result its kind allows.
fn verdicts(p: View<'_>, outcome: &str) -> Result<(), Invalid> {
    let items = p.list("verdicts");
    if items.is_empty() {
        return Ok(());
    }
    ensure(
        p.text("purpose") == "research" && outcome == "ok",
        "payload.verdicts",
    )?;
    let mut invalidations = false;
    for (i, item) in items.iter().enumerate() {
        let item = View(item);
        let at = |member: &str| format!("payload.verdicts[{i}].{member}");
        ensure(
            u64::try_from(i)
                .is_ok_and(|i| item.0.get("candidate").and_then(Value::as_int) == Some(i)),
            &at("candidate"),
        )?;
        let kind = item.text("kind");
        let invalidation = kind == "invalidation";
        ensure(invalidation || !invalidations, &at("kind"))?;
        invalidations |= invalidation;
        let result = item.text("result");
        let fits = matches!(result, "judged" | "forbidden_language")
            || if invalidation {
                result == "unknown_thesis"
            } else {
                THESIS_REFUSALS.contains(&result)
            };
        ensure(fits, &at("result"))?;
    }
    Ok(())
}

/// Rule 152's record of a call that sent nothing: no response, no tokens, no cost, the first that
/// fails in that order. That only such calls record `attempts` 0 is rule 74's, as rule 152 amends
/// it ([`invocation_rules`]).
fn sent_nothing(p: View<'_>) -> Result<(), Invalid> {
    for member in ["response_ref", "reported_identity", "provider_request_id"] {
        ensure(p.is_null(member), &format!("payload.{member}"))?;
    }
    ensure(p.zero_tokens(), "payload.tokens")?;
    ensure(p.text("cost_usd") == "0", "payload.cost_usd")
}

/// Rules 153 and 158 on one draft, against the stream's records `before` it, in order.
pub(crate) fn stream_rule(before: &[&Draft], draft: &Draft) -> Result<(), Invalid> {
    let before = before.iter().copied();
    let stream = draft.stream_id().stream_type();
    let judged = (draft.event_type(), draft.schema_version(), stream);
    let mine = payload_of(draft);
    let call = View(&mine).text("call_id");
    match judged {
        (INVOCATION, 1, StreamType::Agent) => {
            let mut calls = before
                .filter(|d| d.event_type() == INVOCATION)
                .map(payload_of);
            ensure(
                !calls.any(|p| View(&p).text("call_id") == call),
                "payload.call_id",
            )
        }
        (RESERVED, 1, StreamType::Meter | StreamType::DeploymentMeter) => {
            let mut reserved = before
                .filter(|d| d.event_type() == RESERVED)
                .map(payload_of);
            ensure(
                !reserved.any(|p| View(&p).text("call_id") == call),
                "payload.call_id",
            )
        }
        (SETTLED, 1, StreamType::Meter | StreamType::DeploymentMeter) => {
            let reservation = View(&mine).text("reservation");
            let mut reserved_call = None;
            let mut settled = BTreeSet::new();
            for d in before {
                let p = payload_of(d);
                match d.event_type() {
                    RESERVED if d.event_id() == reservation => {
                        reserved_call = Some(View(&p).text("call_id").to_owned());
                    }
                    SETTLED => {
                        settled.insert(View(&p).text("reservation").to_owned());
                    }
                    _ => {}
                }
            }
            let Some(reserved_call) = reserved_call else {
                return Err(Invalid::new(InvalidReason::Schema, "payload.reservation"));
            };
            ensure(reserved_call == call, "payload.call_id")?;
            ensure(!settled.contains(reservation), "payload.reservation")
        }
        _ => Ok(()),
    }
}

fn payload_of(draft: &Draft) -> Value {
    draft
        .fields()
        .get("payload")
        .cloned()
        .unwrap_or(Value::Null)
}

fn ensure(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, path))
    }
}

/// Strictly ascending by bytes, so a repeat is refused as well as a swap (rule 150,
/// `non_canonical`).
fn ascending(items: &[&str], path: &str) -> Result<(), Invalid> {
    if items.windows(2).all(|w| matches!(w, [a, b] if a < b)) {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::NonCanonical, path))
    }
}

/// A normalized payload, read member by member; a `null` or absent member reads as empty text.
#[derive(Clone, Copy)]
struct View<'a>(&'a Value);

impl<'a> View<'a> {
    fn text(self, member: &str) -> &'a str {
        self.0
            .get(member)
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    fn is_null(self, member: &str) -> bool {
        matches!(self.0.get(member), None | Some(Value::Null))
    }

    fn flag(self, member: &str) -> bool {
        self.0.get(member) == Some(&Value::Bool(true))
    }

    fn int(self, member: &str) -> u64 {
        self.0
            .get(member)
            .and_then(Value::as_int)
            .unwrap_or_default()
    }

    fn list(self, member: &str) -> &'a [Value] {
        self.0
            .get(member)
            .and_then(Value::as_array)
            .unwrap_or_default()
    }

    fn texts(self, member: &str) -> Vec<&'a str> {
        self.list(member).iter().filter_map(Value::as_str).collect()
    }

    fn zero_tokens(self) -> bool {
        self.0
            .get("tokens")
            .and_then(Value::as_object)
            .is_some_and(|t| t.values().all(|v| v.as_int() == Some(0)))
    }
}

const MODEL: Ty = Ty::Record(&[
    ("model_id", Ty::Str),
    ("model_version", Ty::Str),
    ("content_hash", Ty::DigestRef),
]);

const TOKENS: Ty = Ty::Record(&[("input", Ty::Int), ("output", Ty::Int), ("cached", Ty::Int)]);

const PURPOSE: Ty = Ty::OneOf(&["research", "llm_signal", "fast_signal"]);

static AGENT_INVOCATION: Ty = Ty::Record(&[
    ("call_id", Ty::Ulid),
    ("purpose", PURPOSE),
    ("model", MODEL),
    ("endpoint", Ty::Str),
    ("request_digest", Ty::Nullable(&Ty::DigestRef)),
    (
        "sampling",
        Ty::Record(&[
            ("temperature", Ty::Decimal),
            ("seed", Ty::Nullable(&Ty::Int)),
        ]),
    ),
    ("prompt_ref", Ty::Nullable(&Ty::DigestRef)),
    (
        "guard_rule",
        Ty::Nullable(&Ty::OneOf(&[
            "credential",
            "account_identifier",
            "personal_data",
            "foreign_workspace",
            "input_contract",
        ])),
    ),
    ("response_ref", Ty::Nullable(&Ty::DigestRef)),
    ("reported_identity", Ty::Nullable(&Ty::Str)),
    ("provider_request_id", Ty::Nullable(&Ty::Str)),
    (
        "outcome",
        Ty::OneOf(&[
            "ok",
            "policy_denied",
            "budget_exhausted",
            "rate_limited_local",
            "meter_unavailable",
            "input_rejected",
            "model_withdrawn",
            "deadline_exceeded",
            "provider_unavailable",
            "rate_limited_provider",
            "credential_invalid",
            "content_refused",
            "schema_invalid",
            "identity_mismatch",
        ]),
    ),
    ("attempts", Ty::Int),
    ("tokens", TOKENS),
    ("cost_usd", Ty::Decimal),
    ("price_table_ref", Ty::DigestRef),
    ("cache_hit", Ty::Bool),
    ("deadline", Ty::Timestamp),
    ("completed_at", Ty::Timestamp),
    ("reservation", Ty::Nullable(&Ty::Ulid)),
    ("observations", Ty::List(&Ty::Ulid)),
    (
        "verdicts",
        Ty::List(&Ty::Record(&[
            ("candidate", Ty::Int),
            ("kind", Ty::OneOf(&["thesis", "invalidation"])),
            (
                "result",
                Ty::OneOf(&[
                    "judged",
                    "unknown_instrument",
                    "unknown_evidence",
                    "duplicate_instrument",
                    "forbidden_language",
                    "revisions_not_enabled",
                    "unknown_thesis",
                ]),
            ),
        ])),
    ),
]);

static METER_STREAM_OPENED: Ty = Ty::Record(&[
    ("stream_type", Ty::OneOf(&["meter"])),
    ("workspace_id", Ty::Ident),
]);

static DEPLOYMENT_METER_STREAM_OPENED: Ty = Ty::Record(&[
    ("stream_type", Ty::OneOf(&["deployment_meter"])),
    ("deployment_id", Ty::Ident),
]);

static METER_RESERVED: Ty = Ty::Record(&[
    ("call_id", Ty::Ulid),
    ("agent_id", Ty::Nullable(&Ty::Ident)),
    (
        "purpose",
        Ty::OneOf(&[
            "research",
            "llm_signal",
            "fast_signal",
            "compiler",
            "evaluation",
        ]),
    ),
    ("model", MODEL),
    ("endpoint", Ty::Str),
    ("key_owner", Ty::OneOf(&["platform", "customer"])),
    ("price_table_ref", Ty::DigestRef),
    ("max_cost_usd", Ty::Decimal),
    ("risk_day", Ty::Date),
]);

static METER_SETTLED: Ty = Ty::Record(&[
    ("call_id", Ty::Ulid),
    ("reservation", Ty::Ulid),
    (
        "outcome",
        Ty::OneOf(&[
            "ok",
            "deadline_exceeded",
            "provider_unavailable",
            "rate_limited_provider",
            "credential_invalid",
            "content_refused",
            "schema_invalid",
            "identity_mismatch",
        ]),
    ),
    ("attempts", Ty::Int),
    ("tokens", TOKENS),
    ("cost_usd", Ty::Decimal),
]);
