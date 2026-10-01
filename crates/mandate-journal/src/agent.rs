//! The agent stream's closed payload schemas (journal spec §9.1, DEC-177, DEC-252): the eight
//! schemas, consistency rules 1 to 13, subject rules 14 and 15, copy rule 16, rule 10's batch
//! clause, and §11's two agent-stream per-range checks. Every rule only refuses a draft or fails a
//! range; none changes what a writer may do.

use std::collections::BTreeMap;

use mandate_canon::{Value, parse};

use crate::schema::{Ty, is_ident};
use crate::{Draft, Invalid, InvalidReason, StoredEvent, StreamId, StreamType, TrustedStart};

/// The event types §9.1 closes on the agent stream.
const CLOSED: [&str; 8] = [
    "StreamOpened",
    "ObservationRecorded",
    "ModelOutputRecorded",
    "DecisionMade",
    "IntentProposed",
    "AgentModeChanged",
    "KillSwitchActivated",
    "OwnerExitRequested",
];

/// Whether §9.1 governs `event_type` on `stream`; every other agent-stream event keeps its own
/// registration, which today is none.
pub(crate) fn governs(stream: &StreamId, event_type: &str) -> bool {
    stream.stream_type() == StreamType::Agent && CLOSED.contains(&event_type)
}

/// The payload normalized against its §9.1 schema, then consistency rules 1 to 13 in number order
/// (reported only on a well-typed payload). `causation_id` is the envelope's, which rule 10 reads.
pub(crate) fn payload(
    event_type: &str,
    schema_version: u64,
    payload: &Value,
    causation_id: Option<&Value>,
) -> Result<Value, Invalid> {
    let schema = schema(event_type, schema_version)
        .ok_or_else(|| Invalid::new(InvalidReason::UnknownSchema, "payload"))?;
    let payload = crate::schema::normalize(schema, payload, "payload")?;
    let caused = causation_id.is_some_and(|c| *c != Value::Null);
    let view = Payload(&payload);
    match event_type {
        "DecisionMade" => {
            action_rules(view)?;
            decision_rules(view)?;
        }
        "IntentProposed" => {
            action_rules(view)?;
            ensure(caused, InvalidReason::Schema, "causation_id")?;
        }
        "AgentModeChanged" => ensure(
            strictness(view.text("to")) >= strictness(view.text("lifecycle")),
            InvalidReason::Schema,
            "payload.to",
        )?,
        "OwnerExitRequested" => owner_exit_rules(view)?,
        "ModelOutputRecorded" => {
            let thesis = view.is_null("thesis_id");
            if thesis != view.is_null("lineage_id") {
                let path = if thesis {
                    "payload.thesis_id"
                } else {
                    "payload.lineage_id"
                };
                return Err(Invalid::new(InvalidReason::Schema, path));
            }
        }
        _ => {}
    }
    Ok(payload)
}

/// Subject rules 14 and 15 (`stream_mismatch`), then copy rule 16, on a payload that passed
/// [`payload`]: reported after `artifact_refs` and `pii_refs` (§9.1's order).
pub(crate) fn subject_and_copy(
    event_type: &str,
    stream: &StreamId,
    payload: &Value,
    causation_id: Option<&Value>,
) -> Result<(), Invalid> {
    let view = Payload(payload);
    let mut segments = stream.as_str().split(':').skip(1);
    let (workspace, agent) = (segments.next(), segments.next());
    match event_type {
        "StreamOpened" => {
            let opened = format!(
                "agent:{}:{}",
                view.text("workspace_id"),
                view.text("agent_id")
            );
            ensure(
                opened == stream.as_str(),
                InvalidReason::StreamMismatch,
                "stream_id",
            )?;
        }
        "KillSwitchActivated" | "OwnerExitRequested" => {
            let named = match view.text("scope") {
                "agent" => agent,
                "workspace" => workspace,
                _ => None,
            };
            ensure(
                named.is_none_or(|own| own == view.text("subject")),
                InvalidReason::StreamMismatch,
                "payload.subject",
            )?;
        }
        _ => {}
    }
    let copy = match event_type {
        "OwnerExitRequested" => true,
        "KillSwitchActivated" => view.text("initiator") == "owner",
        "AgentModeChanged" => OWNER_MODE_REASONS.contains(&view.text("reason")),
        _ => false,
    };
    ensure(
        !copy || causation_id.is_some_and(|c| *c != Value::Null),
        InvalidReason::Schema,
        "causation_id",
    )
}

