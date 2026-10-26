//! The executor's fold of its own `ConnectionChecked` (journal spec §9.8; R0 blocker R14,
//! DEC-969).
//!
//! The record is the result of the executor's own permission checks. It folds as a record only:
//! a later check's halt is the `AccountRestrictionChanged` and `AgentModeApplied` the executor
//! commits before it (journal spec §9.8 "The halt is never behind one of these records"), so the
//! fold must accept it on a replay and must still read its risk clock like every account event.

mod common;

use common::{ACCOUNT_STREAM, event, object, scope, stream_opened, text, with_clock};
use mandate_canon::Value;
use mandate_executor::{ExecutorState, Seq, fold};

fn check_payload() -> Vec<(&'static str, Value)> {
    vec![
        ("account_pii_ref", text("pii_01J0000000000000000000000A")),
        ("connection_id", text("conn-1")),
        ("occasion", text("connect")),
        (
            "results",
            Value::Array(
                ["account", "contract", "environment", "scope"]
                    .iter()
                    .map(|check| {
                        object(&[
                            ("check", text(check)),
                            ("reason", Value::Null),
                            ("result", text("passed")),
                        ])
                    })
                    .collect(),
            ),
        ),
    ]
}

#[test]
fn the_connect_check_folds_after_the_stream_opens() {
    let mut state = ExecutorState::new(scope());
    fold(&mut state, &stream_opened()).expect("seq 1 folds");
    let checked = event(
        ACCOUNT_STREAM,
        2,
        "ConnectionChecked",
        with_clock(&check_payload(), 5),
    );
    fold(&mut state, &checked)
        .unwrap_or_else(|e| panic!("the executor's own check folds on a replay (R14): {e}"));
    assert_eq!(state.head(ACCOUNT_STREAM), Some(Seq(2)));
}

#[test]
fn a_check_with_a_timestamp_risk_clock_folds() {
    let mut state = ExecutorState::new(scope());
    fold(&mut state, &stream_opened()).expect("seq 1 folds");
    let mut members = check_payload();
    members.push(("risk_clock", text("2026-10-19T15:00:00.000000000Z")));
    fold(
        &mut state,
        &event(ACCOUNT_STREAM, 2, "ConnectionChecked", object(&members)),
    )
    .unwrap_or_else(|e| panic!("DEC-946's check, as the runner writes it, folds: {e}"));
}

#[test]
fn a_check_with_no_risk_clock_refuses() {
    let mut state = ExecutorState::new(scope());
    fold(&mut state, &stream_opened()).expect("seq 1 folds");
    let error = fold(
        &mut state,
        &event(
            ACCOUNT_STREAM,
            2,
            "ConnectionChecked",
            object(&check_payload()),
        ),
    )
    .expect_err("every account event carries its risk clock");
    assert_eq!(error.code(), "risk_clock_missing", "{error}");
}

#[test]
fn the_credential_refresh_record_is_still_not_interpreted() {
    let mut state = ExecutorState::new(scope());
    fold(&mut state, &stream_opened()).expect("seq 1 folds");
    let error = fold(
        &mut state,
        &event(
            ACCOUNT_STREAM,
            2,
            "ConnectionCredentialRefreshed",
            with_clock(&[], 5),
        ),
    )
    .expect_err("no story here writes or reads it yet");
    assert_eq!(error.code(), "not_interpreted", "{error}");
}
