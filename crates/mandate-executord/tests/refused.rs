//! DEC-2805: a batch the journal refuses as invalid never stops the executor process. Stopping it
//! would also stop every agent's exits, protection and kill switch, and the restart would read the
//! same broker activity and be refused again, a crash loop (`AGENTS.md` rules 3 and 13). Instead the
//! process journals a hold of every agent's openings under `reconciliation:incomplete`, with one
//! alert to the owner, and goes on: a kill switch after the refusal is applied as before.

mod support;

use mandate_canon::Value;
use mandate_process::State;
use support::{ACCOUNT_STREAM, AGENT, Error, Fault, NOON, Paper, at, deployed, kill, start};

const SWITCH: &str = "01JKILL00000000000000000AC";

fn held(journal: &support::Journal) -> Vec<Value> {
    journal
        .payloads("AgentModeApplied")
        .into_iter()
        .filter(|payload| {
            payload.get("restriction") == Some(&Value::Str("reconciliation:incomplete".to_owned()))
        })
        .collect()
}

#[test]
fn a_refused_batch_holds_the_openings_and_the_kill_switch_still_applies() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    journal.fail_next(&[Fault::Invalid]);
    process.step(at(NOON)?);
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
    process.step(at(NOON)?);
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
