//! The executor's fold of its copy of `ConnectionEstablished` (journal spec §9.8, rule 68;
//! DEC-2341 item 2): a record only, which changes no account state, read with its risk clock
//! like every account event.

mod common;

use common::{ACCOUNT_STREAM, copied, event, object, scope, stream_opened, text, with_clock};
use mandate_canon::Value;
use mandate_executor::{EventId, ExecutorState, Seq, fold};

fn members() -> Vec<(&'static str, Value)> {
    vec![
        ("account_ref", text("01J8Z2ACCT0000000000000001")),
        ("broker", text("alpaca")),
        ("connection_id", text("conn-1")),
        ("environment", text("paper")),
        ("margin_attestation", Value::Null),
        ("scopes", Value::Array(vec![text("data"), text("trading")])),
        ("step_up", Value::Null),
        ("user", text("usr_1")),
    ]
}

fn opened() -> ExecutorState {
    let mut state = ExecutorState::new(scope());
    fold(&mut state, &stream_opened()).expect("seq 1 folds");
    state
}

/// The copy folds after the stream opens, and leaves the state exactly as a record-only event at
/// the same place and clock does: no account state changes.
#[test]
fn the_copy_folds_as_a_record_and_changes_no_account_state() {
    let origin = EventId("ctl:ws1-7".to_owned());
    let mut state = opened();
    let copy = copied(
        ACCOUNT_STREAM,
        2,
        "ConnectionEstablished",
        with_clock(&members(), 5),
        &origin,
    );
    fold(&mut state, &copy).unwrap_or_else(|e| panic!("the copy folds (rule 68): {e}"));
    assert_eq!(state.head(ACCOUNT_STREAM), Some(Seq(2)));
    let mut record = opened();
    let check = object(&[
        ("connection_id", text("conn-1")),
        ("occasion", text("connect")),
        ("results", Value::Array(Vec::new())),
        ("risk_clock", common::int(5)),
    ]);
    fold(
        &mut record,
        &event(ACCOUNT_STREAM, 2, "ConnectionChecked", check),
    )
    .expect("a check folds as a record");
    assert_eq!(state, record);
}

/// A copy with no risk clock is refused, as every account event is.
#[test]
fn a_copy_with_no_risk_clock_refuses() {
    let mut state = opened();
    let copy = copied(
        ACCOUNT_STREAM,
        2,
        "ConnectionEstablished",
        object(&members()),
        &EventId("ctl:ws1-7".to_owned()),
    );
    let error = fold(&mut state, &copy).expect_err("every account event carries its risk clock");
    assert_eq!(error.code(), "risk_clock_missing", "{error}");
}
