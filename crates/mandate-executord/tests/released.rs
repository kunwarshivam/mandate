//! DEC-2347 item 8 and DEC-2344 items 1 and 2 (E6-4): the owner's release takes the released
//! quantity off the agent's sub-ledger, so the retired account's executor exits, and a kill
//! switch's start of it exits once the switch is applied. Until E6-4 folds `PositionReleased`, an
//! account stream holding one is not interpreted, the executor never becomes ready, and its kill
//! switch is unapplied: the manager's crash loop raises `kill_switch_unapplied` (DEC-2344 item 1).

mod support;

use mandate_canon::Value;
use mandate_executor::BrokerRequest;
use mandate_process::{Exit, State};
use support::{
    ACCOUNT, ACCOUNT_STREAM, AGENT, CONNECTION, Error, NOON, OPENED, OPENING, Paper, at, deployed,
    kill_command, object, start, start_allowing, stopped,
};

const COMMAND: &str = "01JCMD000000000000000000AB";
const COMPLETED: State = State::Exited(Exit::Completed);

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

/// The agent's opening of 10 AAPL at 150, filled and journaled by an earlier executor process,
/// which holds the bracket's stop leg as its protection, the owner's release of `qty` of them
/// (mandate spec §3.1) with that leg cancelled, journaled as that process would, then the agent
/// retired, and, if `switched`, a kill switch of its connection.
fn released(qty: &str, switched: bool) -> Result<(support::Journal, Paper), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    broker.resting(OPENING, "AAPL", "10", "150")?;
    broker.fill(OPENING, "150")?;
    let mut earlier = start_allowing(&journal, &broker)?;
    earlier.step(at(NOON)?);
    if !earlier.ready() {
        return Err("the earlier executor never read the fill".into());
    }
    drop(earlier);
    let leg = journal
        .payloads("ProtectionChanged")
        .iter()
        .filter(|payload| payload.get("action").and_then(Value::as_str) == Some("placed"))
        .find_map(|payload| match payload.get("orders") {
            Some(Value::Array(orders)) => orders.first().and_then(Value::as_str).map(str::to_owned),
            _ => None,
        })
        .ok_or("the earlier executor never placed the bracket's stop leg")?;
    broker.resting(&leg, "AAPL", "10", "147")?;
    {
        journal.write(
            ACCOUNT_STREAM,
            "e9r0l0",
            "PositionReleased",
            1,
            Some("01JREMEASE0000000000000000"),
            object(vec![
                ("agent_id", text(AGENT)),
                ("canceled_order_ids", Value::Array(vec![text(&leg)])),
                ("cause", text("owner_release")),
                ("instrument", text("AAPL")),
                ("qty", text(qty)),
                ("risk_clock", text("2026-09-21T15:00:00.000000000Z")),
                (
                    "step_up",
                    object(vec![
                        ("assertion_id", text("sua_01")),
                        ("authenticated_at", text("2026-09-21T14:59:00.000000000Z")),
                        ("method", text("webauthn")),
                    ])?,
                ),
                ("user_id", text("usr_owner")),
                (
                    "warning_ref",
                    text("sha256:7777777777777777777777777777777777777777777777777777777777777777"),
                ),
            ])?,
        )?;
    }
    broker.canceled(&leg)?;
    stopped(&journal, "c3", AGENT)?;
    if switched {
        kill_command(&journal, COMMAND, "connection", CONNECTION)?;
    }
    Ok((journal, broker))
}

/// DEC-2347 item 8: once the owner releases the position, the 10 shares are the owner's, so the
/// retired account's executor exits.
#[test]
fn a_released_position_lets_the_retired_executor_exit() -> Result<(), Error> {
    let (journal, broker) = released("10", false)?;
    let mut process = start(&journal, &broker)?;
    for _ in 0..2 {
        process.step(at(NOON)?);
    }
    assert_eq!(process.lifecycle().state(), COMPLETED);
    Ok(())
}

/// DEC-2344 item 1 with DEC-2347 item 8: a connection's kill switch read after the release, by the
/// start the manager gives the retired account's executor for it, is applied to what the owner
/// holds there, and the executor then exits.
#[test]
fn a_released_position_lets_the_kill_switch_start_exit_once_the_switch_is_applied()
-> Result<(), Error> {
    let (journal, broker) = released("10", true)?;
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(
        journal.applications(),
        [(AGENT.to_owned(), COMMAND.to_owned())],
        "the switch is applied first"
    );
    process.step(at(NOON)?);
    assert_eq!(process.lifecycle().state(), COMPLETED);
    Ok(())
}

/// DEC-2347 items 3.6 and 8: a release of 4 of the 10 shares leaves 6 the agent's lots, so the
/// retired account's executor keeps running, ready, pass after pass, rather than exit. Its switch
/// would sell those 6 and never the owner's 4 (`mandate-executor`'s
/// `a_release_leaves_the_switch_only_what_the_agent_still_holds`).
#[test]
fn unreleased_lots_still_hold_the_retired_executor() -> Result<(), Error> {
    let (journal, broker) = released("4", false)?;
    let mut process = start(&journal, &broker)?;
    for _ in 0..3 {
        process.step(at(NOON)?);
        assert_eq!(process.lifecycle().state(), State::Ready);
    }
    Ok(())
}

