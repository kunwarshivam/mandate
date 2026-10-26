//! E7-2z's scheduled health probe (connections spec §8.2, DEC-2832, DEC-2837): an executor with
//! nothing working still reads the broker's account every [`PROBE_EVERY_S`], a read-only call
//! that only feeds the per-class counts. A lost answer counts as a network failure and a refused
//! credential as an authorization failure, never as healthy, and the probe neither sends an order
//! nor stops the process. The oracles are the requests the broker saw and the records journaled.

mod support;

use mandate_canon::Value;
use mandate_executor::{AccountState, BrokerRequest};
use mandate_executord::ExecutorProcess;
use mandate_executord::health::PROBE_EVERY_S;
use mandate_process::State;
use mandate_time::UtcNanos;
use support::{Error, Fault, Journal, Paper, at, deployed, start_allowing};

/// An idle executor: an agent deployed, no order working, ready after its start.
fn idle() -> Result<(Journal, Paper, ExecutorProcess<Journal, Paper>), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at("2026-09-21T15:00:00Z")?);
    if !process.ready() {
        return Err("the executor is not ready".into());
    }
    Ok((journal, broker, process))
}

/// The instant `secs` after 15:00, the idle executor's start.
fn after(secs: i64) -> Result<UtcNanos, Error> {
    let start = at("2026-09-21T15:00:00Z")?;
    Ok(UtcNanos::from_parts(start.secs().saturating_add(secs), 0)?)
}

/// Steps `passes` passes `every` seconds apart, after 15:00, and fails if the process stops.
fn passes(
    process: &mut ExecutorProcess<Journal, Paper>,
    passes: i64,
    every: i64,
) -> Result<(), Error> {
    for pass in 1..=passes {
        process.step(after(pass.saturating_mul(every))?);
        if matches!(process.lifecycle().state(), State::Exited(_)) {
            return Err(format!("pass {pass}: the process stopped").into());
        }
    }
    Ok(())
}

/// How many account reads the broker saw, and whether it saw anything but reads.
fn reads(broker: &Paper, from: usize) -> (usize, bool) {
    let seen: Vec<BrokerRequest> = broker
        .seen()
        .into_iter()
        .skip(from)
        .map(|seen| seen.request)
        .collect();
    let probes = seen
        .iter()
        .filter(|request| matches!(request, BrokerRequest::GetAccount))
        .count();
    let only_reads = seen.iter().all(|request| {
        matches!(
            request,
            BrokerRequest::GetAccount
                | BrokerRequest::ListOpenOrders
                | BrokerRequest::ListPositions
                | BrokerRequest::ListActivities { .. }
        )
    });
    (probes, only_reads)
}

fn restriction_causes(journal: &Journal) -> Vec<String> {
    journal
        .payloads("AccountRestrictionChanged")
        .iter()
        .filter_map(|payload| {
            payload
                .get("cause")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

#[test]
fn an_idle_executor_probes_once_per_period_with_reads_only() -> Result<(), Error> {
    let (journal, broker, mut process) = idle()?;
    let before = broker.seen().len();
    let every = 10;
    let periods = 4;
    passes(&mut process, periods * PROBE_EVERY_S / every, every)?;
    let (probes, only_reads) = reads(&broker, before);
    assert!(only_reads, "a probe never places or alters an order");
    assert_eq!(
        probes, periods as usize,
        "one probe a period over {periods} periods"
    );
    assert!(restriction_causes(&journal).is_empty());
    assert_eq!(process.state().account_state(), AccountState::Active);
    Ok(())
}

#[test]
fn an_idle_executor_on_a_lost_connection_is_restricted() -> Result<(), Error> {
    let (journal, broker, mut process) = idle()?;
    broker.lose_next_answers(64);
    passes(&mut process, 5, PROBE_EVERY_S)?;
    assert_eq!(restriction_causes(&journal), ["connection_unavailable"]);
    assert_eq!(process.state().account_state(), AccountState::ClosingOnly);
    Ok(())
}

#[test]
fn an_idle_executor_on_a_refused_credential_is_restricted() -> Result<(), Error> {
    let (journal, broker, mut process) = idle()?;
    broker.refuse_next_with(401, 64);
    passes(&mut process, 4, PROBE_EVERY_S)?;
    assert_eq!(restriction_causes(&journal), ["connection_unavailable"]);
    assert_eq!(process.state().account_state(), AccountState::ClosingOnly);
    Ok(())
}

/// Two lost probes then a good one, four times over, restrict nothing (DEC-687: a good result
/// resets its class), while the same probes with three lost in a row do: the control shows the
/// probes ran and were counted.
#[test]
fn a_good_probe_between_lost_ones_restricts_nothing() -> Result<(), Error> {
    for (lost, restricted) in [(2, false), (3, true)] {
        let (journal, broker, mut process) = idle()?;
        let before = broker.seen().len();
        let mut pass = 0;
        for _round in 0..4 {
            broker.lose_next_answers(lost);
            for _probe in 0..=lost {
                pass += 1;
                process.step(after(pass * PROBE_EVERY_S)?);
            }
        }
        let (probes, only_reads) = reads(&broker, before);
        assert!(only_reads);
        assert_eq!(
            probes, pass as usize,
            "one probe a pass, {lost} lost a round"
        );
        assert_eq!(
            !restriction_causes(&journal).is_empty(),
            restricted,
            "{lost} lost probes in a row: {:?}",
            restriction_causes(&journal)
        );
    }
    Ok(())
}

/// A restriction a probe leads to while the journal is down is held like any append (E21-26): it
/// is journaled once appends succeed, and nothing but the probe's reads reached the broker.
#[test]
fn a_probe_s_restriction_waits_out_a_journal_outage() -> Result<(), Error> {
    let (journal, broker, mut process) = idle()?;
    let before = broker.seen().len();
    broker.lose_next_answers(64);
    journal.fail_next(&[Fault::Unavailable; 64]);
    for pass in 1..=4 {
        process.step(after(pass * PROBE_EVERY_S)?);
        if matches!(process.lifecycle().state(), State::Exited(_)) {
            return Err(format!("pass {pass}: the outage stopped the process").into());
        }
    }
    assert!(restriction_causes(&journal).is_empty(), "nothing committed");
    let (probes, only_reads) = reads(&broker, before);
    assert!(only_reads, "only the probe's reads reached the broker");
    assert_eq!(
        probes, 3,
        "probes stop once the restriction is held in doubt"
    );
    journal.clear_faults();
    process.step(after(5 * PROBE_EVERY_S)?);
    assert_eq!(restriction_causes(&journal), ["connection_unavailable"]);
    Ok(())
}
