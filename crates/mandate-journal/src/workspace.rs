//! The records journal spec v0.23 closes in §9.9 to §9.11 (DEC-670 to DEC-672): the workspace
//! API's drafts, the compiler's model call, confirmations that name their agent, owner requests,
//! `ConnectionRevoked`'s reason, the client records, and the hold on new openings, with rules 69
//! to 80 and 84 to 92. §3's `client` actor (rules 81 to 83) is read here and called from
//! [`crate::draft`]; the agent stream's `AgentModeChanged` version 2 is [`crate::agent`]'s (rules
//! 93 to 95). Every rule only refuses a draft.
//!
//! `OwnerCommandIssued` is closed for `hold_openings` and `lift_hold` only. Every other command
//! stays open: its `command` is read, a client's is refused (rule 90), and nothing else about it is
//! judged, so a kill switch from any other principal is always recorded (`AGENTS.md` rule 13).

use mandate_canon::{Key, Object, Value};
use mandate_time::UtcNanos;

use crate::schema::{Ty, normalize};
use crate::{Draft, Invalid, InvalidReason, StreamType};

/// The control-stream event types §9.9 to §9.11 add to [`crate::control::governs`].
pub(crate) const CONTROL: [&str; 6] = [
    "MandateDraftSaved",
    "ModelInvocationRecorded",
    "OwnerRequestSubmitted",
    "ClientConnected",
    "ClientRevoked",
    COMMAND,
];

const COMMAND: &str = "OwnerCommandIssued";
const HOLD: &str = "hold_openings";
const LIFT: &str = "lift_hold";

/// What a `client` actor may write (rule 83): its `propose`, `request`, `read` and `dry_run`, and
/// `hold` scopes, all on the control stream.
const CLIENT_EVENTS: [&str; 4] = [
    "MandateDraftSaved",
    "OwnerRequestSubmitted",
    "RecordsAccessed",
    COMMAND,
];

/// §3's client actor, rules 81 and 82, on an envelope whose types passed: `on_behalf_of` is
/// present exactly on a `client` actor, and is then an `id`, which is put back into the actor; a
/// client's `build` is `null`.
pub(crate) fn client_actor(
    fields: &mut Object,
    on_behalf_of: Option<Value>,
) -> Result<(), Invalid> {
    let actor = fields.get_mut("actor").and_then(|a| match a {
        Value::Object(actor) => Some(actor),
        _ => None,
    });
    let Some(actor) = actor else {
        return Ok(());
    };
    let client = actor.get("kind").and_then(Value::as_str) == Some("client");
    let named = match (client, on_behalf_of) {
        (false, None) => return Ok(()),
        (false, Some(_)) | (true, None) => {
            return Err(Invalid::new(InvalidReason::Schema, "actor.on_behalf_of"));
        }
        (true, Some(named)) => normalize(&Ty::Ident, &named, "actor.on_behalf_of")?,
    };
    if actor.get("build").is_some_and(|b| *b != Value::Null) {
        return Err(Invalid::new(InvalidReason::Schema, "actor.build"));
    }
    let key = Key::new("on_behalf_of")
        .map_err(|_| Invalid::new(InvalidReason::Schema, "actor.on_behalf_of"))?;
    actor.insert(key, named);
    Ok(())
}

/// Rule 83: a `client` actor writes only [`CLIENT_EVENTS`], on the control stream (`schema` at
/// `actor.kind`), checked once the event type's streams are known and before its payload is read.
pub(crate) fn client_writes(
    fields: &Object,
    stream: StreamType,
    event_type: &str,
) -> Result<(), Invalid> {
    let client = fields
        .get("actor")
        .and_then(|a| a.get("kind"))
        .and_then(Value::as_str)
        == Some("client");
    ensure(
        !client || (stream == StreamType::Control && CLIENT_EVENTS.contains(&event_type)),
        "actor.kind",
    )
}

/// The draft origins that start a draft rather than replace a save (rule 69).
const NEW_DRAFT: [&str; 4] = ["description", "goal_answers", "template", "version"];

