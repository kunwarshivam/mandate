//! DEC-2347 item 8 and DEC-2344 items 1 and 2 (E6-4): the owner's release takes the released
//! quantity off the agent's sub-ledger, so the retired account's executor exits, and a kill
//! switch's start of it exits once the switch is applied. Until E6-4 folds `PositionReleased`, an
//! account stream holding one is not interpreted, the executor never becomes ready, and its kill
//! switch is unapplied: the manager's crash loop raises `kill_switch_unapplied` (DEC-2344 item 1).

mod support;

use mandate_canon::Value;
use mandate_process::{Exit, State};
use support::{
    ACCOUNT_STREAM, AGENT, CONNECTION, Error, NOON, OPENED, OPENING, Paper, at, deployed,
    kill_command, object, start, start_allowing, stopped,
};

const COMMAND: &str = "01JCMD000000000000000000AB";
const COMPLETED: State = State::Exited(Exit::Completed);

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

/// The agent's opening of 10 AAPL at 150, filled and journaled by an earlier executor process,
/// the owner's release of all 10 (mandate spec §3.1), journaled as that process would, then the
/// agent retired, and, if `switched`, a kill switch of its connection.
fn released(switched: bool) -> Result<(support::Journal, Paper), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    broker.resting(OPENING, "AAPL", "10", "150")?;
    broker.fill(OPENING, "150")?;
    broker.resting("md-protect", "AAPL", "10", "147")?;
    let mut earlier = start_allowing(&journal, &broker)?;
    earlier.step(at(NOON)?);
    if !earlier.ready() {
        return Err("the earlier executor never read the fill".into());
    }
    drop(earlier);
    {
        journal.write(
            ACCOUNT_STREAM,
            "e9r0l0",
            "PositionReleased",
            1,
            Some("01JRELEASE0000000000000000"),
            object(vec![
                ("agent_id", text(AGENT)),
                ("canceled_order_ids", Value::Array(Vec::new())),
                ("cause", text("owner_release")),
                ("instrument", text("AAPL")),
                ("qty", text("10")),
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
    broker.canceled("md-protect")?;
    stopped(&journal, "c3", AGENT)?;
    if switched {
        kill_command(&journal, COMMAND, "connection", CONNECTION)?;
    }
    Ok((journal, broker))
}

/// DEC-2347 item 8: once the owner releases the position, the 10 shares are the owner's, so the
/// retired account's executor exits.
#[test]
#[ignore = "pending E6-4"]
fn a_released_position_lets_the_retired_executor_exit() -> Result<(), Error> {
    let (journal, broker) = released(false)?;
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
#[ignore = "pending E6-4"]
fn a_released_position_lets_the_kill_switch_start_exit_once_the_switch_is_applied()
-> Result<(), Error> {
    let (journal, broker) = released(true)?;
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
