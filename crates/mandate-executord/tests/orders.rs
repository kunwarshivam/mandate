//! The orders the account's agents propose (E21-30, DEC-2033): each `IntentProposed` handed to the
//! core as the runtime's handoff would be, once, an exit whether or not the process is ready and an
//! opening only while it is and with its mandate's protection, kill switches always first, and
//! nothing sent before it is journaled.

mod support;

use mandate_canon::Value;
use mandate_executor::BrokerRequest;
use mandate_executord::Alert;
use mandate_process::{Exit, State};
use mandate_time::UtcNanos;
use support::{
    ACCOUNT_STREAM, AGENT, Allowing, CannotProtect, Error, Journal, NOON, OTHER_AGENT, Paper,
    agent_stream, at, deploy, deployed, kill, object, owner_exit, owner_exit_stepped_up, propose,
    propose_market_exit, propose_owner_market_exit, start, start_allowing, start_wired, wired,
};

const OPEN: &str = "01J1NTENT0000000000000000A";
const OTHER_EXIT: &str = "01J1NTENT0000000000000000B";
const EXIT: &str = "01J1NTENT0000000000000000C";
const SWITCH: &str = "01JK1MM00000000000000000AA";
const OWNER: &str = "01J0WNER0000000000000000AA";

fn exit(journal: &Journal, agent: &str, id: &str) -> Result<(), Error> {
    propose(journal, agent, id, "sell", "10", "150", "risk_exit")
}

fn opening(journal: &Journal, agent: &str, id: &str) -> Result<(), Error> {
    propose(journal, agent, id, "buy", "10", "150", "open")
}

fn held(broker: &Paper) -> Result<(), Error> {
    broker.holding("AAPL", "10")
}

fn submitted(broker: &Paper) -> Vec<BrokerRequest> {
    broker
        .seen()
        .into_iter()
        .map(|seen| seen.request)
        .filter(|request| matches!(request, BrokerRequest::Submit(_)))
        .collect()
}

/// An exit is handed whether or not the process is ready: risk reduction never waits on the
/// startup reconciliation (`AGENTS.md` rule 13).
#[test]
fn an_exit_is_handed_while_not_ready() -> Result<(), Error> {
    let journal = deployed()?;
    exit(&journal, AGENT, EXIT)?;
    let broker = Paper::new(&journal);
    held(&broker)?;
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert!(!process.ready());
    assert_eq!(journal.received(), [EXIT]);
    Ok(())
}

/// An exit is decided by the gate once, in the batch that receives it: the agent holds none of the
/// account's position here, so the gate refuses it and nothing reaches the broker (trading-domain
/// spec §9, DEC-410 item 3).
#[test]
fn an_exit_is_received_and_decided_in_one_batch() -> Result<(), Error> {
    let journal = deployed()?;
    exit(&journal, AGENT, EXIT)?;
    let broker = Paper::new(&journal);
    held(&broker)?;
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    process.step(at(NOON)?);
    let types = journal.types(ACCOUNT_STREAM);
    let received = types.iter().position(|t| t == "IntentReceived");
    let decided = types.iter().position(|t| t == "GateDecided");
    assert_eq!(decided, received.map(|at| at + 1), "{types:?}");
    assert!(submitted(&broker).is_empty());
    Ok(())
}

/// An opening reaches the broker only after its `OrderSubmitted` is journaled (`AGENTS.md` rule
/// 5), as a bracket whose stop is the mandate's: 2 % below the 150 limit is 147, and 5 % above is
/// 157.5.
#[test]
fn an_opening_is_journaled_before_it_is_sent_with_its_stop_beside_it() -> Result<(), Error> {
    let journal = deployed()?;
    opening(&journal, AGENT, OPEN)?;
    let broker = Paper::new(&journal);
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at(NOON)?);
    assert!(process.ready());
    assert_eq!(journal.received(), [OPEN]);
    let seen = broker.seen();
    let submits: Vec<_> = seen
        .iter()
        .filter(|seen| matches!(seen.request, BrokerRequest::Submit(_)))
        .collect();
    assert_eq!(submits.len(), 1, "{:?}", journal.types(ACCOUNT_STREAM));
    assert!(submits.iter().all(|seen| seen.journaled_first));
    let Some(BrokerRequest::Submit(order)) = submits.first().map(|seen| seen.request.clone())
    else {
        return Err("no submission".into());
    };
    let bracket = order.bracket.ok_or("no bracket")?;
    assert_eq!(bracket.stop.to_string(), "147");
    assert_eq!(bracket.take_profit.to_string(), "157.5");
    Ok(())
}

