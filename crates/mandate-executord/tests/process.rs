//! The executor process (E21-30, DEC-2003): its start, its readiness, the kill switches it
//! applies and in what order, and that nothing reaches the broker before it is journaled.

mod support;

use mandate_canon::Value;
use mandate_executor::BrokerRequest;
use mandate_executord::ExecutorProcess;
use mandate_journal::Environment;
use mandate_process::{Exit, State};
use support::{
    ACCOUNT_STREAM, AGENT, CONTROL_STREAM, Error, Fault, Journal, NOON, OPENED, OPENING,
    OTHER_ACCOUNT, OTHER_AGENT, OTHER_CONNECTION, Paper, at, chained, connect, deploy, deployed,
    kill, kill_by, kill_scoped, mode_applied, object, opened, process_config, start, tick, wiring,
};

const SWITCH: &str = "01JKILL00000000000000000AA";
const OTHER_SWITCH: &str = "01JKILL00000000000000000AB";

/// Paper only (DEC-2003 item 9): any other environment is refused before the account stream is
/// taken, and the refused process takes no input after, so nothing is journaled and nothing is
/// asked of the broker.
#[test]
fn refuses_any_environment_but_paper() -> Result<(), Error> {
    let journal = deployed()?;
    kill(&journal, AGENT, SWITCH)?;
    let broker = Paper::new(&journal);
    let mut process = ExecutorProcess::start(
        journal.clone(),
        broker.clone(),
        wiring()?,
        &process_config(Environment::Live),
        at(NOON)?,
    );
    assert_eq!(process.lifecycle().state(), State::Exited(Exit::Refused));
    process.step(at(NOON)?);
    process.drain(at(NOON)?);
    assert_eq!(process.lifecycle().state(), State::Exited(Exit::Refused));
    assert!(journal.rows_of(ACCOUNT_STREAM).is_empty());
    assert!(broker.seen().is_empty());
    Ok(())
}

/// An empty account stream is opened once, at sequence 1, before the core starts (journal spec
/// §2); a restart replays it and opens nothing again.
#[test]
fn opens_an_empty_account_stream_once() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let mut first = start(&journal, &broker)?;
    first.step(at(NOON)?);
    let mut second = start(&journal, &broker)?;
    second.step(at(NOON)?);
    let types = journal.types(ACCOUNT_STREAM);
    assert_eq!(types.first().map(String::as_str), Some("StreamOpened"));
    assert_eq!(types.iter().filter(|t| *t == "StreamOpened").count(), 1);
    Ok(())
}

/// The core starts on the first pass that reads a tick (every record it leads to carries the
/// risk clock): with no tick, the process takes no input at all and asks the broker nothing.
#[test]
fn waits_for_the_first_tick_before_starting() -> Result<(), Error> {
    let journal = Journal::default();
    connect(&journal, "c1", "conn1", "acct1")?;
    deploy(&journal, "c2", AGENT, "conn1")?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert!(!process.state().started());
    assert!(broker.seen().is_empty());
    tick(&journal, "t1", NOON)?;
    process.step(at(NOON)?);
    assert!(process.state().started());
    Ok(())
}

/// Ready only once the startup reconciliation is clean (design §3.4): the broker's four reads are
/// one snapshot, and a broker position the ledger cannot explain leaves the process not ready.
#[test]
fn ready_only_after_a_clean_startup_reconciliation() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    assert!(!process.ready());
    process.step(at(NOON)?);
    assert!(process.ready(), "{:?}", process.lifecycle().state());
    let reads: Vec<_> = broker.seen().into_iter().map(|s| s.request).collect();
    assert!(matches!(
        reads.as_slice(),
        [
            BrokerRequest::ListOpenOrders,
            BrokerRequest::ListPositions,
            BrokerRequest::GetAccount,
            BrokerRequest::ListActivities { .. }
        ]
    ));

    let journal = deployed()?;
    let broker = Paper::new(&journal);
    broker.holding("AAPL", "10")?;
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert!(!process.ready());
    assert_eq!(process.lifecycle().state(), State::NotReady);
    Ok(())
}

