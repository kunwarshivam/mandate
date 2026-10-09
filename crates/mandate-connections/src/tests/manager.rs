//! The connection manager's start of a connect (connections spec §5.2 step 1, §9.1; journal spec
//! §9.8 `ConnectionRequested`, rule 131; DEC-693, DEC-694, DEC-697, DEC-699). Expected plans are
//! literal or built here from the inputs by hand; the digest's oracle is DEC-693 item 5's vector.

use std::fmt::Debug;

use mandate_canon::Digest;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

use super::support::at;
use crate::ConnectError;
use crate::manager::AccountRefFold::{CannotAnswer as RefUnknown, Unused, Used};
use crate::manager::IdFold::{CannotAnswer as IdUnknown, Established, Free, RequestOpen};
use crate::manager::{
    ManagerEffect, RequestMembers, StartFacts, StartPlan, StartRefusal, StartRequest, StepUpCheck,
    StepUpEvidence, connect_digest, plan_start,
};
use crate::record::Broker::{Alpaca, KrakenDerivativesUs, Robinhood};
use crate::record::Environment::{Live, Paper};
use crate::record::{AccountRef, Broker, ConnectionId};

const CONNECT_BYTES: &[u8] =
    br#"{"action":"connect","broker":"alpaca","environment":"paper","workspace_id":"ws_01"}"#;
const CONNECT_DIGEST: &str =
    "sha256:6aea75170af9b4fea7c631e4c5f2c5f9614eb223395f9ab80cd1d19c92f840d8";
const BROKERS: [Broker; 3] = [Alpaca, Robinhood, KrakenDerivativesUs];

/// The start that asks for `members`, with their step-up verified or not.
fn start_of(members: &RequestMembers, verified: bool) -> StartRequest {
    StartRequest {
        connection_id: members.connection_id.clone(),
        account_ref: members.account_ref.clone(),
        broker: members.broker,
        environment: members.environment,
        user: members.user.clone(),
        step_up: match verified {
            true => StepUpCheck::Verified(members.step_up.clone()),
            false => StepUpCheck::NotVerified,
        },
    }
}

fn request() -> RequestMembers {
    RequestMembers {
        connection_id: ConnectionId::new("conn_01").unwrap(),
        account_ref: AccountRef::new("01J9ZQ3V4X5Y6Z7A8B9C0D1E2F").unwrap(),
        broker: Alpaca,
        environment: Paper,
        user: "usr_01".to_owned(),
        step_up: StepUpEvidence {
            assertion_id: "01J9ZQ3V4X5Y6Z7A8B9C0D1E2G".to_owned(),
            authenticated_at: at(1_700_000_000),
            method: "passkey".to_owned(),
        },
    }
}

const CLEAR: StartFacts = StartFacts {
    id: Free,
    account_ref: Unused,
};

/// DEC-699 I5 and `AGENTS.md` rule 7: the crate's secrets are `secrecy` values, which are neither
/// `Eq` nor `Clone`, so a plan that is has no member a credential, code, or verifier could sit in.
fn holds_no_secret<T: Eq + Clone + Debug>(plan: T) -> T {
    plan
}

fn shown(digest: Digest) -> String {
    format!("sha256:{}", digest.to_hex())
}

#[test]
fn the_connect_vector_is_the_sha256_of_its_canonical_bytes() {
    assert_eq!(shown(Digest::of(CONNECT_BYTES)), CONNECT_DIGEST);
}

/// DEC-693 items 2 and 5: the connect digest is the vector, binds the workspace, and a non-paper
/// environment is refused for every broker, never digested.
#[test]
#[ignore = "pending E10-13"]
fn the_connect_digest_is_dec_693_s_vector_and_paper_only() {
    let paper = connect_digest("ws_01", Alpaca, Paper).unwrap();
    assert_eq!(shown(paper), CONNECT_DIGEST);
    let other = connect_digest("ws_02", Alpaca, Paper).unwrap();
    assert_ne!(shown(other), CONNECT_DIGEST);
    for broker in BROKERS {
        let live = connect_digest("ws_01", broker, Live);
        assert_eq!(live, Err(ConnectError::EnvironmentRefused), "{broker:?}");
    }
}

/// DEC-693 item 1 for any workspace and broker: the canonical object written out by hand here.
#[test]
#[ignore = "pending E10-13"]
fn every_connect_digest_binds_workspace_and_broker() {
    let inputs = (
        "[A-Za-z0-9_-]{1,64}",
        prop::sample::select(BROKERS.to_vec()),
    );
    TestRunner::deterministic()
        .run(&inputs, |(workspace, broker)| {
            let canonical = format!(
                r#"{{"action":"connect","broker":"{}","environment":"paper","workspace_id":"{workspace}"}}"#,
                broker.code()
            );
            let got = connect_digest(&workspace, broker, Paper)
                .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            prop_assert_eq!(got, Digest::of(canonical.as_bytes()));
            Ok(())
        })
        .unwrap();
}

