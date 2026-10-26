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

/// The copy folds after the stream opens, and changes no account state: it only names the
/// connection whose state later records change (DEC-2826), so folding it before or after a
/// record-only check at the same clock leaves the same state.
#[test]
fn the_copy_folds_as_a_record_and_changes_no_account_state() {
    let origin = EventId("ctl:ws1-7".to_owned());
    let copy = |seq| {
        copied(
            ACCOUNT_STREAM,
            seq,
            "ConnectionEstablished",
            with_clock(&members(), 5),
            &origin,
        )
    };
    let check = |seq| {
        event(
            ACCOUNT_STREAM,
            seq,
            "ConnectionChecked",
            object(&[
                ("connection_id", text("conn-1")),
                ("occasion", text("connect")),
                ("results", Value::Array(Vec::new())),
                ("risk_clock", common::int(5)),
            ]),
        )
    };
    let mut state = opened();
    fold(&mut state, &copy(2)).unwrap_or_else(|e| panic!("the copy folds (rule 68): {e}"));
    assert_eq!(state.head(ACCOUNT_STREAM), Some(Seq(2)));
    fold(&mut state, &check(3)).expect("a check folds as a record");
    let mut record = opened();
    fold(&mut record, &check(2)).expect("a check folds as a record");
    fold(&mut record, &copy(3)).expect("the copy folds after a check");
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

/// A state change whose reason fits its `to` (rule 59), so each case fails only on what it names.
fn state_change(from: &str, to: &str, connection: &str) -> Value {
    let reason = match to {
        "suspended" => "credential_expired",
        "active" => "acknowledged",
        _ => "network_errors",
    };
    with_clock(
        &[
            ("connection_id", text(connection)),
            ("from", text(from)),
            ("reason", text(reason)),
            ("to", text(to)),
        ],
        5,
    )
}

fn copy_of(connection: &str, seq: u64) -> mandate_executor::FoldedEvent {
    let mut named = members();
    for member in &mut named {
        if member.0 == "connection_id" {
            member.1 = text(connection);
        }
    }
    copied(
        ACCOUNT_STREAM,
        seq,
        "ConnectionEstablished",
        with_clock(&named, 5),
        &EventId("ctl:ws1-7".to_owned()),
    )
}

/// One account stream holds one connection (rule 68): a copy naming another is refused, and a
/// reconnect's copy of the same one keeps the state the stream holds (CN-12), so a `suspended`
/// connection established again is still `suspended`.
#[test]
fn a_second_copy_keeps_the_state_and_another_connection_is_refused() {
    let mut state = opened();
    fold(&mut state, &copy_of("conn-1", 2)).expect("the copy folds");
    fold(
        &mut state,
        &event(
            ACCOUNT_STREAM,
            3,
            "ConnectionStateChanged",
            state_change("active", "suspended", "conn-1"),
        ),
    )
    .expect("into suspended");
    let error = fold(&mut state, &copy_of("conn-2", 4)).expect_err("another connection");
    assert_eq!(error.code(), "non_canonical_payload", "{error}");
    let mut state = opened();
    fold(&mut state, &copy_of("conn-1", 2)).expect("the copy folds");
    fold(
        &mut state,
        &event(
            ACCOUNT_STREAM,
            3,
            "ConnectionStateChanged",
            state_change("active", "suspended", "conn-1"),
        ),
    )
    .expect("into suspended");
    fold(&mut state, &copy_of("conn-1", 4)).expect("a reconnect's copy");
    let error = fold(
        &mut state,
        &event(
            ACCOUNT_STREAM,
            5,
            "ConnectionStateChanged",
            state_change("active", "degraded", "conn-1"),
        ),
    )
    .expect_err("still suspended after the reconnect");
    assert!(error.to_string().contains("from"), "{error}");
    fold(
        &mut state,
        &event(
            ACCOUNT_STREAM,
            5,
            "ConnectionStateChanged",
            state_change("suspended", "active", "conn-1"),
        ),
    )
    .expect("from the state the stream kept");
}

/// A state change folds only after the copy, for the connection it names and from the state the
/// stream holds (rule 68, rule 60, DEC-2826); any other is refused rather than folded into a
/// state the stream never recorded.
#[test]
fn a_state_change_folds_only_from_the_state_the_stream_holds() {
    let established = || {
        let mut state = opened();
        fold(
            &mut state,
            &copied(
                ACCOUNT_STREAM,
                2,
                "ConnectionEstablished",
                with_clock(&members(), 5),
                &EventId("ctl:ws1-7".to_owned()),
            ),
        )
        .expect("the copy folds");
        state
    };
    for (mut state, payload, field) in [
        (
            opened(),
            state_change("active", "degraded", "conn-1"),
            "connection_id",
        ),
        (
            established(),
            state_change("active", "degraded", "conn-2"),
            "connection_id",
        ),
        (
            established(),
            state_change("degraded", "suspended", "conn-1"),
            "from",
        ),
        (
            established(),
            state_change("active", "lost", "conn-1"),
            "to",
        ),
    ] {
        let seq = state.head(ACCOUNT_STREAM).map_or(1, |s| s.0) + 1;
        let error = fold(
            &mut state,
            &event(ACCOUNT_STREAM, seq, "ConnectionStateChanged", payload),
        )
        .expect_err("refused");
        assert_eq!(error.code(), "non_canonical_payload", "{field}: {error}");
        assert!(error.to_string().contains(field), "{field}: {error}");
    }
    let mut state = established();
    fold(
        &mut state,
        &event(
            ACCOUNT_STREAM,
            3,
            "ConnectionStateChanged",
            state_change("active", "degraded", "conn-1"),
        ),
    )
    .expect("from the state the stream holds");
    fold(
        &mut state,
        &event(
            ACCOUNT_STREAM,
            4,
            "ConnectionStateChanged",
            state_change("degraded", "suspended", "conn-1"),
        ),
    )
    .expect("from the state the last change recorded");
}