/// Rule 10's second clause over one `append` batch whose drafts each passed [`Draft::parse`]: an
/// `IntentProposed` whose `DecisionMade` is in the batch repeats its action members, decimals
/// compared by value. Reported at the first such intent, at its first differing member.
pub fn check_batch(drafts: &[Draft]) -> Result<(), (usize, Invalid)> {
    let decisions: BTreeMap<&str, &Draft> = drafts
        .iter()
        .filter(|d| governs(d.stream_id(), d.event_type()) && d.event_type() == "DecisionMade")
        .map(|d| (d.event_id(), d))
        .collect();
    for (i, draft) in drafts.iter().enumerate() {
        if !governs(draft.stream_id(), draft.event_type()) || draft.event_type() != "IntentProposed"
        {
            continue;
        }
        let fields = draft.fields();
        let cause = fields
            .get("causation_id")
            .and_then(Value::as_str)
            .and_then(|id| decisions.get(id));
        if let (Some(cause), Some(mine)) = (cause, fields.get("payload")) {
            let theirs = cause.fields().get("payload").unwrap_or(&Value::Null);
            if let Some(member) = first_differing_action(mine, theirs) {
                return Err((
                    i,
                    Invalid::new(InvalidReason::Schema, format!("payload.{member}")),
                ));
            }
        }
    }
    Ok(())
}

/// §11's per-range checks that span agent-stream events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStreamCheck {
    /// An `IntentProposed` differs in an action member from the `DecisionMade` it names.
    IntentActionMismatch,
    /// A `KillSwitchActivated`'s `mode_event` names no earlier `AgentModeChanged` with reason
    /// `kill_switch`.
    ModeEventMismatch,
}

impl AgentStreamCheck {
    /// The check's code as the spec writes it.
    pub fn code(self) -> &'static str {
        match self {
            Self::IntentActionMismatch => "intent_action_mismatch",
            Self::ModeEventMismatch => "mode_event_mismatch",
        }
    }
}

/// The first event (by its `seq` column) that fails an agent-stream range check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentStreamFailure {
    pub seq: u64,
    pub check: AgentStreamCheck,
}

/// Runs §11's `intent_action_mismatch` and `mode_event_mismatch` over `rows` of one agent stream,
/// in order, after [`crate::verify_events`] passed them. A reference to an event before the range
/// is not checked, except that on a full chain (`from_seq` 1) a `mode_event` naming no earlier
/// event fails. Rows of any other stream type pass, since neither check applies there.
pub fn verify_agent_stream(
    rows: &[StoredEvent],
    start: TrustedStart,
) -> Result<(), AgentStreamFailure> {
    let mut seen: BTreeMap<String, (String, Value)> = BTreeMap::new();
    for row in rows {
        let agent =
            StreamId::parse(&row.stream_id).is_some_and(|s| s.stream_type() == StreamType::Agent);
        let Some(body) = parse(&row.body).ok().filter(|_| agent) else {
            continue;
        };
        let payload = body.get("payload").cloned().unwrap_or(Value::Null);
        let fail = |check| {
            Err(AgentStreamFailure {
                seq: row.seq,
                check,
            })
        };
        let named = |member: &Value| member.as_str().and_then(|id| seen.get(id));
        match row.event_type.as_str() {
            "IntentProposed" => {
                if let Some((cause_type, cause)) = body.get("causation_id").and_then(named)
                    && cause_type == "DecisionMade"
                    && first_differing_action(&payload, cause).is_some()
                {
                    return fail(AgentStreamCheck::IntentActionMismatch);
                }
            }
            "KillSwitchActivated" => {
                let mode_event = payload.get("mode_event").unwrap_or(&Value::Null);
                if *mode_event != Value::Null {
                    let applied = match named(mode_event) {
                        None => start.from_seq != 1,
                        Some((named_type, named_payload)) => {
                            named_type == "AgentModeChanged"
                                && Payload(named_payload).text("reason") == "kill_switch"
                        }
                    };
                    if !applied {
                        return fail(AgentStreamCheck::ModeEventMismatch);
                    }
                }
            }
            _ => {}
        }
        seen.insert(row.event_id.clone(), (row.event_type.clone(), payload));
    }
    Ok(())
}

