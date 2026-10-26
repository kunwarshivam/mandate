//! E8-10 slice D10 (DEC-2500 items 3, 4 and 6; DEC-2516): the dispatcher as a process. It follows
//! a subject stream only its control stream names, holds its replay until the journal answers, is
//! fenced by a newer dispatcher, settles a drain, and maps each step's result to its lifecycle. The
//! oracles are the notice stream's rows and the lifecycle's state, never the process's own view.

use super::{A1, A2, AUDIENCE, CTL, Checked, Counter, Shared, alerts, id, now, payloads};
use crate::DispatchError;
use crate::process::{Dispatcher, Settings, outcome, serve, watch_terminate};
use crate::step::{Journal, NoticeWriter};
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_notify::{FixtureProvider, Origin};
use mandate_process::{Exit, Input, State};
use mandate_time::UtcNanos;
use std::cell::{Cell, RefCell};
use std::error::Error;
use std::process::Command;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

const BUILD: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";

fn settings(floor: &[&str]) -> Result<Settings, Box<dyn Error>> {
    Ok(Settings {
        workspace: "ws_1".to_owned(),
        floor: floor.iter().filter_map(|s| StreamId::parse(s)).collect(),
        audience: AUDIENCE
            .iter()
            .map(|(recipient, channel)| ((*recipient).to_owned(), *channel))
            .collect(),
        origin: Origin::parse("https://app.example.invalid")?,
        environment: Environment::Paper,
        build: BUILD.to_owned(),
    })
}

/// The shared journal, serving `control` as the control stream's committed rows, as the
/// dispatcher only reads it, and failing every read while `down` is set.
#[derive(Clone)]
struct Laid {
    journal: Shared,
    control: Rc<RefCell<Vec<StoredEvent>>>,
    down: Rc<Cell<bool>>,
}

impl Journal for Laid {
    fn committed(&self, stream: &StreamId) -> Result<Vec<StoredEvent>, DispatchError> {
        if self.down.get() {
            return Err(DispatchError::JournalUnavailable);
        }
        if stream.as_str() == CTL {
            return Ok(self.control.borrow().clone());
        }
        self.journal.committed(stream)
    }
    fn append_notices(
        &mut self,
        writer: &NoticeWriter,
        head: u64,
        at: UtcNanos,
        drafts: &[&[u8]],
    ) -> AppendOutcome {
        self.journal.append_notices(writer, head, at, drafts)
    }
}

/// The control stream's row at `seq` establishing a connection to `account` in `environment`.
fn established(seq: u64, account: &str, environment: &str) -> StoredEvent {
    let body = format!(
        r#"{{"event_type":"ConnectionEstablished","environment":"{environment}","payload":{{"connection_id":"c{seq}","account_ref":"{account}"}}}}"#
    );
    StoredEvent {
        stream_id: CTL.to_owned(),
        seq,
        event_id: id(7, u8::try_from(seq).unwrap_or(u8::MAX)),
        event_type: "ConnectionEstablished".to_owned(),
        schema_version: 2,
        environment: environment.to_owned(),
        recorded_at: super::T.to_owned(),
        prev_hash: mandate_canon::Digest::of(b"prev"),
        hash: mandate_canon::Digest::of(body.as_bytes()),
        body: body.into_bytes(),
    }
}

/// A control stream whose one row names `A1`'s connection, so only the control stream names `A1`.
fn laid(journal: &Shared) -> Laid {
    Laid {
        journal: journal.clone(),
        control: Rc::new(RefCell::new(vec![established(1, "A1", "paper")])),
        down: Rc::new(Cell::new(false)),
    }
}

fn provider() -> Result<FixtureProvider, Box<dyn Error>> {
    let mut vault = Vec::new();
    for &(recipient, channel) in &AUDIENCE {
        let recipient = mandate_notify::Recipient::parse(recipient)?;
        vault.push((
            mandate_notify::AddressHandle { recipient, channel },
            "address".to_owned(),
        ));
    }
    Ok(FixtureProvider::holding(vault)?)
}

fn writer(journal: &Shared) -> Result<NoticeWriter, Box<dyn Error>> {
    Ok(super::writer(journal)?)
}

