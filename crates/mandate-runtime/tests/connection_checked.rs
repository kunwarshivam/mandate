//! The runtime's fold of `ConnectionChecked` (journal spec §9.8, connections spec §8.1; R0 blocker
//! R14, DEC-969).
//!
//! The executor journals its permission checks on the account stream at a connect, at each
//! executor start, and daily. The runtime folds the latest check of its own deployment's
//! connection, and an opening is judged in the dry run as `account_restricted` (trading spec §7.3
//! row 5, the row a later check's failure reaches through `suspended`) while that check failed or
//! is a day old or older. Nothing here ever holds an exit, a protective order, or a kill switch
//! (`AGENTS.md` rule 13). A connection with no check folded is left to the other gates, as every
//! Phase 1 paper connection is. Every "refused" case is paired with the same step proceeding, so a
//! runtime that refuses everything passes none of them.
//!
//! The expected outcomes are computed by hand from the specs and, in the property, by an oracle
//! that reads the generated sequence rather than the runtime's state.

mod common;

use common::escalation::{
    ASKED_AT, Answer, Asked, asking_shell, mark, next_account_seq, next_control_seq, tail,
};
use common::{
    ACCOUNT_STREAM, AllowGate, CONNECTION, FixedPlan, Ran, Shell, TestIds, clock, event,
    fresh_output, int, object, ports, text, universe, with_clock,
};
use mandate_canon::Value;
use mandate_runtime::{Autonomy, FoldedEvent, Input, RuntimeError, RuntimeState, fold};
use proptest::prelude::*;

/// One day of risk clock: the daily check's period (connections spec §8.1), after which a check
/// no longer shows the connection is usable for an opening.
const DAY: i64 = 86_400;

/// The checks every `ConnectionChecked` lists (journal spec §9.8 rule 57), and an MCP
/// connection's `contract` beside them, ascending by bytes.
const CHECKS: [&str; 4] = ["account", "contract", "environment", "scope"];

fn result(check: &str, passed: bool) -> Value {
    if passed {
        object(&[
            ("check", text(check)),
            ("reason", Value::Null),
            ("result", text("passed")),
        ])
    } else {
        object(&[
            ("check", text(check)),
            ("reason", text(failure_reason(check))),
            ("result", text("failed")),
        ])
    }
}

fn failure_reason(check: &str) -> &'static str {
    match check {
        "account" => "account_unreadable",
        "contract" => "tools_missing",
        "environment" => "wrong_environment",
        _ => "scope_mismatch",
    }
}

/// The members of a check of `connection` whose `failing` checks failed and the rest passed.
fn check_members(connection: &str, occasion: &str, failing: &[&str]) -> Vec<(&'static str, Value)> {
    vec![
        ("account_pii_ref", text("pii_01J0000000000000000000000A")),
        ("connection_id", text(connection)),
        ("occasion", text(occasion)),
        (
            "results",
            Value::Array(
                CHECKS
                    .iter()
                    .map(|check| result(check, !failing.contains(check)))
                    .collect(),
            ),
        ),
    ]
}

/// A `ConnectionChecked` of `connection` at `at`, passing when `passed`, else failing `scope`.
fn check_of(seq: u64, connection: &str, passed: bool, at: i64) -> FoldedEvent {
    let failing: &[&str] = if passed { &[] } else { &["scope"] };
    event(
        ACCOUNT_STREAM,
        seq,
        "ConnectionChecked",
        with_clock(&check_members(connection, "daily", failing), at),
    )
}

/// The deployment's own connection's check.
fn check(seq: u64, passed: bool, at: i64) -> FoldedEvent {
    check_of(seq, CONNECTION, passed, at)
}

fn reconciliation(seq: u64, at: i64) -> FoldedEvent {
    event(
        ACCOUNT_STREAM,
        seq,
        "ReconciliationRun",
        with_clock(&[("result", text("clean"))], at),
    )
}

/// A started shell whose account stream holds a clean reconciliation and then `checks`.
fn started_with(checks: &[FoldedEvent], ports: &mandate_runtime::Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell
        .fold_one(&reconciliation(1, 0))
        .unwrap_or_else(|e| panic!("reconciliation: {e}"));
    for one in checks {
        shell
            .fold_one(one)
            .unwrap_or_else(|e| panic!("a well-formed ConnectionChecked folds (R14): {e}"));
    }
    shell.restart(ports).0
}