/// The members `IntentProposed` repeats from its `DecisionMade`, in rule 10's order.
const ACTION: [&str; 7] = [
    "instrument_id",
    "side",
    "type",
    "tif",
    "qty",
    "limit_price",
    "purpose",
];

/// The first action member in which two payloads differ. Both were normalized on the way in
/// (§4.6), so equal decimal values have equal text.
fn first_differing_action(mine: &Value, theirs: &Value) -> Option<&'static str> {
    ACTION
        .into_iter()
        .find(|member| mine.get(member) != theirs.get(member))
}

const OWNER_MODE_REASONS: [&str; 3] = ["owner_pause", "owner_resume", "owner_stop"];
const RISK_ADDING: [&str; 2] = ["open", "increase"];
const MODES: [&str; 4] = ["normal", "exits_only", "paused", "stopped"];
const CLIPS: [&str; 6] = [
    "max_order_usd",
    "position_cap",
    "gross_exposure_cap",
    "target_qty",
    "max_spend_usd",
    "max_avg_price",
];
const BUILTIN: &str = "builtin_risk_reducing";
const DELEGATION: &str = "delegation:";

/// A mode's place in mandate spec §5.9's order (`normal` < `exits_only` < `paused` < `stopped`).
fn strictness(mode: &str) -> Option<usize> {
    MODES.iter().position(|m| *m == mode)
}

/// A normalized payload, read member by member; a `null` or absent member reads as empty text.
#[derive(Clone, Copy)]
struct Payload<'a>(&'a Value);

impl<'a> Payload<'a> {
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

    fn adds_risk(self) -> bool {
        RISK_ADDING.contains(&self.text("purpose"))
    }
}

fn ensure(holds: bool, reason: InvalidReason, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(reason, path))
    }
}

/// Rules 1 to 3, which `DecisionMade` and `IntentProposed` share.
fn action_rules(p: Payload<'_>) -> Result<(), Invalid> {
    let schema = InvalidReason::Schema;
    ensure(
        (p.text("side") == "buy") == p.adds_risk(),
        schema,
        "payload.side",
    )?;
    let limit = p.text("type") == "limit";
    ensure(
        p.is_null("limit_price") != limit,
        schema,
        "payload.limit_price",
    )?;
    ensure(
        p.text("type") != "market" || !p.adds_risk(),
        schema,
        "payload.type",
    )
}

