//! X10d: a broker answer the executor's core cannot apply never stops the process. Stopping it
//! would also stop every agent's exits, protection and kill switch, and the restart would read the
//! same broker activity and fail again, a crash loop (`AGENTS.md` rules 3 and 13). Instead the
//! process holds every agent's openings under `reconciliation:incomplete`, as for a batch the
//! journal refuses (DEC-2805), alerts the owner and, once, the operator, and goes on: a kill switch
//! after it is applied as before.

mod support;

use mandate_canon::Value;
use mandate_executord::Alert;
use mandate_process::State;
use support::{
    ACCOUNT_STREAM, AGENT, Error, OPENED, OPENING, Paper, at, deployed, kill, start_allowing,
};

const SWITCH: &str = "01JKILL00000000000000000AD";
/// A fill whose quantity times price is past what a decimal holds: the fold cannot book it.
const HUGE: &str = "1000000000000000";

fn held(journal: &support::Journal) -> Vec<Value> {
    journal
        .payloads("AgentModeApplied")
        .into_iter()
        .filter(|payload| {
            payload.get("restriction") == Some(&Value::Str("reconciliation:incomplete".to_owned()))
        })
        .collect()
}

fn refusals(alerts: &[Alert]) -> usize {
    alerts
        .iter()
        .filter(|alert| alert.key == "input_refused" && alert.subject.is_none())
        .count()
}

#[test]
fn a_broker_answer_the_core_cannot_apply_holds_the_openings_and_the_kill_switch_applies()
-> Result<(), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    broker.resting(OPENING, "AAPL", "10", "150")?;
    broker.fill(OPENING, "150")?;
    broker.resting("theirs-1", "AAPL", HUGE, HUGE)?;
    broker.fill("theirs-1", HUGE)?;
    let mut process = start_allowing(&journal, &broker)?;

    let first = process.step(at("2026-09-21T15:00:00Z")?);
    assert!(
        !matches!(process.lifecycle().state(), State::Exited(_)),
        "{:?}",
        process.lifecycle().state()
    );
    assert_eq!(refusals(&first), 1, "the operator is alerted");
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

    let again = process.step(at("2026-09-21T15:01:00Z")?);
    assert!(!matches!(process.lifecycle().state(), State::Exited(_)));
    assert_eq!(
        refusals(&again),
        0,
        "the operator is alerted once a process"
    );
    assert_eq!(held(&journal).len(), 1, "and the hold is placed once");

    kill(&journal, AGENT, SWITCH)?;
    process.step(at("2026-09-21T15:02:00Z")?);
    assert_eq!(journal.switched(), [AGENT], "the kill switch still applies");
    assert!(!matches!(process.lifecycle().state(), State::Exited(_)));
    Ok(())
}
