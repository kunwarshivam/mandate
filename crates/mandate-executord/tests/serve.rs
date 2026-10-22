//! The executor's run behind journal rule 68's gate (DEC-2341), and the wait before it on a
//! connection that is still connecting (DEC-2346). The run is driven here as the binary drives it,
//! with a wiring that admits every opening, so only the gate the run installs can hold one.

mod support;

use std::cell::{Cell, RefCell};
use std::time::Duration;

use mandate_canon::Value;
use mandate_executord::serve::{Edge, STARTUP, Waited, serve, starting, wait};
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

/// The account stream's `connect` check of the connection, every result passed.
fn checked(journal: &Journal) -> Result<(), Error> {
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
            ("account_pii_ref", Value::Null),
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

/// Runs `body` with an edge at noon whose tick counts a pass, runs `each` at that pass's number,
/// and answers `lapse` once that many passes have run; SIGTERM arrives after `stop` passes.
fn with_edge<T>(
    stop: usize,
    lapse: Option<usize>,
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
        lapse
            .filter(|after| pass >= *after)
            .map(|_| "credential_lost")
    };
    let terminated = || seen.passes.get() >= stop;
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
/// drain's own; the run is not stopped by it.
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

/// A lease that stops the run drains it, and the lease's code is said before the drain's.
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
        (exit, seen.said.borrow().clone())
    })?;
    assert_eq!(
        said,
        (
            Ok(0),
            vec!["credential_lost".to_owned(), "drained".to_owned()]
        )
    );
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