/// Rules 4 to 9 on `DecisionMade`, each clause in the order §9.1 gives it.
fn decision_rules(p: Payload<'_>) -> Result<(), Invalid> {
    let schema = InvalidReason::Schema;
    let allow = p.text("dry_run") == "allow";
    ensure(
        p.is_null("reason_code") == allow,
        schema,
        "payload.reason_code",
    )?;
    let classified = !p.is_null("autonomy");
    ensure(classified == allow, schema, "payload.autonomy")?;
    let decided_by = p.text("decided_by");
    let labelled = !p.is_null("decided_by") == classified
        && (!classified || (decided_by == BUILTIN) != p.adds_risk());
    ensure(labelled, schema, "payload.decided_by")?;
    ensure(
        p.is_null("decided_by") || is_label(decided_by),
        InvalidReason::NonCanonical,
        "payload.decided_by",
    )?;
    ensure(
        p.text("dry_run") != "defer" || p.text("purpose") == "discretionary_exit",
        schema,
        "payload.dry_run",
    )?;

    let autonomy = p.text("autonomy");
    ensure(
        !classified || autonomy == "auto" || p.adds_risk(),
        schema,
        "payload.autonomy",
    )?;
    let asked = autonomy == "ask";
    ensure(
        p.is_null("ask_suppressed") || asked,
        schema,
        "payload.ask_suppressed",
    )?;
    let lifted = decided_by.strip_prefix(DELEGATION);
    let delegated = match lifted {
        Some(id) => p.text("delegation_id") == id && autonomy == "auto",
        None => p.is_null("delegation_id"),
    };
    ensure(delegated, schema, "payload.delegation_id")?;
    let client = p.text("requested_by") == "client";
    ensure(
        p.is_null("client_id") != client,
        schema,
        "payload.client_id",
    )?;
    ensure(
        !(client && p.adds_risk() && autonomy == "auto"),
        schema,
        "payload.autonomy",
    )?;
    ensure(
        decided_by != "client_ceiling" || (client && asked),
        schema,
        "payload.decided_by",
    )?;

    let discretionary = p.text("purpose") == "discretionary_exit";
    ensure(
        p.is_null("exit_origin") != discretionary,
        schema,
        "payload.exit_origin",
    )?;
    let evaluated = p.adds_risk() || p.text("exit_origin") == "signal";
    let numbers = ["exit_conviction", "buy_conviction", "combined_score"];
    if let Some(member) = numbers.iter().find(|m| p.is_null(m) == evaluated) {
        return Err(Invalid::new(schema, format!("payload.{member}")));
    }
    let lists = ["outputs_used", "model_weights", "clips_applied"];
    if let Some(member) = lists.iter().find(|m| !evaluated && !p.list(m).is_empty()) {
        return Err(Invalid::new(schema, format!("payload.{member}")));
    }

    let non_canonical = InvalidReason::NonCanonical;
    let outputs: Vec<&str> = p
        .list("outputs_used")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    ensure(ascending(&outputs), non_canonical, "payload.outputs_used")?;
    let keys: Vec<&[u8]> = p
        .list("model_weights")
        .iter()
        .filter_map(|w| w.get("key").and_then(Value::as_str))
        .map(str::as_bytes)
        .collect();
    ensure(ascending(&keys), non_canonical, "payload.model_weights")?;
    let clips: Vec<Option<usize>> = p
        .list("clips_applied")
        .iter()
        .map(|c| CLIPS.iter().position(|k| Some(*k) == c.as_str()))
        .collect();
    ensure(ascending(&clips), non_canonical, "payload.clips_applied")
}

/// Rule 12: `confirmed` exactly when the bid members are all present, then the step-up evidence
/// exactly when `step_up_status` is `valid`.
fn owner_exit_rules(p: Payload<'_>) -> Result<(), Invalid> {
    let confirmed = p.0.get("confirmed") == Some(&Value::Bool(true));
    if let Some(member) = ["bid", "bid_size", "floor"]
        .iter()
        .find(|m| p.is_null(m) == confirmed)
    {
        return Err(Invalid::new(
            InvalidReason::Schema,
            format!("payload.{member}"),
        ));
    }
    ensure(
        p.is_null("step_up") != (p.text("step_up_status") == "valid"),
        InvalidReason::Schema,
        "payload.step_up",
    )
}

/// One of `decided_by`'s labels (mandate spec §6.2): a fixed label, or `rule:` or `delegation:`
/// followed by an `id`.
fn is_label(label: &str) -> bool {
    [
        "builtin_risk_reducing",
        "default",
        "admission_ceiling",
        "client_ceiling",
    ]
    .contains(&label)
        || ["rule:", DELEGATION]
            .iter()
            .any(|prefix| label.strip_prefix(prefix).is_some_and(is_ident))
}

/// Strictly ascending, so a repeat is refused as well as a swap.
fn ascending<T: PartialOrd>(items: &[T]) -> bool {
    items.windows(2).all(|w| matches!(w, [a, b] if a < b))
}

