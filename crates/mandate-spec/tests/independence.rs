//! V-047 (mandate spec §4.1, DEC-411): when the workspace's policy requires independent approval, a
//! workspace of fewer than two active users is refused at validation, and again when a version is
//! applied, as V-002 is (`recheck_at_application`, DEC-428).
//!
//! The oracles never call the rule's own predicate. The lone workspaces are named as a set,
//! [`LONE`], and the property tests take the report with the policy off as the reading of every
//! other rule, so a V-047 that moved any other verdict fails. An absent membership is pinned through
//! the fold, which is the one place a count can be absent (#528 round 2: no case-shaped test can
//! catch a change to that default, because every case states the count).

mod common;

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use common::{base, s, with};
use mandate_canon::Digest;
use mandate_domain::Environment;
use mandate_num::Usd;
use mandate_spec::context::{AgentId, ContextArgs, JournaledFact, Membership};
use mandate_spec::document::{ConnectionId, ModelId, Pointer, ProvenanceMap, Source};
use mandate_spec::validate::{RegisteredModel, ValidationReport, recheck_at_application, validate};
use mandate_spec::{Mandate, SpecError, ValidationContext, Violation};
use mandate_time::Date;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

const OURS: &str = "conn_alpaca_paper_01";
const MODEL_HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";

/// The user counts V-047 refuses under the policy: none, and one. Written out, never derived from
/// `< 2`, so a boundary moved in the rule cannot move the oracle with it.
const LONE: [u32; 2] = [0, 1];

/// The rules §4.1 checks again when a version is applied: V-002 and V-047, and no other.
const AT_APPLICATION: [Violation; 2] = [Violation::V002, Violation::V047];

/// A property that drew no lone workspace under the policy would check nothing, so each counts the
/// cases that reach that branch and requires at least this many.
const REACHED_FLOOR: u32 = 100;

fn draft() -> Mandate {
    Mandate::parse(&base()).expect("the base parses")
}

/// The base with an end date, so a validation date past it breaks V-030 beside the rules under test.
fn ending() -> Mandate {
    Mandate::parse(&with("/goal/end_date", Some(s("2026-12-31")))).expect("the document parses")
}

fn usd(text: &str) -> Usd {
    Usd::parse(text).expect("a dollar amount")
}

fn date(text: &str) -> Date {
    Date::parse(text).expect("a date")
}

fn conn(id: &str) -> ConnectionId {
    ConnectionId::parse(id).expect("a connection id")
}

/// The base's context with nothing to refuse but what the test sets: equity enough for the base's
/// allocation, one approver, no registry (V-007 not checked).
fn context(workspace_users: u32, independent_approval_required: bool) -> ValidationContext {
    ValidationContext {
        account_equity_usd: usd("25000"),
        other_allocations_usd: Usd::ZERO,
        validation_date: date("2026-09-24"),
        registry: None,
        provenance: ProvenanceMap::default(),
        workspace_users,
        approver_users: 1,
        independent_approval_required,
        disclosures_accepted: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        connection_environment: Some(Environment::Paper),
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: BTreeSet::new(),
        previous_version: None,
    }
}

fn report(mandate: &Mandate, context: &ValidationContext) -> ValidationReport {
    validate(mandate, context).unwrap_or_else(|e| panic!("validate: {e:?}"))
}

