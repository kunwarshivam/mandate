//! The record journal spec v0.79 §9.34 closes for a held retirement (DEC-2352, E8-17): the agent
//! stream's `RetirementHeld`, with rule 265 on the draft, and stream rule 266 ([`retirement_failure`])
//! over the agent's records in commit order, which a verifier reads with the control and account
//! streams' records about the agent. The record names no order, position, instrument, or quantity
//! (`AGENTS.md` rule 6). Every rule only refuses. [`retirement_held_silent`] is the verifier's
//! finding of an open sequence silent for more than 48 hours (DEC-2835).

use mandate_canon::Value;
use mandate_num::Ratio;
use mandate_time::UtcNanos;

use crate::schema::Ty;
use crate::{Invalid, InvalidReason, StreamType};

pub(crate) const HELD: &str = "RetirementHeld";
const ALERT: &str = "OwnerAlertSent";
const ALERT_KIND: &str = "retirement_held";

/// A day of risk clock: the least wait from `held_since` to the first period (rule 265) and between
/// periods (rule 266).
const DAY_S: i64 = 86_400;

/// Whether §9.34 governs `event_type`: the catalogue admits `RetirementHeld` on the agent stream
/// alone, so the stream is already the agent's here.
pub(crate) fn governs(event_type: &str) -> bool {
    event_type == HELD
}

/// The §9.34 schema of `event_type` at `schema_version` on `stream`, or `None`.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    match (event_type, schema_version, stream) {
        (HELD, 1, StreamType::Agent) => Some(&RETIREMENT_HELD),
        _ => None,
    }
}

/// Rule 265 on a payload its schema has normalized, in the rule's order: the first clause that
/// fails is reported. `actor` and `causation_id` are the envelope's.
pub(crate) fn rules(
    event_type: &str,
    payload: &Value,
    actor: Option<&Value>,
    causation_id: Option<&Value>,
) -> Result<(), Invalid> {
    if event_type != HELD {
        return Ok(());
    }
    let runtime = actor.and_then(|a| a.get("kind")).and_then(Value::as_str) == Some("agent");
    ensure(runtime, "actor.kind")?;
    ensure(
        causation_id.is_some_and(|cause| *cause != Value::Null),
        "causation_id",
    )?;
    let period = payload.get("period").and_then(Value::as_int).unwrap_or(0);
    ensure(period >= 1, "payload.period")?;
    let due = clock(payload, "held_since").and_then(|since| since.checked_add(DAY_S));
    let written = clock(payload, "risk_clock");
    ensure(
        due.zip(written).is_some_and(|(due, at)| at >= due),
        "payload.risk_clock",
    )?;
    let waiting: Vec<&str> = payload
        .get("waiting_on")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    ensure(
        !waiting.is_empty()
            && waiting
                .iter()
                .zip(waiting.iter().skip(1))
                .all(|(before, after)| before < after),
        "payload.waiting_on",
    )
}

/// `member`'s risk clock in seconds, or `None` when it does not read as a timestamp.
fn clock(payload: &Value, member: &str) -> Option<i64> {
    payload
        .get(member)
        .and_then(Value::as_str)
        .and_then(|text| UtcNanos::parse(text).ok())
        .map(UtcNanos::secs)
}

fn ensure(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, path))
    }
}

static RETIREMENT_HELD: Ty = Ty::Record(&[
    ("held_since", Ty::RiskClock),
    ("period", Ty::Int),
    (
        "waiting_on",
        Ty::List(&Ty::OneOf(&["position", "working_order"])),
    ),
    ("risk_clock", Ty::RiskClock),
]);

fn text<'a>(value: &'a Value, member: &str) -> &'a str {
    value
        .get(member)
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn payload(record: &Value) -> &Value {
    record.get("payload").unwrap_or(&Value::Null)
}

/// The open sequence: its cause, its last record's period, its `held_since`, the version its last
/// record named, a version applied to the agent since that record, and that record's risk clock.
struct Open<'a> {
    cause: &'a str,
    period: u64,
    since: &'a str,
    version: &'a str,
    moved_to: Option<&'a str>,
    at: i64,
}

