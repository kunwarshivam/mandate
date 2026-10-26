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
use mandate_journal::{StreamType, stream_event_types};
use mandate_runtime::{RuntimeError, RuntimeState, fold};

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

/// The account-stream types the journal admits that no writer emits there yet, which the runtime
/// still refuses until a story decides what they mean to it: whether a suspended connection or a
/// rotated credential should hold its openings, say (connection_checked.rs pins two of them).
const REFUSED_UNTIL_INTERPRETED: [&str; 4] = [
    "ConnectionCredentialRefreshed",
    "ConnectionCredentialRotated",
    "ConnectionStateChanged",
    "OwnerAcknowledgmentRequested",
];

/// Every type the journal admits on the streams the runtime folds, its account and agent streams,
/// has an arm in its fold (DEC-85), but [`REFUSED_UNTIL_INTERPRETED`]: none other is refused as not
/// interpreted. A type a writer adds to the catalogue without an arm here would stop every runtime
/// following that stream at its start, so this reads the catalogue's own list
/// (`mandate_journal::stream_event_types`). A malformed payload may still be refused; only "not
/// interpreted" fails it, and each named refusal is checked to be refused still.
#[test]
fn every_type_the_runtime_folds_a_stream_of_has_an_arm() {
    let mut missing = Vec::new();
    for (stream, kind) in [
        (ACCOUNT_STREAM, StreamType::Account),
        (common::AGENT_STREAM, StreamType::Agent),
    ] {
        for event_type in stream_event_types(kind) {
            if event_type == "StreamOpened" {
                continue;
            }
            let mut state = RuntimeState::new(common::deployment());
            let opened = object(&[("stream_type", text("account"))]);
            fold(&mut state, &event(stream, 1, "StreamOpened", opened))
                .unwrap_or_else(|e| panic!("{e}"));
            if let Err(RuntimeError::NotInterpreted { .. }) =
                fold(&mut state, &event(stream, 2, event_type, object(&[])))
            {
                missing.push(format!("{stream}: {event_type}"));
            }
        }
    }
    let mut expected: Vec<String> = REFUSED_UNTIL_INTERPRETED
        .iter()
        .map(|event_type| format!("{ACCOUNT_STREAM}: {event_type}"))
        .collect();
    expected.sort();
    missing.sort();
    assert_eq!(
        missing, expected,
        "the runtime's fold refuses exactly the named account types"
    );
}

/// The executor's account-stream record of a kill switch it applied (journal spec §9.5): `scope`,
/// `subject`, and the members the executor writes, with `initiator` an owner's.
fn executor_switch(scope: &str, subject: &str) -> Value {
    object(&[
        ("bid", Value::Null),
        ("bid_size", Value::Null),
        ("cancels", Value::Array(Vec::new())),
        ("closes", Value::Array(Vec::new())),
        ("copied_from", Value::Null),
        ("deferred", Value::Array(Vec::new())),
        ("floor", Value::Null),
        ("initiator", text("owner")),
        ("purpose", text("owner_exit")),
        ("scope", text(scope)),
        ("subject", text(subject)),
    ])
}

/// An agent-scope switch the executor applied to another agent on the same account is that agent's,
/// and folds here as inert: before, it stopped every runtime following the account, so killing one
/// agent stopped the others (lane DR's cell run on 2026-10-26, once the executor could journal it).
#[test]
fn another_agents_switch_on_the_account_stream_folds_as_inert() {
    let other = folded("KillSwitchActivated", executor_switch("agent", "agent-b"));
    assert_eq!(other, inert());
    assert_ne!(other.effective_mode(), mandate_runtime::Mode::Stopped);
}

/// The executor's record of a switch on this agent, or on the whole account, still stops it, as it
/// did before: the addressee check only ever spares another agent's.
#[test]
fn a_switch_on_this_agent_or_the_account_still_stops_it() {
    for (scope, subject) in [
        ("agent", common::AGENT),
        ("account", "01J8Z3MACCOUNTREF000000001"),
        ("workspace", "ws-other"),
        ("connection", "conn-other"),
    ] {
        let state = folded("KillSwitchActivated", executor_switch(scope, subject));
        assert_ne!(state, inert(), "{scope} {subject}");
        assert_eq!(
            state.effective_mode(),
            mandate_runtime::Mode::Stopped,
            "{scope} {subject}"
        );
    }
}

/// A switch naming no scope or no subject is never spared: the check spares only what it can read
/// as another agent's.
#[test]
fn a_switch_without_a_scope_or_a_subject_still_stops_it() {
    for missing in ["scope", "subject"] {
        let Value::Object(mut members) = executor_switch("agent", "agent-b") else {
            panic!("the switch is an object");
        };
        members.retain(|key, _| key.as_str() != missing);
        let state = folded("KillSwitchActivated", Value::Object(members));
        assert_eq!(
            state.effective_mode(),
            mandate_runtime::Mode::Stopped,
            "no {missing}"
        );
    }
}
