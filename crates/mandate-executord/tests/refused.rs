//! DEC-2805: a batch the journal refuses as invalid never stops the executor process. Stopping it
//! would also stop every agent's exits, protection and kill switch, and the restart would read the
//! same broker activity and be refused again, a crash loop (`AGENTS.md` rules 3 and 13). Instead the
//! process journals a hold of every agent's openings under `reconciliation:incomplete`, with one
//! alert to the owner, and goes on: a kill switch after the refusal is applied as before.

mod support;

use mandate_canon::Value;
use mandate_executord::Alert;
use mandate_process::State;
use support::{
    ACCOUNT_STREAM, AGENT, Error, Fault, NOON, OPENED, OPENING, Paper, at, deployed, kill, propose,
    start, start_allowing,
};

const SWITCH: &str = "01JK1MM00000000000000000AC";
const EXIT: &str = "01J1NTENT0000000000000000S";

fn held(journal: &support::Journal) -> Vec<Value> {
    journal
        .payloads("AgentModeApplied")
        .into_iter()
        .filter(|payload| {
            payload.get("restriction") == Some(&Value::Str("reconciliation:incomplete".to_owned()))
        })
        .collect()
}

/// The operator's alerts of a pass that a refusal raised (DEC-2805 item 1).
fn refusals(alerts: &[Alert]) -> usize {
    alerts
        .iter()
        .filter(|alert| alert.key == "journal_refused" && alert.subject.is_none())
        .count()
}

#[test]
fn a_refused_batch_holds_the_openings_and_the_kill_switch_still_applies() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    journal.fail_next(&[Fault::Invalid]);
    let first = process.step(at(NOON)?);
    assert_eq!(refusals(&first), 1, "the operator is alerted");
    assert!(
        !matches!(process.lifecycle().state(), State::Exited(_)),
        "a refused batch never stops the process: {:?}",
        process.lifecycle().state()
    );
    let hold = held(&journal);
    assert_eq!(hold.len(), 1, "{:?}", journal.types(ACCOUNT_STREAM));
    assert_eq!(
        hold.first().and_then(|p| p.get("agent")),
        Some(&Value::Str("*".to_owned()))
    );
    assert_eq!(
        hold.first().and_then(|p| p.get("to")),
        Some(&Value::Str("exits_only".to_owned()))
    );
    assert!(
        journal
            .payloads("OwnerAlertSent")
            .iter()
            .any(|alert| alert.get("kind") == Some(&Value::Str("reconciliation".to_owned()))),
        "the owner is alerted"
    );

    kill(&journal, AGENT, SWITCH)?;
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT], "the kill switch still applies");
    assert!(!matches!(process.lifecycle().state(), State::Exited(_)));

    journal.fail_next(&[Fault::Invalid]);
    let again = process.step(at(NOON)?);
    assert_eq!(
        refusals(&again),
        0,
        "the operator is alerted once a process"
    );
    process.step(at(NOON)?);
    assert_eq!(
        held(&journal).len(),
        1,
        "a second refusal adds no second hold"
    );
    assert!(!matches!(process.lifecycle().state(), State::Exited(_)));
    Ok(())
}

/// The hold itself refused too leaves nothing safe to record, so the process stops, as before
/// DEC-2805, rather than act on what it cannot journal (`AGENTS.md` rule 5).
#[test]
fn a_refused_hold_stops_the_process() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    journal.fail_next(&[Fault::Invalid, Fault::Invalid]);
    process.step(at(NOON)?);
    assert!(
        matches!(process.lifecycle().state(), State::Exited(_)),
        "{:?}",
        process.lifecycle().state()
    );
    assert!(held(&journal).is_empty());
    Ok(())
}

/// A refused exit's batch reaches the broker with nothing: its `OrderSubmitted` never committed,
/// so its order is not sent (`AGENTS.md` rule 5). The process holds the openings and goes on, and
/// the exit, handed again at the next pass, is journaled and sent (rule 13).
#[test]
fn a_refused_exit_sends_nothing_and_goes_at_the_next_pass() -> Result<(), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    broker.resting(OPENING, "AAPL", "10", "150")?;
    broker.fill(OPENING, "150")?;
    broker.resting("md-protect", "AAPL", "10", "147")?;
    let mut process = start_allowing(&journal, &broker)?;
    let exit = format!("md-{EXIT}");

    journal.fail_next_of("IntentReceived", &[Fault::Invalid]);
    propose(&journal, AGENT, EXIT, "sell", "10", "149", "risk_exit")?;
    process.step(at("2026-09-21T15:00:00Z")?);
    assert!(
        !matches!(process.lifecycle().state(), State::Exited(_)),
        "{:?}",
        process.lifecycle().state()
    );
    assert_eq!(held(&journal).len(), 1, "the refusal holds the openings");
    assert!(
        !journal.received().contains(&EXIT.to_owned()),
        "nothing of the refused batch committed"
    );
    assert!(
        !broker.submitted().contains(&exit),
        "and its order is not sent"
    );

    process.step(at("2026-09-21T15:01:00Z")?);
    assert!(journal.received().contains(&EXIT.to_owned()));
    assert!(
        broker.submitted().contains(&exit),
        "the exit goes: {:?}",
        broker.submitted()
    );
    Ok(())
}