/// Stream rule 266 over `records`, the agent `agent`'s records in commit order: its agent stream
/// with the control stream's `AgentStopped` and `OwnerCommandIssued` and the account stream's
/// `MandateVersionApplied` about it. Each record is an object with its `event_type`, `event_id`,
/// `causation_id`, `config_refs`, and `payload`.
///
/// One sequence is open at a time. Its records run 1, 2, ... under one cause and one `held_since`,
/// each at least a day of risk clock after the one before, under the version in effect: the
/// sequence's, or the `new_version` of a version applied to the agent since its last record. This
/// agent's `AgentStopped`, or an owner's `resume` or `stop` scoped to it, ends the sequence, and no
/// record of an ended sequence's cause follows. A record of another cause is refused while one is
/// open, unless a version was applied since its last record: then the open one has ended and the new
/// one begins from 1. Each record is followed by its `OwnerAlertSent` (`retirement_held`, naming
/// it), and each such alert follows the record it names; an alert of another kind answers no record
/// here, which reads §9.34's "its `OwnerAlertSent` (`retirement_held`, `subject` it)" as written
/// (DEC-2831). Another agent's records, another command, and a rejected version end and move
/// nothing.
///
/// Returns the index of the first record that breaks the rule, or `None`; later records are not read.
#[must_use]
pub fn retirement_failure(agent: &str, records: &[Value]) -> Option<usize> {
    let mut open: Option<Open<'_>> = None;
    let mut finished: Vec<&str> = Vec::new();
    for (i, record) in records.iter().enumerate() {
        let p = payload(record);
        match text(record, "event_type") {
            "AgentStopped" | "OwnerCommandIssued" => {
                if ends(agent, record)
                    && let Some(ended) = open.take()
                {
                    finished.push(ended.cause);
                }
            }
            "MandateVersionApplied" => {
                let moves = text(p, "agent_id") == agent && text(p, "result") == "applied";
                if let Some(o) = open.as_mut().filter(|_| moves) {
                    o.moved_to = Some(text(p, "new_version"));
                }
            }
            ALERT if text(p, "kind") == ALERT_KIND => {
                let tied = i
                    .checked_sub(1)
                    .and_then(|b| records.get(b))
                    .is_some_and(|b| {
                        text(b, "event_type") == HELD && text(b, "event_id") == text(p, "subject")
                    });
                if !tied {
                    return Some(i);
                }
            }
            HELD if !held(
                record,
                records.get(i.saturating_add(1)),
                &mut open,
                &mut finished,
            ) =>
            {
                return Some(i);
            }
            _ => {}
        }
    }
    None
}

/// Whether `record` ends `agent`'s open sequence: its `AgentStopped`, or an owner's `resume` or
/// `stop` scoped to it.
fn ends(agent: &str, record: &Value) -> bool {
    let p = payload(record);
    match text(record, "event_type") {
        "AgentStopped" => text(p, "agent_id") == agent,
        "OwnerCommandIssued" => {
            text(p, "scope") == "agent"
                && text(p, "subject") == agent
                && matches!(text(p, "command"), "resume" | "stop")
        }
        _ => false,
    }
}

/// Folds one `RetirementHeld` into the open sequence; whether rule 266 admits it.
fn held<'a>(
    record: &'a Value,
    next: Option<&Value>,
    open: &mut Option<Open<'a>>,
    finished: &mut Vec<&'a str>,
) -> bool {
    let p = payload(record);
    let alerted = next.is_some_and(|n| {
        text(n, "event_type") == ALERT
            && text(payload(n), "kind") == ALERT_KIND
            && text(payload(n), "subject") == text(record, "event_id")
    });
    let (Some(period), Some(at)) = (
        p.get("period").and_then(Value::as_int),
        clock(p, "risk_clock"),
    ) else {
        return false;
    };
    if !alerted {
        return false;
    }
    let cause = text(record, "causation_id");
    let version = record
        .get("config_refs")
        .map_or("", |refs| text(refs, "mandate_version"));
    let since = text(p, "held_since");
    if let Some(o) = open.as_ref().filter(|o| o.cause != cause) {
        if o.moved_to.is_none() {
            return false;
        }
        finished.push(o.cause);
        *open = None;
    }
    let continues = match open.as_ref() {
        None => !finished.contains(&cause) && period == 1,
        Some(o) => {
            Some(period) == o.period.checked_add(1)
                && since == o.since
                && (version == o.version || o.moved_to == Some(version))
                && at >= o.at.saturating_add(DAY_S)
        }
    };
    if continues {
        *open = Some(Open {
            cause,
            period,
            since,
            version,
            moved_to: None,
            at,
        });
    }
    continues
}

