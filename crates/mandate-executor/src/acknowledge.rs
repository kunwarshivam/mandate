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

/// The prefix of every reconciliation hold's restriction (`crate::state::restriction_for`).
const RECONCILIATION: &str = "reconciliation:";

/// Mandate spec §6.1's freshness window, in seconds, inclusive.
const STEP_UP_WINDOW_S: i64 = 300;

/// The members a request and the acknowledgment answering it must agree on (§9.28 check 4, rule
/// 240 restated on the account stream), besides their subjects, which [`request_subject`] and
/// [`subject`] read (DEC-2838).
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
    /// Every reconciliation hold, by its `event_id`: the executor's own `exits_only` or `paused`
    /// `AgentModeApplied` under `reconciliation:<subject>`, as its agent and restriction (§9.28
    /// v0.104, DEC-2830).
    holds: BTreeMap<String, (String, String)>,
    /// The latest `AgentModeApplied` of each agent and restriction: a hold stands while it is its
    /// key's latest, so applying it again or removing it supersedes it.
    latest: BTreeMap<(String, String), String>,
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
    /// Whether the control-stream record `original` has been copied.
    pub(crate) fn copied(&self, original: &str) -> bool {
        self.copied.contains(original)
    }

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
            let named = required_text(payload, "event")?;
            held.copied.insert(original.clone());
            if let Some(request) = optional_text(payload, "request") {
                held.named.insert(request.to_owned());
            }
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
        "AgentModeApplied" => {
            if let (Some(agent), Some(restriction)) = (
                optional_text(payload, "agent"),
                optional_text(payload, "restriction"),
            ) {
                let key = (agent.to_owned(), restriction.to_owned());
                let holds = payload.get("originated") == Some(&Value::Bool(true))
                    && matches!(optional_text(payload, "to"), Some("exits_only" | "paused"))
                    && restriction.starts_with(RECONCILIATION);
                if holds {
                    held.holds.insert(event.event_id.0.clone(), key.clone());
                }
                held.latest.insert(key, event.event_id.0.clone());
            }
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
    let hold = held_by(&batch.view.acknowledgments, handoff).cloned();
    let copy = batch.journal_raw(
        kind,
        handoff.schema_version,
        Some(original.event_id.clone()),
        original.payload.clone(),
    )?;
    if let (true, None, Some((agent, restriction))) = (kind == ACKNOWLEDGED, verdict, hold) {
        batch.journal(
            "AgentModeApplied",
            Some(copy),
            vec![
                ("agent", text(agent)),
                ("to", text("normal")),
                ("restriction", text(restriction)),
                ("originated", Value::Bool(true)),
            ],
        )?;
    }
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

/// What a handed acknowledgment's `event` names: a version 1 acknowledgment names a limit, and a
/// version 2 one what its `subject` says.
fn subject(handoff: &AcknowledgmentHandoff) -> Option<&str> {
    if handoff.schema_version == 1 {
        Some("limit")
    } else {
        optional_text(&handoff.original.payload, "subject")
    }
}

/// The subject a copied request names, read as DEC-2838 reads rule 240: version 2's `subject`, and
/// `limit` for a version-1 request, which carries none (§9.28 makes `subject` required from
/// version 2). `None` for a `subject` that is not text, which matches no acknowledgment.
fn request_subject(request: &Value) -> Option<&str> {
    match request.get("subject") {
        None => Some("limit"),
        Some(subject) => subject.as_str(),
    }
}

/// The reconciliation hold an acknowledgment of `subject` `reconciliation` names, as its agent and
/// restriction, if the account stream holds it.
fn held_by<'h>(
    held: &'h Acknowledgments,
    handoff: &AcknowledgmentHandoff,
) -> Option<&'h (String, String)> {
    if subject(handoff) != Some("reconciliation") {
        return None;
    }
    optional_text(&handoff.original.payload, "event").and_then(|id| held.holds.get(id))
}