/// A lost answer on a reconciliation read stops nothing: the reads change nothing at the broker,
/// so the pass goes on to the kill switch (`AGENTS.md` rule 13), and the reconciliation, still due,
/// is read again after it.
#[test]
fn a_lost_read_is_read_again_and_holds_no_kill_switch() -> Result<(), Error> {
    let journal = deployed()?;
    kill(&journal, AGENT, SWITCH)?;
    let broker = Paper::new(&journal);
    broker.lose_next_answers(1);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    assert!(process.ready());
    let reads = broker
        .seen()
        .iter()
        .filter(|seen| matches!(seen.request, BrokerRequest::ListOpenOrders))
        .count();
    assert_eq!(reads, 2);
    Ok(())
}

/// A process that is not ready still applies the kill switch (design §3.4, `AGENTS.md` rule 13).
#[test]
fn a_process_that_is_not_ready_applies_the_kill_switch() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    broker.holding("AAPL", "10")?;
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert!(!process.ready());
    kill(&journal, AGENT, SWITCH)?;
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    Ok(())
}

/// The kill switch is applied as the core's own switch of the agent, once: a later pass or a
/// restart never applies it again.
#[test]
fn a_kill_switch_is_applied_once_across_passes_and_restarts() -> Result<(), Error> {
    let journal = deployed()?;
    kill(&journal, AGENT, SWITCH)?;
    let broker = Paper::new(&journal);
    for _ in 0..3 {
        let mut process = start(&journal, &broker)?;
        for _ in 0..3 {
            process.step(at(NOON)?);
        }
    }
    assert_eq!(journal.switched(), [AGENT]);
    Ok(())
}

/// Whatever scope the runtime's copy records, the executor's switch touches only that agent
/// (`AGENTS.md` rule 13): its record names the agent scope, and no account-wide request reaches
/// the broker.
#[test]
fn a_wider_switch_is_applied_to_the_agent_alone() -> Result<(), Error> {
    for (scope, subject) in [("workspace", "ws1"), ("connection", "conn1")] {
        let journal = deployed()?;
        kill_scoped(&journal, AGENT, SWITCH, scope, subject)?;
        let broker = Paper::new(&journal);
        let mut process = start(&journal, &broker)?;
        process.step(at(NOON)?);
        let scopes: Vec<_> = journal
            .payloads("KillSwitchActivated")
            .iter()
            .map(|p| (p.get("scope").cloned(), p.get("subject").cloned()))
            .collect();
        assert_eq!(
            scopes,
            [(
                Some(Value::Str("agent".to_owned())),
                Some(Value::Str(AGENT.to_owned()))
            )],
            "{scope}"
        );
        assert!(broker.seen().iter().all(|seen| !matches!(
            seen.request,
            BrokerRequest::CancelAll(_) | BrokerRequest::ClosePosition(..)
        )));
    }
    Ok(())
}