/// Inference spec §3.3's refusals: nothing left the deployment (rule 73).
const REFUSALS: [&str; 6] = [
    "policy_denied",
    "budget_exhausted",
    "rate_limited_local",
    "meter_unavailable",
    "input_rejected",
    "model_withdrawn",
];

/// The §9.9 to §9.11 schema of `event_type` at `schema_version` on `stream`, or `None` when this
/// module registers none there.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    match (event_type, schema_version, stream) {
        ("MandateDraftSaved", 1, StreamType::Control) => Some(&MANDATE_DRAFT_SAVED),
        ("ModelInvocationRecorded", 1, StreamType::Control) => Some(&COMPILER_INVOCATION),
        ("OwnerRequestSubmitted", 1, StreamType::Control) => Some(&OWNER_REQUEST_SUBMITTED),
        ("ClientConnected", 1, StreamType::Control) => Some(&CLIENT_CONNECTED),
        ("ClientRevoked", 1, StreamType::Control) => Some(&CLIENT_REVOKED),
        (COMMAND, 1, StreamType::Control) => Some(&HOLD_COMMAND),
        ("MandateConfirmed", 2, StreamType::Control) => Some(&MANDATE_CONFIRMED_V2),
        ("ConnectionRevoked", 2, StreamType::Control) => Some(&CONNECTION_REVOKED_V2),
        ("OwnerCommandRefused", 2, StreamType::Agent) => Some(&OWNER_COMMAND_REFUSED_V2),
        _ => None,
    }
}

/// An `OwnerCommandIssued` §9.11 leaves open: any command but the two hold commands. Its `command`
/// is text (`schema` at `payload.command` otherwise) and a client never issues it (rule 90).
pub(crate) fn open_command(
    payload: &Value,
    actor: Option<&Value>,
) -> Option<Result<Value, Invalid>> {
    let command = payload.get("command");
    if matches!(command.and_then(Value::as_str), Some(HOLD | LIFT)) {
        return None;
    }
    let checked = if command.and_then(Value::as_str).is_none() {
        Err(Invalid::new(InvalidReason::Schema, "payload.command"))
    } else if kind(actor) == "client" {
        Err(Invalid::new(InvalidReason::Schema, "actor.kind"))
    } else {
        Ok(payload.clone())
    };
    Some(checked)
}