fn schema(event_type: &str, schema_version: u64) -> Option<&'static Ty> {
    if schema_version != 1 {
        return None;
    }
    match event_type {
        "StreamOpened" => Some(&STREAM_OPENED),
        "ObservationRecorded" => Some(&OBSERVATION_RECORDED),
        "ModelOutputRecorded" => Some(&MODEL_OUTPUT_RECORDED),
        "DecisionMade" => Some(&DECISION_MADE),
        "IntentProposed" => Some(&INTENT_PROPOSED),
        "AgentModeChanged" => Some(&AGENT_MODE_CHANGED),
        "KillSwitchActivated" => Some(&KILL_SWITCH_ACTIVATED),
        "OwnerExitRequested" => Some(&OWNER_EXIT_REQUESTED),
        _ => None,
    }
}

const SIDE: Ty = Ty::OneOf(&["buy", "sell"]);
const ORDER_TYPE: Ty = Ty::OneOf(&["limit", "market"]);
const TIF: Ty = Ty::OneOf(&["day", "gtc", "ioc"]);
const MODE: Ty = Ty::OneOf(&MODES);

static STREAM_OPENED: Ty = Ty::Record(&[
    ("stream_type", Ty::OneOf(&["agent"])),
    ("workspace_id", Ty::Ident),
    ("agent_id", Ty::Ident),
]);

static OBSERVATION_RECORDED: Ty = Ty::Record(&[
    ("source", Ty::Str),
    ("instrument_id", Ty::Nullable(&Ty::Str)),
    ("as_of", Ty::Timestamp),
    ("data_ref", Ty::DigestRef),
]);

static MODEL_OUTPUT_RECORDED: Ty = Ty::Record(&[
    ("model_id", Ty::Str),
    ("model_version", Ty::Str),
    ("content_hash", Ty::DigestRef),
    ("instrument_id", Ty::Str),
    ("as_of", Ty::Timestamp),
    ("expires_at", Ty::Timestamp),
    ("direction", Ty::Str),
    ("conviction", Ty::Decimal),
    ("confidence", Ty::Decimal),
    ("horizon_s", Ty::Int),
    ("thesis_ref", Ty::Nullable(&Ty::DigestRef)),
    ("evidence", Ty::List(&Ty::Ulid)),
    ("invalidation", Ty::Nullable(&Ty::Str)),
    ("thesis_id", Ty::Nullable(&Ty::Ident)),
    ("lineage_id", Ty::Nullable(&Ty::Ident)),
    (
        "ignored",
        Ty::Nullable(&Ty::OneOf(&[
            "not_pinned",
            "model_withdrawn",
            "output_limits",
            "not_in_universe",
            "direction_not_allowed",
            "horizon_mismatch",
            "revision_without_predecessor",
        ])),
    ),
]);

static DECISION_MADE: Ty = Ty::Record(&[
    ("instrument_id", Ty::Str),
    ("side", SIDE),
    ("type", ORDER_TYPE),
    ("tif", TIF),
    ("qty", Ty::Decimal),
    ("limit_price", Ty::Nullable(&Ty::Decimal)),
    (
        "purpose",
        Ty::OneOf(&["open", "increase", "discretionary_exit", "risk_exit"]),
    ),
    (
        "exit_origin",
        Ty::Nullable(&Ty::OneOf(&[
            "signal",
            "goal_completion",
            "removed_instrument",
        ])),
    ),
    ("exit_conviction", Ty::Nullable(&Ty::Decimal)),
    ("buy_conviction", Ty::Nullable(&Ty::Decimal)),
    ("combined_score", Ty::Nullable(&Ty::Decimal)),
    ("outputs_used", Ty::List(&Ty::Ulid)),
    (
        "model_weights",
        Ty::List(&Ty::Record(&[("key", Ty::Str), ("value", Ty::Decimal)])),
    ),
    ("clips_applied", Ty::List(&Ty::OneOf(&CLIPS))),
    ("dry_run", Ty::OneOf(&["allow", "deny", "defer"])),
    ("reason_code", Ty::Nullable(&Ty::Ident)),
    (
        "autonomy",
        Ty::Nullable(&Ty::OneOf(&["auto", "ask", "deny"])),
    ),
    (
        "ask_suppressed",
        Ty::Nullable(&Ty::OneOf(&["budget", "skipped_today", "recent_timeout"])),
    ),
    ("decided_by", Ty::Nullable(&Ty::Str)),
    ("delegation_id", Ty::Nullable(&Ty::Ident)),
    ("requested_by", Ty::OneOf(&["agent", "owner", "client"])),
    ("client_id", Ty::Nullable(&Ty::Ident)),
]);

