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

use common::escalation::reconciliation;
use common::{
    ACCOUNT_STREAM, AGENT, AllowGate, FixedPlan, Shell, TestIds, event, ports, text, universe,
    with_clock,
};
use mandate_runtime::{FoldedEvent, Input, Mode, Purpose};
use proptest::prelude::*;

/// An `AgentModeApplied` on the account stream naming `agent`, moving it `to`.
fn applied(seq: u64, agent: &str, to: &str, at: i64) -> FoldedEvent {
    event(
        ACCOUNT_STREAM,
        seq,
        "AgentModeApplied",
        with_clock(
            &[
                ("agent", text(agent)),
                ("to", text(to)),
                ("restriction", text("reconciliation")),
                ("originated", mandate_canon::Value::Bool(true)),
            ],
            at,
        ),
    )
}

/// A started shell past its startup hold, then each record of `records` followed in turn.
fn after(records: &[(&str, &str)]) -> Shell {
    let (ids, gate, plan, view) = (TestIds, AllowGate, FixedPlan::silent(), universe(&["AAPL"]));
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell
        .fold_one(&reconciliation(1, 100))
        .unwrap_or_else(|e| panic!("reconciliation: {e}"));
    let (mut shell, _) = shell.restart(&ports);
    for (offset, (agent, to)) in records.iter().enumerate() {
        let seq = 2 + u64::try_from(offset).unwrap();
        let record = applied(seq, agent, to, 110 + i64::try_from(offset).unwrap());
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
    let record = applied(2, "*", "paused", 110);
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
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]

    /// Over any interleaving of this agent's, the account's and a sibling's records, the runtime's
    /// mode is the strictest of the last record naming this agent and the last naming `*`, as the
    /// executor's is; a sibling's records count for nothing.
    #[test]
    fn the_mode_is_the_strictest_of_this_agents_and_the_accounts(
        records in proptest::collection::vec(
            (
                prop_oneof![Just(AGENT), Just("*"), Just("agent-b")],
                prop_oneof![Just("normal"), Just("exits_only"), Just("paused"), Just("stopped")],
            ),
            1..10,
        ),
    ) {
        let last = |who: &str| {
            records
                .iter()
                .rev()
                .find(|(agent, _)| *agent == who)
                .map_or(0, |(_, to)| rank(to))
        };
        let want = mode_of(last(AGENT).max(last("*")));
        prop_assert_eq!(after(&records).state.effective_mode(), want);
    }
}
