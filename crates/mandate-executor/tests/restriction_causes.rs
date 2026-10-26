//! E7-2v slice A (journal spec v0.96 §9.5, rule 309; DEC-2816): every account-restriction cause in
//! force is journaled, the account state is the strictest restriction any cause in force set,
//! never the last record's, and a record that leaves the account state unchanged moves no agent
//! but still alerts the owner (trading spec §7.3).

mod common;

use common::{
    ACCOUNT_STREAM, FixedInstruments, FixedMandate, OTHER_AGENT, Shell, TestIds, agent,
    broker_account, broker_reject, config, event, ports, text, with_clock,
};
use mandate_canon::Value;
use mandate_executor::{AccountState, BrokerAccount, BrokerUpdate, Effect, Input, Mode};

/// A started executor on an opened stream, reconciled, as a production shell starts.
fn opened(ports: &mandate_executor::Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell
        .fold_one(&common::stream_opened())
        .expect("the stream opens");
    shell.restart_ready(ports)
}

fn blocked() -> Input {
    Input::BrokerUpdate(BrokerUpdate::Account(BrokerAccount {
        trading_blocked: true,
        ..broker_account()
    }))
}

fn closing() -> Input {
    Input::BrokerUpdate(BrokerUpdate::Reject(broker_reject(
        None,
        403,
        "account is closing transactions only",
    )))
}

/// The restrictions a run journaled, as `(restriction, cause)`.
fn restrictions(ran: &common::Ran) -> Vec<(String, String)> {
    ran.drafts
        .iter()
        .filter(|draft| draft.event_type == "AccountRestrictionChanged")
        .map(|draft| {
            let member = |name: &str| {
                draft
                    .payload
                    .get(name)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            (member("restriction"), member("cause"))
        })
        .collect()
}

fn alerts(ran: &common::Ran) -> usize {
    ran.effects
        .iter()
        .filter(|effect| matches!(effect, Effect::Notify(_)))
        .count()
}

fn modes(ran: &common::Ran) -> usize {
    ran.drafts
        .iter()
        .filter(|draft| draft.event_type == "AgentModeApplied")
        .count()
}

/// A reject's closing-only restriction while a notice blocks the account is a second cause in
/// force: journaled and alerted, but it moves no agent, and the account stays blocked, live and
/// after a replay from the stream alone.
#[test]
fn a_looser_cause_beside_a_block_is_journaled_and_alerted_and_leaves_the_account_blocked() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = opened(&ports);
    let first = shell.run(blocked(), &ports);
    assert_eq!(
        restrictions(&first),
        [("blocked".to_owned(), "broker_notice".to_owned())]
    );
    let second = shell.run(closing(), &ports);
    assert_eq!(
        restrictions(&second),
        [("closing_only".to_owned(), "broker_reject".to_owned())],
        "the reject is a cause in force of its own (DEC-2816 item 2)"
    );
    assert_eq!(
        modes(&second),
        0,
        "the account state is unchanged, so no agent moves"
    );
    assert_eq!(alerts(&second), 1, "the owner is told of the new cause");
    assert_eq!(shell.state.account_state(), AccountState::Blocked);
    assert_eq!(
        shell.state.effective_mode(&agent(OTHER_AGENT)),
        Mode::Paused
    );
    let (replayed, _) = shell.restart(&ports);
    assert_eq!(
        replayed.state.account_state(),
        AccountState::Blocked,
        "a replay takes the strictest cause in force, never the last record's"
    );
}

/// A block after a reject's closing-only restriction makes the account stricter: journaled, every
/// agent paused, the owner alerted.
#[test]
fn a_stricter_cause_after_a_restriction_blocks_the_account() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = opened(&ports);
    shell.run(closing(), &ports);
    let ran = shell.run(blocked(), &ports);
    assert_eq!(
        restrictions(&ran),
        [("blocked".to_owned(), "broker_notice".to_owned())]
    );
    assert_eq!(modes(&ran), 1);
    assert_eq!(alerts(&ran), 1);
    assert_eq!(shell.state.account_state(), AccountState::Blocked);
}

/// A cause already in force is not journaled again (rule 309): a second closing message, and a
/// second blocked account report, journal nothing and alert nothing.
#[test]
fn a_cause_already_in_force_is_journaled_once() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    for input in [closing, blocked] {
        let mut shell = opened(&ports);
        shell.run(input(), &ports);
        let again = shell.run(input(), &ports);
        assert!(restrictions(&again).is_empty(), "{:?}", again.draft_types());
        assert_eq!(alerts(&again), 0);
    }
}

/// A second cause at the restriction already in force leaves the account state unchanged: a
/// connection's closing-only restriction on the stream (E7-2z's writer, read back by a replay),
/// then a reject's, is journaled and alerted and moves no agent, since every agent already holds
/// the mode the account imposes.
#[test]
fn a_second_cause_at_the_same_restriction_moves_no_agent() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = opened(&ports);
    let seq = shell.head().0.saturating_add(1);
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            seq,
            "AccountRestrictionChanged",
            with_clock(
                &[
                    ("restriction", text("closing_only")),
                    ("cause", text("connection_unavailable")),
                    ("reason_code", text("account_restricted")),
                ],
                1,
            ),
        ))
        .expect("the connection's restriction folds");
    assert_eq!(shell.state.account_state(), AccountState::ClosingOnly);
    let ran = shell.run(closing(), &ports);
    assert_eq!(
        restrictions(&ran),
        [("closing_only".to_owned(), "broker_reject".to_owned())]
    );
    assert_eq!(
        modes(&ran),
        0,
        "the account state is unchanged, so no agent moves"
    );
    assert_eq!(alerts(&ran), 1);
}