/// Rules 69 to 80, 84 to 89 (rule 84's batch clause is [`check_batch`]'s), and 90 to 92 on a
/// payload its schema has normalized, in rule order: the first that fails is reported. `config_refs`, `actor` and `causation_id` are the envelope's.
pub(crate) fn rules(
    event_type: &str,
    schema_version: u64,
    payload: &Value,
    config_refs: Option<&Value>,
    actor: Option<&Value>,
    causation_id: Option<&Value>,
) -> Result<(), Invalid> {
    let p = View(payload);
    let caused = causation_id.is_some_and(|c| *c != Value::Null);
    let writer = kind(actor);
    let writer_id = actor
        .and_then(|a| a.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    match (event_type, schema_version) {
        ("MandateDraftSaved", 1) => {
            let origin = p.text("origin");
            ensure(
                p.is_null("base_draft") == NEW_DRAFT.contains(&origin),
                "payload.base_draft",
            )?;
            ensure(
                p.is_null("base_version") != (origin == "version"),
                "payload.base_version",
            )?;
            ensure(origin != "compile" || caused, "causation_id")?;
            ensure(
                writer == "user" || (writer == "client" && origin == "version"),
                "actor.kind",
            )
        }
        ("ModelInvocationRecorded", 1) => compiler_rules(p, config_refs, writer),
        ("MandateConfirmed", 2) => {
            ensure(
                p.is_null("base_version") == p.is_null("agent_id"),
                "payload.base_version",
            )?;
            ensure(
                p.text("base_version") != p.text("mandate_version"),
                "payload.base_version",
            )?;
            ensure(writer == "user", "actor.kind")
        }
        ("OwnerRequestSubmitted", 1) => {
            let expected = match writer {
                "user" => "owner",
                "client" => "client",
                _ => return Err(Invalid::new(InvalidReason::Schema, "actor.kind")),
            };
            ensure(p.text("requested_by") == expected, "payload.requested_by")?;
            let client_id = if expected == "client" { writer_id } else { "" };
            ensure(p.text("client_id") == client_id, "payload.client_id")?;
            let quantity = p.text("quantity");
            ensure(
                p.is_null("quantity") || (!quantity.starts_with('-') && quantity != "0"),
                "payload.quantity",
            )
        }
        ("ConnectionRevoked", 2) => {
            ensure(
                caused == (p.text("reason") == "compromised"),
                "causation_id",
            )?;
            ensure(writer == "user", "actor.kind")
        }
        ("ClientConnected", 1) => {
            ensure(!p.list("scopes").is_empty(), "payload.scopes")?;
            ensure(!p.list("agents").is_empty(), "payload.agents")?;
            ascending(&p.texts("scopes"), "payload.scopes")?;
            ascending(&p.texts("agents"), "payload.agents")?;
            ensure(writer == "user", "actor.kind")?;
            ensure(p.text("user") == human(actor), "payload.user")
        }
        ("ClientRevoked", 1) => {
            let reason = p.text("reason");
            let allowed: &[&str] = match reason {
                "owner" | "admin" => &["user"],
                "deprovisioned" => &["system"],
                _ => &["user", "system"],
            };
            ensure(allowed.contains(&writer), "actor.kind")?;
            match reason {
                "owner" => ensure(p.text("user") == writer_id, "payload.user"),
                "admin" => ensure(p.text("user") != writer_id, "payload.user"),
                _ => Ok(()),
            }
        }
        (COMMAND, 1) => {
            let hold = p.text("command") == HOLD;
            let allowed = writer == "user" || (hold && writer == "client");
            ensure(allowed, "actor.kind")?;
            ensure(p.text("user") == human(actor), "payload.user")?;
            ensure(p.text("subject") == p.text("agent"), "payload.subject")?;
            ensure(!hold || p.is_null("step_up"), "payload.step_up")
        }
        _ => Ok(()),
    }
}

/// Rules 71 to 76 on the compiler's control-stream `ModelInvocationRecorded`.
fn compiler_rules(p: View<'_>, config_refs: Option<&Value>, writer: &str) -> Result<(), Invalid> {
    let bound = config_refs
        .and_then(|refs| refs.get("model_version"))
        .and_then(Value::as_str);
    let named =
        p.0.get("model")
            .and_then(|m| m.get("content_hash"))
            .and_then(Value::as_str);
    ensure(
        bound.is_none_or(|bound| Some(bound) == named),
        "payload.model.content_hash",
    )?;
    let outcome = p.text("outcome");
    let ok = outcome == "ok";
    if ok {
        for member in ["response_ref", "reported_identity"] {
            ensure(!p.is_null(member), &format!("payload.{member}"))?;
        }
    }
    let attempts =
        p.0.get("attempts")
            .and_then(Value::as_int)
            .unwrap_or_default();
    let cost = p.text("cost_usd");
    if REFUSALS.contains(&outcome) {
        for member in ["response_ref", "reported_identity", "provider_request_id"] {
            ensure(p.is_null(member), &format!("payload.{member}"))?;
        }
        ensure(attempts == 0, "payload.attempts")?;
        let tokens = p.0.get("tokens").and_then(Value::as_object);
        let spent = tokens.is_some_and(|t| t.values().any(|v| v.as_int() != Some(0)));
        ensure(!spent, "payload.tokens")?;
        ensure(cost == "0", "payload.cost_usd")?;
    }
    if p.0.get("cache_hit") == Some(&Value::Bool(true)) {
        ensure(ok, "payload.cache_hit")?;
        ensure(attempts == 0, "payload.attempts")?;
        ensure(
            p.is_null("provider_request_id"),
            "payload.provider_request_id",
        )?;
        ensure(cost == "0", "payload.cost_usd")?;
    } else if !REFUSALS.contains(&outcome) {
        ensure(attempts >= 1, "payload.attempts")?;
    }
    ensure(!cost.starts_with('-'), "payload.cost_usd")?;
    if ok {
        let at = |member| UtcNanos::parse(p.text(member)).ok();
        let in_time =
            matches!((at("completed_at"), at("deadline")), (Some(done), Some(due)) if done <= due);
        ensure(in_time, "payload.completed_at")?;
    }
    ensure(writer == "system", "actor.kind")
}

/// §9.10's batch clause of rule 84, over one `append` batch whose drafts each passed
/// [`Draft::parse`]: a compromised revocation's `causation_id` names an `OwnerCommandIssued`
/// earlier in the batch whose `command` is `kill_switch`, whose `scope` is `connection`, and whose
/// `subject` is the revoked connection (`schema` at `causation_id`).
pub(crate) fn check_batch(drafts: &[Draft]) -> Result<(), (usize, Invalid)> {
    let payload = |draft: &Draft| {
        draft
            .fields()
            .get("payload")
            .cloned()
            .unwrap_or(Value::Null)
    };
    for (i, draft) in drafts.iter().enumerate() {
        let revoked = payload(draft);
        let compromised = draft.event_type() == "ConnectionRevoked"
            && draft.schema_version() == 2
            && View(&revoked).text("reason") == "compromised";
        if !compromised {
            continue;
        }
        let cause = draft
            .fields()
            .get("causation_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let earlier = drafts.get(..i).unwrap_or_default();
        let named = earlier.iter().find(|d| d.event_id() == cause);
        let ok = named.is_some_and(|d| {
            let command = payload(d);
            let c = View(&command);
            d.event_type() == COMMAND
                && c.text("command") == "kill_switch"
                && c.text("scope") == "connection"
                && c.text("subject") == View(&revoked).text("connection_id")
        });
        if !ok {
            return Err((i, Invalid::new(InvalidReason::Schema, "causation_id")));
        }
    }
    Ok(())
}

/// The actor's kind, or empty text.
fn kind(actor: Option<&Value>) -> &str {
    actor
        .and_then(|a| a.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// The person behind an actor (§3): a client's `on_behalf_of`, anyone else's `id`.
fn human(actor: Option<&Value>) -> &str {
    let member = if kind(actor) == "client" {
        "on_behalf_of"
    } else {
        "id"
    };
    actor
        .and_then(|a| a.get(member))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn ensure(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, path))
    }
}

/// Strictly ascending by bytes, so a repeat is refused as well as a swap (`non_canonical`).
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

    fn list(self, member: &str) -> &'a [Value] {
        self.0
            .get(member)
            .and_then(Value::as_array)
            .unwrap_or_default()
    }

    fn texts(self, member: &str) -> Vec<&'a str> {
        self.list(member).iter().filter_map(Value::as_str).collect()
    }
}

