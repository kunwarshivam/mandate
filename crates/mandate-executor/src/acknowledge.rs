//! The owner's acknowledgments, copied from the control stream into the account stream and judged
//! (journal spec §9.28, DEC-2828; E7-4 slice 5). Each copy is the original's payload and version
//! unchanged, caused by it, and an acknowledgment copy that fails a check is followed, as the very
//! next record of its batch, by `OwnerCommandRefused` with `command` `acknowledge`. A request copy
//! is never judged by itself.
//!
//! Everything the four checks read is either on the account stream, folded into
//! [`Acknowledgments`], or in the [`AcknowledgmentHandoff`] the shell computed from the control
//! stream, so a restart between two copies changes no judgment, and a replay reads the copies and
//! refusals as journaled and never judges again. What an accepted acknowledgment lifts is not this
//! module's: a limit's lift is the account risk fold's (`RiskLimitLifted`, `owner_acknowledged`).

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Value;

use crate::batch::Batch;
use crate::error::ExecutorError;
use crate::payload::{object, optional_int, optional_text, required_text, risk_clock_stamp, text};
use crate::state::ExecutorState;
use crate::types::{AcknowledgmentHandoff, FoldedEvent, RiskClock};

const ACKNOWLEDGED: &str = "OwnerAcknowledged";
const REQUESTED: &str = "OwnerAcknowledgmentRequested";
const REFUSED: &str = "OwnerCommandRefused";

/// Mandate spec §6.1's freshness window, in seconds, inclusive.
const STEP_UP_WINDOW_S: i64 = 300;

/// The members a request and the acknowledgment answering it must agree on (§9.28 check 4, rule
/// 240 restated on the account stream).
const MATCHED: [&str; 4] = ["agent", "event", "requested_by", "independent_required"];

/// What §9.28's judgment reads from the account stream, folded from it alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Acknowledgments {
    /// Every `RiskLimitTriggered` by its `event_id`.
    triggers: BTreeMap<String, Trigger>,
    /// Every request copy, by the control-stream original its `causation_id` names.
    requests: BTreeMap<String, Value>,
    /// Every request an acknowledgment copy named, refused or not: a request is spent by the first
    /// acknowledgment that names it (rule 241).
    named: BTreeSet<String>,
    /// Every control-stream original copied, so none is copied twice.
    copied: BTreeSet<String>,
    /// The acknowledgment copy folded last, until the next record says whether it was refused.
    pending: Option<Pending>,
}

/// One triggered limit: whose and which, and whether it still stands (§9.28 check 2).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Trigger {
    agent: String,
    limit: String,
    stands: bool,
}

/// An acknowledgment copy read as accepted until the next record: the original it copies and the
/// triggers its acceptance superseded, which its refusal, the very next record, restores.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    original: String,
    superseded: Vec<String>,
}

impl Acknowledgments {
    /// Marks every standing trigger of `agent`'s `limit` superseded but `keep`, answering those it
    /// marked.
    fn supersede(&mut self, agent: &str, limit: &str, keep: Option<&str>) -> Vec<String> {
        let mut marked = Vec::new();
        for (id, trigger) in &mut self.triggers {
            if Some(id.as_str()) != keep
                && trigger.stands
                && trigger.agent == agent
                && trigger.limit == limit
            {
                trigger.stands = false;
                marked.push(id.clone());
            }
        }
        marked
    }
}

