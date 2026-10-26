//! The account-stream records the runtime follows but does not act on (DEC-85: interpreted and
//! inert, never unknown). The executor and the control CLI write them on the account stream the
//! runtime folds (journal spec §9.8, §9.5):
//! - the copy of `ConnectionEstablished`, which `mandate connection record` appends;
//! - `AccountCloseRequested`, which an account kill switch journals before each close;
//! - `OwnerConfirmationReceived`, which an owner's exit journals beside its intent.
//!
//! The runtime reads the connection from the control stream, the kill switch from
//! `KillSwitchActivated`, and the exit's hand-off from `IntentReceived`, so each folds to exactly
//! the state an inert record at the same position folds to. Refusing one stopped every runtime
//! following that account at its start (lane DR's cell run, 2026-10-26).

mod common;

use common::{ACCOUNT_STREAM, event, int, object, text};
use mandate_canon::Value;
use mandate_runtime::{RuntimeState, fold};

/// The state after `StreamOpened` and then `event_type` with `payload` at sequence 2.
fn folded(event_type: &str, payload: Value) -> RuntimeState {
    let mut state = RuntimeState::new(common::deployment());
    let opened = object(&[
        ("account_ref", text("01J8Z3MACCOUNTREF000000001")),
        ("broker", text("alpaca")),
        ("stream_type", text("account")),
        ("workspace_id", text("ws1")),
    ]);
    fold(
        &mut state,
        &event(ACCOUNT_STREAM, 1, "StreamOpened", opened),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    fold(&mut state, &event(ACCOUNT_STREAM, 2, event_type, payload))
        .unwrap_or_else(|e| panic!("{event_type}: {e}"));
    state
}

/// The same position folded from a record the runtime already names as inert.
fn inert() -> RuntimeState {
    folded("AccountStateObserved", object(&[]))
}

#[test]
fn the_establishments_copy_folds_as_inert() {
    let copy = object(&[
        ("account_ref", text("01J8Z3MACCOUNTREF000000001")),
        ("broker", text("alpaca")),
        ("connection_id", text(common::CONNECTION)),
        ("environment", text("paper")),
        ("scopes", Value::Array(Vec::new())),
    ]);
    assert_eq!(folded("ConnectionEstablished", copy), inert());
}

#[test]
fn an_account_close_request_folds_as_inert() {
    let close = object(&[
        (
            "instrument_id",
            text("b0b6dd9d-8b9b-48a9-ba46-b9d54906e415"),
        ),
        ("kill_switch_id", text("01J8Z3MKILL00000000000000K")),
    ]);
    assert_eq!(folded("AccountCloseRequested", close), inert());
}

#[test]
fn an_owner_confirmation_folds_as_inert() {
    let confirmed = object(&[
        ("bid", text("501.25")),
        ("intent_id", text("01J8Z3MINTENT0000000000001")),
    ]);
    assert_eq!(folded("OwnerConfirmationReceived", confirmed), inert());
}

/// An account record of a type nothing names is still refused, never guessed at (DEC-85).
#[test]
fn an_unknown_account_record_is_still_refused() {
    let mut state = RuntimeState::new(common::deployment());
    let opened = object(&[("stream_type", text("account"))]);
    fold(
        &mut state,
        &event(ACCOUNT_STREAM, 1, "StreamOpened", opened),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        fold(
            &mut state,
            &event(ACCOUNT_STREAM, 2, "NoSuchRecord", object(&[("n", int(1))]))
        )
        .is_err()
    );
}
