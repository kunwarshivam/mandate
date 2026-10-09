//! The connection manager's start and finish of a connect (connections spec §5.2 steps 1, 5, and
//! 6, §9.1; journal spec §9.8 `ConnectionRequested`, rule 131; DEC-693, DEC-694, DEC-697, DEC-699,
//! DEC-884). Expected plans are literal or built here from the inputs by hand; the digest's oracle
//! is DEC-693 item 5's vector, and the refusal's oracle is [`JOURNAL_REASONS`], copied from journal
//! spec §9.8's reasons table.

use std::collections::BTreeSet;
use std::fmt::Debug;

use mandate_canon::Digest;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

use super::support::at;
use crate::ConnectError;
use crate::checks::Reason;
use crate::manager::AccountRefFold::{CannotAnswer as RefUnknown, Unused, Used};
use crate::manager::IdFold::{CannotAnswer as IdUnknown, Established, Free, RequestOpen};
use crate::manager::StartRefusal as R;
use crate::manager::{
    AccountRefFold, ConnectOutcome, FinishPlan, FinishRefusal, IdFold, ManagerEffect, RefusalCause,
    RefusedAs, RequestFold, RequestMembers, StartFacts, StartPlan, StartRequest, StepUpCheck,
    StepUpEvidence, connect_digest, plan_finish, plan_start,
};
use crate::record::Broker::{Alpaca, KrakenDerivativesUs, Robinhood};
use crate::record::Environment::{Live, Paper};
use crate::record::{AccountRef, Broker, ConnectionId, Environment};

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
fn a_clear_start_plans_exactly_its_request() {
    let plan = plan_start(&start_of(&request(), true), &CLEAR).unwrap();
    let expected = StartPlan::Commit(vec![ManagerEffect::Requested(request())]);
    assert_eq!(holds_no_secret(plan), expected);
}