/// One evaluation at `at` with a fresh model output, as a cycle runs it.
fn evaluated(shell: &mut Shell, ports: &mandate_runtime::Ports<'_>, at: i64) -> Ran {
    shell.run(Input::ModelOutput(fresh_output(at)), ports);
    shell.run(Input::Tick(clock(at)), ports)
}

fn proposed(ran: &Ran) -> bool {
    ran.draft_types().contains(&"IntentProposed")
}

/// The dry run the evaluation's `DecisionMade` recorded, and its reason code.
fn decision(ran: &Ran) -> (Option<String>, Option<String>) {
    let made = ran
        .drafts
        .iter()
        .find(|d| d.event_type == "DecisionMade")
        .unwrap_or_else(|| panic!("the evaluation decided: {:?}", ran.draft_types()));
    let member = |key: &str| {
        made.payload
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    (member("dry_run"), member("reason_code"))
}

fn restricted(ran: &Ran) -> bool {
    decision(ran)
        == (
            Some("deny".to_owned()),
            Some("account_restricted".to_owned()),
        )
}

#[test]
fn a_passing_check_folds_and_an_opening_proceeds() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started_with(&[check(2, true, 100)], &ports);
    let ran = evaluated(&mut shell, &ports, 200);
    assert!(
        proposed(&ran) && ran.handed.len() == 1,
        "a passing check of the deployment's connection leaves the opening to the gate: {:?}",
        ran.draft_types()
    );
    assert_eq!(decision(&ran), (Some("allow".to_owned()), None));
}

#[test]
fn a_failed_check_refuses_an_opening_as_account_restricted() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started_with(&[check(2, false, 100)], &ports);
    let ran = evaluated(&mut shell, &ports, 200);
    assert!(
        !proposed(&ran) && ran.handed.is_empty(),
        "a failed check halts openings (connections spec §8.1 'failure later', §9.1): {:?}",
        ran.draft_types()
    );
    assert!(
        restricted(&ran),
        "the dry run records why, as trading spec §7.3 row 5 does: {:?}",
        decision(&ran)
    );
    assert!(
        ran.notifications.contains(&"decision_skipped"),
        "and the skip is notified as any dry-run deny is: {:?}",
        ran.notifications
    );
}

#[test]
fn every_failed_check_refuses_an_opening() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    for failing in CHECKS {
        let failed = event(
            ACCOUNT_STREAM,
            2,
            "ConnectionChecked",
            with_clock(&check_members(CONNECTION, "connect", &[failing]), 100),
        );
        let mut shell = started_with(&[failed], &ports);
        let ran = evaluated(&mut shell, &ports, 200);
        assert!(
            !proposed(&ran) && restricted(&ran),
            "a failed {failing} check refuses the opening: {:?}",
            ran.draft_types()
        );
    }
}

#[test]
fn a_failed_check_never_holds_an_exit() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::exiting(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started_with(&[check(2, false, 100)], &ports);
    let ran = evaluated(&mut shell, &ports, 200);
    assert!(
        proposed(&ran) && ran.handed.len() == 1,
        "risk reduction is never denied by the connection's check (AGENTS.md rule 13): {:?}",
        ran.draft_types()
    );
    let mut shell = started_with(&[check(2, true, 100)], &ports);
    let ran = evaluated(&mut shell, &ports, 100 + 5 * DAY);
    assert!(
        proposed(&ran),
        "nor by a stale one: {:?}",
        ran.draft_types()
    );
}

#[test]
fn a_check_a_day_old_refuses_an_opening() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started_with(&[check(2, true, 100)], &ports);
    let ran = evaluated(&mut shell, &ports, 100 + DAY - 1);
    assert!(
        proposed(&ran),
        "a check under a day old covers the opening: {:?}",
        ran.draft_types()
    );
    let mut shell = started_with(&[check(2, true, 100)], &ports);
    let ran = evaluated(&mut shell, &ports, 100 + DAY);
    assert!(
        !proposed(&ran) && restricted(&ran),
        "a check a day old no longer shows the connection usable (the daily check is due): {:?}",
        ran.draft_types()
    );
}

#[test]
fn the_latest_check_of_the_connection_governs() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started_with(&[check(2, false, 100), check(3, true, 150)], &ports);
    assert!(
        proposed(&evaluated(&mut shell, &ports, 200)),
        "a later passing check restores openings"
    );
    let mut shell = started_with(&[check(2, true, 100), check(3, false, 150)], &ports);
    let ran = evaluated(&mut shell, &ports, 200);
    assert!(
        !proposed(&ran) && restricted(&ran),
        "a later failed check halts them again: {:?}",
        ran.draft_types()
    );
    let mut shell = started_with(&[check(2, true, 100), check(3, true, 100 + DAY)], &ports);
    assert!(
        proposed(&evaluated(&mut shell, &ports, 100 + DAY + 1)),
        "a later passing check renews a stale one"
    );
}

