//! The owner's acknowledgments the executor copies from the control stream (journal spec §9.28,
//! DEC-2828; E7-4b, DEC-2830), and what the executor's judgment needs with each that its account
//! stream does not hold: the step-up assertions the control stream spent before the original and
//! before the request it names, the policy overlay's independence requirement, and the methods the
//! environment allows. The process hands each, in `seq` order, before the agents' proposals.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Value;
use mandate_executor::{AcknowledgmentHandoff, AgentId, RiskClock};
use mandate_journal::{Environment, StoredEvent};

use crate::follow::folded;

/// Why the control stream's acknowledgments could not all be handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread {
    /// An acknowledgment, request or spending record did not read: the process holds every
    /// opening, as for any followed record that does not read, rather than skip what the owner
    /// wrote.
    Record,
    /// A membership record's `independent_approval_required` did not read, so the overlay is
    /// unknown: no acknowledgment is handed, and so none is judged or lifts anything, until it
    /// reads. An unknown overlay is never read as `false`.
    Policy,
}

const REQUESTED: &str = "OwnerAcknowledgmentRequested";
const ACKNOWLEDGED: &str = "OwnerAcknowledged";

/// Mandate spec §6.1's four record types whose step-up assertion a later one may not reuse.
const SPENDING: [&str; 4] = [
    "ApprovalResponseSubmitted",
    "OwnerCommandIssued",
    REQUESTED,
    ACKNOWLEDGED,
];

/// The membership records that carry the workspace's effective `independent_approval_required` at
/// their instant (DEC-648 item 7, `mandate_identity::check_independence`).
const MEMBERSHIP: [&str; 3] = ["MemberActivated", "MemberRoleChanged", "MemberReactivated"];