/// The account stream's account-scope kill switch after the release, as the process that applied
/// it journals it (DEC-2333): every agent of the account stopped, then the switch closing
/// `closes`.
fn account_switched(journal: &support::Journal, closes: &[&str]) -> Result<(), Error> {
    let clock = "2026-09-21T15:00:00.000000000Z";
    journal.write(
        ACCOUNT_STREAM,
        "e9s0m0",
        "AgentModeApplied",
        1,
        None,
        object(vec![
            ("agent", text("*")),
            ("originated", Value::Bool(true)),
            ("restriction", text("kill_switch")),
            ("risk_clock", text(clock)),
            ("to", text("stopped")),
        ])?,
    )?;
    journal.write(
        ACCOUNT_STREAM,
        "e9s0k0",
        "KillSwitchActivated",
        1,
        Some("e9s0m0"),
        object(vec![
            ("bid", Value::Null),
            ("bid_size", Value::Null),
            ("cancels", Value::Array(Vec::new())),
            (
                "closes",
                Value::Array(closes.iter().map(|instrument| text(instrument)).collect()),
            ),
            ("copied_from", Value::Null),
            ("deferred", Value::Array(Vec::new())),
            ("floor", Value::Null),
            ("initiator", text("owner")),
            ("purpose", text("owner_exit")),
            ("risk_clock", text(clock)),
            ("scope", text("account")),
            ("subject", text(ACCOUNT)),
        ])?,
    )
}

/// DEC-2347 item 6 (DEC-2364): an account switch's close of the owner's 10 released shares that
/// the broker expires unfilled leaves them held, so the switch still owes their close, and the
/// executor keeps running, ready, pass after pass. No agent holds lots, no order works and the
/// broker lists none working, so only the switch's close holds it.
#[test]
fn an_account_switch_still_closing_keeps_the_executor_running() -> Result<(), Error> {
    let (journal, broker) = released("10", false)?;
    account_switched(&journal, &["AAPL"])?;
    broker.accept_account_wide(false);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert!(
        broker
            .seen()
            .iter()
            .any(|seen| matches!(seen.request, BrokerRequest::ClosePosition(..))),
        "the switch asked its close"
    );
    process.step(at(NOON)?);
    assert_eq!(process.lifecycle().state(), State::Ready, "its close works");
    broker.expire_closes();
    for _ in 0..3 {
        process.step(at(NOON)?);
        assert_eq!(process.lifecycle().state(), State::Ready);
    }
    Ok(())
}

/// DEC-2364: once the switch's close has sold the 10 shares and a read shows the account flat, the
/// switch owes nothing, and the executor exits.
#[test]
fn an_account_switch_whose_close_filled_lets_the_executor_exit() -> Result<(), Error> {
    let (journal, broker) = released("10", false)?;
    account_switched(&journal, &["AAPL"])?;
    broker.accept_account_wide(true);
    let mut process = start(&journal, &broker)?;
    for _ in 0..2 {
        process.step(at(NOON)?);
    }
    assert_eq!(process.lifecycle().state(), COMPLETED);
    Ok(())
}

/// DEC-2364: a close-position whose answer is lost, the close then expired unfilled, leaves the
/// switch's 10 shares held, so the executor keeps running, ready or not, pass after pass.
#[test]
fn an_account_switch_whose_close_answer_is_lost_keeps_the_executor_running() -> Result<(), Error> {
    let (journal, broker) = released("10", false)?;
    account_switched(&journal, &["AAPL"])?;
    broker.accept_account_wide(false);
    broker.lose_close_answers();
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert!(
        broker
            .seen()
            .iter()
            .any(|seen| matches!(seen.request, BrokerRequest::ClosePosition(..))),
        "the switch asked its close"
    );
    broker.expire_closes();
    for _ in 0..3 {
        process.step(at(NOON)?);
        assert!(!matches!(process.lifecycle().state(), State::Exited(_)));
    }
    Ok(())
}

/// DEC-2364: a switch whose close the broker refuses unsent leaves the 10 shares held. The
/// startup read is the switch's confirming read (DEC-2333 items 2 and 3), so the close is
/// requested at once; the refusal fails the cycle, a failed exit the manager restarts, never
/// `completed`.
#[test]
fn an_account_switch_whose_close_is_refused_unsent_never_exits_completed() -> Result<(), Error> {
    let (journal, broker) = released("10", false)?;
    account_switched(&journal, &["AAPL"])?;
    let mut process = start(&journal, &broker)?;
    for _ in 0..3 {
        process.step(at(NOON)?);
        assert_ne!(process.lifecycle().state(), COMPLETED);
    }
    assert_eq!(
        journal.payloads("AccountCloseRequested").len(),
        1,
        "the close was requested once"
    );
    Ok(())
}

/// DEC-2364 item 1: only an instrument the switch closes holds the exit. A switch that closes
/// nothing, journaled while the owner's 10 shares sat outside it, owes nothing, and the executor
/// exits with them still held.
#[test]
fn an_instrument_the_account_switch_does_not_close_never_holds_the_exit() -> Result<(), Error> {
    let (journal, broker) = released("10", false)?;
    account_switched(&journal, &[])?;
    broker.accept_account_wide(false);
    let mut process = start(&journal, &broker)?;
    for _ in 0..2 {
        process.step(at(NOON)?);
    }
    assert_eq!(process.lifecycle().state(), COMPLETED);
    Ok(())
}