#[test]
fn a_check_of_another_connection_governs_nothing_here() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started_with(&[check_of(2, "conn-2", false, 100)], &ports);
    assert!(
        proposed(&evaluated(&mut shell, &ports, 200)),
        "another connection's failure says nothing of this one"
    );
    let mut shell = started_with(
        &[check(2, false, 100), check_of(3, "conn-2", true, 150)],
        &ports,
    );
    let ran = evaluated(&mut shell, &ports, 200);
    assert!(
        !proposed(&ran) && restricted(&ran),
        "and another connection's pass never clears this one's failure: {:?}",
        ran.draft_types()
    );
}

#[test]
fn a_timestamp_risk_clock_dates_the_check() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let stamped = || {
        let mut members = check_members(CONNECTION, "connect", &[]);
        members.push(("risk_clock", text("1970-01-01T00:01:40.000000000Z")));
        event(ACCOUNT_STREAM, 2, "ConnectionChecked", object(&members))
    };
    let mut shell = started_with(&[stamped()], &ports);
    assert!(
        proposed(&evaluated(&mut shell, &ports, 100 + DAY - 1)),
        "the connect check's §4.7 timestamp (DEC-946 item 6) is second 100"
    );
    let mut shell = started_with(&[stamped()], &ports);
    assert!(
        restricted(&evaluated(&mut shell, &ports, 100 + DAY)),
        "and is a day old one day later"
    );
}

#[test]
fn a_grant_after_a_failed_check_is_skipped_and_one_after_a_pass_acts() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    for (passed, acts) in [(false, false), (true, true)] {
        let (mut shell, asked) = asking_shell(&ports, Some("155"));
        let checked = check(next_account_seq(&shell), passed, ASKED_AT + 10);
        tail(&mut shell, &checked, &ports);
        let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 20).event();
        let ran = tail(&mut shell, &grant, &ports);
        let revalidated = ran
            .drafts
            .iter()
            .find(|d| d.event_type == "ApprovalRevalidated")
            .unwrap_or_else(|| panic!("the grant is re-validated: {:?}", ran.draft_types()));
        let member = |key: &str| revalidated.payload.get(key).and_then(Value::as_str);
        assert_eq!(
            proposed(&ran),
            acts,
            "a grant read after a {} check: {:?}",
            if passed { "passing" } else { "failed" },
            ran.draft_types()
        );
        assert_eq!(ran.handed.len(), usize::from(acts));
        if acts {
            assert_eq!(member("result"), Some("act"));
        } else {
            assert_eq!(
                (
                    member("result"),
                    member("dry_run"),
                    member("dry_run_reason")
                ),
                (Some("skip"), Some("deny"), Some("account_restricted")),
                "check 11 skips the opening as the gate would (mandate spec §6.4)"
            );
        }
    }
}

/// A check at 100, an ask 30 seconds before it is a day old, and the owner's grant read 29 or 30
/// seconds later: the first is under a day after the check, the second a day after it.
#[test]
fn a_grant_read_a_day_after_the_check_is_skipped() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let asked_at = 100 + DAY - 30;
    for (read_after, acts) in [(29, true), (30, false)] {
        let mut shell = started_with(&[check(2, true, 100), mark(3, "155", 100)], &ports);
        let batch = evaluated(&mut shell, &ports, asked_at);
        let request = batch
            .drafts
            .iter()
            .find(|d| d.event_type == "ApprovalRequested")
            .cloned()
            .unwrap_or_else(|| panic!("the opening is asked: {:?}", batch.draft_types()));
        let content_hash = request
            .payload
            .get("content_hash")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| panic!("the request states its content hash"));
        let asked = Asked {
            approval: request.event_id.clone(),
            content_hash,
            request,
            batch,
        };
        let grant = Answer::grant(next_control_seq(&shell), &asked, asked_at + read_after).event();
        let ran = tail(&mut shell, &grant, &ports);
        assert_eq!(
            proposed(&ran),
            acts,
            "a grant read {read_after} s after an ask made {} s after the check: {:?}",
            DAY - 30,
            ran.draft_types()
        );
    }
}