/// A switch cancels the agent's working order by its own id, journals before sending, and asks
/// nothing account-wide (`AGENTS.md` rules 5 and 13); the pass goes on to the next agent's switch.
#[test]
fn a_switch_cancels_the_agents_working_order_by_its_id() -> Result<(), Error> {
    let journal = deployed()?;
    deploy(&journal, "c3", OTHER_AGENT, "conn1")?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    broker.resting(OPENING, "AAPL", "10", "150")?;
    kill(&journal, AGENT, SWITCH)?;
    kill(&journal, OTHER_AGENT, OTHER_SWITCH)?;
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    let mut switched = journal.switched();
    switched.sort();
    assert_eq!(switched, [AGENT, OTHER_AGENT]);
    let cancels: Vec<String> = broker
        .seen()
        .into_iter()
        .filter_map(|seen| match seen.request {
            BrokerRequest::Cancel { client_order_id } => Some(client_order_id.as_str().to_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(cancels, [OPENING]);
    assert!(broker.seen().iter().all(|seen| !matches!(
        seen.request,
        BrokerRequest::CancelAll(_) | BrokerRequest::ClosePosition(..)
    )));
    Ok(())
}

/// The account stream's last records: every agent stopped and an account-scoped kill switch
/// journaled after the recorded opening, as a process that crashed before its cancel-all, or
/// before its answer, leaves them (DEC-2333).
const ACCOUNT_SWITCHED: &str = include_str!("fixtures/account_switched.jsonl");

/// An account switch whose cancel-all is not confirmed resumes on the process's next start, and
/// the confirming read is the startup reconciliation the process gathers, not a separate listing
/// (DEC-2333 items 2 and 3): its open orders show the opening still working, so the opening is
/// cancelled by its own id. The cancel-all is never sent again.
#[test]
fn an_unconfirmed_account_switch_resumes_from_the_startup_read() -> Result<(), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    journal.seed(ACCOUNT_STREAM, ACCOUNT_SWITCHED)?;
    let broker = Paper::new(&journal);
    broker.resting(OPENING, "AAPL", "10", "150")?;
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    let sent: Vec<BrokerRequest> = broker.seen().into_iter().map(|seen| seen.request).collect();
    assert!(
        sent.iter().any(|request| matches!(
            request,
            BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == OPENING
        )),
        "the opening still working is cancelled by its id: {sent:?}"
    );
    assert!(
        !sent
            .iter()
            .any(|request| matches!(request, BrokerRequest::CancelAll(_))),
        "the cancel-all is never sent again: {sent:?}"
    );
    Ok(())
}

/// Every switch waiting in a pass is applied in that pass, for each agent of the account.
#[test]
fn every_switch_waiting_in_a_pass_is_applied_in_it() -> Result<(), Error> {
    let journal = deployed()?;
    deploy(&journal, "c3", OTHER_AGENT, "conn1")?;
    kill(&journal, AGENT, SWITCH)?;
    kill(&journal, OTHER_AGENT, OTHER_SWITCH)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    let mut switched = journal.switched();
    switched.sort();
    assert_eq!(switched, [AGENT, OTHER_AGENT]);
    Ok(())
}

/// Only the account's agents are followed: an agent deployed on another account's connection asks
/// this executor nothing, while the account's own agent's switch, in the same pass, is applied.
#[test]
fn only_the_accounts_agents_are_followed() -> Result<(), Error> {
    let journal = deployed()?;
    connect(&journal, "c3", OTHER_CONNECTION, OTHER_ACCOUNT)?;
    deploy(&journal, "c4", OTHER_AGENT, OTHER_CONNECTION)?;
    kill(&journal, OTHER_AGENT, OTHER_SWITCH)?;
    kill(&journal, AGENT, SWITCH)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    Ok(())
}

/// A version-1 `AgentDeployed` names no connection, so it neither reaches the executor nor ends
/// the following an earlier version-2 deployment on the account's connection began: the agent's
/// switch is still applied (DEC-1171 item 3, `AGENTS.md` rule 13).
#[test]
fn a_later_version_1_deployment_keeps_the_agent_followed() -> Result<(), Error> {
    let journal = deployed()?;
    journal.write(
        CONTROL_STREAM,
        "c4",
        "AgentDeployed",
        1,
        None,
        object(vec![
            ("agent_id", Value::Str(AGENT.to_owned())),
            ("mandate_version", Value::Str(support::VERSION.to_owned())),
        ])?,
    )?;
    kill(&journal, AGENT, SWITCH)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    Ok(())
}

/// An agent whose deployment stopped is still followed: a switch on its stream is applied, since
/// a stopped agent may still hold orders or a position (`AGENTS.md` rule 13).
#[test]
fn a_stopped_agents_switch_is_still_applied() -> Result<(), Error> {
    let journal = deployed()?;
    journal.write(
        CONTROL_STREAM,
        "c4",
        "AgentStopped",
        1,
        None,
        object(vec![("agent_id", Value::Str(AGENT.to_owned()))])?,
    )?;
    kill(&journal, AGENT, SWITCH)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    Ok(())
}

/// A revoked connection's agents are not followed until the connection is established again: the
/// latest establishment or revocation decides (DEC-1171 item 4).
#[test]
fn a_revoked_connection_is_followed_again_once_established_again() -> Result<(), Error> {
    let journal = deployed()?;
    journal.write(
        CONTROL_STREAM,
        "c3",
        "ConnectionRevoked",
        1,
        None,
        object(vec![("connection_id", Value::Str("conn1".to_owned()))])?,
    )?;
    kill(&journal, AGENT, SWITCH)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert!(journal.switched().is_empty());
    connect(&journal, "c4", "conn1", "acct1")?;
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    Ok(())
}

/// An append whose answer is lost is offered again first, as the same batch; the journal answers
/// that it holds it, and the switch is recorded once.
#[test]
fn a_lost_answer_is_offered_again_and_written_once() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    kill(&journal, AGENT, SWITCH)?;
    journal.fail_next(&[Fault::LostAnswer]);
    process.step(at(NOON)?);
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    Ok(())
}

/// One minute before noon, on the risk clock: a restriction an earlier executor journaled.
fn before_noon() -> Result<u64, Error> {
    Ok(u64::try_from(at(NOON)?.secs())?.saturating_sub(60))
}

/// A pause of every agent (`*`, a drawdown rung) does not make a risk limit's kill switch count
/// as applied: whether a switch was applied is read from the switch's own record, never from a
/// mode another restriction set (DEC-2003 item 4; the #1596 review's major).
#[test]
fn a_pause_of_every_agent_does_not_swallow_a_kill_switch() -> Result<(), Error> {
    let journal = deployed()?;
    opened(&journal)?;
    mode_applied(&journal, "m1", "*", "paused", "drawdown", before_noon()?)?;
    kill_by(&journal, AGENT, SWITCH, "risk_limit")?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    Ok(())
}

/// An earlier pause of the agent itself does not swallow its kill switch either.
#[test]
fn an_earlier_pause_of_the_agent_does_not_swallow_a_kill_switch() -> Result<(), Error> {
    let journal = deployed()?;
    opened(&journal)?;
    mode_applied(
        &journal,
        "m1",
        AGENT,
        "paused",
        "owner_pause",
        before_noon()?,
    )?;
    kill_by(&journal, AGENT, SWITCH, "risk_limit")?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    Ok(())
}

/// An account the broker blocks from trading does not swallow a kill switch.
#[test]
fn a_blocked_account_does_not_swallow_a_kill_switch() -> Result<(), Error> {
    let journal = deployed()?;
    kill_by(&journal, AGENT, SWITCH, "risk_limit")?;
    let broker = Paper::new(&journal);
    broker.block();
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    Ok(())
}

/// After an applied switch is lifted (its agent back to `normal`), a restart does not apply the
/// same switch again.
#[test]
fn a_lifted_switch_is_not_applied_again() -> Result<(), Error> {
    let journal = deployed()?;
    kill(&journal, AGENT, SWITCH)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    let noon = u64::try_from(at(NOON)?.secs())?;
    mode_applied(&journal, "m9", AGENT, "normal", "kill_switch", noon)?;
    let mut restarted = start(&journal, &broker)?;
    restarted.step(at(NOON)?);
    restarted.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    Ok(())
}

/// A row on one agent's stream that cannot be read stops the process only after the switches of
/// the agents whose streams read are applied (`AGENTS.md` rule 13).
#[test]
fn an_unreadable_row_stops_the_process_after_the_readable_switches() -> Result<(), Error> {
    let journal = deployed()?;
    deploy(&journal, "c3", OTHER_AGENT, "conn1")?;
    journal.write(
        &support::agent_stream(AGENT),
        SWITCH,
        "KillSwitchActivated",
        1,
        Some("command"),
        object(vec![("initiator", Value::Str("someone".to_owned()))])?,
    )?;
    kill(&journal, OTHER_AGENT, OTHER_SWITCH)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [OTHER_AGENT]);
    assert_eq!(
        process.lifecycle().state(),
        State::Exited(Exit::CycleFailed)
    );
    Ok(())
}

