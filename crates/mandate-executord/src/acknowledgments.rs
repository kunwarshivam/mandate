//! The owner's acknowledgments the executor copies from the control stream (journal spec §9.28,
//! DEC-2828; E7-4b, DEC-2830), and what the executor's judgment needs with each that its account
//! stream does not hold: the step-up assertions the control stream spent before the original and
//! before the request it names, the policy overlay's independence requirement, and the methods the
//! environment allows. The process hands each, in `seq` order, before the agents' proposals.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Value;
use mandate_executor::{AcknowledgmentHandoff, AgentId, EventId, RiskClock};
use mandate_journal::{Environment, StoredEvent};

use crate::follow::{Unreadable, folded};

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
/// `agents` and whose original `copied` does not hold, as the executor's handoff at the processing
/// second `processed_at`.
///
/// `independent_overlay` is the latest membership record's `independent_approval_required`, the
/// effective policy as the workspace's writers record it, and with none `false`: a requirement no
/// level sets is not required (mandate spec §4.3). A membership record whose member does not read
/// leaves the requirement unknown, which is read as `true`, never `false`, until a later one reads:
/// an acknowledgment by its owner alone is then copied and refused `not_independent`, in the
/// journal, and one an independent user gave still passes. The overlay applies to every agent of
/// the account alike, so it is the strictest across them. No effective-policy fold exists yet
/// (backlog E9-6).
///
/// `methods` are journal spec v0.106 §9.28 check 3's (DEC-2523): `passkey` everywhere and
/// `cli_confirm` in paper. Check 3 also admits `cli_confirm` on the founder's own live connection
/// (DEC-529 item 3, DEC-923), which this process cannot tell from another, so it admits only
/// `passkey` in live.
///
/// # Errors
/// [`Unreadable`] for an acknowledgment, request or spending record whose members do not read: the
/// process then holds every opening rather than judge a step-up against a spent set it could not
/// read in full.
pub fn acknowledgments(
    control: &[StoredEvent],
    agents: &BTreeSet<AgentId>,
    environment: Environment,
    processed_at: RiskClock,
    copied: impl Fn(&EventId) -> bool,
) -> Result<Vec<AcknowledgmentHandoff>, Unreadable> {
    let mut spent: BTreeSet<String> = BTreeSet::new();
    let mut spent_before_request: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut overlay = false;
    let mut out = Vec::new();
    let mut methods = BTreeSet::from(["passkey".to_owned()]);
    if environment == Environment::Paper {
        methods.insert("cli_confirm".to_owned());
    }
    for row in control
        .iter()
        .filter(|row| row.environment == environment.as_str())
    {
        let kind = row.event_type.as_str();
        if MEMBERSHIP.contains(&kind) {
            let read = folded(row).ok();
            overlay = read
                .as_ref()
                .and_then(|event| event.payload.get("independent_approval_required"))
                .is_none_or(|required| *required != Value::Bool(false));
            continue;
        }
        if !SPENDING.contains(&kind) {
            continue;
        }
        let event = folded(row)?;
        if kind == REQUESTED || kind == ACKNOWLEDGED {
            let agent = event
                .payload
                .get("agent")
                .and_then(Value::as_str)
                .ok_or(Unreadable)?;
            if kind == REQUESTED {
                spent_before_request.insert(event.event_id.0.clone(), spent.clone());
            }
            if agents.contains(&AgentId(agent.to_owned())) && !copied(&event.event_id) {
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
                    .ok_or(Unreadable)?;
                spent.insert(assertion.to_owned());
            }
        }
    }
    for handoff in &mut out {
        handoff.independent_overlay = overlay;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use mandate_canon::{Digest, Key, Object, Value, to_canonical};
    use mandate_executor::{AcknowledgmentHandoff, AgentId, EventId, RiskClock};
    use mandate_journal::{Environment, StoredEvent};

    use super::acknowledgments;
    use crate::follow::Unreadable;

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

    fn row_in(environment: &str, seq: u64, id: &str, kind: &str, payload: Value) -> StoredEvent {
        let body = to_canonical(&object(&[("payload", payload)]));
        StoredEvent {
            stream_id: "ctl:ws1".to_owned(),
            seq,
            event_id: id.to_owned(),
            event_type: kind.to_owned(),
            schema_version: 2,
            environment: environment.to_owned(),
            recorded_at: "2026-10-26T12:00:00.000000000Z".to_owned(),
            prev_hash: Digest::ZERO,
            hash: Digest::of(&body),
            body,
        }
    }

    fn row(seq: u64, id: &str, kind: &str, payload: Value) -> StoredEvent {
        row_in("paper", seq, id, kind, payload)
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

    fn handed_in(
        control: &[StoredEvent],
        environment: Environment,
    ) -> Result<Vec<AcknowledgmentHandoff>, Unreadable> {
        acknowledgments(
            control,
            &ours(),
            environment,
            RiskClock::from_secs(100),
            |_| false,
        )
    }

    fn handed(control: &[StoredEvent]) -> Result<Vec<AcknowledgmentHandoff>, Unreadable> {
        handed_in(control, Environment::Paper)
    }

    fn overlays(control: &[StoredEvent]) -> Result<Vec<bool>, Unreadable> {
        handed(control).map(|h| h.iter().map(|x| x.independent_overlay).collect())
    }

    #[test]
    fn the_latest_membership_record_sets_the_overlay_and_none_is_false() {
        let ack = acknowledged(3, "ak", "agenta", None, "a1");
        assert_eq!(overlays(std::slice::from_ref(&ack)), Ok(vec![false]));
        let latest = [
            member(1, Value::Bool(false)),
            member(2, Value::Bool(true)),
            ack.clone(),
        ];
        assert_eq!(overlays(&latest), Ok(vec![true]));
        let after_the_acknowledgment = [
            member(1, Value::Bool(true)),
            ack,
            member(4, Value::Bool(false)),
        ];
        assert_eq!(
            overlays(&after_the_acknowledgment),
            Ok(vec![false]),
            "the overlay at processing, not at the acknowledgment"
        );
    }

    #[test]
    fn an_unreadable_membership_record_requires_independence_until_a_later_one_reads() {
        let ack = acknowledged(3, "ak", "agenta", None, "a1");
        for unreadable in [s("no"), Value::Null] {
            let control = [
                member(1, Value::Bool(false)),
                member(2, unreadable.clone()),
                ack.clone(),
            ];
            assert_eq!(overlays(&control), Ok(vec![true]), "{unreadable:?}");
            let read_again = [
                member(1, unreadable.clone()),
                ack.clone(),
                member(4, Value::Bool(false)),
            ];
            assert_eq!(overlays(&read_again), Ok(vec![false]), "{unreadable:?}");
        }
        let missing = row(2, "m2", "MemberActivated", object(&[]));
        assert_eq!(overlays(&[missing, ack.clone()]), Ok(vec![true]));
        let bad_body = StoredEvent {
            body: b"not json".to_vec(),
            ..member(2, Value::Bool(false))
        };
        assert_eq!(overlays(&[bad_body, ack]), Ok(vec![true]));
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
            row(
                4,
                "other",
                "PolicyChanged",
                object(&[("step_up", step_up("x1"))]),
            ),
            acknowledged(5, "ak", "agenta", Some("rq"), "a1"),
        ];
        let handed = handed(&control).unwrap_or_default();
        let (Some(request), Some(ack)) = (handed.first(), handed.get(1)) else {
            unreachable!("two handed: {handed:?}");
        };
        assert_eq!(handed.len(), 2);
        assert_eq!(request.spent, BTreeSet::from(["c1".to_owned()]));
        assert_eq!(
            ack.spent,
            BTreeSet::from(["c1".to_owned(), "r1".to_owned(), "p1".to_owned()])
        );
        assert_eq!(ack.request_spent, Some(BTreeSet::from(["c1".to_owned()])));
        assert_eq!(request.request_spent, None);
        assert_eq!(ack.processed_at, RiskClock::from_secs(100));
        assert_eq!(
            ack.methods,
            BTreeSet::from(["cli_confirm".to_owned(), "passkey".to_owned()])
        );
        assert_eq!(ack.original.event_id, EventId("ak".to_owned()));
        assert_eq!(ack.schema_version, 2);
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
    fn a_copied_original_is_not_handed_again_but_still_spends() {
        let control = [
            acknowledged(1, "done", "agenta", None, "d1"),
            acknowledged(2, "ak", "agenta", None, "a1"),
        ];
        let handed = acknowledgments(
            &control,
            &ours(),
            Environment::Paper,
            RiskClock::from_secs(100),
            |id| id.0 == "done",
        )
        .unwrap_or_default();
        assert_eq!(
            handed
                .iter()
                .map(|h| (h.original.event_id.0.clone(), h.spent.clone()))
                .collect::<Vec<_>>(),
            [("ak".to_owned(), BTreeSet::from(["d1".to_owned()]))]
        );
    }

    #[test]
    fn another_environments_records_count_for_nothing() {
        let control = [
            row_in(
                "live",
                1,
                "lm",
                "MemberRoleChanged",
                object(&[("independent_approval_required", Value::Bool(true))]),
            ),
            row_in(
                "live",
                2,
                "lc",
                "OwnerCommandIssued",
                object(&[("step_up", step_up("l1"))]),
            ),
            row_in(
                "live",
                3,
                "la",
                "OwnerAcknowledged",
                object(&[("agent", s("agenta")), ("step_up", step_up("l2"))]),
            ),
            row_in("live", 4, "lx", "OwnerAcknowledged", object(&[])),
            acknowledged(5, "ak", "agenta", None, "a1"),
        ];
        let handed = handed(&control).unwrap_or_default();
        assert_eq!(
            handed
                .iter()
                .map(|h| (
                    h.original.event_id.0.clone(),
                    h.spent.len(),
                    h.independent_overlay
                ))
                .collect::<Vec<_>>(),
            [("ak".to_owned(), 0, false)]
        );
    }

    #[test]
    fn passkey_is_allowed_everywhere_and_cli_confirm_only_in_paper() {
        let control = [row_in(
            "live",
            1,
            "ak",
            "OwnerAcknowledged",
            object(&[("agent", s("agenta"))]),
        )];
        let handed = handed_in(&control, Environment::Live).unwrap_or_default();
        assert_eq!(
            handed.iter().map(|h| h.methods.clone()).collect::<Vec<_>>(),
            [BTreeSet::from(["passkey".to_owned()])]
        );
    }

    #[test]
    fn an_acknowledgment_or_spending_record_that_does_not_read_is_unreadable() {
        let bad = row(
            1,
            "ak",
            "OwnerAcknowledged",
            object(&[("step_up", step_up("a1"))]),
        );
        assert_eq!(handed(&[bad]).err(), Some(Unreadable));
        let bad_step_up = row(
            1,
            "ak",
            "OwnerAcknowledged",
            object(&[("agent", s("agenta")), ("step_up", s("x"))]),
        );
        assert_eq!(handed(&[bad_step_up]).err(), Some(Unreadable));
        let bad_body = StoredEvent {
            body: b"not json".to_vec(),
            ..row(1, "cm", "OwnerCommandIssued", object(&[]))
        };
        assert_eq!(handed(&[bad_body]).err(), Some(Unreadable));
        let no_step_up = row(
            1,
            "cm",
            "OwnerCommandIssued",
            object(&[("step_up", Value::Null)]),
        );
        assert_eq!(handed(&[no_step_up]).map(|h| h.len()), Ok(0));
    }
}
