//! The journal-outage hold, tested for exactly what the design discloses (E21-26; infrastructure
//! design §2.1, OPS-4): with the journal unavailable and a risk exit owed, no order of any kind is
//! sent, no resting protective order is canceled, the hold raises its alert, and the owed exit is
//! the first order sent once appends succeed. What the executor may do beyond that waits for the
//! founder (DEC-434 item 21); these tests change with that decision.

mod support;

use mandate_executor::{BrokerRequest, Purpose};
use mandate_executord::{Alert, ExecutorProcess};
use mandate_process::State;
use support::{
    ACCOUNT_STREAM, AGENT, Error, Fault, Journal, OPENED, OPENING, Paper, at, deployed, propose,
    start_allowing,
};

const EXIT: &str = "01JINTENT0000000000000000C";
/// More failed appends than any test here makes: the journal stays down until it is cleared.
const DOWN: usize = 64;

/// An executor up and ready, its agent holding the 10 AAPL its opening bought, the opening's
/// protection resting at the broker, and a risk exit of all 10 proposed and not yet taken.
fn owed_exit() -> Result<(Journal, Paper, ExecutorProcess<Journal, Paper>), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    broker.resting(OPENING, "AAPL", "10", "150")?;
    broker.fill(OPENING, "150")?;
    broker.resting("md-protect", "AAPL", "10", "147")?;
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at("2026-09-21T15:00:00Z")?);
    if !process.ready() {
        return Err("the executor is not ready before the outage".into());
    }
    propose(&journal, AGENT, EXIT, "sell", "10", "149", "risk_exit")?;
    Ok((journal, broker, process))
}

/// The passes of the outage, a minute apart from 15:01.
fn minutes(from: u32, passes: u32) -> Vec<String> {
    (from..from + passes)
        .map(|minute| format!("2026-09-21T15:{minute:02}:00Z"))
        .collect()
}

/// While the journal is down nothing reaches the broker, not the owed exit and not a cancel of
/// the protection resting there, and the process holds rather than stops.
#[test]
fn nothing_is_sent_while_the_journal_is_down() -> Result<(), Error> {
    let (journal, broker, mut process) = owed_exit()?;
    let before = broker.seen().len();
    journal.fail_next(&[Fault::Unavailable; DOWN]);
    for minute in minutes(1, 5) {
        process.step(at(&minute)?);
        assert!(
            !matches!(process.lifecycle().state(), State::Exited(_)),
            "{minute}: the hold is not a stop"
        );
    }
    let sent: Vec<_> = broker.seen().into_iter().skip(before).collect();
    assert!(sent.is_empty(), "sent during the outage: {sent:?}");
    assert!(
        !broker
            .seen()
            .iter()
            .any(|seen| matches!(seen.request, BrokerRequest::Cancel { .. })),
        "no resting protective order is canceled"
    );
    Ok(())
}

/// Once appends succeed, the owed exit is the first order the broker sees, journaled first, and
/// nothing goes ahead of it.
#[test]
fn the_owed_exit_is_the_first_order_once_appends_succeed() -> Result<(), Error> {
    let (journal, broker, mut process) = owed_exit()?;
    let before = broker.seen().len();
    journal.fail_next(&[Fault::Unavailable; DOWN]);
    for minute in minutes(1, 3) {
        process.step(at(&minute)?);
    }
    journal.clear_faults();
    process.step(at("2026-09-21T15:04:00Z")?);
    let first = broker
        .seen()
        .into_iter()
        .skip(before)
        .find(|seen| matches!(seen.request, BrokerRequest::Submit(_)))
        .ok_or("no order was sent once appends succeeded")?;
    let BrokerRequest::Submit(order) = &first.request else {
        return Err(format!("the first request is not the exit: {first:?}").into());
    };
    assert_eq!(order.client_order_id.as_str(), format!("md-{EXIT}"));
    assert_eq!(order.purpose, Purpose::RiskExit);
    assert!(
        first.journaled_first,
        "the exit is journaled before it is sent"
    );
    Ok(())
}

/// The alerts of a pass that name the hold: the fixed key, and no subject (`AGENTS.md` rule 6).
fn holds(alerts: &[Alert]) -> usize {
    alerts
        .iter()
        .filter(|alert| alert.key == "journal_unavailable" && alert.subject.is_none())
        .count()
}

/// The hold raises one opaque alert when an append first fails, none while it goes on, none once
/// appends succeed, and one again for the next outage, so a persistent outage is never silent and
/// never floods (design §8.2, RB-07).
#[test]
fn the_hold_raises_its_alert_once_an_outage() -> Result<(), Error> {
    let (journal, _broker, mut process) = owed_exit()?;
    journal.fail_next(&[Fault::Unavailable; DOWN]);
    let raised: Vec<usize> = minutes(1, 3)
        .iter()
        .map(|minute| Ok(holds(&process.step(at(minute)?))))
        .collect::<Result<_, Error>>()?;
    assert_eq!(raised, [1, 0, 0], "one alert as the outage starts");
    journal.clear_faults();
    assert_eq!(holds(&process.step(at("2026-09-21T15:04:00Z")?)), 0);
    propose(
        &journal,
        AGENT,
        "01JINTENT0000000000000000D",
        "sell",
        "1",
        "149",
        "risk_exit",
    )?;
    journal.fail_next(&[Fault::Unavailable; DOWN]);
    assert_eq!(
        holds(&process.step(at("2026-09-21T15:05:00Z")?)),
        1,
        "the next outage raises it again"
    );
    Ok(())
}