/// Connections spec §5.2 step 1 and rule 131: exactly one `ConnectionRequested` with the start's
/// connection id, account ref, broker, environment, user, and step-up evidence.
#[test]
#[ignore = "pending E10-13"]
fn a_clear_start_plans_exactly_its_request() {
    let plan = plan_start(&start_of(&request(), true), &CLEAR).unwrap();
    let expected = StartPlan::Commit(vec![ManagerEffect::Requested(request())]);
    assert_eq!(holds_no_secret(plan), expected);
}

/// Each start precondition alone refuses, with its own reason, and commits nothing.
#[test]
#[ignore = "pending E10-13"]
fn a_start_is_refused_for_each_reason_alone() {
    let cases = [
        (Paper, false, Free, Unused, StartRefusal::StepUpNotVerified),
        (Live, true, Free, Unused, StartRefusal::EnvironmentRefused),
        (Paper, true, RequestOpen, Unused, StartRefusal::RequestOpen),
        (
            Paper,
            true,
            Established,
            Unused,
            StartRefusal::AlreadyEstablished,
        ),
        (Paper, true, Free, Used, StartRefusal::AccountRefUsed),
        (
            Paper,
            true,
            IdUnknown,
            Unused,
            StartRefusal::FoldCannotAnswer,
        ),
        (
            Paper,
            true,
            Free,
            RefUnknown,
            StartRefusal::FoldCannotAnswer,
        ),
    ];
    for (environment, verified, id, account_ref, refusal) in cases {
        let facts = StartFacts { id, account_ref };
        let input = start_of(
            &RequestMembers {
                environment,
                ..request()
            },
            verified,
        );
        let plan = plan_start(&input, &facts).unwrap();
        assert_eq!(
            plan,
            StartPlan::Refused(refusal),
            "{environment:?} {facts:?}"
        );
    }
}

fn members() -> impl Strategy<Value = RequestMembers> {
    (
        "[A-Za-z0-9_-]{1,64}",
        "[0-7][0-9A-HJKMNP-TV-Z]{25}",
        prop::sample::select(BROKERS.to_vec()),
        prop::sample::select(vec![Paper, Live]),
        "[a-z0-9_]{1,16}",
        ("[0-9A-Z]{26}", 0..4_000_000_000_i64, "passkey|cli_confirm"),
    )
        .prop_map(
            |(id, account_ref, broker, environment, user, (assertion, secs, method))| {
                RequestMembers {
                    connection_id: ConnectionId::new(&id).unwrap(),
                    account_ref: AccountRef::new(&account_ref).unwrap(),
                    broker,
                    environment,
                    user,
                    step_up: StepUpEvidence {
                        assertion_id: assertion,
                        authenticated_at: at(secs),
                        method,
                    },
                }
            },
        )
}

fn start_inputs() -> impl Strategy<Value = (RequestMembers, bool, StartFacts)> {
    let id = prop::sample::select(vec![Free, RequestOpen, Established, IdUnknown]);
    let account_ref = prop::sample::select(vec![Unused, Used, RefUnknown]);
    let facts = (id, account_ref).prop_map(|(id, account_ref)| StartFacts { id, account_ref });
    (members(), any::<bool>(), facts)
}

/// DEC-693 items 2 and 7, rule 131 (opening), DEC-697: for any start, the plan is exactly the
/// request repeating every member when no precondition fails, and otherwise a refusal for a
/// reason that holds; a fold that cannot answer, a live environment, or no step-up never commits.
/// The same members started clear (paper, verified, fresh) always commit exactly their request.
#[test]
#[ignore = "pending E10-13"]
fn a_start_commits_only_its_request_and_only_when_clear() {
    TestRunner::deterministic()
        .run(&start_inputs(), |(members, verified, facts)| {
            let input = start_of(&members, verified);
            let mut reasons = Vec::new();
            if !verified {
                reasons.push(StartRefusal::StepUpNotVerified);
            }
            if input.environment != Paper {
                reasons.push(StartRefusal::EnvironmentRefused);
            }
            reasons.extend(match facts.id {
                Free => None,
                RequestOpen => Some(StartRefusal::RequestOpen),
                Established => Some(StartRefusal::AlreadyEstablished),
                IdUnknown => Some(StartRefusal::FoldCannotAnswer),
            });
            reasons.extend(match facts.account_ref {
                Unused => None,
                Used => Some(StartRefusal::AccountRefUsed),
                RefUnknown => Some(StartRefusal::FoldCannotAnswer),
            });
            let plan =
                plan_start(&input, &facts).map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            match plan {
                StartPlan::Commit(effects) => {
                    prop_assert!(reasons.is_empty(), "committed despite {reasons:?}");
                    prop_assert_eq!(effects, vec![ManagerEffect::Requested(members.clone())]);
                }
                StartPlan::Refused(refusal) => {
                    prop_assert!(reasons.contains(&refusal), "{refusal:?} for {reasons:?}");
                }
            }
            let paper = RequestMembers {
                environment: Paper,
                ..members
            };
            let clear = plan_start(&start_of(&paper, true), &CLEAR)
                .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            prop_assert_eq!(
                clear,
                StartPlan::Commit(vec![ManagerEffect::Requested(paper)])
            );
            Ok(())
        })
        .unwrap();
}