/// Each start precondition alone refuses, with its own reason, and commits nothing. The route
/// serves Alpaca paper OAuth only (connections spec §5.2, DEC-821 item 7, DEC-883 item 2).
#[test]
fn a_start_is_refused_for_each_reason_alone() {
    let cases = [
        (Paper, Alpaca, false, Free, Unused, R::StepUpNotVerified),
        (Live, Alpaca, true, Free, Unused, R::EnvironmentRefused),
        (Paper, Robinhood, true, Free, Unused, R::BrokerRefused),
        (
            Paper,
            KrakenDerivativesUs,
            true,
            Free,
            Unused,
            R::BrokerRefused,
        ),
        (Paper, Alpaca, true, IdUnknown, Unused, R::FoldCannotAnswer),
        (Paper, Alpaca, true, Free, RefUnknown, R::FoldCannotAnswer),
        (Paper, Alpaca, true, RequestOpen, Unused, R::RequestOpen),
        (
            Paper,
            Alpaca,
            true,
            Established,
            Unused,
            R::AlreadyEstablished,
        ),
        (Paper, Alpaca, true, Free, Used, R::AccountRefUsed),
    ];
    for (environment, broker, verified, id, account_ref, refusal) in cases {
        let facts = StartFacts { id, account_ref };
        let asked = RequestMembers {
            environment,
            broker,
            ..request()
        };
        let plan = plan_start(&start_of(&asked, verified), &facts).unwrap();
        assert_eq!(plan, StartPlan::Refused(refusal), "{asked:?} {facts:?}");
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

/// DEC-883's order, by this test's own `if` chain: the first refusal that holds, or `None`.
fn first_refusal(start: &StartRequest, facts: StartFacts) -> Option<R> {
    if start.step_up == StepUpCheck::NotVerified {
        Some(R::StepUpNotVerified)
    } else if start.environment != Paper {
        Some(R::EnvironmentRefused)
    } else if start.broker != Alpaca {
        Some(R::BrokerRefused)
    } else if facts.id == IdUnknown || facts.account_ref == RefUnknown {
        Some(R::FoldCannotAnswer)
    } else if facts.id == RequestOpen {
        Some(R::RequestOpen)
    } else if facts.id == Established {
        Some(R::AlreadyEstablished)
    } else if facts.account_ref == Used {
        Some(R::AccountRefUsed)
    } else {
        None
    }
}

/// DEC-883 with DEC-693 items 2 and 7, rule 131 (opening) and DEC-697: for any members, over
/// every step-up, environment, broker, and fold answer, a start refuses with the first reason
/// that holds, in DEC-883's order, and otherwise commits exactly its request, every member
/// repeated. Only a verified, paper, Alpaca start with both folds answering clear commits.
#[test]
fn a_start_refuses_with_the_first_reason_or_commits_only_its_request() {
    TestRunner::deterministic()
        .run(&members(), |members| {
            for (verified, environment, broker, id, account_ref) in every_combination() {
                let asked = RequestMembers {
                    environment,
                    broker,
                    ..members.clone()
                };
                let start = start_of(&asked, verified);
                let facts = StartFacts { id, account_ref };
                let expected = match first_refusal(&start, facts) {
                    Some(refusal) => StartPlan::Refused(refusal),
                    None => StartPlan::Commit(vec![ManagerEffect::Requested(asked)]),
                };
                let plan = plan_start(&start, &facts)
                    .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
                prop_assert_eq!(plan, expected, "{:?}", facts);
            }
            Ok(())
        })
        .unwrap();
}

/// Every combination of step-up, environment, broker, id fold, and `account_ref` fold.
fn every_combination() -> Vec<(bool, Environment, Broker, IdFold, AccountRefFold)> {
    let mut all = Vec::new();
    for verified in [true, false] {
        for environment in [Paper, Live] {
            for broker in BROKERS {
                for id in [Free, RequestOpen, Established, IdUnknown] {
                    for account_ref in [Unused, Used, RefUnknown] {
                        all.push((verified, environment, broker, id, account_ref));
                    }
                }
            }
        }
    }
    all
}

/// Journal spec §9.8's reasons table, by hand: each executor reason with its check and code.
const JOURNAL_REASONS: [(Reason, &str, &str); 9] = [
    (Reason::ScopeMismatch, "scope", "scope_mismatch"),
    (Reason::FundMovement, "scope", "fund_movement"),
    (
        Reason::PermissionsUnreadable,
        "scope",
        "permissions_unreadable",
    ),
    (Reason::WrongEnvironment, "environment", "wrong_environment"),
    (Reason::ReachesBoth, "environment", "reaches_both"),
    (Reason::AccountUnreadable, "account", "account_unreadable"),
    (Reason::NotDedicated, "account", "not_dedicated"),
    (Reason::ToolsMissing, "contract", "tools_missing"),
    (Reason::ContractDrift, "contract", "contract_drift"),
];

/// Rule 54's teardowns: `check` is `null`, and the reason is one of these four.
fn teardowns() -> [(RefusalCause, &'static str); 4] {
    [
        (RefusalCause::Timeout, "timeout"),
        (RefusalCause::RestartPastDeadline, "restart_past_deadline"),
        (RefusalCause::ExecutorStopped, "executor_stopped"),
        (RefusalCause::StartFailed, "start_failed"),
    ]
}

fn other() -> ConnectionId {
    ConnectionId::new("conn_02").unwrap()
}

/// `ConnectionRefused`'s `check`, `reason`, and `existing_connection_id` for `cause`, read from
/// this file's tables, never from the crate's codes.
fn journal_as(cause: &RefusalCause) -> RefusedAs {
    let (check, reason, existing) = match cause {
        RefusalCause::Check(failed) => {
            let row = JOURNAL_REASONS.iter().find(|row| row.0 == *failed).unwrap();
            (Some(row.1), row.2, None)
        }
        RefusalCause::AccountMismatch => (Some("account"), "account_mismatch", None),
        RefusalCause::AlreadyConnected(id) => {
            (Some("uniqueness"), "already_connected", Some(id.clone()))
        }
        teardown => {
            let rows = teardowns();
            let row = rows.iter().find(|row| row.0 == *teardown).unwrap();
            (None, row.1, None)
        }
    };
    RefusedAs {
        check,
        reason,
        existing_connection_id: existing,
    }
}

/// Every refusal cause: the nine executor reasons, the manager's two, and the four teardowns.
fn every_cause(existing: &ConnectionId) -> Vec<RefusalCause> {
    let mut all: Vec<RefusalCause> = JOURNAL_REASONS
        .iter()
        .map(|row| RefusalCause::Check(row.0))
        .collect();
    all.push(RefusalCause::AccountMismatch);
    all.push(RefusalCause::AlreadyConnected(existing.clone()));
    all.extend(teardowns().into_iter().map(|row| row.0));
    all
}

fn scopes() -> BTreeSet<String> {
    BTreeSet::from(["data".to_owned(), "trading".to_owned()])
}

fn finish(fold: &RequestFold, outcome: &ConnectOutcome) -> FinishPlan {
    holds_no_secret((fold.clone(), outcome.clone()));
    holds_no_secret(plan_finish(fold, outcome).unwrap())
}

/// Connections spec §5.2 step 5 and rule 131 (closing): exactly one version-2
/// `ConnectionEstablished` repeating every member of the request, `account_ref` among them.
#[test]
fn a_passed_connect_plans_exactly_its_establishment() {
    let plan = finish(
        &RequestFold::Open(request()),
        &ConnectOutcome::Passed { scopes: scopes() },
    );
    let expected = FinishPlan::Commit(vec![ManagerEffect::Established {
        request: request(),
        scopes: scopes(),
    }]);
    assert_eq!(plan, expected);
}

/// Connections spec §5.2 steps 5 and 6, journal spec §9.8 rules 54, 55, and 131: each cause alone
/// closes the request with exactly one `ConnectionRefused` and its own check and reason.
#[test]
fn each_refusal_plans_exactly_its_connect_refused() {
    let none = None;
    let cases = [
        (
            RefusalCause::Check(Reason::ScopeMismatch),
            Some("scope"),
            "scope_mismatch",
        ),
        (
            RefusalCause::Check(Reason::ReachesBoth),
            Some("environment"),
            "reaches_both",
        ),
        (
            RefusalCause::Check(Reason::NotDedicated),
            Some("account"),
            "not_dedicated",
        ),
        (
            RefusalCause::Check(Reason::ToolsMissing),
            Some("contract"),
            "tools_missing",
        ),
        (
            RefusalCause::AccountMismatch,
            Some("account"),
            "account_mismatch",
        ),
        (RefusalCause::Timeout, none, "timeout"),
        (
            RefusalCause::RestartPastDeadline,
            none,
            "restart_past_deadline",
        ),
        (RefusalCause::ExecutorStopped, none, "executor_stopped"),
        (RefusalCause::StartFailed, none, "start_failed"),
    ];
    for (cause, check, reason) in cases {
        let plan = finish(
            &RequestFold::Open(request()),
            &ConnectOutcome::Refused(cause),
        );
        let refused = RefusedAs {
            check,
            reason,
            existing_connection_id: None,
        };
        let expected = FinishPlan::Commit(vec![ManagerEffect::ConnectRefused {
            request: request(),
            refused,
        }]);
        assert_eq!(plan, expected, "{reason}");
    }
    let taken = RefusalCause::AlreadyConnected(other());
    let plan = finish(
        &RequestFold::Open(request()),
        &ConnectOutcome::Refused(taken),
    );
    let refused = RefusedAs {
        check: Some("uniqueness"),
        reason: "already_connected",
        existing_connection_id: Some(other()),
    };
    let expected = FinishPlan::Commit(vec![ManagerEffect::ConnectRefused {
        request: request(),
        refused,
    }]);
    assert_eq!(plan, expected);
}

/// DEC-699 item 5 and DEC-884: a request a revoke closed plans nothing for any outcome, and no
/// open request, or a fold that cannot answer, refuses; neither ever carries an effect.
#[test]
fn a_request_that_is_not_open_plans_no_effect() {
    let mut outcomes = vec![ConnectOutcome::Passed { scopes: scopes() }];
    outcomes.extend(
        every_cause(&other())
            .into_iter()
            .map(ConnectOutcome::Refused),
    );
    for outcome in outcomes {
        let revoked = finish(&RequestFold::ClosedByRevoke, &outcome);
        assert_eq!(revoked, FinishPlan::ClosedByRevoke, "{outcome:?}");
        let closed = finish(&RequestFold::NotOpen, &outcome);
        assert_eq!(closed, FinishPlan::Refused(FinishRefusal::NotOpen));
        let unknown = finish(&RequestFold::CannotAnswer, &outcome);
        assert_eq!(
            unknown,
            FinishPlan::Refused(FinishRefusal::FoldCannotAnswer)
        );
    }
}

/// DEC-884 item 2 (rule 64) and rule 55: a live request is not established here but is still
/// refused and torn down, and `already_connected` naming the request itself is refused.
#[test]
fn a_live_establishment_and_a_self_conflict_are_refused() {
    let live = RequestMembers {
        environment: Live,
        ..request()
    };
    let passed = ConnectOutcome::Passed { scopes: scopes() };
    let plan = finish(&RequestFold::Open(live.clone()), &passed);
    assert_eq!(plan, FinishPlan::Refused(FinishRefusal::LiveNotServed));
    let plan = finish(
        &RequestFold::Open(live.clone()),
        &ConnectOutcome::Refused(RefusalCause::Timeout),
    );
    let refused = RefusedAs {
        check: None,
        reason: "timeout",
        existing_connection_id: None,
    };
    let expected = ManagerEffect::ConnectRefused {
        request: live,
        refused,
    };
    assert_eq!(plan, FinishPlan::Commit(vec![expected]));
    let itself = RefusalCause::AlreadyConnected(request().connection_id);
    let plan = finish(
        &RequestFold::Open(request()),
        &ConnectOutcome::Refused(itself),
    );
    assert_eq!(plan, FinishPlan::Refused(FinishRefusal::ExistingIsSelf));
}

/// DEC-884's order, by this test's own `match`, with [`journal_as`] for the refusal.
fn expected_finish(fold: &RequestFold, outcome: &ConnectOutcome) -> FinishPlan {
    let request = match fold {
        RequestFold::CannotAnswer => return FinishPlan::Refused(FinishRefusal::FoldCannotAnswer),
        RequestFold::NotOpen => return FinishPlan::Refused(FinishRefusal::NotOpen),
        RequestFold::ClosedByRevoke => return FinishPlan::ClosedByRevoke,
        RequestFold::Open(request) => request.clone(),
    };
    let effect = match outcome {
        ConnectOutcome::Passed { .. } if request.environment == Live => {
            return FinishPlan::Refused(FinishRefusal::LiveNotServed);
        }
        ConnectOutcome::Refused(RefusalCause::AlreadyConnected(id))
            if *id == request.connection_id =>
        {
            return FinishPlan::Refused(FinishRefusal::ExistingIsSelf);
        }
        ConnectOutcome::Passed { scopes } => ManagerEffect::Established {
            request,
            scopes: scopes.clone(),
        },
        ConnectOutcome::Refused(cause) => ManagerEffect::ConnectRefused {
            request,
            refused: journal_as(cause),
        },
    };
    FinishPlan::Commit(vec![effect])
}

/// DEC-697, DEC-699 items 4 and 5, DEC-884, rule 131 (closing): for any members, scopes, and
/// existing id, over every fold and every outcome, a finish is the oracle's plan: one effect
/// repeating every member, or a plan with none.
#[test]
fn a_finish_closes_its_request_once_or_plans_no_effect() {
    let scope_sets = prop::collection::btree_set("[a-z_]{1,12}", 0..4);
    let inputs = (members(), scope_sets, "[A-Za-z0-9_-]{1,8}");
    TestRunner::deterministic()
        .run(&inputs, |(members, scopes, existing)| {
            let folds = [
                RequestFold::Open(members.clone()),
                RequestFold::ClosedByRevoke,
                RequestFold::NotOpen,
                RequestFold::CannotAnswer,
            ];
            let mut outcomes = vec![ConnectOutcome::Passed { scopes }];
            let causes = every_cause(&ConnectionId::new(&existing).unwrap());
            outcomes.extend(causes.into_iter().map(ConnectOutcome::Refused));
            let itself = RefusalCause::AlreadyConnected(members.connection_id.clone());
            outcomes.push(ConnectOutcome::Refused(itself));
            for fold in &folds {
                for outcome in &outcomes {
                    let plan = plan_finish(fold, outcome)
                        .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
                    prop_assert_eq!(plan, expected_finish(fold, outcome), "{:?}", outcome);
                }
            }
            Ok(())
        })
        .unwrap();
}