/// §9.28's checks 1 and 2 for what the acknowledgment names: a limit's trigger (§9.28), or a
/// reconciliation hold (v0.104, DEC-2830). Any other subject names nothing the executor lifts yet,
/// so it is refused `trigger_unknown` (a restriction's lift is E7-2y slice C's).
fn named_stands(held: &Acknowledgments, handoff: &AcknowledgmentHandoff) -> Option<&'static str> {
    let p = &handoff.original.payload;
    match subject(handoff) {
        Some("limit") => {
            let agent = optional_text(p, "agent");
            let trigger = optional_text(p, "event").and_then(|id| held.triggers.get(id));
            let Some(trigger) = trigger.filter(|t| Some(t.agent.as_str()) == agent) else {
                return Some("trigger_unknown");
            };
            (!trigger.stands).then_some("trigger_superseded")
        }
        Some("reconciliation") => {
            let event = optional_text(p, "event");
            let Some(key) = held_by(held, handoff) else {
                return Some("trigger_unknown");
            };
            (held.latest.get(key).map(String::as_str) != event).then_some("trigger_superseded")
        }
        _ => Some("trigger_unknown"),
    }
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
    if let Some(failed) = named_stands(held, handoff) {
        return Some(failed);
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
    let asked = request_subject(copied);
    if asked.is_none() || asked != subject(handoff) {
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

    use super::{fold, request_subject, step_up_failure};
    use crate::payload::{int, object, text};
    use crate::state::ExecutorState;
    use crate::types::{AccountRef, AccountScope, EventId, FoldedEvent, Seq, WorkspaceId};

    fn state() -> ExecutorState {
        ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        })
    }

    fn record(
        id: &str,
        kind: &str,
        causation: Option<&str>,
        pairs: Vec<(&str, Value)>,
    ) -> FoldedEvent {
        FoldedEvent {
            stream: "acct:ws1:acct-1".to_owned(),
            seq: Seq(1),
            event_id: EventId(id.to_owned()),
            event_type: kind.to_owned(),
            causation_id: causation.map(|c| EventId(c.to_owned())),
            payload: object(pairs).unwrap_or(Value::Null),
        }
    }

    fn trigger(id: &str) -> FoldedEvent {
        record(
            id,
            "RiskLimitTriggered",
            None,
            vec![
                ("agent_id", text("agent_a")),
                ("limit", text("max_daily_loss")),
            ],
        )
    }

    fn acknowledgment(original: &str, event: &str) -> FoldedEvent {
        record(
            &format!("copy-{original}"),
            "OwnerAcknowledged",
            Some(original),
            vec![("event", text(event)), ("request", Value::Null)],
        )
    }

    fn refusal(command: &str, causation: &str) -> FoldedEvent {
        record(
            "refusal",
            "OwnerCommandRefused",
            Some(causation),
            vec![
                ("command", text(command)),
                ("reason", text("step_up_stale")),
            ],
        )
    }

    fn stands(state: &ExecutorState, id: &str) -> bool {
        state
            .acknowledgments
            .triggers
            .get(id)
            .is_some_and(|t| t.stands)
    }

    /// Only the very refusal of the pending copy, `acknowledge` and naming its original, restores
    /// what the copy superseded: another original's refusal, a refused resume of the same original,
    /// or any other record leaves the copy accepted.
    #[test]
    fn only_the_refusal_of_a_copy_restores_its_trigger() {
        for (next, restored) in [
            (refusal("acknowledge", "ak-1"), true),
            (refusal("acknowledge", "ak-9"), false),
            (refusal("resume", "ak-1"), false),
            (
                record(
                    "x",
                    "AgentModeApplied",
                    Some("ak-1"),
                    vec![("command", text("acknowledge"))],
                ),
                false,
            ),
        ] {
            let mut state = state();
            for event in [
                trigger("tg-1"),
                acknowledgment("ak-1", "tg-1"),
                next.clone(),
            ] {
                assert!(fold(&mut state, &event).is_ok());
            }
            assert_eq!(stands(&state, "tg-1"), restored, "after {next:?}");
        }
    }

    /// Only the executor's own lift of an instrument's hold ends its recorded mismatch (DEC-2830):
    /// a copied mode to `normal` under the same restriction leaves it.
    #[test]
    fn only_an_originated_lift_ends_a_mismatch() -> Result<(), crate::error::ExecutorError> {
        for (originated, ends) in [(true, true), (false, false)] {
            let mut state = state();
            let instrument = mandate_accounting::InstrumentId::new("AAPL")?;
            state.mismatched.insert(instrument.clone());
            let lift = record(
                "lift",
                "AgentModeApplied",
                Some("origin"),
                vec![
                    ("agent", text("*")),
                    ("to", text("normal")),
                    ("restriction", text("reconciliation:AAPL")),
                    ("originated", Value::Bool(originated)),
                ],
            );
            assert!(crate::fold::agent_mode_applied(&mut state, &lift.payload).is_ok());
            assert_eq!(
                !state.mismatched.contains(&instrument),
                ends,
                "originated {originated}"
            );
        }
        Ok(())
    }

    /// An acknowledgment of a trigger already superseded supersedes nothing more: the newer
    /// trigger of the same limit still stands once the copy is settled, refused or not.
    #[test]
    fn an_acknowledgment_of_a_superseded_trigger_leaves_the_newer_one_standing() {
        let mut state = state();
        for event in [
            trigger("tg-1"),
            trigger("tg-2"),
            acknowledgment("ak-1", "tg-1"),
            record("x", "ClockAdvanced", None, vec![]),
        ] {
            assert!(fold(&mut state, &event).is_ok());
        }
        assert!(!stands(&state, "tg-1"));
        assert!(stands(&state, "tg-2"));
    }

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

    #[test]
    fn a_request_names_its_subject_a_limit_without_one_and_none_when_it_is_not_text()
    -> Result<(), crate::error::ExecutorError> {
        let none = object(vec![])?;
        let named = object(vec![("subject", text("reconciliation"))])?;
        let not_text = object(vec![("subject", Value::Bool(true))])?;
        assert_eq!(request_subject(&none), Some("limit"));
        assert_eq!(request_subject(&named), Some("reconciliation"));
        assert_eq!(request_subject(&not_text), None);
        Ok(())
    }

    mod differential {
        use std::collections::BTreeSet;

        use mandate_canon::{Digest, Value, to_canonical};
        use mandate_journal::StoredEvent;
        use mandate_journal::holds::{Holder, Reason, standing};
        use proptest::prelude::*;

        use super::super::{fold, named_stands};
        use super::state;
        use crate::payload::{object, text};
        use crate::types::{AcknowledgmentHandoff, EventId, FoldedEvent, RiskClock, Seq};

        const AGENTS: [&str; 2] = ["agent_a", "agent_b"];
        const LIMITS: [&str; 2] = ["max_daily_loss", "profit_stop"];
        const MODE_AGENTS: [&str; 3] = ["agent_a", "agent_b", "*"];
        const RESTRICTIONS: [&str; 3] =
            ["reconciliation:cash", "reconciliation:SPY", "kill_switch"];
        const MODES: [&str; 4] = ["exits_only", "paused", "normal", "stopped"];

        fn pick<const N: usize>(names: &[&'static str; N], at: usize) -> &'static str {
            names.get(at).copied().unwrap_or_default()
        }

        #[derive(Debug, Clone)]
        enum Op {
            Trigger(usize, usize),
            Lift(usize, usize),
            Mode(usize, usize, usize, bool),
            Copy(usize, usize),
            Refuse(bool, usize),
        }

        fn op() -> impl Strategy<Value = Op> {
            prop_oneof![
                (0..2usize, 0..2usize).prop_map(|(a, l)| Op::Trigger(a, l)),
                (0..2usize, 0..2usize).prop_map(|(a, l)| Op::Lift(a, l)),
                (0..3usize, 0..3usize, 0..4usize, any::<bool>())
                    .prop_map(|(a, r, t, o)| Op::Mode(a, r, t, o)),
                (any::<usize>(), 0..3usize).prop_map(|(e, c)| Op::Copy(e, c)),
                (any::<bool>(), 0..3usize).prop_map(|(ack, c)| Op::Refuse(ack, c)),
            ]
        }

        /// One generated record, as the executor folds it and as the journal stores it.
        fn record(
            id: &str,
            kind: &str,
            causation: Option<&str>,
            pairs: Vec<(&str, Value)>,
        ) -> (FoldedEvent, StoredEvent) {
            let payload = object(pairs).unwrap_or(Value::Null);
            let body = to_canonical(
                &object(vec![
                    ("causation_id", causation.map_or(Value::Null, text)),
                    ("event_id", text(id)),
                    ("payload", payload.clone()),
                ])
                .unwrap_or(Value::Null),
            );
            let folded = FoldedEvent {
                stream: "acct:ws1:acct-1".to_owned(),
                seq: Seq(1),
                event_id: EventId(id.to_owned()),
                event_type: kind.to_owned(),
                causation_id: causation.map(|c| EventId(c.to_owned())),
                payload,
            };
            let stored = StoredEvent {
                stream_id: "acct:ws1:acct-1".to_owned(),
                seq: 1,
                event_id: id.to_owned(),
                event_type: kind.to_owned(),
                schema_version: 1,
                environment: "paper".to_owned(),
                recorded_at: "2026-10-26T12:00:00.000000000Z".to_owned(),
                prev_hash: Digest::ZERO,
                hash: Digest::of(&body),
                body,
            };
            (folded, stored)
        }

        fn handoff(event: &str, subject: &str, agent: &str) -> AcknowledgmentHandoff {
            AcknowledgmentHandoff {
                original: FoldedEvent {
                    stream: "ctl:ws1".to_owned(),
                    seq: Seq(1),
                    event_id: EventId("probe".to_owned()),
                    event_type: "OwnerAcknowledged".to_owned(),
                    causation_id: None,
                    payload: object(vec![
                        ("agent", text(agent)),
                        ("event", text(event)),
                        ("subject", text(subject)),
                    ])
                    .unwrap_or(Value::Null),
                },
                schema_version: 2,
                processed_at: RiskClock::from_secs(0),
                spent: BTreeSet::new(),
                request_spent: None,
                independent_overlay: false,
                methods: BTreeSet::new(),
            }
        }

        proptest! {
            /// E7-4b, DEC-2144: over any account stream of triggers, lifts, mode records,
            /// acknowledgment copies and refusals, `mandate_journal::holds::standing` lists a
            /// limit's trigger or a reconciliation hold exactly when the executor's §9.28 checks 1
            /// and 2 pass for an acknowledgment naming it, so every reader lists what the executor
            /// would lift.
            #[test]
            fn the_shared_fold_lists_exactly_what_the_executor_s_checks_let_stand(
                ops in prop::collection::vec(op(), 1..32)
            ) {
                let mut executor = state();
                let mut stored = Vec::new();
                let mut triggers: Vec<(String, &str)> = Vec::new();
                let mut ids: Vec<String> = Vec::new();
                for (i, op) in ops.iter().enumerate() {
                    let id = format!("e{i}");
                    let (folded, row) = match *op {
                        Op::Trigger(a, l) | Op::Lift(a, l) => {
                            let kind = if matches!(op, Op::Trigger(..)) {
                                triggers.push((id.clone(), pick(&AGENTS, a)));
                                "RiskLimitTriggered"
                            } else {
                                "RiskLimitLifted"
                            };
                            record(&id, kind, None, vec![
                                ("agent_id", text(pick(&AGENTS, a))),
                                ("limit", text(pick(&LIMITS, l))),
                            ])
                        }
                        Op::Mode(a, r, t, originated) => record(&id, "AgentModeApplied", None, vec![
                            ("agent", text(pick(&MODE_AGENTS, a))),
                            ("originated", Value::Bool(originated)),
                            ("restriction", text(pick(&RESTRICTIONS, r))),
                            ("to", text(pick(&MODES, t))),
                        ]),
                        Op::Copy(named, c) => {
                            let event = named.checked_rem(ids.len()).and_then(|at| ids.get(at)).cloned().unwrap_or_default();
                            record(&id, "OwnerAcknowledged", Some(&format!("ctl{c}")), vec![
                                ("event", text(&event)),
                                ("request", Value::Null),
                            ])
                        }
                        Op::Refuse(acknowledge, c) => record(
                            &id,
                            "OwnerCommandRefused",
                            Some(&format!("ctl{c}")),
                            vec![("command", text(if acknowledge { "acknowledge" } else { "resume" }))],
                        ),
                    };
                    fold(&mut executor, &folded).map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
                    stored.push(row);
                    ids.push(id);
                }
                let listed = standing(&stored).map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
                let held = &executor.acknowledgments;
                for (id, _) in &triggers {
                    for agent in AGENTS {
                        let lifts = named_stands(held, &handoff(id, "limit", agent)).is_none();
                        let lists = listed.iter().any(|h| {
                            h.event == *id
                                && matches!(h.reason, Reason::Limit(_))
                                && h.holder == Holder::Agent(agent.to_owned())
                        });
                        prop_assert_eq!(lifts, lists, "the trigger {} acknowledged as {}", id, agent);
                    }
                }
                for id in &ids {
                    let lifts = named_stands(held, &handoff(id, "reconciliation", "agent_a")).is_none();
                    let lists = listed
                        .iter()
                        .any(|h| h.event == *id && matches!(h.reason, Reason::Reconciliation { .. }));
                    prop_assert_eq!(lifts, lists, "the hold {}", id);
                }
            }
        }
    }
}