static INTENT_PROPOSED: Ty = Ty::Record(&[
    ("instrument_id", Ty::Str),
    ("side", SIDE),
    ("type", ORDER_TYPE),
    ("tif", TIF),
    ("qty", Ty::Decimal),
    ("limit_price", Ty::Nullable(&Ty::Decimal)),
    (
        "purpose",
        Ty::OneOf(&[
            "open",
            "increase",
            "discretionary_exit",
            "risk_exit",
            "owner_exit",
        ]),
    ),
]);

static AGENT_MODE_CHANGED: Ty = Ty::Record(&[
    ("from", MODE),
    ("to", MODE),
    (
        "reason",
        Ty::OneOf(&[
            "restriction_changed",
            "awaiting_reconciliation",
            "owner_pause",
            "owner_resume",
            "owner_stop",
            "kill_switch",
        ]),
    ),
    ("lifecycle", Ty::OneOf(&["normal", "paused", "stopped"])),
]);

static KILL_SWITCH_ACTIVATED: Ty = Ty::Record(&[
    ("scope", Ty::OneOf(&["agent", "connection", "workspace"])),
    ("subject", Ty::Ident),
    (
        "initiator",
        Ty::OneOf(&["owner", "risk_limit", "platform_operator"]),
    ),
    ("mode_event", Ty::Nullable(&Ty::Ulid)),
]);

static OWNER_EXIT_REQUESTED: Ty = Ty::Record(&[
    (
        "scope",
        Ty::OneOf(&["instrument", "agent", "connection", "workspace"]),
    ),
    ("subject", Ty::Ident),
    ("confirmed", Ty::Bool),
    ("bid", Ty::Nullable(&Ty::Decimal)),
    ("bid_size", Ty::Nullable(&Ty::Decimal)),
    ("floor", Ty::Nullable(&Ty::Decimal)),
    ("user", Ty::Str),
    ("step_up_status", Ty::OneOf(&["valid", "absent", "stale"])),
    (
        "step_up",
        Ty::Nullable(&Ty::Record(&[
            ("assertion_id", Ty::Str),
            ("authenticated_at", Ty::Timestamp),
            ("method", Ty::Str),
        ])),
    ),
]);

/// The journal vectors' `agent_stream` section, read here as well as by `mandate-refcases`, so
/// every §9.1 rule is judged by this crate's own tests: each chain event and valid draft parses,
/// and each invalid draft is refused with its reason at its path.
#[cfg(test)]
mod tests {
    use std::path::Path;

    use mandate_canon::{Digest, Key, Object, Value, parse, to_canonical};

    use super::{AgentStreamCheck, AgentStreamFailure, check_batch, verify_agent_stream};
    use crate::{Draft, StoredEvent, TrustedStart};

    fn section() -> Result<Value, String> {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let fixture = parse(&bytes).map_err(|e| format!("{e:?}"))?;
        fixture
            .get("agent_stream")
            .cloned()
            .ok_or_else(|| "no agent_stream".to_owned())
    }

