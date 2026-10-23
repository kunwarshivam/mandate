//! A connection record on the control stream that does not read is ambiguity (DEC-2357, `AGENTS.md`
//! rules 3 and 13): it holds every opening the executor would hand, and nothing else. Every risk
//! exit, protective order and kill switch keeps going, and the executor never exits `completed` on
//! a reading it is unsure of.

mod support;

use mandate_canon::Value;
use mandate_process::State;
use support::{
    AGENT, CONNECTION, CONTROL_STREAM, Error, Journal, NOON, Paper, at, deploy, deployed, kill,
    kill_command, object, propose, start, start_allowing, stopped, tick,
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

/// DEC-2357 item 3: a version-1 establishment reads, so it sets nothing aside. It holds openings
/// as before (DEC-1171 item 3), and once the connection is established again at version 2 the
/// agent's opening is handed.
#[test]
fn a_version_1_establishment_reads_and_a_later_one_hands_openings_again() -> Result<(), Error> {
    let journal = deployed()?;
    journal.write(
        CONTROL_STREAM,
        "c3",
        "ConnectionEstablished",
        1,
        None,
        object(vec![("connection_id", text(support::CONNECTION))])?,
    )?;
    support::connect(&journal, "c4", support::CONNECTION, support::ACCOUNT)?;
    propose(&journal, AGENT, OPEN, "buy", "10", "150", "open")?;
    let broker = Paper::new(&journal);
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(
        journal.received(),
        [OPEN],
        "established again, the opening is handed"
    );
    Ok(())
}

/// DEC-2357 item 1, the review of #2329 (P1): an agent whose connection's only establishment does
/// not read is still followed while unsure, so its risk exit is handed and its kill switch applied,
/// never left silent.
#[test]
#[ignore = "pending E7-4"]
fn an_agent_on_an_unreadable_establishment_still_gets_its_exit_and_switch() -> Result<(), Error> {
    let journal = Journal::default();
    journal.write(
        CONTROL_STREAM,
        "c1",
        "ConnectionEstablished",
        2,
        None,
        object(vec![
            ("account_ref", text(support::ACCOUNT)),
            ("environment", text("paper")),
        ])?,
    )?;
    deploy(&journal, "c2", AGENT, CONNECTION)?;
    tick(&journal, "t1", NOON)?;
    propose(&journal, AGENT, EXIT, "sell", "10", "150", "risk_exit")?;
    kill(&journal, AGENT, COPY)?;
    let broker = Paper::new(&journal);
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at(NOON)?);
    process.step(at(NOON)?);
    assert_eq!(journal.received(), [EXIT], "the exit is handed");
    assert_eq!(journal.switched(), [AGENT], "and the switch applied");
    Ok(())
}

/// DEC-2357 item 1, the review of #2329 (P2): a revocation that does not read may be the compromise
/// revocation DEC-2343 reads a connection's kill command for, so while unsure the executor applies
/// the command itself, with no runtime copy.
#[test]
#[ignore = "pending E7-4"]
fn an_unreadable_revocation_still_applies_the_connections_kill_command() -> Result<(), Error> {
    let journal = deployed()?;
    kill_command(
        &journal,
        "01JCMD000000000000000000AA",
        "connection",
        CONNECTION,
    )?;
    journal.write(
        CONTROL_STREAM,
        "c4",
        "ConnectionRevoked",
        1,
        None,
        object(vec![])?,
    )?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(
        journal.applications(),
        [(AGENT.to_owned(), "01JCMD000000000000000000AA".to_owned())],
        "the command is applied with no copy"
    );
    Ok(())
}