/// A deployment or connection record on the control stream that cannot be read is never skipped:
/// the process stops rather than drop an agent it may owe a kill switch (DEC-85).
#[test]
fn an_unreadable_control_record_stops_the_process() -> Result<(), Error> {
    for (event_type, version) in [
        ("AgentDeployed", 2),
        ("ConnectionEstablished", 2),
        ("AgentStopped", 1),
        ("ConnectionRevoked", 1),
    ] {
        let journal = deployed()?;
        journal.write(
            CONTROL_STREAM,
            "c9",
            event_type,
            version,
            None,
            object(vec![])?,
        )?;
        let broker = Paper::new(&journal);
        let mut process = start(&journal, &broker)?;
        process.step(at(NOON)?);
        assert_eq!(
            process.lifecycle().state(),
            State::Exited(Exit::CycleFailed),
            "{event_type}"
        );
    }
    Ok(())
}

/// A stream that cannot be read raises one opaque alert when it stops reading, and another only
/// after it has read again (`AGENTS.md` rule 6: a fixed key, no subject).
#[test]
fn a_stream_that_stops_reading_raises_one_opaque_alert() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    let unreadable = |alerts: &[mandate_executord::Alert]| {
        alerts
            .iter()
            .filter(|a| a.key == "stream_unreadable" && a.subject.is_none())
            .count()
    };
    journal.unreadable(CONTROL_STREAM);
    assert_eq!(unreadable(&process.step(at(NOON)?)), 1);
    assert_eq!(unreadable(&process.step(at(NOON)?)), 0);
    journal.readable(CONTROL_STREAM);
    assert_eq!(unreadable(&process.step(at(NOON)?)), 0);
    journal.unreadable(CONTROL_STREAM);
    assert_eq!(unreadable(&process.step(at(NOON)?)), 1);
    Ok(())
}