/// §9.2's step-up evidence: `authenticated_at` a timestamp.
const STEP_UP: Ty = Ty::Record(&[
    ("assertion_id", Ty::Str),
    ("authenticated_at", Ty::Timestamp),
    ("method", Ty::Str),
]);

static MANDATE_DRAFT_SAVED: Ty = Ty::Record(&[
    ("draft_id", Ty::Ident),
    ("draft", Ty::DigestRef),
    (
        "origin",
        Ty::OneOf(&[
            "description",
            "goal_answers",
            "template",
            "version",
            "edit",
            "compile",
        ]),
    ),
    ("base_draft", Ty::Nullable(&Ty::DigestRef)),
    ("base_version", Ty::Nullable(&Ty::DigestRef)),
]);

static COMPILER_INVOCATION: Ty = Ty::Record(&[
    ("call_id", Ty::Ulid),
    ("purpose", Ty::OneOf(&["compiler"])),
    ("draft_id", Ty::Ident),
    ("draft", Ty::DigestRef),
    (
        "model",
        Ty::Record(&[
            ("model_id", Ty::Str),
            ("model_version", Ty::Str),
            ("content_hash", Ty::DigestRef),
        ]),
    ),
    ("endpoint", Ty::Str),
    ("request_digest", Ty::DigestRef),
    (
        "sampling",
        Ty::Record(&[
            ("temperature", Ty::Decimal),
            ("seed", Ty::Nullable(&Ty::Int)),
        ]),
    ),
    ("prompt_ref", Ty::DigestRef),
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
    (
        "tokens",
        Ty::Record(&[("input", Ty::Int), ("output", Ty::Int), ("cached", Ty::Int)]),
    ),
    ("cost_usd", Ty::Decimal),
    ("price_table_ref", Ty::DigestRef),
    ("cache_hit", Ty::Bool),
    ("deadline", Ty::Timestamp),
    ("completed_at", Ty::Timestamp),
]);