/// A process that is not ready takes no opening, and still applies the kill switch and hands an
/// exit (design §3.4, `AGENTS.md` rule 13).
#[test]
fn a_process_that_is_not_ready_takes_no_opening_but_still_switches() -> Result<(), Error> {
    let journal = deployed()?;
    opening(&journal, AGENT, OPEN)?;
    let broker = Paper::new(&journal);
    broker.holding("AAPL", "10")?;
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at(NOON)?);
    assert!(!process.ready());
    assert!(journal.received().is_empty());
    kill(&journal, AGENT, SWITCH)?;
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    assert!(journal.received().is_empty());
    assert!(submitted(&broker).is_empty());
    Ok(())
}

/// An opening that a later kill switch follows on the same stream is never handed, and the switch
/// is applied once, across passes and a restart (DEC-2033 item 2, `AGENTS.md` rule 3).
#[test]
fn an_opening_before_a_kill_switch_never_goes() -> Result<(), Error> {
    let journal = deployed()?;
    opening(&journal, AGENT, OPEN)?;
    kill(&journal, AGENT, SWITCH)?;
    let broker = Paper::new(&journal);
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at(NOON)?);
    process.step(at(NOON)?);
    let mut restarted = start_allowing(&journal, &broker)?;
    restarted.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    assert!(journal.received().is_empty());
    assert!(submitted(&broker).is_empty());
    Ok(())
}

/// An opening that a later change of the agent's mode to anything but `normal` follows is never
/// handed; an exit before the change still is (DEC-2033 item 2).
#[test]
fn a_mode_change_drops_a_waiting_opening_but_not_an_exit() -> Result<(), Error> {
    let journal = deployed()?;
    opening(&journal, AGENT, OPEN)?;
    propose(&journal, AGENT, EXIT, "sell", "5", "150", "risk_exit")?;
    journal.write(
        &agent_stream(AGENT),
        "m1",
        "AgentModeChanged",
        1,
        None,
        object(vec![
            ("from", Value::Str("normal".to_owned())),
            ("to", Value::Str("paused".to_owned())),
            ("reason", Value::Str("owner".to_owned())),
        ])?,
    )?;
    let broker = Paper::new(&journal);
    broker.holding("AAPL", "10")?;
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.received(), [EXIT]);
    Ok(())
}

/// The gate decides: the process's default wiring has no binding-gate fact, so the core refuses
/// the opening before anything is journaled. The process holds it, alerts once, and keeps going:
/// an exit proposed after it is still received (DEC-2033 item 5, `AGENTS.md` rules 3 and 13).
#[test]
fn an_opening_without_gate_facts_is_held_and_an_exit_behind_it_still_goes() -> Result<(), Error> {
    let journal = deployed()?;
    opening(&journal, AGENT, OPEN)?;
    propose(&journal, AGENT, EXIT, "sell", "10", "150", "risk_exit")?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    let first = process.step(at(NOON)?);
    let second = process.step(at(NOON)?);
    assert!(process.ready());
    assert!(!matches!(process.lifecycle().state(), State::Exited(_)));
    assert_eq!(journal.received(), [EXIT]);
    let held = |alerts: &[Alert]| {
        alerts
            .iter()
            .filter(|alert| alert.key == "opening_held" && alert.subject.as_deref() == Some(OPEN))
            .count()
    };
    assert_eq!((held(&first), held(&second)), (1, 0));
    Ok(())
}

/// An opening whose protection cannot be read stops the process before anything is journaled for
/// it: never an opening handed unprotected (TI-4, DEC-2033 item 3).
#[test]
fn an_opening_whose_protection_cannot_be_read_stops_the_process() -> Result<(), Error> {
    let journal = deployed()?;
    opening(&journal, AGENT, OPEN)?;
    let broker = Paper::new(&journal);
    let wiring = wired(Box::new(Allowing), Box::new(CannotProtect))?;
    let mut process = start_wired(&journal, &broker, wiring)?;
    process.step(at(NOON)?);
    assert!(
        matches!(
            process.lifecycle().state(),
            State::Exited(Exit::CycleFailed)
        ),
        "{:?}",
        journal.types(ACCOUNT_STREAM)
    );
    assert!(journal.received().is_empty());
    assert!(submitted(&broker).is_empty());
    Ok(())
}