#[test]
fn a_stream_only_the_control_stream_names_is_followed_and_its_alerts_issued() -> Checked {
    let journal = super::world()?;
    alerts(&journal, A1, 1, &[(1, "risk_limit")])?;
    let mut process = Dispatcher::start(
        laid(&journal),
        provider()?,
        Counter(0),
        writer(&journal)?,
        settings(&[])?,
        now()?,
    );
    process.step(now()?);
    assert_eq!(process.lifecycle().state(), State::Ready);
    let issued: Vec<String> = payloads(&journal, "NoticeIssued")
        .iter()
        .map(|p| super::text(p, "cause"))
        .collect();
    assert_eq!(
        issued,
        [id(1, 1)],
        "the alert on A1, found through the control stream"
    );
    Ok(())
}

#[test]
fn an_unread_journal_holds_the_replay_and_sends_no_heartbeat() -> Checked {
    let journal = super::world()?;
    alerts(&journal, A1, 1, &[(1, "risk_limit")])?;
    let flaky = laid(&journal);
    let down = Rc::clone(&flaky.down);
    down.set(true);
    let mut process = Dispatcher::start(
        flaky,
        provider()?,
        Counter(0),
        writer(&journal)?,
        settings(&[])?,
        now()?,
    );
    process.step(now()?);
    assert_ne!(process.lifecycle().state(), State::Ready);
    assert!(payloads(&journal, "NoticeIssued").is_empty());
    down.set(false);
    process.step(now()?);
    assert_eq!(process.lifecycle().state(), State::Ready);
    assert_eq!(payloads(&journal, "NoticeIssued").len(), 1);
    Ok(())
}

#[test]
fn a_newer_dispatcher_fences_this_one_and_a_drain_settles() -> Checked {
    let journal = super::world()?;
    let mut old = Dispatcher::start(
        laid(&journal),
        provider()?,
        Counter(0),
        writer(&journal)?,
        settings(&[])?,
        now()?,
    );
    old.step(now()?);
    let mut newer = Dispatcher::start(
        laid(&journal),
        provider()?,
        Counter(50),
        writer(&journal)?,
        settings(&[])?,
        now()?,
    );
    alerts(&journal, A1, 1, &[(1, "risk_limit")])?;
    old.step(now()?);
    assert_eq!(old.lifecycle().state(), State::Exited(Exit::Fenced));
    assert!(
        payloads(&journal, "NoticeIssued").is_empty(),
        "the fenced one issued nothing"
    );
    newer.step(now()?);
    assert_eq!(payloads(&journal, "NoticeIssued").len(), 1);
    newer.drain(now()?);
    newer.step(now()?);
    assert_eq!(newer.lifecycle().state(), State::Exited(Exit::Drained));
    Ok(())
}

/// DEC-2500 item 4: what each step result does to the lifecycle, written out per result.
#[test]
fn each_step_result_has_its_lifecycle_input() {
    let cases: [(Result<(), DispatchError>, Option<Input>); 9] = [
        (Ok(()), Some(Input::Heartbeat)),
        (Err(DispatchError::JournalUnavailable), None),
        (
            Err(DispatchError::Fenced { current_epoch: 2 }),
            Some(Input::Fenced),
        ),
        (
            Err(DispatchError::NotCommitted {
                outcome: "Unavailable",
            }),
            None,
        ),
        (
            Err(DispatchError::NotCommitted {
                outcome: "Ambiguous",
            }),
            None,
        ),
        (
            Err(DispatchError::NotCommitted {
                outcome: "HeadMismatch",
            }),
            None,
        ),
        (
            Err(DispatchError::NotCommitted { outcome: "Invalid" }),
            Some(Input::Failed),
        ),
        (
            Err(DispatchError::NotCommitted {
                outcome: "IdempotencyConflict",
            }),
            Some(Input::Failed),
        ),
        (Err(DispatchError::NoMessageId), Some(Input::Failed)),
    ];
    for (ran, want) in cases {
        assert_eq!(outcome(&ran), want, "{ran:?}");
    }
}