#[test]
fn a_restart_replays_the_check_to_the_same_answer() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut live = started_with(&[], &ports);
    let failed = check(2, false, 100);
    tail(&mut live, &failed, &ports);
    let (mut replayed, _) = live.restart(&ports);
    let before = evaluated(&mut live, &ports, 200);
    let after = evaluated(&mut replayed, &ports, 200);
    assert!(restricted(&before) && restricted(&after));
    assert_eq!(
        before.draft_types(),
        after.draft_types(),
        "the check is state derived from the journal alone (ES-21)"
    );
}

#[test]
fn a_replay_of_the_same_events_folds_the_same_state() {
    let events = [
        reconciliation(1, 0),
        check(2, true, 100),
        check_of(3, "conn-2", false, 120),
        check(4, false, 150),
        check(5, true, 160),
    ];
    let mut first = RuntimeState::new(common::deployment());
    let mut second = RuntimeState::new(common::deployment());
    for one in &events {
        fold(&mut first, one).unwrap_or_else(|e| panic!("{e}"));
    }
    for one in &events {
        fold(&mut second, one).unwrap_or_else(|e| panic!("{e}"));
    }
    assert_eq!(
        first, second,
        "the fold is a function of the events (ES-21)"
    );
    let mut shorter = RuntimeState::new(common::deployment());
    for one in &events[..4] {
        fold(&mut shorter, one).unwrap_or_else(|e| panic!("{e}"));
    }
    assert_ne!(
        first, shorter,
        "and the latest check is part of the state, not discarded"
    );
}

fn refused(payload: Value) -> RuntimeError {
    let mut state = RuntimeState::new(common::deployment());
    fold(&mut state, &reconciliation(1, 0)).unwrap_or_else(|e| panic!("{e}"));
    fold(
        &mut state,
        &event(ACCOUNT_STREAM, 2, "ConnectionChecked", payload),
    )
    .expect_err("a malformed ConnectionChecked refuses rather than guesses (DEC-85)")
}

fn with_member(name: &'static str, value: Value) -> Value {
    let mut members = check_members(CONNECTION, "daily", &[]);
    members.push(("risk_clock", int(100)));
    members.retain(|(key, _)| *key != name);
    members.push((name, value));
    object(&members)
}

fn without_member(name: &str) -> Value {
    let mut members = check_members(CONNECTION, "daily", &[]);
    members.push(("risk_clock", int(100)));
    members.retain(|(key, _)| *key != name);
    object(&members)
}

fn results(items: Vec<Value>) -> Value {
    with_member("results", Value::Array(items))
}

#[test]
fn a_malformed_check_refuses_the_fold() {
    let three = |extra: Value| {
        vec![
            result("account", true),
            result("environment", true),
            result("scope", true),
            extra,
        ]
    };
    let cases = [
        (
            "no connection id",
            without_member("connection_id"),
            "connection_id",
        ),
        (
            "a numeric connection id",
            with_member("connection_id", int(7)),
            "connection_id",
        ),
        ("no occasion", without_member("occasion"), "occasion"),
        (
            "an unknown occasion",
            with_member("occasion", text("hourly")),
            "occasion",
        ),
        ("no results", without_member("results"), "results"),
        (
            "results not a list",
            with_member("results", text("passed")),
            "results",
        ),
        ("no risk clock", without_member("risk_clock"), "risk_clock"),
        (
            "a risk clock that is no timestamp",
            with_member("risk_clock", text("yesterday")),
            "risk_clock",
        ),
        (
            "a risk clock off the whole second",
            with_member("risk_clock", text("1970-01-01T00:01:40.500000000Z")),
            "risk_clock",
        ),
        (
            "an unknown check",
            results(three(result("uniqueness", true))),
            "results",
        ),
        (
            "an unknown result",
            results(vec![
                result("account", true),
                result("environment", true),
                object(&[
                    ("check", text("scope")),
                    ("reason", Value::Null),
                    ("result", text("skipped")),
                ]),
            ]),
            "results",
        ),
        (
            "a result that is no object",
            results(three(text("contract"))),
            "results",
        ),
        (
            "a failure with no reason",
            results(vec![
                result("account", true),
                result("environment", true),
                object(&[
                    ("check", text("scope")),
                    ("reason", Value::Null),
                    ("result", text("failed")),
                ]),
            ]),
            "results",
        ),
        (
            "a pass with a reason",
            results(vec![
                result("account", true),
                result("environment", true),
                object(&[
                    ("check", text("scope")),
                    ("reason", text("scope_mismatch")),
                    ("result", text("passed")),
                ]),
            ]),
            "results",
        ),
        (
            "no scope check",
            results(vec![result("account", true), result("environment", true)]),
            "results",
        ),
        (
            "no environment check",
            results(vec![result("account", true), result("scope", true)]),
            "results",
        ),
        (
            "no account check",
            results(vec![result("environment", true), result("scope", true)]),
            "results",
        ),
        ("no checks at all", results(Vec::new()), "results"),
    ];
    for (what, payload, field) in cases {
        let error = refused(payload);
        assert_eq!(
            error,
            RuntimeError::NonCanonicalPayload {
                field: field.to_owned()
            },
            "{what} refuses at {field}"
        );
    }
}

