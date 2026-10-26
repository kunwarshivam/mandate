//! The executor's run behind journal rule 68's gate (DEC-2341), and the wait before it on a
//! connection that is still connecting (DEC-2346). The run is driven here as the binary drives it,
//! with a wiring that admits every opening, so only the gate the run installs can hold one.

mod support;

use std::cell::{Cell, RefCell};
use std::time::Duration;

use mandate_canon::Value;
use mandate_executord::alpaca::AlpacaBroker;
use mandate_executord::paper::FixturePaper;
use mandate_executord::run;
use mandate_executord::serve::{Edge, STARTUP, Waited, serve, serve_run, starting, wait};
use mandate_journal::Environment;
use mandate_process::probe::{Probe, Status, answer};
use mandate_process::{Exit, Snapshot, State};
use mandate_time::UtcNanos;
use support::{
    ACCOUNT, ACCOUNT_STREAM, AGENT, Allowing, CONNECTION, CONTROL_STREAM, Error, Journal, NOON,
    Paper, TwoPercentStop, at, connect, deployed, object, opened, process_config, propose, tick,
    wired,
};

const CHECK: &str = "e1h1o1";
const ESTABLISHED: &str = "c9";
const OPENING: &str = "md-01JINTENT0000000000000000A";

fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

/// A journal whose account stream's latest `connect` check passed for the connection, with no
/// establishment of it yet, and an opening the agent proposed.
fn connecting() -> Result<Journal, Error> {
    let journal = deployed()?;
    opened(&journal)?;
    checked(&journal)?;
    propose(
        &journal,
        AGENT,
        "01JINTENT0000000000000000A",
        "buy",
        "10",
        "150",
        "open",
    )?;
    Ok(journal)
}

/// The opaque reference the connect check names the account by (journal spec rule 62).
const PII: &str = "pii_01J0000000000000000000000A";

/// The account stream's `connect` check of the connection, every result passed.
fn checked(journal: &Journal) -> Result<(), Error> {
    checked_naming(journal, Some(PII))
}

/// That check, naming the account by `reference`.
fn checked_naming(journal: &Journal, reference: Option<&str>) -> Result<(), Error> {
    let row = |check: &str| {
        object(vec![
            ("check", text(check)),
            ("reason", Value::Null),
            ("result", text("passed")),
        ])
    };
    journal.write(
        ACCOUNT_STREAM,
        CHECK,
        "ConnectionChecked",
        1,
        None,
        object(vec![
            ("account_pii_ref", reference.map_or(Value::Null, text)),
            ("connection_id", text(CONNECTION)),
            ("occasion", text("connect")),
            (
                "results",
                Value::Array(vec![row("account")?, row("environment")?, row("scope")?]),
            ),
            ("risk_clock", text("2026-09-21T14:59:00.000000000Z")),
        ])?,
    )
}

/// The control services' version-2 `ConnectionEstablished` of that check.
fn establish(journal: &Journal) -> Result<(), Error> {
    journal.write(
        CONTROL_STREAM,
        ESTABLISHED,
        "ConnectionEstablished",
        2,
        Some(CHECK),
        object(vec![
            ("account_ref", text(ACCOUNT)),
            ("broker", text("alpaca")),
            ("connection_id", text(CONNECTION)),
            ("environment", text("paper")),
            ("margin_attestation", Value::Null),
            ("scopes", Value::Array(vec![text("data"), text("trading")])),
            ("step_up", Value::Null),
            ("user", text("usr_1")),
        ])?,
    )
}

/// A `ConnectionRefused` of the connect.
fn refuse(journal: &Journal) -> Result<(), Error> {
    journal.write(
        CONTROL_STREAM,
        "c10",
        "ConnectionRefused",
        1,
        None,
        object(vec![
            ("connection_id", text(CONNECTION)),
            ("occasion", text("connect")),
        ])?,
    )
}

fn copies(journal: &Journal) -> usize {
    journal
        .types(ACCOUNT_STREAM)
        .iter()
        .filter(|t| *t == "ConnectionEstablished")
        .count()
}

/// What an edge saw: its passes, which each tick once, and the lines it said.
#[derive(Default)]
struct Seen {
    passes: Cell<usize>,
    said: RefCell<Vec<String>>,
}

/// The pass at which a test's edge stops a run that neither SIGTERM nor a stop code drained: its
/// tick answers [`UNDRAINED`] and SIGTERM arrives together, so such a run ends there, and saying
/// that code, instead of spinning until the mutation run's timeout.
const PASS_BOUND: usize = 1_000;