    fn list<'a>(value: &'a Value, name: &str) -> &'a [Value] {
        value
            .get(name)
            .and_then(Value::as_array)
            .unwrap_or_default()
    }

    fn text<'a>(value: &'a Value, name: &str) -> &'a str {
        value.get(name).and_then(Value::as_str).unwrap_or_default()
    }

    /// The chain event at `seq` as a writer's draft: the body without the journal's fields.
    fn base(section: &Value, seq: u64) -> Result<Object, String> {
        let entry = list(section, "chain")
            .iter()
            .find(|e| e.get("seq").and_then(Value::as_int) == Some(seq))
            .ok_or_else(|| format!("no chain seq {seq}"))?;
        let mut body = entry
            .get("body")
            .and_then(Value::as_object)
            .cloned()
            .ok_or("no body")?;
        for field in ["seq", "prev_hash", "recorded_at"] {
            body.remove(field);
        }
        Ok(body)
    }

    /// Applies a `{path, value}` or `{path, delete: true}` change, dotted from the envelope.
    fn apply(draft: &mut Object, change: &Value) -> Result<(), String> {
        let path = text(change, "path");
        let mut names: Vec<&str> = path.split('.').collect();
        let last = names.pop().ok_or("empty path")?;
        let mut node = draft;
        for name in names {
            node = match node.get_mut(name) {
                Some(Value::Object(inner)) => inner,
                _ => return Err(format!("`{path}` has no object `{name}`")),
            };
        }
        if change.get("delete") == Some(&Value::Bool(true)) {
            node.remove(last)
                .map(|_| ())
                .ok_or(format!("`{path}` is absent"))
        } else {
            let key = Key::new(last).map_err(|_| format!("bad key `{last}`"))?;
            let value = change.get("value").cloned().ok_or("no value")?;
            node.insert(key, value);
            Ok(())
        }
    }

    fn draft(section: &Value, case: &Value) -> Result<Vec<u8>, String> {
        let seq = case
            .get("base_seq")
            .and_then(Value::as_int)
            .unwrap_or_default();
        let mut body = base(section, seq)?;
        for change in list(case, "changes") {
            apply(&mut body, change)?;
        }
        Ok(to_canonical(&Value::Object(body)))
    }

    #[test]
    fn every_chain_event_and_valid_draft_parses() -> Result<(), String> {
        let section = section()?;
        let chain = list(&section, "chain");
        let valid = list(&section, "valid_drafts");
        assert_eq!((chain.len(), valid.len()), (13, 16));
        for entry in chain {
            let seq = entry.get("seq").and_then(Value::as_int).unwrap_or_default();
            let bytes = to_canonical(&Value::Object(base(&section, seq)?));
            let parsed = Draft::parse(&bytes).map(|d| d.event_type().to_owned());
            assert_eq!(
                parsed,
                Ok(text(entry, "event_type").to_owned()),
                "seq {seq}"
            );
        }
        for case in valid {
            let parsed = Draft::parse(&draft(&section, case)?).map(|_| ());
            assert_eq!(parsed, Ok(()), "{}", text(case, "name"));
        }
        Ok(())
    }

    #[test]
    fn every_invalid_draft_is_refused_with_its_reason_at_its_path() -> Result<(), String> {
        let section = section()?;
        let invalid = list(&section, "invalid_drafts");
        assert_eq!(invalid.len(), 67);
        for case in invalid {
            let expect = case.get("expect").ok_or("no expect")?;
            let refused = Draft::parse(&draft(&section, case)?)
                .err()
                .map(|e| (e.reason.code().to_owned(), e.path));
            let wanted = (
                text(expect, "reason").to_owned(),
                text(expect, "path").to_owned(),
            );
            assert_eq!(refused, Some(wanted), "{}", text(case, "name"));
        }
        Ok(())
    }

    fn number(value: &Value, name: &str) -> u64 {
        value.get(name).and_then(Value::as_int).unwrap_or_default()
    }

    #[test]
    fn each_batch_commits_or_is_refused_at_its_draft_and_member() -> Result<(), String> {
        let section = section()?;
        let valid = list(&section, "valid_batches");
        let invalid = list(&section, "invalid_batches");
        assert_eq!((valid.len(), invalid.len()), (1, 3));
        for case in valid.iter().chain(invalid) {
            let drafts = list(case, "drafts")
                .iter()
                .map(|member| Draft::parse(&draft(&section, member)?).map_err(|e| format!("{e}")))
                .collect::<Result<Vec<_>, String>>()?;
            let expect = case.get("expect").ok_or("no expect")?;
            let got = check_batch(&drafts)
                .err()
                .map(|(i, e)| (u64::try_from(i).ok(), e.reason.code().to_owned(), e.path));
            let wanted = (text(expect, "outcome") == "Invalid").then(|| {
                (
                    Some(number(expect, "draft_index")),
                    text(expect, "reason").to_owned(),
                    text(expect, "path").to_owned(),
                )
            });
            assert_eq!(got, wanted, "{}", text(case, "name"));
        }
        Ok(())
    }

    /// The chain's bodies with `changes` applied, as stored rows; the range checks read only the
    /// columns and the body, so the hashes are not recomputed here.
    fn rows(section: &Value, changes: &[Value]) -> Result<Vec<StoredEvent>, String> {
        let mut out = Vec::new();
        for entry in list(section, "chain") {
            let seq = number(entry, "seq");
            let mut body = entry
                .get("body")
                .and_then(Value::as_object)
                .cloned()
                .ok_or("no body")?;
            for change in changes.iter().filter(|c| number(c, "seq") == seq) {
                apply(&mut body, change)?;
            }
            let body = Value::Object(body);
            out.push(StoredEvent {
                stream_id: text(&body, "stream_id").to_owned(),
                seq,
                event_id: text(&body, "event_id").to_owned(),
                event_type: text(&body, "event_type").to_owned(),
                schema_version: 1,
                environment: text(&body, "environment").to_owned(),
                recorded_at: text(&body, "recorded_at").to_owned(),
                prev_hash: Digest::ZERO,
                hash: Digest::ZERO,
                body: to_canonical(&body),
            });
        }
        Ok(out)
    }

    #[test]
    fn each_tampered_range_fails_its_check_at_its_seq_and_the_chain_verifies() -> Result<(), String>
    {
        let section = section()?;
        let chain = rows(&section, &[])?;
        assert_eq!(chain.len(), 13);
        assert_eq!(verify_agent_stream(&chain, TrustedStart::GENESIS), Ok(()));
        let cases = list(&section, "range_verification");
        assert_eq!(cases.len(), 4);
        for case in cases {
            let tampered = rows(&section, list(case, "changes"))?;
            let expect = case.get("expect").ok_or("no expect")?;
            let check = match text(expect, "code") {
                "intent_action_mismatch" => AgentStreamCheck::IntentActionMismatch,
                _ => AgentStreamCheck::ModeEventMismatch,
            };
            assert_eq!(check.code(), text(expect, "code"));
            let start = TrustedStart {
                from_seq: number(case, "from_seq"),
                prev_hash: Digest::ZERO,
            };
            let wanted = AgentStreamFailure {
                seq: number(expect, "seq"),
                check,
            };
            assert_eq!(
                verify_agent_stream(&tampered, start),
                Err(wanted),
                "{}",
                text(case, "name")
            );
            let from_the_switch: Vec<StoredEvent> =
                tampered.iter().filter(|r| r.seq >= 12).cloned().collect();
            let partial = TrustedStart {
                from_seq: 12,
                prev_hash: Digest::ZERO,
            };
            let unresolved = text(case, "name") == "kill_switch_names_no_event_on_the_full_chain";
            if unresolved {
                assert_eq!(verify_agent_stream(&from_the_switch, partial), Ok(()));
            }
        }
        let other_stream: Vec<StoredEvent> = chain
            .iter()
            .cloned()
            .map(|mut r| {
                r.stream_id = "acct:ws_1:ACCT1".to_owned();
                r
            })
            .collect();
        let tampered = rows(&section, list(cases.first().ok_or("no case")?, "changes"))?;
        let moved: Vec<StoredEvent> = tampered
            .into_iter()
            .zip(other_stream)
            .map(|(mut r, o)| {
                r.stream_id = o.stream_id;
                r
            })
            .collect();
        assert_eq!(verify_agent_stream(&moved, TrustedStart::GENESIS), Ok(()));
        Ok(())
    }
}