/// Within a pass, a kill switch goes before any other agent's order (ES-06): the switch of one
/// agent is journaled before the exit another agent proposed earlier is received.
#[test]
fn a_kill_switch_goes_before_another_agents_earlier_exit() -> Result<(), Error> {
    let journal = deployed()?;
    deploy(&journal, "c3", OTHER_AGENT, "conn1")?;
    exit(&journal, OTHER_AGENT, OTHER_EXIT)?;
    kill(&journal, AGENT, SWITCH)?;
    let broker = Paper::new(&journal);
    held(&broker)?;
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    let types = journal.types(ACCOUNT_STREAM);
    let switch = types
        .iter()
        .position(|t| t == "KillSwitchActivated")
        .ok_or("no switch")?;
    let received = types
        .iter()
        .position(|t| t == "IntentReceived")
        .ok_or("no intent")?;
    assert!(switch < received, "{types:?}");
    assert_eq!(journal.received(), [OTHER_EXIT]);
    Ok(())
}

/// A proposal is taken once: a second pass and a restart never receive it again (the core's own
/// intent fold, journal spec §5.2); what the core does with an intent it holds is its own.
#[test]
fn a_proposal_is_taken_once_across_passes_and_restarts() -> Result<(), Error> {
    let journal = deployed()?;
    exit(&journal, AGENT, EXIT)?;
    let broker = Paper::new(&journal);
    held(&broker)?;
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    process.step(at(NOON)?);
    let mut restarted = start(&journal, &broker)?;
    restarted.step(at(NOON)?);
    restarted.step(at(NOON)?);
    assert_eq!(journal.received(), [EXIT]);
    Ok(())
}

/// Every exit waiting in a pass is handed in it, each agent's in its own stream order.
#[test]
fn every_exit_waiting_in_a_pass_is_handed_in_it() -> Result<(), Error> {
    let journal = deployed()?;
    deploy(&journal, "c3", OTHER_AGENT, "conn1")?;
    exit(&journal, AGENT, EXIT)?;
    exit(&journal, OTHER_AGENT, OTHER_EXIT)?;
    let broker = Paper::new(&journal);
    held(&broker)?;
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.received(), [EXIT, OTHER_EXIT]);
    Ok(())
}

/// A proposal whose members do not read stops the process rather than be skipped (DEC-85).
#[test]
fn an_unreadable_proposal_stops_the_process() -> Result<(), Error> {
    let journal = deployed()?;
    journal.write(
        &agent_stream(AGENT),
        EXIT,
        "IntentProposed",
        1,
        Some("decision"),
        object(vec![("type", Value::Str("limit".to_owned()))])?,
    )?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert!(matches!(process.lifecycle().state(), State::Exited(_)));
    assert!(journal.received().is_empty());
    Ok(())
}

/// An owner's exit at the market goes as a limit at the floor the owner confirmed in the
/// `OwnerExitRequested` it follows from: the worst price they accepted (DEC-2033 item 4).
#[test]
fn an_owner_exit_at_the_market_goes_as_a_limit_at_its_confirmed_floor() -> Result<(), Error> {
    let journal = deployed()?;
    owner_exit(&journal, AGENT, OWNER, Some("142.5"))?;
    propose_owner_market_exit(&journal, AGENT, EXIT, OWNER)?;
    let broker = Paper::new(&journal);
    held(&broker)?;
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.received(), [EXIT]);
    let limits: Vec<_> = journal
        .payloads("IntentReceived")
        .iter()
        .filter_map(|payload| payload.get("limit_price").cloned())
        .collect();
    assert_eq!(limits, [Value::Str("142.5".to_owned())]);
    Ok(())
}

/// An owner's exit at the market whose bid the owner confirmed with valid step-up is handed as
/// proposed, with no limit, and its confirmation is journaled beside it, expiring 300 s after the
/// step-up (DEC-2040 items 1, 4 and 7). The floor still binds: in the session it goes as a limit,
/// never at the market (item 6).
#[test]
fn a_stepped_up_owner_exit_at_the_market_is_handed_its_confirmation() -> Result<(), Error> {
    let journal = deployed()?;
    owner_exit_stepped_up(
        &journal,
        AGENT,
        OWNER,
        "142.5",
        "2026-09-21T14:59:00.000000000Z",
    )?;
    propose_owner_market_exit(&journal, AGENT, EXIT, OWNER)?;
    let broker = Paper::new(&journal);
    held(&broker)?;
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.received(), [EXIT]);
    let received: Vec<_> = journal
        .payloads("IntentReceived")
        .iter()
        .map(|payload| {
            (
                payload.get("type").cloned(),
                payload.get("limit_price").cloned(),
            )
        })
        .collect();
    assert_eq!(
        received,
        [(Some(Value::Str("market".to_owned())), Some(Value::Null))],
        "received as proposed"
    );
    let confirmed = journal.payloads("OwnerConfirmationReceived");
    let expires_at = UtcNanos::parse("2026-09-21T15:04:00.000000000Z")?.secs();
    assert_eq!(
        confirmed
            .iter()
            .map(|payload| (
                payload.get("floor").cloned(),
                payload.get("expires_at").and_then(Value::as_int)
            ))
            .collect::<Vec<_>>(),
        [(
            Some(Value::Str("142.5".to_owned())),
            u64::try_from(expires_at).ok()
        )]
    );
    Ok(())
}

