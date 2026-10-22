//! The executor exits `completed` only when the account owes nothing more (DEC-2347, DEC-2344
//! (A)): every agent it follows stopped, every kill switch it read applied, nothing in flight,
//! every order terminal, no agent holding lots, and the process ready. Anything else keeps it
//! running, which never adds risk.

mod support;

use mandate_process::{Exit, State};
use support::{
    ACCOUNT, ACCOUNT_STREAM, AGENT, CONNECTION, Error, Journal, NOON, OPENED, OPENING,
    OTHER_ACCOUNT, OTHER_AGENT, OTHER_CONNECTION, Paper, at, connect, deploy, deployed, exchanged,
    kill, kill_command, refused, requested, revoke, start, start_issued, stopped, tick,
};

const COMMAND: &str = "01JCMD000000000000000000AA";
const SWITCH: &str = "01JKILL00000000000000000AA";
const COMPLETED: State = State::Exited(Exit::Completed);

/// Item 3: its only agent stopped, nothing on the account, so the first pass exits `completed`,
/// status 0.
#[test]
#[ignore = "pending E7-4"]
fn a_stopped_agent_and_nothing_owed_exit_completed() -> Result<(), Error> {
    let journal = deployed()?;
    stopped(&journal, "c3", AGENT)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(process.lifecycle().state(), COMPLETED);
    assert_eq!(Exit::Completed.exit_status(), 0);
    Ok(())
}

/// Item 3.1: an agent not stopped is still desired, so the executor keeps running, ready, beside a
/// stopped one, pass after pass.
#[test]
fn a_desired_agent_keeps_it_running() -> Result<(), Error> {
    let journal = deployed()?;
    deploy(&journal, "c3", OTHER_AGENT, CONNECTION)?;
    stopped(&journal, "c4", OTHER_AGENT)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    for _ in 0..3 {
        process.step(at(NOON)?);
    }
    assert_eq!(process.lifecycle().state(), State::Ready);
    Ok(())
}

/// Item 3.2: the pass that reads a switch applies it and does not exit; the next pass, with
/// nothing left to read, does.
#[test]
#[ignore = "pending E7-4"]
fn a_switch_read_is_applied_before_the_exit() -> Result<(), Error> {
    let journal = deployed()?;
    stopped(&journal, "c3", AGENT)?;
    kill_command(&journal, COMMAND, "connection", CONNECTION)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(
        journal.applications(),
        [(AGENT.to_owned(), COMMAND.to_owned())]
    );
    assert_eq!(process.lifecycle().state(), State::Ready);
    process.step(at(NOON)?);
    assert_eq!(process.lifecycle().state(), COMPLETED);
    Ok(())
}

/// Item 3.4: the switch's cancel of the agent's working order, its answer lost, leaves the order
/// pending its cancel, which keeps the executor running; once the broker confirms it, a restart
/// reads the order canceled and exits.
#[test]
#[ignore = "pending E7-4"]
fn a_cancel_not_yet_confirmed_keeps_it_running() -> Result<(), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    stopped(&journal, "c3", AGENT)?;
    let broker = Paper::new(&journal);
    broker.resting(OPENING, "AAPL", "10", "150")?;
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(process.lifecycle().state(), State::Ready, "its order works");
    kill(&journal, AGENT, SWITCH)?;
    broker.lose_next_answers(1);
    for _ in 0..3 {
        process.step(at(NOON)?);
    }
    assert_eq!(journal.switched(), [AGENT]);
    assert_eq!(process.lifecycle().state(), State::Ready);
    broker.canceled(OPENING)?;
    let mut restarted = start(&journal, &broker)?;
    restarted.step(at(NOON)?);
    assert_eq!(restarted.lifecycle().state(), COMPLETED);
    Ok(())
}

/// Item 3.6: a process whose reconciliation found a position the journal does not explain is not
/// ready, and does not exit.
#[test]
fn a_process_not_ready_keeps_running() -> Result<(), Error> {
    let journal = deployed()?;
    stopped(&journal, "c3", AGENT)?;
    let broker = Paper::new(&journal);
    broker.holding("AAPL", "10")?;
    let mut process = start(&journal, &broker)?;
    for _ in 0..3 {
        process.step(at(NOON)?);
    }
    assert_eq!(process.lifecycle().state(), State::NotReady);
    Ok(())
}