/// Folds one account-stream record into [`Acknowledgments`]: called for every account event before
/// it is interpreted, so an acknowledgment copy pending its next record is settled by that record.
/// A limit record whose agent or limit does not read is the account risk fold's to refuse; here it
/// tracks no trigger, so an acknowledgment naming it is refused `trigger_unknown` and lifts nothing.
///
/// # Errors
/// [`ExecutorError::NonCanonicalPayload`] for an acknowledgment record, a refusal or a limit
/// record whose members do not read.
pub(crate) fn fold(state: &mut ExecutorState, event: &FoldedEvent) -> Result<(), ExecutorError> {
    let held = &mut state.acknowledgments;
    if let Some(pending) = held.pending.take() {
        let refuses = event.event_type == REFUSED
            && optional_text(&event.payload, "command") == Some("acknowledge")
            && event.causation_id.as_ref().map(|id| id.0.as_str())
                == Some(pending.original.as_str());
        if refuses {
            for id in pending.superseded {
                if let Some(trigger) = held.triggers.get_mut(&id) {
                    trigger.stands = true;
                }
            }
        }
    }
    let payload = &event.payload;
    match event.event_type.as_str() {
        "RiskLimitTriggered" => {
            if let (Some(agent), Some(limit)) = (
                optional_text(payload, "agent_id"),
                optional_text(payload, "limit"),
            ) {
                let (agent, limit) = (agent.to_owned(), limit.to_owned());
                held.supersede(&agent, &limit, Some(&event.event_id.0));
                held.triggers.insert(
                    event.event_id.0.clone(),
                    Trigger {
                        agent,
                        limit,
                        stands: true,
                    },
                );
            }
        }
        "RiskLimitLifted" => {
            if let (Some(agent), Some(limit)) = (
                optional_text(payload, "agent_id"),
                optional_text(payload, "limit"),
            ) {
                held.supersede(agent, limit, None);
            }
        }
        REQUESTED => {
            let original = origin(event)?;
            held.copied.insert(original.clone());
            held.requests
                .entry(original)
                .or_insert_with(|| payload.clone());
        }
        ACKNOWLEDGED => {
            let original = origin(event)?;
            held.copied.insert(original.clone());
            if let Some(request) = optional_text(payload, "request") {
                held.named.insert(request.to_owned());
            }
            let named = required_text(payload, "event")?;
            let superseded = match held.triggers.get(named).cloned() {
                Some(trigger) if trigger.stands => {
                    held.supersede(&trigger.agent, &trigger.limit, None)
                }
                _ => Vec::new(),
            };
            held.pending = Some(Pending {
                original,
                superseded,
            });
        }
        _ => {}
    }
    Ok(())
}

/// The control-stream original a copy names as its `causation_id`.
fn origin(event: &FoldedEvent) -> Result<String, ExecutorError> {
    event
        .causation_id
        .as_ref()
        .map(|id| id.0.clone())
        .ok_or_else(|| ExecutorError::CopyWithoutCausation {
            event_type: event.event_type.clone(),
        })
}

/// Copies one acknowledgment or request and judges it (§9.28 checks 1 to 4). An original already
/// copied drafts nothing.
///
/// # Errors
/// [`ExecutorError::NotInterpreted`] for a handed record of another type; a draft's own error.
pub(crate) fn copy(
    batch: &mut Batch<'_, '_>,
    handoff: &AcknowledgmentHandoff,
) -> Result<(), ExecutorError> {
    let original = &handoff.original;
    let kind = original.event_type.as_str();
    if kind != ACKNOWLEDGED && kind != REQUESTED {
        return Err(ExecutorError::NotInterpreted {
            what: format!("an acknowledgment handoff of {kind}"),
            story: "E7-4",
        });
    }
    if batch
        .view
        .acknowledgments
        .copied
        .contains(&original.event_id.0)
    {
        return Ok(());
    }
    let at = batch.at().max(handoff.processed_at);
    let verdict = if kind == ACKNOWLEDGED {
        judged(&batch.view.acknowledgments, handoff, at)
    } else {
        None
    };
    batch.journal_raw(
        kind,
        handoff.schema_version,
        Some(original.event_id.clone()),
        original.payload.clone(),
    )?;
    if let Some(reason) = verdict {
        let refusal = object(vec![
            ("command", text("acknowledge")),
            ("reason", text(reason)),
            ("effective_at", risk_clock_stamp(at)?),
        ])?;
        batch.journal_raw(REFUSED, 1, Some(original.event_id.clone()), refusal)?;
    }
    Ok(())
}