/// DEC-2516 item 1: an account the control stream names only after the process started, and an
/// account a live row names, which the paper dispatcher never follows. The alert on `A2` becomes a
/// notice at the first step after its connection is recorded, never before.
#[test]
fn a_connection_recorded_mid_run_is_followed_from_the_next_step() -> Checked {
    let journal = super::world()?;
    alerts(&journal, A2, 1, &[(1, "risk_limit")])?;
    let running = laid(&journal);
    let control = Rc::clone(&running.control);
    control.borrow_mut().push(established(2, "A2", "live"));
    let mut process = Dispatcher::start(
        running,
        provider()?,
        Counter(0),
        writer(&journal)?,
        settings(&[])?,
        now()?,
    );
    process.step(now()?);
    assert!(
        payloads(&journal, "NoticeIssued").is_empty(),
        "A2 named only by a live row"
    );
    control.borrow_mut().push(established(3, "A2", "paper"));
    process.step(now()?);
    let issued: Vec<String> = payloads(&journal, "NoticeIssued")
        .iter()
        .map(|p| super::text(p, "cause"))
        .collect();
    assert_eq!(
        issued,
        [id(1, 1)],
        "the alert on A2, once its connection is recorded"
    );
    Ok(())
}

/// How many clock reads a loop under test may make before the test fails: far more than any of
/// these runs needs, so a loop that never exits fails fast rather than hanging.
const READS: u64 = 10_000;

/// A fixed clock that fails the test once it has been read [`READS`] times.
fn bounded(at: UtcNanos) -> impl Fn() -> UtcNanos + Clone + Send + Sync + 'static {
    let reads = Arc::new(AtomicU64::new(0));
    move || {
        let read = reads.fetch_add(1, Ordering::SeqCst);
        assert!(read < READS, "the loop never exited");
        at
    }
}

fn socket(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "mandate-dispatcher-{}-{name}.sock",
        std::process::id()
    ))
}

/// DEC-2500 item 6: a SIGTERM seen before the first step drains the process at once. Nothing is
/// sent, the alert waiting on `A1` included, and the loop returns the drained status, 0.
#[test]
fn a_drain_requested_before_a_step_sends_nothing_and_exits_drained() -> Checked {
    let journal = super::world()?;
    alerts(&journal, A1, 1, &[(1, "risk_limit")])?;
    let process = Dispatcher::start(
        laid(&journal),
        provider()?,
        Counter(0),
        writer(&journal)?,
        settings(&[])?,
        now()?,
    );
    let terminate = AtomicBool::new(true);
    let status = serve(
        process,
        &socket("drained"),
        &terminate,
        bounded(now()?),
        Duration::ZERO,
    )?;
    assert_eq!(status, 0);
    assert!(payloads(&journal, "NoticeIssued").is_empty());
    Ok(())
}

/// DEC-2500 item 4: a dispatcher a newer one fenced exits from its loop with the fenced status,
/// 3, having issued nothing.
#[test]
fn a_fenced_dispatcher_leaves_its_loop_with_the_fenced_status() -> Checked {
    let journal = super::world()?;
    let old = Dispatcher::start(
        laid(&journal),
        provider()?,
        Counter(0),
        writer(&journal)?,
        settings(&[])?,
        now()?,
    );
    let _newer = writer(&journal)?;
    alerts(&journal, A1, 1, &[(1, "risk_limit")])?;
    let terminate = AtomicBool::new(false);
    let status = serve(
        old,
        &socket("fenced"),
        &terminate,
        bounded(now()?),
        Duration::ZERO,
    )?;
    assert_eq!(status, 3);
    assert!(payloads(&journal, "NoticeIssued").is_empty());
    Ok(())
}

/// DEC-2500 item 6: once watched, a SIGTERM sets the drain flag rather than killing the process.
/// Without the handler the signal would end this test's own process.
#[test]
fn a_sigterm_once_watched_sets_the_drain_flag() -> Checked {
    let terminate = Arc::new(AtomicBool::new(false));
    watch_terminate(Arc::clone(&terminate))?;
    let sent = Command::new("kill")
        .args(["-TERM", &std::process::id().to_string()])
        .status()?;
    assert!(sent.success());
    for _ in 0..1000 {
        if terminate.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(terminate.load(Ordering::SeqCst));
    Ok(())
}