/// An owner's exit at the market with no confirmed floor is handed with no limit, received as a
/// market order, and the exit behind it still goes (DEC-2037 item 7, `AGENTS.md` rule 13).
#[test]
fn an_owner_exit_at_the_market_without_a_floor_is_handed_with_no_limit() -> Result<(), Error> {
    let journal = deployed()?;
    owner_exit(&journal, AGENT, OWNER, None)?;
    propose_owner_market_exit(&journal, AGENT, OTHER_EXIT, OWNER)?;
    exit(&journal, AGENT, EXIT)?;
    let broker = Paper::new(&journal);
    held(&broker)?;
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    process.step(at(NOON)?);
    assert!(!matches!(process.lifecycle().state(), State::Exited(_)));
    assert_eq!(journal.received(), [OTHER_EXIT, EXIT]);
    let received: Vec<_> = journal
        .payloads("IntentReceived")
        .iter()
        .map(|payload| {
            (
                payload.get("type").cloned(),
                payload.get("limit_price").cloned(),
            )
        })
        .collect();
    assert_eq!(
        received[0],
        (Some(Value::Str("market".to_owned())), Some(Value::Null))
    );
    Ok(())
}

/// An exit goes under its agent's deployed mandate version, read from its `AgentDeployed`, even
/// when the mandate port knows none, as the binary's fixture port knows none: the version every
/// followed agent has, so no exit waits on the port (DEC-2034 item 1, `AGENTS.md` rule 13).
#[test]
fn an_exit_goes_under_its_agents_deployed_version_when_the_port_knows_none() -> Result<(), Error> {
    let journal = deployed()?;
    exit(&journal, AGENT, EXIT)?;
    let broker = Paper::new(&journal);
    held(&broker)?;
    let mut wiring = wired(Box::new(Allowing), Box::new(support::TwoPercentStop))?;
    wiring.mandates = Box::new(support::NoVersion);
    let mut process = start_wired(&journal, &broker, wiring)?;
    let alerts = process.step(at(NOON)?);
    assert!(
        alerts.iter().all(|alert| alert.key != "exit_held"),
        "{alerts:?}"
    );
    assert_eq!(journal.received(), [EXIT]);
    assert_eq!(
        journal.mandate_refs("IntentReceived"),
        [Some(support::VERSION.to_owned())]
    );
    assert!(!matches!(process.lifecycle().state(), State::Exited(_)));
    Ok(())
}

/// A risk or discretionary exit at the market is handed with no limit, received as a market
/// order, and the exit behind it still goes, on this pass and after a restart, which hands
/// nothing again (DEC-2037 item 7, `AGENTS.md` rule 13).
#[test]
fn an_exit_at_the_market_is_handed_with_no_limit() -> Result<(), Error> {
    for purpose in ["risk_exit", "discretionary_exit"] {
        let journal = deployed()?;
        propose_market_exit(&journal, AGENT, OTHER_EXIT, purpose, "decision")?;
        exit(&journal, AGENT, EXIT)?;
        let broker = Paper::new(&journal);
        held(&broker)?;
        let mut process = start(&journal, &broker)?;
        process.step(at(NOON)?);
        process.step(at(NOON)?);
        assert!(
            !matches!(process.lifecycle().state(), State::Exited(_)),
            "{purpose}"
        );
        assert_eq!(journal.received(), [OTHER_EXIT, EXIT], "{purpose}");
        let types: Vec<_> = journal
            .payloads("IntentReceived")
            .iter()
            .filter_map(|payload| payload.get("type").cloned())
            .collect();
        assert_eq!(
            types,
            [
                Value::Str("market".to_owned()),
                Value::Str("limit".to_owned())
            ],
            "{purpose}"
        );
        let mut restarted = start(&journal, &broker)?;
        restarted.step(at(NOON)?);
        assert!(
            !matches!(restarted.lifecycle().state(), State::Exited(_)),
            "{purpose}"
        );
        assert_eq!(journal.received(), [OTHER_EXIT, EXIT], "{purpose}");
    }
    Ok(())
}
