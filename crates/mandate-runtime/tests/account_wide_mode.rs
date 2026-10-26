//! The executor's account-wide restriction, `AgentModeApplied` with `agent` `*` (journal spec §9.6,
//! trading spec §7.3; lane DR's follow-up to #2579). The executor writes it for an account kill
//! switch, an order status it cannot map, a fee-step pause, and a reconciliation mismatch no order
//! names, and holds every agent of the account in it: each agent's effective mode is the strictest
//! of the account's, `*`'s and its own (`ExecutorState::effective_mode`). The runtime folds `*` into
//! a slot of its own beside this agent's, so it never proposes what the executor would refuse, and
//! neither slot lifts the other: an account-wide `normal` never lifts this agent's own restriction,
//! nor this agent's `normal` the account's (`AGENTS.md` rules 1 and 3).
//!
//! The expected modes are computed by an oracle that reads only the generated records, never the
//! runtime's state.

mod common;

use std::collections::BTreeMap;

use common::escalation::reconciliation;
use common::{
    ACCOUNT_STREAM, AGENT, AllowGate, FixedPlan, Shell, TestIds, event, ports, text, universe,
    with_clock,
};
use mandate_runtime::{FoldedEvent, Input, Mode, Purpose};
use proptest::prelude::*;

/// An `AgentModeApplied` on the account stream naming `agent`, moving it `to` for `restriction`.
fn applied(seq: u64, (agent, to, restriction): (&str, &str, &str), at: i64) -> FoldedEvent {
    event(
        ACCOUNT_STREAM,
        seq,
        "AgentModeApplied",
        with_clock(
            &[
                ("agent", text(agent)),
                ("to", text(to)),
                ("restriction", text(restriction)),
                ("originated", mandate_canon::Value::Bool(true)),
            ],
            at,
        ),
    )
}

/// A started shell past its startup hold, then each record of `records` followed in turn, each
/// for the `reconciliation` restriction.
fn after(records: &[(&str, &str)]) -> Shell {
    let keyed: Vec<(&str, &str, &str)> = records
        .iter()
        .map(|(agent, to)| (*agent, *to, "reconciliation"))
        .collect();
    after_keyed(&keyed)
}

/// A started shell past its startup hold, then each `(agent, to, restriction)` record in turn.
fn after_keyed(records: &[(&str, &str, &str)]) -> Shell {
    let (ids, gate, plan, view) = (TestIds, AllowGate, FixedPlan::silent(), universe(&["AAPL"]));
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell
        .fold_one(&reconciliation(1, 100))
        .unwrap_or_else(|e| panic!("reconciliation: {e}"));
    let (mut shell, _) = shell.restart(&ports);
    for (offset, record) in records.iter().enumerate() {
        let seq = 2 + u64::try_from(offset).unwrap();
        let record = applied(seq, *record, 110 + i64::try_from(offset).unwrap());
        shell
            .fold_one(&record)
            .unwrap_or_else(|e| panic!("an AgentModeApplied folds: {e}"));
        shell.run(Input::Journal(record), &ports);
    }
    shell
}

#[test]
fn an_account_wide_restriction_holds_this_agent() {
    for (to, mode) in [
        ("exits_only", Mode::ExitsOnly),
        ("paused", Mode::Paused),
        ("stopped", Mode::Stopped),
    ] {
        let shell = after(&[("*", to)]);
        assert_eq!(shell.state.effective_mode(), mode, "{to}");
        assert!(!shell.state.permits(Purpose::Open), "{to}: no opening");
    }
    let shell = after(&[("*", "exits_only")]);
    assert!(
        shell.state.permits(Purpose::DiscretionaryExit),
        "an exit still goes (rule 13)"
    );
}

#[test]
fn the_runtime_journals_the_tightening_as_a_copy_of_the_account_record() {
    let (ids, gate, plan, view) = (TestIds, AllowGate, FixedPlan::silent(), universe(&["AAPL"]));
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell
        .fold_one(&reconciliation(1, 100))
        .unwrap_or_else(|e| panic!("reconciliation: {e}"));
    let (mut shell, _) = shell.restart(&ports);
    let record = applied(2, ("*", "paused", "fees"), 110);
    shell.fold_one(&record).unwrap_or_else(|e| panic!("{e}"));
    let ran = shell.run(Input::Journal(record.clone()), &ports);
    let copy = ran
        .drafts
        .iter()
        .find(|draft| draft.event_type == "AgentModeChanged")
        .unwrap_or_else(|| panic!("the tightening is journaled: {:?}", ran.draft_types()));
    assert_eq!(copy.causation_id.as_ref(), Some(&record.event_id));
    assert_eq!(
        copy.payload
            .get("to")
            .and_then(mandate_canon::Value::as_str),
        Some("paused")
    );
}