/// Invariant 4: a restart after the exit applies a switch journaled since, then exits again.
#[test]
#[ignore = "pending E7-4"]
fn a_restart_after_the_exit_applies_a_later_switch_and_exits_again() -> Result<(), Error> {
    let journal = deployed()?;
    stopped(&journal, "c3", AGENT)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(process.lifecycle().state(), COMPLETED);
    kill_command(&journal, COMMAND, "connection", CONNECTION)?;
    let mut restarted = start(&journal, &broker)?;
    for _ in 0..2 {
        restarted.step(at(NOON)?);
    }
    assert_eq!(
        journal.applications(),
        [(AGENT.to_owned(), COMMAND.to_owned())]
    );
    assert_eq!(restarted.lifecycle().state(), COMPLETED);
    Ok(())
}

/// Item 3.1: an executor that follows no agent at all waits, ready, rather than exit.
#[test]
fn an_executor_following_no_agent_keeps_running() -> Result<(), Error> {
    let journal = Journal::default();
    connect(&journal, "c1", CONNECTION, ACCOUNT)?;
    tick(&journal, "t1", NOON)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    for _ in 0..3 {
        process.step(at(NOON)?);
    }
    assert_eq!(process.lifecycle().state(), State::Ready);
    Ok(())
}

/// Item 3.7 (lane D1's finding): an OAuth connect's executor, started `exchanged`, is wanted until
/// its connection closes, agent or none (DEC-2335). Its only agent stopped, it keeps running while
/// the connection is open, and exits once the connection is revoked.
#[test]
#[ignore = "pending E7-4"]
fn an_open_connect_keeps_it_running_until_its_connection_is_revoked() -> Result<(), Error> {
    let journal = Journal::default();
    requested(&journal, "c0", CONNECTION, ACCOUNT)?;
    exchanged(&journal, "c1", ACCOUNT)?;
    connect(&journal, "c2", CONNECTION, ACCOUNT)?;
    deploy(&journal, "c3", AGENT, CONNECTION)?;
    stopped(&journal, "c4", AGENT)?;
    tick(&journal, "t1", NOON)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    for _ in 0..3 {
        process.step(at(NOON)?);
    }
    assert_eq!(
        process.lifecycle().state(),
        State::Ready,
        "the connect is open"
    );
    revoke(&journal, "c5", CONNECTION, false)?;
    process.step(at(NOON)?);
    assert_eq!(process.lifecycle().state(), COMPLETED);
    Ok(())
}

/// Item 3.7: a connect refused closes its `exchanged` start too. Another connect of the account,
/// started after the stopped agent's, holds the executor until it is refused.
#[test]
#[ignore = "pending E7-4"]
fn a_refused_connect_lets_it_exit() -> Result<(), Error> {
    let journal = deployed()?;
    stopped(&journal, "c3", AGENT)?;
    requested(&journal, "c4", OTHER_CONNECTION, ACCOUNT)?;
    exchanged(&journal, "c5", ACCOUNT)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    for _ in 0..3 {
        process.step(at(NOON)?);
    }
    assert_eq!(
        process.lifecycle().state(),
        State::Ready,
        "the connect is open"
    );
    refused(&journal, "c6", OTHER_CONNECTION)?;
    process.step(at(NOON)?);
    assert_eq!(process.lifecycle().state(), COMPLETED);
    Ok(())
}

/// Item 3.1 with 3.7: a connect's executor with no agent never exits `completed`, connecting,
/// established, or after its connection is revoked; the manager stops it (DEC-2335 item 4).
#[test]
fn a_connects_executor_with_no_agent_never_exits_completed() -> Result<(), Error> {
    let journal = Journal::default();
    requested(&journal, "c0", CONNECTION, ACCOUNT)?;
    exchanged(&journal, "c1", ACCOUNT)?;
    tick(&journal, "t1", NOON)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    connect(&journal, "c2", CONNECTION, ACCOUNT)?;
    process.step(at(NOON)?);
    revoke(&journal, "c3", CONNECTION, false)?;
    process.step(at(NOON)?);
    assert_eq!(process.lifecycle().state(), State::Ready);
    Ok(())
}

/// Item 3.9 reads only this account's executor's `exchanged` start: another account's, a token
/// exchange's, and this executor's `deployed` start hold nothing.
#[test]
#[ignore = "pending E7-4"]
fn only_this_executors_exchanged_start_holds_it() -> Result<(), Error> {
    let journal = deployed()?;
    stopped(&journal, "c3", AGENT)?;
    start_issued(&journal, "c4", "executor", OTHER_ACCOUNT, "exchanged")?;
    start_issued(&journal, "c5", "token_exchange", ACCOUNT, "exchanged")?;
    start_issued(&journal, "c6", "executor", ACCOUNT, "deployed")?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(process.lifecycle().state(), COMPLETED);
    Ok(())
}