/// §9.28's four checks, in order, at the second `at`: `None` when the acknowledgment passes all
/// four, or the first failing check's reason. A member that does not read fails the check that
/// reads it, so an unreadable acknowledgment never lifts.
fn judged(
    held: &Acknowledgments,
    handoff: &AcknowledgmentHandoff,
    at: RiskClock,
) -> Option<&'static str> {
    let p = &handoff.original.payload;
    let agent = optional_text(p, "agent");
    let trigger = optional_text(p, "event").and_then(|id| held.triggers.get(id).map(|t| (id, t)));
    let Some((_, trigger)) = trigger.filter(|(_, t)| Some(t.agent.as_str()) == agent) else {
        return Some("trigger_unknown");
    };
    if !trigger.stands {
        return Some("trigger_superseded");
    }
    if let Some(failed) = step_up_failure(
        p.get("step_up"),
        at.secs(),
        &handoff.spent,
        &handoff.methods,
    ) {
        return Some(failed);
    }
    let carried = p.get("independent_required") != Some(&Value::Bool(false));
    if !(carried || handoff.independent_overlay) {
        return None;
    }
    let request = optional_text(p, "request");
    let user = optional_text(p, "user");
    let Some(request) =
        request.filter(|_| user.is_some() && user != optional_text(p, "requested_by"))
    else {
        return Some("not_independent");
    };
    let Some(copied) = held.requests.get(request) else {
        return Some("request_invalid");
    };
    if MATCHED
        .iter()
        .any(|m| copied.get(m) != p.get(m) || copied.get(m).is_none())
    {
        return Some("request_invalid");
    }
    let submitted = optional_int(copied, "submitted_at").and_then(|s| i64::try_from(s).ok());
    let request_spent = handoff.request_spent.as_ref();
    let request_failed = match (submitted, request_spent) {
        (Some(submitted), Some(spent)) => {
            step_up_failure(copied.get("step_up"), submitted, spent, &handoff.methods)
        }
        _ => Some("request_invalid"),
    };
    if request_failed.is_some() || held.named.contains(request) {
        return Some("request_invalid");
    }
    None
}

/// Mandate spec §6.1 at the second `at`: the evidence is present, no older than the window and not
/// from the future, its assertion not spent before, and its method one the environment allows.
fn step_up_failure(
    step_up: Option<&Value>,
    at: i64,
    spent: &BTreeSet<String>,
    methods: &BTreeSet<String>,
) -> Option<&'static str> {
    let Some(evidence) = step_up.filter(|v| **v != Value::Null) else {
        return Some("step_up_missing");
    };
    let authenticated =
        optional_int(evidence, "authenticated_at").and_then(|s| i64::try_from(s).ok());
    let fresh =
        authenticated.is_some_and(|then| (0..=STEP_UP_WINDOW_S).contains(&at.saturating_sub(then)));
    if !fresh {
        return Some("step_up_stale");
    }
    match optional_text(evidence, "assertion_id") {
        Some(assertion) if !spent.contains(assertion) => {}
        _ => return Some("step_up_reused"),
    }
    match optional_text(evidence, "method") {
        Some(method) if methods.contains(method) => None,
        _ => Some("step_up_method"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use mandate_canon::Value;

    use super::step_up_failure;
    use crate::payload::{int, object, text};

    fn evidence(assertion: &str, at: u64, method: &str) -> Value {
        object(vec![
            ("assertion_id", text(assertion)),
            ("authenticated_at", int(at).unwrap_or(Value::Null)),
            ("method", text(method)),
        ])
        .unwrap_or(Value::Null)
    }

    fn paper() -> BTreeSet<String> {
        BTreeSet::from(["cli_confirm".to_owned()])
    }

    #[test]
    fn the_window_is_inclusive_at_300_seconds_and_never_in_the_future() {
        let step_up = evidence("a", 1_000, "cli_confirm");
        let none = BTreeSet::new();
        assert_eq!(
            step_up_failure(Some(&step_up), 1_300, &none, &paper()),
            None
        );
        assert_eq!(
            step_up_failure(Some(&step_up), 1_301, &none, &paper()),
            Some("step_up_stale")
        );
        assert_eq!(
            step_up_failure(Some(&step_up), 999, &none, &paper()),
            Some("step_up_stale")
        );
        assert_eq!(
            step_up_failure(Some(&step_up), 1_000, &none, &paper()),
            None
        );
    }

    #[test]
    fn missing_spent_and_unallowed_evidence_each_fail_with_their_own_reason() {
        let step_up = evidence("a", 1_000, "cli_confirm");
        let spent = BTreeSet::from(["a".to_owned()]);
        assert_eq!(
            step_up_failure(None, 1_000, &spent, &paper()),
            Some("step_up_missing")
        );
        assert_eq!(
            step_up_failure(Some(&Value::Null), 1_000, &spent, &paper()),
            Some("step_up_missing")
        );
        assert_eq!(
            step_up_failure(Some(&step_up), 1_000, &spent, &paper()),
            Some("step_up_reused")
        );
        assert_eq!(
            step_up_failure(Some(&step_up), 1_000, &BTreeSet::new(), &BTreeSet::new()),
            Some("step_up_method")
        );
    }
}