#[test]
fn neither_slot_lifts_the_other() {
    assert_eq!(
        after(&[(AGENT, "paused"), ("*", "normal")])
            .state
            .effective_mode(),
        Mode::Paused,
        "an account-wide normal never lifts this agent's own restriction"
    );
    assert_eq!(
        after(&[("*", "paused"), (AGENT, "normal")])
            .state
            .effective_mode(),
        Mode::Paused,
        "this agent's normal never lifts the account's"
    );
    assert_eq!(
        after(&[("*", "paused"), ("*", "normal")])
            .state
            .effective_mode(),
        Mode::Normal,
        "the account's own lift lifts it, as the executor's does"
    );
    assert_eq!(
        after(&[("*", "paused"), ("agent-b", "normal")])
            .state
            .effective_mode(),
        Mode::Paused,
        "a sibling's record moves neither"
    );
}

/// Each cause lifts only itself (trading spec §7.3), as the executor keys its restrictions by
/// agent and restriction: a later, looser record for another cause neither overwrites nor lifts
/// an earlier one (the review of #2584).
#[test]
fn a_lift_or_a_looser_record_moves_only_its_own_restriction() {
    for (records, want, why) in [
        (
            vec![
                ("*", "paused", "fees"),
                ("*", "exits_only", "broker_status_unmapped"),
            ],
            Mode::Paused,
            "a looser account-wide cause never overwrites a stricter one",
        ),
        (
            vec![
                ("*", "paused", "fees"),
                ("*", "exits_only", "external_activity"),
                ("*", "normal", "external_activity"),
            ],
            Mode::Paused,
            "lifting one account-wide cause leaves the other",
        ),
        (
            vec![("*", "paused", "fees"), ("*", "normal", "fees")],
            Mode::Normal,
            "lifting the only cause lifts it",
        ),
        (
            vec![
                (AGENT, "paused", "daily_loss"),
                (AGENT, "exits_only", "drawdown"),
            ],
            Mode::Paused,
            "the same holds for this agent's own causes",
        ),
        (
            vec![
                (AGENT, "paused", "daily_loss"),
                (AGENT, "exits_only", "drawdown"),
                (AGENT, "normal", "drawdown"),
            ],
            Mode::Paused,
            "lifting one of this agent's causes leaves the other",
        ),
        (
            vec![("*", "stopped", "kill_switch"), ("*", "normal", "fees")],
            Mode::Stopped,
            "a lift of another cause never lifts the account kill switch",
        ),
    ] {
        assert_eq!(after_keyed(&records).state.effective_mode(), want, "{why}");
    }
}

/// Under an account-wide `paused`, a restart re-hands only what the executor still takes in
/// `paused`: no opening (DEC-131 item 22, `AGENTS.md` rule 13).
#[test]
fn a_restart_under_an_account_wide_pause_re_hands_no_opening() {
    let shell = after_keyed(&[("*", "paused", "fees")]);
    assert!(!shell.state.permits_rehand(Purpose::Open));
    assert!(!shell.state.permits_rehand(Purpose::Increase));
    assert!(shell.state.permits_rehand(Purpose::Flatten));
}

#[test]
fn a_restart_folds_the_account_wide_restriction_again() {
    let (ids, gate, plan, view) = (TestIds, AllowGate, FixedPlan::silent(), universe(&["AAPL"]));
    let ports = ports(&ids, &gate, &plan, &view);
    let shell = after(&[("*", "exits_only")]);
    let (restarted, _) = shell.restart(&ports);
    assert_eq!(restarted.state.effective_mode(), Mode::ExitsOnly);
}

fn rank(mode: &str) -> u8 {
    match mode {
        "normal" => 0,
        "exits_only" => 1,
        "paused" => 2,
        _ => 3,
    }
}

fn mode_of(rank: u8) -> Mode {
    match rank {
        0 => Mode::Normal,
        1 => Mode::ExitsOnly,
        2 => Mode::Paused,
        _ => Mode::Stopped,
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// Over any interleaving of this agent's, the account's and a sibling's records for any of
    /// three causes, the runtime's mode is the executor's: each `(agent, restriction)` holds its
    /// last mode until a `normal` for it lifts it, and the mode is the strictest held for this
    /// agent or for `*`; a sibling's records count for nothing.
    #[test]
    fn the_mode_is_the_strictest_restriction_held_for_this_agent_or_the_account(
        records in proptest::collection::vec(
            (
                prop_oneof![Just(AGENT), Just("*"), Just("agent-b")],
                prop_oneof![Just("normal"), Just("exits_only"), Just("paused"), Just("stopped")],
                prop_oneof![Just("fees"), Just("reconciliation:cash"), Just("external_activity")],
            ),
            1..12,
        ),
    ) {
        let mut held: BTreeMap<(&str, &str), u8> = BTreeMap::new();
        for (agent, to, restriction) in &records {
            if *to == "normal" {
                held.remove(&(*agent, *restriction));
            } else {
                held.insert((*agent, *restriction), rank(to));
            }
        }
        let want = mode_of(
            held.iter()
                .filter(|((agent, _), _)| *agent == AGENT || *agent == "*")
                .map(|(_, rank)| *rank)
                .max()
                .unwrap_or(0),
        );
        prop_assert_eq!(after_keyed(&records).state.effective_mode(), want);
    }
}
