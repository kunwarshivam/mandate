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
    broker.resting_sell("md-protect", "AAPL", "10", "147")?;
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

/// DEC-2846: each refusal's cause, the refused record's type and the journal's reason and path,
/// reaches the operator, once per new cause: the cell's lost exit was refused on every pass, and
/// the once-a-process `journal_refused` alert never said why. A refusal for the same cause again
/// says nothing new; the cause carries no payload (`AGENTS.md` rule 6).
#[test]
fn a_refusals_cause_is_alerted_once_per_cause() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    let causes = |alerts: &[Alert]| -> Vec<String> {
        alerts
            .iter()
            .filter(|alert| alert.key == "journal_refused_cause")
            .filter_map(|alert| alert.subject.clone())
            .collect()
    };
    journal.fail_next(&[Fault::Invalid]);
    let first = causes(&process.step(at(NOON)?));
    assert_eq!(first.len(), 1, "{first:?}");
    assert!(
        first.iter().all(|cause| cause.ends_with(":schema:payload")),
        "the record type, the reason code and the path: {first:?}"
    );
    journal.fail_next(&[Fault::Invalid]);
    let again = causes(&process.step(at(NOON)?));
    assert!(again.is_empty(), "the same cause again: {again:?}");
    Ok(())
}

/// A deployment whose references read only once `readable` is set (DEC-2846).
struct Readable(std::rc::Rc<std::cell::Cell<bool>>);

impl mandate_executor::BindingGateSource for Readable {
    fn input(
        &self,
        _request: &mandate_executor::BindingGateRequest<'_>,
    ) -> Option<mandate_executor::BindingGateInput> {
        None
    }

    fn config_refs(
        &self,
        _agent: &mandate_executor::AgentId,
        _instrument: &mandate_accounting::InstrumentId,
    ) -> Option<mandate_executor::BindingGateConfigRefs> {
        self.0.get().then(support::deployed_refs)
    }
}

/// DEC-2846 and `AGENTS.md` rule 13: the kill switch never depends on configuration reading. An
/// owner's kill switch whose flatten cannot be journaled, because its deployment's references do
/// not read, is applied at once: `KillSwitchActivated` and the agent's stop are journaled. Only the
/// flatten waits, journaled `exit_unpriced` and alerted once, and it sells at the first pass that
/// reads them. The process never stops. (The process's mandate port falls back to the version the
/// control stream deployed, so a followed agent's version always reads here; the core's own test
/// covers a version that does not.)
#[test]
fn a_kill_switch_applies_at_once_and_its_flatten_waits_for_its_configuration() -> Result<(), Error>
{
    for unread in ["references"] {
        let journal = deployed()?;
        journal.seed(ACCOUNT_STREAM, OPENED)?;
        let broker = Paper::new(&journal);
        broker.resting(OPENING, "AAPL", "10", "150")?;
        broker.fill(OPENING, "150")?;
        let references = std::rc::Rc::new(std::cell::Cell::new(false));
        let wiring = support::wired(
            Box::new(Readable(std::rc::Rc::clone(&references))),
            Box::new(support::TwoPercentStop),
        )?;
        let mut process = support::start_wired(&journal, &broker, wiring)?;
        process.step(at(NOON)?);
        kill(&journal, AGENT, SWITCH)?;
        for _ in 0..3 {
            process.step(at(NOON)?);
        }
        assert!(
            !matches!(process.lifecycle().state(), State::Exited(_)),
            "{unread}: never stops: {:?}",
            process.lifecycle().state()
        );
        assert_eq!(journal.switched(), [AGENT], "{unread}: applied at once");
        let sells = |broker: &Paper| {
            broker
                .seen()
                .iter()
                .filter(|seen| matches!(&seen.request, mandate_executor::BrokerRequest::Submit(order) if order.side == mandate_accounting::Side::Sell))
                .count()
        };
        assert_eq!(sells(&broker), 0, "{unread}: the flatten waits");
        let waiting = journal
            .payloads("ProtectionChanged")
            .iter()
            .filter(|p| p.get("action") == Some(&Value::Str("exit_unpriced".to_owned())))
            .count();
        assert_eq!(
            waiting,
            1,
            "{unread}: journaled once: {:?}",
            journal.types(ACCOUNT_STREAM)
        );
        assert!(
            journal
                .payloads("OwnerAlertSent")
                .iter()
                .any(|p| p.get("kind") == Some(&Value::Str("exit_stalled".to_owned()))),
            "{unread}: and the owner alerted"
        );
        references.set(true);
        process.step(at(NOON)?);
        assert_eq!(sells(&broker), 1, "{unread}: sold once it reads");
    }
    Ok(())
}