/// The stop code of a run that reached [`PASS_BOUND`].
const UNDRAINED: &str = "undrained_at_pass_bound";

/// Runs `body` with an edge at noon whose tick counts a pass, runs `each` at that pass's number,
/// and answers `lapse` once that many passes have run; SIGTERM arrives after `stop` passes. Either
/// alone must drain the run; [`PASS_BOUND`] ends one that did not.
fn with_edge<T>(
    stop: usize,
    lapse: Option<usize>,
    each: &dyn Fn(usize) -> Result<(), Error>,
    body: impl FnOnce(Edge<'_>, &Seen) -> T,
) -> Result<T, Error> {
    with_edge_stopping(
        stop,
        lapse.map(|after| (after, "credential_lost")),
        each,
        body,
    )
}

/// [`with_edge`] with the lease's stop code chosen: `lapse` is the pass and the code its tick
/// answers from then on.
fn with_edge_stopping<T>(
    stop: usize,
    lapse: Option<(usize, &'static str)>,
    each: &dyn Fn(usize) -> Result<(), Error>,
    body: impl FnOnce(Edge<'_>, &Seen) -> T,
) -> Result<T, Error> {
    let noon = at(NOON)?;
    let seen = Seen::default();
    let failed = RefCell::new(None);
    let now = move || noon;
    let mut tick = |_: UtcNanos| {
        let pass = seen.passes.get().saturating_add(1);
        seen.passes.set(pass);
        if let Err(e) = each(pass) {
            failed.borrow_mut().get_or_insert(e.to_string());
        }
        if pass >= PASS_BOUND {
            return Some(UNDRAINED);
        }
        lapse
            .filter(|(after, _)| pass >= *after)
            .map(|(_, code)| code)
    };
    let terminated = || seen.passes.get() >= stop.min(PASS_BOUND);
    let mut say = |line: &str| seen.said.borrow_mut().push(line.to_owned());
    let edge = Edge {
        now: &now,
        tick: &mut tick,
        terminated: &terminated,
        say: &mut say,
        interval: Duration::ZERO,
    };
    let out = body(edge, &seen);
    if let Some(e) = failed.into_inner() {
        return Err(e.into());
    }
    Ok(out)
}

fn starting_status() -> Status {
    Status::new(Snapshot::unbounded(State::Starting))
}

/// The run installs the gate itself: with a wiring that admits every opening, nothing opens
/// while the stream is connecting. A pass follows each step, so the step after the control
/// services establish the connection finds its copy written, and the held opening then goes.
#[test]
fn the_run_holds_an_opening_until_its_copy_and_passes_after_each_step() -> Result<(), Error> {
    let journal = connecting()?;
    let broker = Paper::new(&journal);
    let before = RefCell::new(None);
    let copied_by = RefCell::new(None);
    let each = |pass: usize| {
        if pass == 4 {
            before.replace(Some(broker.submitted()));
            establish(&journal)?;
        }
        if pass == 5 {
            copied_by.replace(Some(copies(&journal)));
        }
        Ok(())
    };
    let wiring = wired(Box::new(Allowing), Box::new(TwoPercentStop))?;
    let status = starting_status();
    let exit = with_edge(9, None, &each, |edge, _| {
        serve(
            journal.clone(),
            broker.clone(),
            wiring,
            &process_config(Environment::Paper),
            &status,
            edge,
        )
    })?;
    assert_eq!(exit, Ok(0));
    assert_eq!(
        before.into_inner(),
        Some(Vec::new()),
        "nothing opened while connecting"
    );
    assert_eq!(
        copied_by.into_inner(),
        Some(1),
        "the pass after that step copied it"
    );
    assert_eq!(copies(&journal), 1);
    assert!(broker.submitted().iter().any(|id| id == OPENING));
    assert_eq!(status.snapshot().state(), State::Exited(Exit::Drained));
    Ok(())
}

/// A closed connection's code is said once, however many passes keep the gate shut, then the
/// drain's own; the run is not stopped by it, and SIGTERM alone drains it at its pass.
#[test]
fn a_gate_code_is_said_once_per_change() -> Result<(), Error> {
    let journal = connecting()?;
    refuse(&journal)?;
    let broker = Paper::new(&journal);
    let wiring = wired(Box::new(Allowing), Box::new(TwoPercentStop))?;
    let said = with_edge(6, None, &|_| Ok(()), |edge, seen| {
        let exit = serve(
            journal.clone(),
            broker.clone(),
            wiring,
            &process_config(Environment::Paper),
            &starting_status(),
            edge,
        );
        (exit, seen.passes.get(), seen.said.borrow().clone())
    })?;
    assert_eq!(
        said,
        (
            Ok(0),
            6,
            vec!["connection_closed".to_owned(), "drained".to_owned()]
        )
    );
    assert!(broker.submitted().is_empty());
    Ok(())
}

/// A lease that stops the run drains it at that pass, with no SIGTERM, and the lease's code is
/// said before the drain's.
#[test]
fn a_lapsed_lease_drains_the_run() -> Result<(), Error> {
    let journal = connecting()?;
    establish(&journal)?;
    let broker = Paper::new(&journal);
    let wiring = wired(Box::new(Allowing), Box::new(TwoPercentStop))?;
    let said = with_edge(usize::MAX, Some(3), &|_| Ok(()), |edge, seen| {
        let exit = serve(
            journal.clone(),
            broker.clone(),
            wiring,
            &process_config(Environment::Paper),
            &starting_status(),
            edge,
        );
        (exit, seen.passes.get(), seen.said.borrow().clone())
    })?;
    assert_eq!(
        said,
        (
            Ok(0),
            3,
            vec!["credential_lost".to_owned(), "drained".to_owned()]
        )
    );
    Ok(())
}

/// The causes of the restrictions the account stream holds, in order.
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

fn member(payload: &Value, name: &str) -> String {
    payload
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// An input still in doubt when the lease is lost is handed again first, and the restriction
/// follows it once it commits: with the journal down from the first pass and back at the lapse,
/// the start's records commit and the account is restricted (DEC-2839 item 2).
#[test]
fn a_lost_credential_hands_an_input_in_doubt_first() -> Result<(), Error> {
    let journal = connecting()?;
    establish(&journal)?;
    let broker = Paper::new(&journal);
    let wiring = wired(Box::new(Allowing), Box::new(TwoPercentStop))?;
    let each = |pass: usize| {
        if pass == 1 {
            journal.fail_next(&[support::Fault::Unavailable; 64]);
        }
        if pass == 3 {
            journal.clear_faults();
        }
        Ok(())
    };
    let before = journal.types(ACCOUNT_STREAM).len();
    let exit = with_edge_stopping(3, Some((3, "credential_lost")), &each, |edge, _| {
        serve(
            journal.clone(),
            broker.clone(),
            wiring,
            &process_config(Environment::Paper),
            &starting_status(),
            edge,
        )
    })?;
    assert_eq!(exit, Ok(0));
    let types = journal.types(ACCOUNT_STREAM);
    let restricted = types
        .iter()
        .position(|kind| kind == "AccountRestrictionChanged")
        .ok_or("no restriction")?;
    assert!(
        restricted > before,
        "what was in doubt committed ahead of the restriction: {types:?}"
    );
    assert_eq!(restriction_causes(&journal), ["connection_unavailable"]);
    Ok(())
}

/// An input in doubt when the lease is lost is handed again first and never replaced: with the
/// journal down for one more append, the restriction is held in doubt and the drain commits it
/// (DEC-2839).
#[test]
fn a_lost_credential_with_the_journal_down_restricts_through_the_drain() -> Result<(), Error> {
    let journal = connecting()?;
    establish(&journal)?;
    let broker = Paper::new(&journal);
    let wiring = wired(Box::new(Allowing), Box::new(TwoPercentStop))?;
    let each = |pass: usize| {
        if pass == 3 {
            journal.fail_next(&[support::Fault::Unavailable]);
        }
        Ok(())
    };
    let exit = with_edge_stopping(3, Some((3, "credential_lost")), &each, |edge, _| {
        serve(
            journal.clone(),
            broker.clone(),
            wiring,
            &process_config(Environment::Paper),
            &starting_status(),
            edge,
        )
    })?;
    assert_eq!(exit, Ok(0), "the drain committed what was in doubt");
    assert_eq!(restriction_causes(&journal), ["connection_unavailable"]);
    Ok(())
}

/// DEC-2839: a credential the lease lost restricts the account to closing, cause
/// `connection_unavailable`, before the run drains (connections spec §9.2: at lease expiry the
/// connection is treated as `suspended` for openings), so the restart finds it in force. A
/// rotation, or SIGTERM, drains with no restriction: neither is a trouble of the connection.
#[test]
fn a_lost_credential_restricts_the_account_before_the_drain() -> Result<(), Error> {
    for (lapse, restricted) in [
        (Some((3, "credential_lost")), true),
        (Some((3, "credential_rotated")), false),
        (None, false),
    ] {
        let journal = connecting()?;
        establish(&journal)?;
        let broker = Paper::new(&journal);
        let wiring = wired(Box::new(Allowing), Box::new(TwoPercentStop))?;
        let exit = with_edge_stopping(3, lapse, &|_| Ok(()), |edge, _| {
            serve(
                journal.clone(),
                broker.clone(),
                wiring,
                &process_config(Environment::Paper),
                &starting_status(),
                edge,
            )
        })?;
        assert_eq!(exit, Ok(0), "{lapse:?}");
        let expected: &[&str] = if restricted {
            &["connection_unavailable"]
        } else {
            &[]
        };
        assert_eq!(restriction_causes(&journal), expected, "{lapse:?}");
        if restricted {
            let types = journal.types(ACCOUNT_STREAM);
            let at = |kind: &str| types.iter().position(|t| t == kind);
            let (restricted_at, changed_at) = (
                at("AccountRestrictionChanged"),
                at("ConnectionStateChanged"),
            );
            assert!(
                restricted_at.is_some() && changed_at.is_some() && restricted_at < changed_at,
                "the restriction, then the state change: {types:?}"
            );
            let change = journal.payloads("ConnectionStateChanged");
            assert_eq!(
                change
                    .iter()
                    .map(|p| (member(p, "from"), member(p, "to"), member(p, "reason")))
                    .collect::<Vec<_>>(),
                [(
                    "active".to_owned(),
                    "suspended".to_owned(),
                    "lease_expired".to_owned()
                )]
            );
            let alerts = journal.payloads("OwnerAlertSent");
            assert!(
                alerts
                    .iter()
                    .any(|p| member(p, "kind") == "connection_unavailable"),
                "the owner's alert is journaled for the dispatcher: {alerts:?}"
            );
            let modes = journal.payloads("AgentModeApplied");
            assert!(
                modes.iter().any(|payload| {
                    payload.get("to").and_then(Value::as_str) == Some("exits_only")
                }),
                "every agent exits only: {modes:?}"
            );
        }
    }
    Ok(())
}

/// Runs `journal`'s connection over `broker` for one pass at each of `offsets` (seconds after
/// noon 2026-09-21), `before` run ahead of each pass with its index, then SIGTERM.
fn run_at(
    journal: &Journal,
    broker: &Paper,
    offsets: &[i64],
    before: &dyn Fn(usize),
) -> Result<Result<u8, &'static str>, Error> {
    let noon = at(NOON)?.secs();
    let pass = Cell::new(0usize);
    let instants: Vec<UtcNanos> = offsets
        .iter()
        .map(|offset| UtcNanos::from_parts(noon + offset, 0))
        .collect::<Result<_, _>>()?;
    let last = instants.last().copied().ok_or("no pass")?;
    let now = || instants.get(pass.get()).copied().unwrap_or(last);
    let mut tick = |_: UtcNanos| {
        before(pass.get());
        None
    };
    let terminated = || {
        let next = pass.get().saturating_add(1);
        pass.set(next);
        next > offsets.len()
    };
    let mut say = |_: &str| {};
    let wiring = wired(Box::new(Allowing), Box::new(TwoPercentStop))?;
    Ok(serve(
        journal.clone(),
        broker.clone(),
        wiring,
        &process_config(Environment::Paper),
        &starting_status(),
        Edge {
            now: &now,
            tick: &mut tick,
            terminated: &terminated,
            say: &mut say,
            interval: Duration::ZERO,
        },
    ))
}

/// The occasions of the account stream's `ConnectionChecked` records, in order.
fn check_occasions(journal: &Journal) -> Vec<String> {
    journal
        .payloads("ConnectionChecked")
        .iter()
        .map(|payload| member(payload, "occasion"))
        .collect()
}

/// DEC-2840: an established run checks the connection's permissions once at its start and once
/// each later New York day (connections spec §8.1), naming the account by the connect check's
/// reference; a passed check restricts nothing.
#[test]
fn an_established_run_checks_its_permissions_at_start_and_daily() -> Result<(), Error> {
    let journal = connecting()?;
    establish(&journal)?;
    let broker = Paper::new(&journal);
    let day = 86_400;
    let exit = run_at(
        &journal,
        &broker,
        &[0, 60, 120, day, day + 60, 2 * day],
        &|_| {},
    )?;
    assert_eq!(exit, Ok(0));
    assert_eq!(
        check_occasions(&journal),
        ["connect", "executor_start", "daily", "daily"]
    );
    let later = journal.payloads("ConnectionChecked");
    assert!(
        later
            .iter()
            .skip(1)
            .all(|payload| member(payload, "account_pii_ref") == PII),
        "{later:?}"
    );
    assert!(restriction_causes(&journal).is_empty());
    Ok(())
}

/// DEC-2840: a later check that fails suspends the connection (journal spec §9.8: a failed
/// `account` check suspends with `check_failed`): the restriction first, then the state change.
/// Here no connect-time check named the account, so the later one cannot name it and fails.
#[test]
fn a_failed_later_check_suspends_the_connection() -> Result<(), Error> {
    let journal = deployed()?;
    opened(&journal)?;
    checked_naming(&journal, None)?;
    establish(&journal)?;
    let broker = Paper::new(&journal);
    let exit = run_at(&journal, &broker, &[0, 60, 120, 180], &|_| {})?;
    assert_eq!(exit, Ok(0));
    assert_eq!(check_occasions(&journal), ["connect", "executor_start"]);
    let failed = journal.payloads("ConnectionChecked");
    assert_eq!(
        failed
            .get(1)
            .map(|payload| member(payload, "account_pii_ref")),
        Some(String::new()),
        "an unread account names no reference (rule 62)"
    );
    assert_eq!(restriction_causes(&journal), ["connection_unavailable"]);
    let changes: Vec<(String, String)> = journal
        .payloads("ConnectionStateChanged")
        .iter()
        .map(|payload| (member(payload, "to"), member(payload, "reason")))
        .collect();
    assert_eq!(
        changes,
        [("suspended".to_owned(), "check_failed".to_owned())]
    );
    Ok(())
}

/// DEC-2840: a check whose account read is lost cannot run, so nothing is journaled for it and it
/// stays due; it counts toward the connection's network failures, never as passed.
#[test]
fn a_check_whose_read_is_lost_runs_on_a_later_pass() -> Result<(), Error> {
    let journal = connecting()?;
    establish(&journal)?;
    let broker = Paper::new(&journal);
    let exit = run_at(&journal, &broker, &[0, 60, 120, 180, 240], &|pass| {
        if pass == 1 {
            broker.lose_next_answers(1_000);
        }
        if pass == 3 {
            broker.lose_next_answers(0);
        }
    })?;
    assert_eq!(exit, Ok(0));
    assert_eq!(check_occasions(&journal), ["connect", "executor_start"]);
    Ok(())
}

/// DEC-2346: the wait reads the journal until the control services establish the connection,
/// appends nothing meanwhile, and is live and not ready to the probes.
#[test]
fn the_wait_appends_nothing_until_the_connection_is_due() -> Result<(), Error> {
    let journal = connecting()?;
    let rows = journal.rows_of(ACCOUNT_STREAM).len();
    let status = starting_status();
    let probed = RefCell::new(Vec::new());
    let each = |pass: usize| {
        let snapshot = status.snapshot();
        let noon = at(NOON)?;
        probed.borrow_mut().push((
            answer(Probe::Live, snapshot, noon).ok,
            answer(Probe::Ready, snapshot, noon).ok,
        ));
        if pass == 3 {
            establish(&journal)?;
        }
        Ok(())
    };
    let waited = with_edge(usize::MAX, None, &each, |mut edge, seen| {
        let waited = wait(
            &journal,
            &process_config(Environment::Paper),
            &status,
            &mut edge,
        );
        (waited, seen.passes.get())
    })?;
    assert_eq!(waited, (Ok(Waited::Ready), 3));
    assert_eq!(journal.rows_of(ACCOUNT_STREAM).len(), rows);
    assert_eq!(probed.into_inner(), [(true, false); 3]);
    Ok(())
}

/// A stream already holding its copy, or due it, ends the wait at once.
#[test]
fn an_established_or_due_stream_ends_the_wait_at_once() -> Result<(), Error> {
    let journal = connecting()?;
    establish(&journal)?;
    let waited = with_edge(usize::MAX, None, &|_| Ok(()), |mut edge, seen| {
        let waited = wait(
            &journal,
            &process_config(Environment::Paper),
            &starting_status(),
            &mut edge,
        );
        (waited, seen.passes.get())
    })?;
    assert_eq!(waited, (Ok(Waited::Ready), 0));
    Ok(())
}

/// A connection refused before its copy, a lease that lapses, and a record that does not read
/// each end the wait with their code, before anything started; SIGTERM ends it as stopped.
#[test]
fn the_wait_ends_fail_safe() -> Result<(), Error> {
    let closed = connecting()?;
    refuse(&closed)?;
    let unreadable = connecting()?;
    unreadable.write(
        CONTROL_STREAM,
        "c10",
        "ConnectionRefused",
        1,
        None,
        object(vec![("occasion", text("connect"))])?,
    )?;
    let routes = [
        (closed, usize::MAX, None, Err("connection_closed")),
        (connecting()?, usize::MAX, Some(2), Err("credential_lost")),
        (
            unreadable,
            usize::MAX,
            None,
            Err("connection_record_unreadable"),
        ),
        (connecting()?, 2, None, Ok(Waited::Stopped)),
    ];
    for (journal, stop, lapse, expected) in routes {
        let rows = journal.rows_of(ACCOUNT_STREAM).len();
        let waited = with_edge(stop, lapse, &|_| Ok(()), |mut edge, _| {
            wait(
                &journal,
                &process_config(Environment::Paper),
                &starting_status(),
                &mut edge,
            )
        })?;
        assert_eq!(waited, expected);
        assert_eq!(journal.rows_of(ACCOUNT_STREAM).len(), rows);
    }
    Ok(())
}

/// DEC-2346 item 2 and DEC-2347: a connection refused before its copy, on an account where no
/// agent is deployed, ends the wait cleanly, since nothing was started and nothing is owed; with
/// an agent deployed it ends as `connection_closed`, for the manager.
#[test]
fn a_closed_connection_with_no_agent_ends_the_wait_cleanly() -> Result<(), Error> {
    let agentless = Journal::default();
    connect(&agentless, "c1", CONNECTION, ACCOUNT)?;
    tick(&agentless, "t1", NOON)?;
    let deployed = connecting()?;
    for journal in [&agentless, &deployed] {
        if journal.types(ACCOUNT_STREAM).is_empty() {
            opened(journal)?;
            checked(journal)?;
        }
        refuse(journal)?;
    }
    let ended = |journal: &Journal| {
        with_edge(usize::MAX, None, &|_| Ok(()), |mut edge, _| {
            wait(
                journal,
                &process_config(Environment::Paper),
                &starting_status(),
                &mut edge,
            )
        })
    };
    assert_eq!(ended(&agentless)?, Ok(Waited::Closed));
    assert_eq!(ended(&deployed)?, Err("connection_closed"));
    Ok(())
}

/// The probe holds a start live for [`STARTUP`] and no longer, so a start that hangs (a vault
/// login that never answers) is restarted.
#[test]
fn a_start_is_live_only_within_its_bound() -> Result<(), Error> {
    let start = at(NOON)?;
    let snapshot = starting(start);
    assert_eq!(snapshot.state(), State::Starting);
    let after = |secs: u64| -> Result<UtcNanos, Error> {
        Ok(UtcNanos::from_parts(
            start.secs() + i64::try_from(secs)?,
            0,
        )?)
    };
    assert!(answer(Probe::Live, snapshot, start).ok);
    assert!(answer(Probe::Live, snapshot, after(STARTUP.as_secs())?).ok);
    assert!(!answer(Probe::Live, snapshot, after(STARTUP.as_secs() + 1)?).ok);
    assert!(!answer(Probe::Ready, snapshot, start).ok);
    Ok(())
}

/// DEC-2359 item 9: the binary's serve path runs a paired run as the scripted one runs: the
/// fixture's, on a journal that has not yet started, drains at SIGTERM and publishes the drain.
#[test]
fn a_fixture_run_serves_until_drained() -> Result<(), Error> {
    let journal = deployed()?;
    let run = run::fixture(
        AlpacaBroker::new(FixturePaper::new(at(NOON)?))?,
        &process_config(Environment::Paper).scope,
    )?;
    let status = starting_status();
    let (exit, said) = with_edge(2, None, &|_| Ok(()), |edge, seen| {
        let exit = serve_run(
            journal.clone(),
            run,
            &process_config(Environment::Paper),
            &status,
            edge,
        );
        (exit, seen.said.borrow().clone())
    })?;
    assert_eq!(exit, Ok(0));
    assert!(said.iter().any(|line| line == "drained"), "{said:?}");
    assert_eq!(status.snapshot().state(), State::Exited(Exit::Drained));
    Ok(())
}
