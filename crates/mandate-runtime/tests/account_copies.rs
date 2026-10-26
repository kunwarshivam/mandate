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

/// The state after `StreamOpened` and then each `AgentModeApplied` of `applied`, as `(agent,
/// restriction, to)`, at sequences 2 onward.
fn applied(applied: &[(&str, &str, &str)]) -> RuntimeState {
    let mut state = RuntimeState::new(common::deployment());
    let opened = object(&[("stream_type", text("account"))]);
    fold(
        &mut state,
        &event(ACCOUNT_STREAM, 1, "StreamOpened", opened),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    for (seq, (agent, restriction, to)) in (2..).zip(applied) {
        let payload = object(&[
            ("agent", text(agent)),
            ("originated", Value::Bool(true)),
            ("restriction", text(restriction)),
            ("to", text(to)),
        ]);
        fold(
            &mut state,
            &event(ACCOUNT_STREAM, seq, "AgentModeApplied", payload),
        )
        .unwrap_or_else(|e| panic!("{agent} {restriction} {to}: {e}"));
    }
    state
}

/// The executor's account-wide restriction (`agent` `*`: an account kill switch, or the every-agent
/// exits-only of external activity or a broker's closing-only reject) holds this runtime too, as it
/// holds the executor's binding gate, so the runtime stops proposing what the gate would refuse.
#[test]
fn an_account_wide_restriction_holds_this_runtime() {
    use mandate_runtime::Mode;
    assert_eq!(
        applied(&[("*", "external_activity", "exits_only")]).effective_mode(),
        Mode::ExitsOnly
    );
    assert_eq!(
        applied(&[("*", "kill_switch", "stopped")]).effective_mode(),
        Mode::Stopped
    );
    assert_eq!(
        applied(&[("agent-b", "kill_switch", "stopped")]).effective_mode(),
        Mode::Normal,
        "another agent's restriction is that agent's"
    );
}

/// Restrictions are kept per agent and restriction, as the executor keeps them, and the copied mode
/// is the strictest: an account-wide one never loosens this agent's own, and lifting one lifts only
/// that one.
#[test]
fn the_copied_mode_is_the_strictest_restriction_and_a_lift_lifts_only_its_own() {
    use mandate_runtime::Mode;
    let own_then_wide = applied(&[
        (common::AGENT, "drawdown", "stopped"),
        ("*", "external_activity", "exits_only"),
    ]);
    assert_eq!(own_then_wide.effective_mode(), Mode::Stopped);
    let wide_lifted = applied(&[
        (common::AGENT, "drawdown", "exits_only"),
        ("*", "external_activity", "stopped"),
        ("*", "external_activity", "normal"),
    ]);
    assert_eq!(wide_lifted.effective_mode(), Mode::ExitsOnly);
    let own_lifted = applied(&[
        ("*", "external_activity", "exits_only"),
        (common::AGENT, "drawdown", "stopped"),
        (common::AGENT, "drawdown", "normal"),
    ]);
    assert_eq!(own_lifted.effective_mode(), Mode::ExitsOnly);
}
