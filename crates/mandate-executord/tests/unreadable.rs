//! A connection record on the control stream that does not read is ambiguity (DEC-2357, `AGENTS.md`
//! rules 3 and 13): it holds every opening the executor would hand, and nothing else. Every risk
//! exit, protective order and kill switch keeps going, and the executor never exits `completed` on
//! a reading it is unsure of.

mod support;

use mandate_canon::Value;
use mandate_process::State;
use support::{
    AGENT, CONTROL_STREAM, Error, Journal, NOON, Paper, at, deployed, kill, object, propose, start,
    start_allowing, stopped,
};

const OPEN: &str = "01JINTENT0000000000000000A";
const EXIT: &str = "01JINTENT0000000000000000C";
const COPY: &str = "01JKILL00000000000000000AA";

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

/// Each connection record the executor reads, written so it does not read: a member it needs is
/// missing.
fn unreadable() -> Result<Vec<(&'static str, u64, Value)>, Error> {
    Ok(vec![
        (
            "ConnectionRefused",
            1,
            object(vec![("occasion", text("connect"))])?,
        ),
        ("ConnectionRevoked", 1, object(vec![])?),
        (
            "ConnectionRequested",
            1,
            object(vec![("account_ref", text(support::ACCOUNT))])?,
        ),
        (
            "ConnectionEstablished",
            2,
            object(vec![
                ("account_ref", text(support::ACCOUNT)),
                ("environment", text("paper")),
            ])?,
        ),
        (
            "ProcessStartIssued",
            1,
            object(vec![("process", text("executor"))])?,
        ),
    ])
}

/// The deployed account with `record` journaled on the control stream after the deployment.
fn with_unreadable(event_type: &str, version: u64, payload: Value) -> Result<Journal, Error> {
    let journal = deployed()?;
    journal.write(CONTROL_STREAM, "u1", event_type, version, None, payload)?;
    Ok(journal)
}

/// Rule 13: the agent's risk exit is handed and its opening is not, and the process keeps running
/// rather than stop on the record.
#[test]
#[ignore = "pending E7-4"]
fn an_unreadable_connection_record_holds_openings_and_hands_every_exit() -> Result<(), Error> {
    for (event_type, version, payload) in unreadable()? {
        let journal = with_unreadable(event_type, version, payload)?;
        propose(&journal, AGENT, OPEN, "buy", "10", "150", "open")?;
        propose(&journal, AGENT, EXIT, "sell", "10", "150", "risk_exit")?;
        let broker = Paper::new(&journal);
        let mut process = start_allowing(&journal, &broker)?;
        process.step(at(NOON)?);
        process.step(at(NOON)?);
        assert_eq!(journal.received(), [EXIT], "{event_type}: the exit only");
        assert!(
            !matches!(process.lifecycle().state(), State::Exited(_)),
            "{event_type}: still running: {:?}",
            process.lifecycle().state()
        );
    }
    Ok(())
}

/// Rule 13: the agent's kill switch is applied beside an unreadable connection record.
#[test]
#[ignore = "pending E7-4"]
fn a_kill_switch_applies_beside_an_unreadable_connection_record() -> Result<(), Error> {
    for (event_type, version, payload) in unreadable()? {
        let journal = with_unreadable(event_type, version, payload)?;
        kill(&journal, AGENT, COPY)?;
        let broker = Paper::new(&journal);
        let mut process = start(&journal, &broker)?;
        process.step(at(NOON)?);
        assert_eq!(journal.switched(), [AGENT], "{event_type}");
    }
    Ok(())
}

/// Rule 3: with every agent stopped, a connection record that does not read keeps the executor
/// running, never `completed`, since the connection it may name could still want it.
#[test]
#[ignore = "pending E7-4"]
fn an_unreadable_connection_record_never_lets_the_executor_complete() -> Result<(), Error> {
    for (event_type, version, payload) in unreadable()? {
        let journal = with_unreadable(event_type, version, payload)?;
        stopped(&journal, "s1", AGENT)?;
        let broker = Paper::new(&journal);
        let mut process = start(&journal, &broker)?;
        for _ in 0..2 {
            process.step(at(NOON)?);
        }
        assert!(
            !matches!(process.lifecycle().state(), State::Exited(_)),
            "{event_type}: {:?}",
            process.lifecycle().state()
        );
    }
    Ok(())
}