/// A retry that commits does not end the pass: a switch that arrived meanwhile is applied in the
/// same pass.
#[test]
fn a_pass_goes_on_after_a_retry_commits() -> Result<(), Error> {
    let journal = deployed()?;
    deploy(&journal, "c3", OTHER_AGENT, "conn1")?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    kill(&journal, AGENT, SWITCH)?;
    journal.fail_next(&[Fault::Unavailable]);
    process.step(at(NOON)?);
    assert!(journal.switched().is_empty());
    kill(&journal, OTHER_AGENT, OTHER_SWITCH)?;
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT, OTHER_AGENT]);
    Ok(())
}

/// One input may lead only `max_chain` broker answers deep: at 0 the process stops at the first
/// answer after `Input::Started` commits, and at 1 the startup reconciliation, one answer deep,
/// makes it ready.
#[test]
fn the_chain_bound_stops_the_process_one_answer_past_it() -> Result<(), Error> {
    for (bound, ready) in [(0, false), (1, true)] {
        let journal = deployed()?;
        let broker = Paper::new(&journal);
        let mut process = ExecutorProcess::start(
            journal.clone(),
            broker.clone(),
            wiring()?,
            &chained(Environment::Paper, bound),
            at(NOON)?,
        );
        process.step(at(NOON)?);
        assert!(process.state().started());
        assert_eq!(process.ready(), ready, "bound {bound}");
        if !ready {
            assert_eq!(
                process.lifecycle().state(),
                State::Exited(Exit::CycleFailed)
            );
        }
    }
    Ok(())
}

/// A second process fences the first: the first's next append answers `Fenced`, and it exits
/// before applying anything more.
#[test]
fn a_fenced_process_does_nothing_more() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let mut first = start(&journal, &broker)?;
    first.step(at(NOON)?);
    let _second = start(&journal, &broker)?;
    kill(&journal, AGENT, SWITCH)?;
    first.step(at(NOON)?);
    assert_eq!(first.lifecycle().state(), State::Exited(Exit::Fenced));
    assert!(journal.switched().is_empty());
    Ok(())
}

/// A drain hands nothing new: a batch in doubt is tried once more, and the process exits
/// `Drained`, or `DrainIncomplete` when it is still in doubt.
#[test]
fn a_drain_settles_what_is_in_doubt_or_says_it_did_not() -> Result<(), Error> {
    for (faults, exit, switched) in [
        (1, Exit::Drained, vec![AGENT]),
        (2, Exit::DrainIncomplete, vec![]),
    ] {
        let journal = deployed()?;
        let broker = Paper::new(&journal);
        let mut process = start(&journal, &broker)?;
        process.step(at(NOON)?);
        kill(&journal, AGENT, SWITCH)?;
        journal.fail_next(&vec![Fault::Unavailable; faults]);
        process.step(at(NOON)?);
        process.drain(at(NOON)?);
        assert_eq!(process.lifecycle().state(), State::Exited(exit));
        assert_eq!(journal.switched(), switched);
    }
    Ok(())
}

/// An agent stream whose `KillSwitchActivated` does not read is never skipped: the process stops.
#[test]
fn an_unreadable_switch_stops_the_process() -> Result<(), Error> {
    let journal = deployed()?;
    journal.write(
        &support::agent_stream(AGENT),
        SWITCH,
        "KillSwitchActivated",
        1,
        Some("command"),
        object(vec![("initiator", Value::Str("someone".to_owned()))])?,
    )?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(
        process.lifecycle().state(),
        State::Exited(Exit::CycleFailed)
    );
    Ok(())
}

/// A control stream that cannot be read applies nothing that pass and stops nothing: the next
/// readable pass applies what is waiting.
#[test]
fn an_unreadable_control_stream_holds_nothing_back_for_good() -> Result<(), Error> {
    let journal = deployed()?;
    kill(&journal, AGENT, SWITCH)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    journal.unreadable(CONTROL_STREAM);
    process.step(at(NOON)?);
    assert!(journal.switched().is_empty());
    assert!(!matches!(process.lifecycle().state(), State::Exited(_)));
    journal.readable(CONTROL_STREAM);
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    Ok(())
}