static MANDATE_CONFIRMED_V2: Ty = Ty::Record(&[
    ("mandate_version", Ty::DigestRef),
    ("confirmed_paths", Ty::List(&Ty::Pointer)),
    ("record_ref", Ty::DigestRef),
    ("agent_id", Ty::Nullable(&Ty::Ident)),
    ("base_version", Ty::Nullable(&Ty::DigestRef)),
]);

static OWNER_REQUEST_SUBMITTED: Ty = Ty::Record(&[
    ("agent_id", Ty::Ident),
    ("instrument_id", Ty::AssetId),
    ("side", Ty::OneOf(&["buy", "sell"])),
    ("quantity", Ty::Nullable(&Ty::Decimal)),
    ("requested_by", Ty::OneOf(&["owner", "client"])),
    ("client_id", Ty::Nullable(&Ty::Ident)),
]);

static CONNECTION_REVOKED_V2: Ty = Ty::Record(&[
    ("connection_id", Ty::Ident),
    ("reason", Ty::OneOf(&["owner", "compromised"])),
    ("step_up", STEP_UP),
]);

static CLIENT_CONNECTED: Ty = Ty::Record(&[
    ("client_id", Ty::Ident),
    ("user", Ty::Str),
    (
        "scopes",
        Ty::List(&Ty::OneOf(&[
            "dry_run", "hold", "propose", "read", "request",
        ])),
    ),
    ("agents", Ty::List(&Ty::Ident)),
    ("step_up", STEP_UP),
]);

static CLIENT_REVOKED: Ty = Ty::Record(&[
    ("client_id", Ty::Ident),
    ("user", Ty::Str),
    (
        "reason",
        Ty::OneOf(&[
            "owner",
            "admin",
            "member_deactivated",
            "deprovisioned",
            "compromised",
        ]),
    ),
]);

static HOLD_COMMAND: Ty = Ty::Record(&[
    ("agent", Ty::Ident),
    ("command", Ty::OneOf(&[HOLD, LIFT])),
    ("scope", Ty::OneOf(&["agent"])),
    ("subject", Ty::Ident),
    ("release", Ty::Null),
    ("warning_shown", Ty::Null),
    ("bid", Ty::Null),
    ("bid_size", Ty::Null),
    ("floor", Ty::Null),
    ("user", Ty::Str),
    ("submitted_at", Ty::Int),
    ("step_up", Ty::Nullable(&crate::control::ANSWER_STEP_UP)),
]);

static OWNER_COMMAND_REFUSED_V2: Ty = Ty::Record(&[
    (
        "command",
        Ty::OneOf(&["resume", "stop", "acknowledge", LIFT]),
    ),
    (
        "reason",
        Ty::OneOf(&[
            "step_up_missing",
            "step_up_stale",
            "step_up_reused",
            "step_up_method",
            "not_independent",
        ]),
    ),
    ("effective_at", Ty::Timestamp),
]);