/// Every acknowledgment and request on `control`, in `environment`, whose `agent` is one of
/// `agents`, as the executor's handoff at the processing second `processed_at`.
///
/// `independent_overlay` is the latest membership record's `independent_approval_required`, the
/// effective policy as the workspace's writers record it, and with none `false`: a requirement no
/// level sets is not required (mandate spec §4.3). It applies to every agent of the account alike,
/// so it is the strictest across them. No effective-policy fold exists yet (backlog E9-6a). `methods` is `cli_confirm` in paper and nothing elsewhere: live
/// step-up is the founder's own connection's (DEC-529 item 3), which no executor here serves.
///
/// # Errors
/// [`Unread::Record`] for an acknowledgment, request or spending record whose members do not read;
/// [`Unread::Policy`] for a membership record whose `independent_approval_required` does not.
pub fn acknowledgments(
    control: &[StoredEvent],
    agents: &BTreeSet<AgentId>,
    environment: Environment,
    processed_at: RiskClock,
) -> Result<Vec<AcknowledgmentHandoff>, Unread> {
    let mut spent: BTreeSet<String> = BTreeSet::new();
    let mut spent_before_request: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut policy = Some(false);
    let mut out = Vec::new();
    let methods: BTreeSet<String> = if environment == Environment::Paper {
        BTreeSet::from(["cli_confirm".to_owned()])
    } else {
        BTreeSet::new()
    };
    for row in control
        .iter()
        .filter(|row| row.environment == environment.as_str())
    {
        let kind = row.event_type.as_str();
        if MEMBERSHIP.contains(&kind) {
            let read = folded(row).ok();
            policy = match read
                .as_ref()
                .and_then(|event| event.payload.get("independent_approval_required"))
            {
                Some(Value::Bool(required)) => Some(*required),
                _ => None,
            };
            continue;
        }
        if !SPENDING.contains(&kind) {
            continue;
        }
        let event = folded(row).map_err(|_| Unread::Record)?;
        if kind == REQUESTED || kind == ACKNOWLEDGED {
            let agent = event
                .payload
                .get("agent")
                .and_then(Value::as_str)
                .ok_or(Unread::Record)?;
            if kind == REQUESTED {
                spent_before_request.insert(event.event_id.0.clone(), spent.clone());
            }
            if agents.contains(&AgentId(agent.to_owned())) {
                let request_spent = event
                    .payload
                    .get("request")
                    .and_then(Value::as_str)
                    .and_then(|request| spent_before_request.get(request).cloned());
                out.push(AcknowledgmentHandoff {
                    original: event.clone(),
                    schema_version: row.schema_version,
                    processed_at,
                    spent: spent.clone(),
                    request_spent,
                    independent_overlay: false,
                    methods: methods.clone(),
                });
            }
        }
        match event.payload.get("step_up") {
            Some(Value::Null) | None => {}
            Some(step_up) => {
                let assertion = step_up
                    .get("assertion_id")
                    .and_then(Value::as_str)
                    .ok_or(Unread::Record)?;
                spent.insert(assertion.to_owned());
            }
        }
    }
    let overlay = policy.ok_or(Unread::Policy)?;
    for handoff in &mut out {
        handoff.independent_overlay = overlay;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use mandate_canon::{Digest, Key, Object, Value, to_canonical};
    use mandate_executor::{AgentId, RiskClock};
    use mandate_journal::{Environment, StoredEvent};

    use super::{Unread, acknowledgments};

    fn object(members: &[(&str, Value)]) -> Value {
        let mut out = Object::new();
        for (name, value) in members {
            if let Ok(key) = Key::new(name) {
                out.insert(key, value.clone());
            }
        }
        Value::Object(out)
    }

    fn s(text: &str) -> Value {
        Value::Str(text.to_owned())
    }

    fn row(seq: u64, id: &str, kind: &str, payload: Value) -> StoredEvent {
        let body = to_canonical(&object(&[("payload", payload)]));
        StoredEvent {
            stream_id: "ctl:ws1".to_owned(),
            seq,
            event_id: id.to_owned(),
            event_type: kind.to_owned(),
            schema_version: 2,
            environment: "paper".to_owned(),
            recorded_at: "2026-10-26T12:00:00.000000000Z".to_owned(),
            prev_hash: Digest::ZERO,
            hash: Digest::of(&body),
            body,
        }
    }

    fn step_up(assertion: &str) -> Value {
        object(&[("assertion_id", s(assertion))])
    }

    fn acknowledged(
        seq: u64,
        id: &str,
        agent: &str,
        request: Option<&str>,
        assertion: &str,
    ) -> StoredEvent {
        row(
            seq,
            id,
            "OwnerAcknowledged",
            object(&[
                ("agent", s(agent)),
                ("request", request.map_or(Value::Null, s)),
                ("step_up", step_up(assertion)),
            ]),
        )
    }

    fn member(seq: u64, required: Value) -> StoredEvent {
        row(
            seq,
            &format!("m{seq}"),
            "MemberRoleChanged",
            object(&[("independent_approval_required", required)]),
        )
    }

    fn ours() -> BTreeSet<AgentId> {
        BTreeSet::from([AgentId("agenta".to_owned())])
    }

    fn handed(
        control: &[StoredEvent],
    ) -> Result<Vec<mandate_executor::AcknowledgmentHandoff>, Unread> {
        acknowledgments(
            control,
            &ours(),
            Environment::Paper,
            RiskClock::from_secs(100),
        )
    }

    #[test]
    fn the_latest_membership_record_sets_the_overlay_and_none_is_false() {
        let ack = acknowledged(3, "ak", "agenta", None, "a1");
        let none = handed(std::slice::from_ref(&ack));
        assert_eq!(
            none.map(|h| h.iter().map(|x| x.independent_overlay).collect::<Vec<_>>()),
            Ok(vec![false])
        );
        let latest = handed(&[
            member(1, Value::Bool(false)),
            member(2, Value::Bool(true)),
            ack.clone(),
        ]);
        assert_eq!(
            latest.map(|h| h.iter().map(|x| x.independent_overlay).collect::<Vec<_>>()),
            Ok(vec![true])
        );
        let later = handed(&[
            member(1, Value::Bool(true)),
            ack,
            member(4, Value::Bool(false)),
        ]);
        assert_eq!(
            later.map(|h| h.iter().map(|x| x.independent_overlay).collect::<Vec<_>>()),
            Ok(vec![false])
        );
    }

    #[test]
    fn an_unreadable_membership_record_hands_no_acknowledgment_and_never_reads_as_false() {
        let ack = acknowledged(3, "ak", "agenta", None, "a1");
        for unreadable in [s("no"), Value::Null] {
            assert_eq!(
                handed(&[
                    member(1, Value::Bool(false)),
                    member(2, unreadable.clone()),
                    ack.clone()
                ])
                .err(),
                Some(Unread::Policy),
                "{unreadable:?}"
            );
        }
        let missing = row(2, "m2", "MemberActivated", object(&[]));
        assert_eq!(handed(&[missing, ack]).err(), Some(Unread::Policy));
    }

    #[test]
    fn spent_assertions_are_those_before_the_original_and_its_request() {
        let control = [
            row(
                1,
                "cm",
                "OwnerCommandIssued",
                object(&[("step_up", step_up("c1"))]),
            ),
            row(
                2,
                "rq",
                "OwnerAcknowledgmentRequested",
                object(&[("agent", s("agenta")), ("step_up", step_up("r1"))]),
            ),
            row(
                3,
                "ap",
                "ApprovalResponseSubmitted",
                object(&[("step_up", step_up("p1"))]),
            ),
            acknowledged(4, "ak", "agenta", Some("rq"), "a1"),
        ];
        let handed = handed(&control).unwrap_or_default();
        assert_eq!(handed.len(), 2, "the request and the acknowledgment");
        let (Some(request), Some(ack)) = (handed.first(), handed.get(1)) else {
            unreachable!("two handed: {handed:?}");
        };
        assert_eq!(
            ack.spent,
            BTreeSet::from(["c1".to_owned(), "r1".to_owned(), "p1".to_owned()])
        );
        assert_eq!(ack.request_spent, Some(BTreeSet::from(["c1".to_owned()])));
        assert_eq!(request.request_spent, None);
        assert_eq!(ack.processed_at, RiskClock::from_secs(100));
        assert_eq!(ack.methods, BTreeSet::from(["cli_confirm".to_owned()]));
    }

    #[test]
    fn another_accounts_agent_is_not_handed_but_spends_its_assertion() {
        let control = [
            acknowledged(1, "other", "agentz", None, "z1"),
            acknowledged(2, "ak", "agenta", None, "a1"),
        ];
        let handed = handed(&control).unwrap_or_default();
        assert_eq!(
            handed.iter().map(|h| h.spent.clone()).collect::<Vec<_>>(),
            [BTreeSet::from(["z1".to_owned()])]
        );
    }

    #[test]
    fn an_acknowledgment_that_does_not_read_holds_the_record() {
        let bad = row(
            1,
            "ak",
            "OwnerAcknowledged",
            object(&[("step_up", step_up("a1"))]),
        );
        assert_eq!(handed(&[bad]).err(), Some(Unread::Record));
        let bad_step_up = row(
            1,
            "ak",
            "OwnerAcknowledged",
            object(&[("agent", s("agenta")), ("step_up", s("x"))]),
        );
        assert_eq!(handed(&[bad_step_up]).err(), Some(Unread::Record));
    }
}