/// What a mandate version says of a retirement's cause (§9.34): the `profit_level` a `profit_stop`
/// is read against, the `end_date`, and `on_complete`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetirementTerms {
    pub profit_level: Ratio,
    /// `YYYY-MM-DD`, compared as text: the journal's dates are fixed-width (§4).
    pub end_date: String,
    pub on_complete: String,
}

/// Silence: more than 48 hours of risk clock since the last record.
const SILENT_AFTER_S: i64 = 2 * DAY_S;

/// The verifier's `retirement_held_silent` finding for `agent` at risk clock `now` (journal spec
/// §9.34, DEC-2352 item 4): its sequence's last `RetirementHeld` in `records`, the agent's records in
/// commit order as [`retirement_failure`] reads them, is more than 48 hours old, with no end and no
/// version that cancels it since. `cause` is the record that began the retirement and `terms` reads
/// a mandate version's terms by its hash.
///
/// An end is the agent's own, as rule 266 reads it. A version cancels only once applied to this
/// agent, and only by its cause: a `profit_stop` by a `profit_level` above the return at the trigger,
/// (`agent_equity` − `capital_base`) / `capital_base` from its `RiskLimitTriggered`; a passed
/// `end_date` by a later one; a goal's `release` by a changed `on_complete`, each against the version
/// the sequence's first record names. Any other cause, or terms that cannot be read, cancels nothing,
/// so the finding is reported rather than missed (`AGENTS.md` rule 3, DEC-2835).
#[must_use]
pub fn retirement_held_silent(
    agent: &str,
    cause: &Value,
    records: &[Value],
    terms: impl Fn(&str) -> Option<RetirementTerms>,
    now: i64,
) -> bool {
    let mut last: Option<i64> = None;
    let mut held_under: Option<Option<RetirementTerms>> = None;
    for record in records {
        let p = payload(record);
        match text(record, "event_type") {
            HELD => {
                last = clock(p, "risk_clock");
                if held_under.is_none() {
                    let version = record
                        .get("config_refs")
                        .map_or("", |refs| text(refs, "mandate_version"));
                    held_under = Some(terms(version));
                }
            }
            "AgentStopped" | "OwnerCommandIssued" if ends(agent, record) => last = None,
            "MandateVersionApplied"
                if last.is_some()
                    && text(p, "agent_id") == agent
                    && text(p, "result") == "applied" =>
            {
                let new = terms(text(p, "new_version"));
                let under = held_under.as_ref().and_then(Option::as_ref);
                if new.is_some_and(|new| cancels(cause, under, &new)) {
                    last = None;
                }
            }
            _ => {}
        }
    }
    last.is_some_and(|at| now.saturating_sub(at) > SILENT_AFTER_S)
}

/// Whether `new` cancels the retirement `cause` began, held under `under`.
fn cancels(cause: &Value, under: Option<&RetirementTerms>, new: &RetirementTerms) -> bool {
    let p = payload(cause);
    match text(cause, "event_type") {
        "RiskLimitTriggered" => text(p, "limit") == "profit_stop" && raised(p, new.profit_level),
        "GoalCompleted" if text(p, "reason") == "end_date" => {
            under.is_some_and(|under| new.end_date > under.end_date)
        }
        "GoalCompleted" if text(p, "on_complete") == "release" => {
            under.is_some_and(|under| new.on_complete != under.on_complete)
        }
        _ => false,
    }
}

/// Whether `level` is above the return at a `profit_stop` trigger: `level` × `capital_base` above
/// `agent_equity` − `capital_base`, the same comparison without a division, for a positive base. A
/// figure that does not read, or a base that is not positive, raises nothing.
fn raised(trigger: &Value, level: Ratio) -> bool {
    let figure = |member: &str| Ratio::parse(text(trigger, member)).ok();
    let (Some(equity), Some(base)) = (figure("agent_equity"), figure("capital_base")) else {
        return false;
    };
    if base <= Ratio::ZERO {
        return false;
    }
    match (level.checked_mul(base), equity.checked_sub(base)) {
        (Ok(scaled), Ok(gain)) => scaled > gain,
        _ => false,
    }
}