#[test]
fn a_check_without_contract_and_every_occasion_fold() {
    for occasion in [
        "connect",
        "reconnect",
        "reauthorize",
        "executor_start",
        "daily",
    ] {
        let mut members = check_members(CONNECTION, occasion, &[]);
        members.retain(|(key, _)| *key != "results");
        members.push((
            "results",
            Value::Array(vec![
                result("account", true),
                result("environment", true),
                result("scope", true),
            ]),
        ));
        let mut state = RuntimeState::new(common::deployment());
        fold(
            &mut state,
            &event(
                ACCOUNT_STREAM,
                1,
                "ConnectionChecked",
                with_clock(&members, 5),
            ),
        )
        .unwrap_or_else(|e| panic!("a {occasion} check of a non-MCP connection folds: {e}"));
    }
}

#[test]
fn the_other_connection_records_are_still_not_interpreted() {
    for kind in [
        "ConnectionStateChanged",
        "ConnectionCredentialRefreshed",
        "ConnectionCheckedSomehow",
    ] {
        let mut state = RuntimeState::new(common::deployment());
        let error = fold(
            &mut state,
            &event(ACCOUNT_STREAM, 1, kind, with_clock(&[], 5)),
        )
        .expect_err("no story here interprets it");
        assert_eq!(error.code(), "not_interpreted", "{kind}: {error}");
    }
}

/// One generated check: whose it is, whether it passed, and how many seconds after the previous
/// one it was journaled.
#[derive(Debug, Clone)]
struct Generated {
    own: bool,
    passed: bool,
    gap: i64,
}

fn generated() -> impl Strategy<Value = Generated> {
    (any::<bool>(), any::<bool>(), 0_i64..(2 * DAY)).prop_map(|(own, passed, gap)| Generated {
        own,
        passed,
        gap,
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Over any sequence of checks, of this connection and another, an opening is proposed exactly
    /// when no check of this connection was folded, or its latest one passed under a day before the
    /// evaluation; an exit is proposed whatever the checks say (rule 13). The oracle reads the
    /// generated sequence, never the runtime's state.
    #[test]
    fn openings_follow_the_latest_own_check_and_exits_never_do(
        sequence in proptest::collection::vec(generated(), 0..8),
        after in 0_i64..(2 * DAY),
    ) {
        let mut at = 1_i64;
        let mut events = Vec::new();
        let mut latest: Option<(bool, i64)> = None;
        for (index, one) in sequence.iter().enumerate() {
            at = at.saturating_add(one.gap);
            let seq = u64::try_from(index).unwrap_or(u64::MAX).saturating_add(2);
            let connection = if one.own { CONNECTION } else { "conn-2" };
            events.push(check_of(seq, connection, one.passed, at));
            if one.own {
                latest = Some((one.passed, at));
            }
        }
        let now = at.saturating_add(after);
        let expected = latest.is_none_or(|(passed, checked)| passed && now - checked < DAY);

        let (ids, gate, view) = (TestIds, AllowGate, universe(&["AAPL"]));
        let opening = FixedPlan::opening(Autonomy::Auto);
        let ports_open = ports(&ids, &gate, &opening, &view);
        let mut shell = started_with(&events, &ports_open);
        let ran = evaluated(&mut shell, &ports_open, now);
        prop_assert_eq!(proposed(&ran), expected, "{:?}", ran.draft_types());
        prop_assert_eq!(restricted(&ran), !expected);
        prop_assert_eq!(ran.handed.len(), usize::from(expected));

        let exiting = FixedPlan::exiting(Autonomy::Auto);
        let ports_exit = ports(&ids, &gate, &exiting, &view);
        let mut shell = started_with(&events, &ports_exit);
        let ran = evaluated(&mut shell, &ports_exit, now);
        prop_assert!(proposed(&ran), "an exit is never held: {:?}", ran.draft_types());
        prop_assert_eq!(ran.handed.len(), 1);
    }
}
