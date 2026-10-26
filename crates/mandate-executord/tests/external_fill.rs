//! DEC-2812 (X10c; trading spec §6.1, §7.1): a fill on the account that names no order of ours is
//! booked as `FillApplied` version 2, with `client_order_id` `null`. Before it, the journal refused
//! every such batch, so the activity cursor never passed the fill and every later reconciliation
//! read it again and was refused again (DEC-2805). Now the reconciliation that reads it commits, its
//! checkpoint moves past the fill, and the fill is applied once.

mod support;

use mandate_canon::Value;
use mandate_executor::BrokerRequest;
use mandate_process::State;
use support::{ACCOUNT_STREAM, Error, OPENED, OPENING, Paper, at, deployed, start_allowing};

const THEIRS: &str = "theirs-1";

#[test]
fn an_external_fill_is_booked_once_and_the_cursor_passes_it() -> Result<(), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    broker.resting(OPENING, "AAPL", "10", "150")?;
    broker.fill(OPENING, "150")?;
    broker.resting(THEIRS, "AAPL", "3", "151")?;
    broker.fill(THEIRS, "151")?;
    let mut process = start_allowing(&journal, &broker)?;
    for minute in ["15:00", "15:01", "15:02"] {
        process.step(at(&format!("2026-09-21T{minute}:00Z"))?);
        assert!(
            !matches!(process.lifecycle().state(), State::Exited(_)),
            "{minute}: {:?}",
            process.lifecycle().state()
        );
    }

    let theirs: Vec<Value> = journal
        .payloads("FillApplied")
        .into_iter()
        .filter(|fill| fill.get("client_order_id") == Some(&Value::Null))
        .collect();
    assert_eq!(theirs.len(), 1, "booked once, naming no order: {theirs:?}");
    assert_eq!(
        theirs.first().and_then(|fill| fill.get("qty_gross")),
        Some(&Value::Str("3".to_owned()))
    );
    assert!(
        journal.payloads("AgentModeApplied").iter().all(|applied| {
            applied.get("restriction") != Some(&Value::Str("reconciliation:incomplete".to_owned()))
        }),
        "no batch was refused, so DEC-2805's hold is never placed"
    );

    let cursors: Vec<String> = broker
        .seen()
        .into_iter()
        .filter_map(|seen| match seen.request {
            BrokerRequest::ListActivities { since } => Some(since.0),
            _ => None,
        })
        .collect();
    assert!(cursors.len() >= 2, "{cursors:?}");
    assert!(
        cursors.iter().skip(1).all(|since| since == "2"),
        "every read after the first starts past both fills: {cursors:?}"
    );
    Ok(())
}
