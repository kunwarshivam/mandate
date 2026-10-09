//! The platform-side revoke, ordinary and on a compromised credential (workspace API §4.5, §5.6;
//! connections spec §5.4, §9.1; DEC-693, DEC-694). Expected plans are literal effect lists and
//! expected digests are DEC-693 item 5's literal vectors, never computed by the code under test.

use std::collections::BTreeSet;

use mandate_canon::Digest;
use mandate_domain::AssetId;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

use crate::record::ConnectionId;
use crate::revoke::AgentsFold::{AllStopped, CannotAnswer, SomeNotStopped};
use crate::revoke::RevokeRefusal::{
    AgentsNotStopped, FoldCannotAnswer, PositionsHeld, StepUpNotVerified,
};
use crate::revoke::{
    AgentsFold, OrdinaryFacts, PositionsFold, RemainingPositions, RevokeEffect, RevokePlan,
    RevokeReason, StepUp, plan_compromised, plan_ordinary, remaining_positions, revoke_digest,
};

/// DEC-693 item 5, with workspace `ws_01` and connection `conn_01`.
const ORDINARY_BYTES: &[u8] =
    br#"{"compromised":false,"connection_id":"conn_01","workspace_id":"ws_01"}"#;
const ORDINARY_DIGEST: &str =
    "sha256:fde6740d31abaff7b41651470ffbfc60ab0cd853534383d877196441ff7417db";
const COMPROMISED_BYTES: &[u8] =
    br#"{"compromised":true,"connection_id":"conn_01","workspace_id":"ws_01"}"#;
const COMPROMISED_DIGEST: &str =
    "sha256:aece2f391435e53c580b832edbf832a4b3aa3a4d454fe144756ee7dbdb40fb9d";

fn conn(text: &str) -> ConnectionId {
    ConnectionId::new(text).unwrap()
}

fn asset(n: u8) -> AssetId {
    AssetId::parse(&format!("00000000-0000-4000-8000-0000000000{n:02x}")).unwrap()
}

fn shown(digest: Digest) -> String {
    format!("sha256:{}", digest.to_hex())
}

fn flat() -> PositionsFold {
    PositionsFold::Answered(BTreeSet::new())
}

fn facts(agents: AgentsFold, positions: PositionsFold) -> OrdinaryFacts {
    OrdinaryFacts { agents, positions }
}

fn kill_switch(id: &str) -> RevokeEffect {
    RevokeEffect::KillSwitch {
        connection_id: conn(id),
    }
}

fn revocation(id: &str, reason: RevokeReason) -> RevokeEffect {
    RevokeEffect::Revoke {
        connection_id: conn(id),
        reason,
    }
}

#[test]
fn the_worked_vectors_are_the_sha256_of_their_canonical_bytes() {
    assert_eq!(shown(Digest::of(ORDINARY_BYTES)), ORDINARY_DIGEST);
    assert_eq!(shown(Digest::of(COMPROMISED_BYTES)), COMPROMISED_DIGEST);
}

/// DEC-693 item 5's vectors: an absent `compromised` binds `false`, and the workspace is bound.
#[test]
fn the_revoke_digests_are_dec_693_s_vectors() {
    let digest = |workspace: &str, compromised| {
        shown(revoke_digest(workspace, &conn("conn_01"), compromised).unwrap())
    };
    assert_eq!(digest("ws_01", Some(false)), ORDINARY_DIGEST);
    assert_eq!(digest("ws_01", None), ORDINARY_DIGEST);
    assert_eq!(digest("ws_01", Some(true)), COMPROMISED_DIGEST);
    assert_ne!(digest("ws_02", Some(true)), COMPROMISED_DIGEST);
}

/// DEC-693 items 1, 3 and 4 for any ids: the digest is the SHA-256 of the canonical object written
/// out by hand here, so evidence for one workspace, connection, or path never fits another.
#[test]
fn every_revoke_digest_binds_workspace_connection_and_path() {
    let ids = (
        "[A-Za-z0-9_-]{1,64}",
        "[A-Za-z0-9_-]{1,64}",
        any::<Option<bool>>(),
    );
    TestRunner::deterministic()
        .run(&ids, |(workspace, connection, compromised)| {
            let id = conn(&connection);
            let bound = compromised.unwrap_or(false);
            let canonical = format!(
                r#"{{"compromised":{bound},"connection_id":"{connection}","workspace_id":"{workspace}"}}"#
            );
            let got = revoke_digest(&workspace, &id, compromised)
                .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            prop_assert_eq!(got, Digest::of(canonical.as_bytes()));
            let other_path = revoke_digest(&workspace, &id, Some(!bound))
                .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            prop_assert_ne!(got, other_path);
            Ok(())
        })
        .unwrap();
}

#[test]
fn an_ordinary_revoke_commits_one_owner_revocation_when_stopped_and_flat() {
    let clear = facts(AllStopped, flat());
    let plan = plan_ordinary(&conn("conn_01"), StepUp::Verified, &clear).unwrap();
    let expected = vec![revocation("conn_01", RevokeReason::Owner)];
    assert_eq!(plan, RevokePlan::Commit(expected));
}