/// A whole number of cents in the canonical decimal form `Usd::parse` takes: no trailing zero after
/// the point, and no point for whole dollars.
fn dollars(cents: u32) -> String {
    let text = format!("{}.{:02}", cents / 100, cents % 100);
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn rechecked(mandate: &Mandate, context: &ValidationContext) -> BTreeSet<Violation> {
    recheck_at_application(mandate, context)
        .unwrap_or_else(|e| panic!("recheck_at_application: {e:?}"))
}

fn codes(codes: &[Violation]) -> BTreeSet<Violation> {
    codes.iter().copied().collect()
}

/// The draft is agent `a` on our connection, on 2026-09-24, under the policy given.
fn args(membership: Option<(u32, u32)>, independent_approval_required: bool) -> ContextArgs {
    ContextArgs {
        agent: AgentId::new("a"),
        connection_id: conn(OURS),
        validation_date: date("2026-09-24"),
        membership: membership.map(|(workspace_users, approver_users)| Membership {
            workspace_users,
            approver_users,
        }),
        independent_approval_required,
        instrument_groups: BTreeMap::new(),
        eligibility_failures: BTreeSet::new(),
    }
}

/// Every fact the base needs to break no rule: equity, the connection's environment, its model, and
/// an owner who entered and confirmed the whole document.
fn journal(mandate: &Mandate) -> Vec<JournaledFact> {
    let version = mandate.version().expect("the mandate has a version");
    vec![
        JournaledFact::AccountSnapshot {
            connection_id: conn(OURS),
            equity_usd: usd("25000"),
        },
        JournaledFact::ConnectionEstablished {
            connection_id: conn(OURS),
            environment: Environment::Paper,
        },
        JournaledFact::ModelRegistered {
            id: ModelId::parse("quant.momentum").expect("a model id"),
            model: RegisteredModel {
                version: "1.0.0".to_owned(),
                content_hash: Digest::from_hex(MODEL_HASH).expect("a digest"),
                params: BTreeSet::from(["lookback_bars".to_owned()]),
                admits_instruments: false,
            },
        },
        JournaledFact::MandateVersionCreated {
            version,
            sources: BTreeMap::from([(Pointer::new(""), Source::UserEntered)]),
        },
        JournaledFact::MandateConfirmed {
            version,
            confirmed_paths: BTreeSet::from([Pointer::new("")]),
        },
    ]
}

fn folded(
    mandate: &Mandate,
    facts: &[JournaledFact],
    membership: Option<(u32, u32)>,
    required: bool,
) -> ValidationContext {
    ValidationContext::from_journal(mandate, args(membership, required), facts)
        .expect("the facts fold")
}

/// V-047 as §4.1 states it, on the base and at each count around the boundary: refused exactly when
/// the policy is on and the workspace has none or one user, and the rest of the report is the
/// report with the policy off. MC-V69 to MC-V71 are the same three shapes on the case file's base.
#[test]
#[ignore = "pending E10-1"]
fn v047_refuses_the_policy_in_a_workspace_of_fewer_than_two_users() {
    let mandate = draft();
    for users in [0, 1, 2, 3, u32::MAX] {
        let on = report(&mandate, &context(users, true));
        let off = report(&mandate, &context(users, false));
        let lone = LONE.contains(&users);
        assert_eq!(
            on.violations.contains(&Violation::V047),
            lone,
            "{users} users under the policy: {:?}",
            on.violations
        );
        assert!(
            !off.violations.contains(&Violation::V047),
            "{users} users without the policy: {:?}",
            off.violations
        );
        let mut without = on.violations.clone();
        without.remove(&Violation::V047);
        assert_eq!(
            (without, &on.warnings, &on.worst_case),
            (off.violations.clone(), &off.warnings, &off.worst_case),
            "{users} users: V-047 changed another verdict"
        );
    }
    assert_eq!(
        report(&mandate, &context(1, true)).violations,
        codes(&[Violation::V047]),
        "MC-V69's shape: the policy and one user is V-047 alone"
    );
    assert!(
        report(&mandate, &context(2, true)).is_valid(),
        "MC-V70's shape: two users pass V-047"
    );
    assert!(
        report(&mandate, &context(1, false)).is_valid(),
        "MC-V71's shape: one user without the policy is fine"
    );
}

/// V-047 reads the policy and the count and nothing else: over drawn contexts that break other rules
/// too (V-002, V-024, V-030), it fires exactly on the lone workspaces under the policy, and every
/// other code, warning, and figure is the report with the policy off.
#[test]
#[ignore = "pending E10-1"]
fn v047_reads_the_policy_and_the_count_and_moves_no_other_verdict() {
    let mut runner = TestRunner::new(ProptestConfig {
        cases: 1024,
        max_shrink_iters: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    });
    let draw = (
        0u32..5,
        any::<bool>(),
        0u32..3,
        prop_oneof![Just("0"), Just("15000"), Just("15000.01")],
        prop_oneof![Just("2026-09-24"), Just("2027-01-02")],
    );
    let mandate = ending();
    let reached = Cell::new(0u32);
    let outcome = runner.run(
        &draw,
        |(users, required, approvers, others, validated_on)| {
            let mut on = context(users, required);
            on.approver_users = approvers;
            on.other_allocations_usd = usd(others);
            on.validation_date = date(validated_on);
            let mut off = on.clone();
            off.independent_approval_required = false;
            let with =
                validate(&mandate, &on).map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            let without =
                validate(&mandate, &off).map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            let lone_under_policy = required && LONE.contains(&users);
            if lone_under_policy {
                reached.set(reached.get() + 1);
            }
            prop_assert_eq!(
                with.violations.contains(&Violation::V047),
                lone_under_policy,
                "{} users, policy {}: {:?}",
                users,
                required,
                &with.violations
            );
            let mut rest = with.violations.clone();
            rest.remove(&Violation::V047);
            prop_assert_eq!(&rest, &without.violations);
            prop_assert_eq!(&with.warnings, &without.warnings);
            prop_assert_eq!(&with.worst_case, &without.worst_case);
            Ok(())
        },
    );
    if let Err(failure) = outcome {
        panic!("V-047: {failure}");
    }
    assert!(
        reached.get() >= REACHED_FLOOR,
        "V-047 reached a lone workspace under the policy in {} cases, fewer than {REACHED_FLOOR}",
        reached.get()
    );
}

/// An absent membership is a count not known, which never counts as a second user (§4.1 V-047, rule
/// 3): folded with no membership, the policy is V-047, beside the V-024 no approver already gives.
/// With two users it is not, and the fold carries the policy it was given.
#[test]
#[ignore = "pending E10-1"]
fn an_absent_membership_is_no_second_user_under_the_policy() {
    let mandate = draft();
    let facts = journal(&mandate);
    let unknown = folded(&mandate, &facts, None, true);
    assert!(
        unknown.independent_approval_required,
        "the fold keeps the policy"
    );
    assert_eq!(
        report(&mandate, &unknown).violations,
        codes(&[Violation::V024, Violation::V047]),
        "no membership under the policy"
    );
    assert_eq!(
        rechecked(&mandate, &unknown),
        codes(&[Violation::V047]),
        "no membership at application under the policy"
    );
    let two = folded(&mandate, &facts, Some((2, 1)), true);
    assert!(
        report(&mandate, &two).is_valid(),
        "two users under the policy: {:?}",
        report(&mandate, &two).violations
    );
    assert!(
        report(&mandate, &folded(&mandate, &facts, None, false))
            .violations
            .eq(&codes(&[Violation::V024])),
        "no membership without the policy is V-024 alone"
    );
}

/// The apply-time case (#528 round 2, major 1): a version validated and confirmed in a two-user
/// workspace under the policy, whose second user is deactivated before the version is applied. The
/// recheck against the facts at application refuses it with V-047; with the second user still there,
/// or without the policy, it applies.
#[test]
#[ignore = "pending E10-1"]
fn a_second_user_deactivated_before_application_refuses_the_version() {
    let mandate = draft();
    let facts = journal(&mandate);
    let at_confirmation = folded(&mandate, &facts, Some((2, 1)), true);
    assert!(
        report(&mandate, &at_confirmation).is_valid(),
        "validated and confirmed with two users"
    );
    assert_eq!(
        rechecked(&mandate, &folded(&mandate, &facts, Some((1, 1)), true)),
        codes(&[Violation::V047]),
        "one user left at application"
    );
    assert_eq!(
        rechecked(&mandate, &at_confirmation),
        BTreeSet::new(),
        "both users still there at application"
    );
    assert_eq!(
        rechecked(&mandate, &folded(&mandate, &facts, Some((1, 1)), false)),
        BTreeSet::new(),
        "one user left, no policy"
    );
}

/// V-002's half of the recheck, atomically with the application: another agent's version applied
/// between confirmation and application leaves the account short, so the version is refused.
#[test]
#[ignore = "pending E10-1"]
fn another_allocation_applied_before_application_refuses_the_version() {
    let mandate = draft();
    let mut facts = journal(&mandate);
    let confirmed = folded(&mandate, &facts, Some((2, 1)), true);
    assert!(
        report(&mandate, &confirmed).is_valid(),
        "valid when confirmed"
    );
    let elsewhere = |allocation: &str| JournaledFact::AgentVersionActive {
        agent: AgentId::new("b"),
        connection_id: conn(OURS),
        environment: Environment::Paper,
        allocation_usd: usd(allocation),
        pinned: BTreeSet::new(),
    };
    facts.push(elsewhere("15000"));
    assert_eq!(
        rechecked(&mandate, &folded(&mandate, &facts, Some((2, 1)), true)),
        BTreeSet::new(),
        "$15,000 elsewhere and the base's $10,000 are exactly the $25,000 of equity"
    );
    facts.push(elsewhere("15000.01"));
    assert_eq!(
        rechecked(&mandate, &folded(&mandate, &facts, Some((2, 1)), true)),
        codes(&[Violation::V002]),
        "$15,000.01 elsewhere and the base's $10,000 exceed $25,000"
    );
    assert_eq!(
        rechecked(&mandate, &folded(&mandate, &facts, Some((1, 1)), true)),
        codes(&[Violation::V002, Violation::V047]),
        "both at once are both reported"
    );
}

/// The recheck reports exactly the apply-time rules: over drawn contexts that break other rules too,
/// it is V-002 when the allocations exceed equity (summed here, apart from the rule) and V-047 on a
/// lone workspace under the policy, and nothing else, V-024 and V-030 included.
#[test]
#[ignore = "pending E10-1"]
fn the_recheck_reports_v002_and_v047_and_no_other_rule() {
    let mut runner = TestRunner::new(ProptestConfig {
        cases: 1024,
        max_shrink_iters: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    });
    let draw = (
        0u32..5,
        any::<bool>(),
        0u32..3,
        prop_oneof![0u32..2_000_001, Just(1_500_000), Just(1_500_001)],
        prop_oneof![Just("2026-09-24"), Just("2027-01-02")],
    );
    let mandate = ending();
    let allocation_cents = 1_000_000u64;
    let equity_cents = 2_500_000u64;
    let reached = Cell::new(0u32);
    let outcome = runner.run(
        &draw,
        |(users, required, approvers, others_cents, validated_on)| {
            let mut ctx = context(users, required);
            ctx.approver_users = approvers;
            ctx.other_allocations_usd = Usd::parse(&dollars(others_cents))
                .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            ctx.validation_date = date(validated_on);
            let short = u64::from(others_cents) + allocation_cents > equity_cents;
            let lone_under_policy = required && LONE.contains(&users);
            if lone_under_policy {
                reached.set(reached.get() + 1);
            }
            let expected: BTreeSet<Violation> = AT_APPLICATION
                .into_iter()
                .zip([short, lone_under_policy])
                .filter_map(|(code, broken)| broken.then_some(code))
                .collect();
            let got = recheck_at_application(&mandate, &ctx)
                .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            prop_assert_eq!(got, expected);
            Ok(())
        },
    );
    if let Err(failure) = outcome {
        panic!("recheck_at_application: {failure}");
    }
    assert!(
        reached.get() >= REACHED_FLOOR,
        "the recheck reached a lone workspace under the policy in {} cases, fewer than {REACHED_FLOOR}",
        reached.get()
    );
}

/// The new code is the spec's, and the stub refuses rather than answers: until V-047 lands, the
/// policy on is `unimplemented`, never a report that leaves the rule out.
#[test]
fn v047_is_the_specs_code_and_the_policy_is_never_validated_without_it() {
    assert_eq!(Violation::V047.code(), "V-047");
    let mandate = draft();
    match validate(&mandate, &context(1, true)) {
        Err(SpecError::Unimplemented) => {}
        Ok(report) => assert!(
            report.violations.contains(&Violation::V047),
            "a report under the policy in a one-user workspace must carry V-047: {:?}",
            report.violations
        ),
        Err(other) => panic!("validate: {other:?}"),
    }
}