/// Workspace API §4.5 and DEC-693 item 7, one reason at a time: a fold that cannot answer is
/// never read as "no positions" or "stopped".
#[test]
fn an_ordinary_revoke_is_refused_for_each_reason_alone() {
    let held = PositionsFold::Answered(BTreeSet::from([asset(1)]));
    let cases = [
        (StepUp::NotVerified, AllStopped, flat(), StepUpNotVerified),
        (StepUp::Verified, AllStopped, held, PositionsHeld),
        (StepUp::Verified, SomeNotStopped, flat(), AgentsNotStopped),
        (StepUp::Verified, CannotAnswer, flat(), FoldCannotAnswer),
        (
            StepUp::Verified,
            AllStopped,
            PositionsFold::CannotAnswer,
            FoldCannotAnswer,
        ),
    ];
    for (step_up, agents, positions, refusal) in cases {
        let plan = plan_ordinary(&conn("conn_01"), step_up, &facts(agents, positions)).unwrap();
        assert_eq!(plan, RevokePlan::Refused(refusal), "{step_up:?} {agents:?}");
    }
}

/// DEC-693 item 8: positions nothing authoritative answers for are "unknown", never none.
#[test]
fn remaining_positions_are_unknown_when_the_fold_cannot_answer() {
    let unknown = remaining_positions(&PositionsFold::CannotAnswer).unwrap();
    assert_eq!(unknown, RemainingPositions::Unknown);
    let none = remaining_positions(&flat()).unwrap();
    assert_eq!(none, RemainingPositions::Known(BTreeSet::new()));
    let held = BTreeSet::from([asset(1), asset(2)]);
    let known = remaining_positions(&PositionsFold::Answered(held.clone())).unwrap();
    assert_eq!(known, RemainingPositions::Known(held));
}

fn agents_fold() -> impl Strategy<Value = AgentsFold> {
    prop::sample::select(vec![AllStopped, SomeNotStopped, CannotAnswer])
}

fn positions_fold() -> impl Strategy<Value = PositionsFold> {
    prop_oneof![
        Just(PositionsFold::CannotAnswer),
        prop::collection::btree_set(any::<u8>().prop_map(asset), 0..4)
            .prop_map(PositionsFold::Answered),
    ]
}

fn step_up() -> impl Strategy<Value = StepUp> {
    prop_oneof![Just(StepUp::Verified), Just(StepUp::NotVerified)]
}

/// For any facts, an ordinary revoke commits exactly `[ConnectionRevoked (owner)]` on this
/// connection only when step-up is verified, every agent is stopped, and the folds answer "no
/// positions"; otherwise it is refused for a reason that holds, and a fold that cannot answer
/// always refuses. It never plans a kill switch.
#[test]
fn an_ordinary_revoke_commits_only_on_answered_clear_facts() {
    let inputs = (
        "[A-Za-z0-9_-]{1,64}",
        step_up(),
        agents_fold(),
        positions_fold(),
    );
    TestRunner::deterministic()
        .run(&inputs, |(id, step_up, agents, positions)| {
            let mut reasons = Vec::new();
            if step_up == StepUp::NotVerified {
                reasons.push(StepUpNotVerified);
            }
            match agents {
                AllStopped => {}
                SomeNotStopped => reasons.push(AgentsNotStopped),
                CannotAnswer => reasons.push(FoldCannotAnswer),
            }
            match &positions {
                PositionsFold::Answered(held) if held.is_empty() => {}
                PositionsFold::Answered(_) => reasons.push(PositionsHeld),
                PositionsFold::CannotAnswer => reasons.push(FoldCannotAnswer),
            }
            let plan = plan_ordinary(&conn(&id), step_up, &facts(agents, positions))
                .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            match plan {
                RevokePlan::Commit(effects) => {
                    prop_assert!(reasons.is_empty(), "committed despite {reasons:?}");
                    prop_assert_eq!(effects, vec![revocation(&id, RevokeReason::Owner)]);
                }
                RevokePlan::Refused(refusal) => {
                    prop_assert!(reasons.contains(&refusal), "{refusal:?} for {reasons:?}");
                }
                other @ RevokePlan::KillSwitchOnly { .. } => {
                    prop_assert!(false, "an ordinary revoke planned {other:?}");
                }
            }
            Ok(())
        })
        .unwrap();
}

/// Workspace API §5.6, API-7, DEC-158 option (c), journal spec §9.10 rule 84 and `AGENTS.md` rule
/// 13, for any connection: a compromised revoke is never refused. With verified step-up it is
/// exactly this connection's kill switch, then its compromised revocation, in one batch; without,
/// the kill switch still commits and only the revocation is refused.
#[test]
fn a_compromised_revoke_is_never_refused() {
    let inputs = ("[A-Za-z0-9_-]{1,64}", step_up());
    TestRunner::deterministic()
        .run(&inputs, |(id, step_up)| {
            let plan = plan_compromised(&conn(&id), step_up)
                .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            let expected = match step_up {
                StepUp::Verified => RevokePlan::Commit(vec![
                    kill_switch(&id),
                    revocation(&id, RevokeReason::Compromised),
                ]),
                StepUp::NotVerified => RevokePlan::KillSwitchOnly {
                    kill_switch: kill_switch(&id),
                    revocation: StepUpNotVerified,
                },
            };
            prop_assert_eq!(plan, expected);
            Ok(())
        })
        .unwrap();
}
