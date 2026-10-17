//! E8-10 slice D10a (DEC-2500 item 3, notifications spec v0.8 §5.9 "Its reads can fail", NT-8): a
//! journal read that fails ends the step with the closed `JournalUnavailable` before it issues,
//! sends, or appends anything more, and is never read as an empty stream. The in-memory journal
//! gains a failing mode, [`Unreadable`], that fails the reads a test names; the oracles are its own
//! log of what the step tried after a failure, the provider's captures, and NT-8's journal-only
//! oracle from `crash.rs`.

use super::crash::nt8;
use super::{
    A1, AUDIENCE, BUILD, CTL, Checked, Counter, Shared, alerts, kill_switch, now, payloads,
    provider, text, world, writer,
};
use crate::DispatchError;
use crate::step::{Config, Journal, NoticeWriter, step};
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_notify::{
    AddressHandle, FixtureProvider, IdempotencyKey, Notification, NotifyError, Origin, Outcome,
    Provider,
};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::error::Error;
use std::rc::Rc;

/// The subject streams: one user kill switch, seen on the control stream and on `A1`, and a
/// risk-limit alert on `A1`, so two notices and four sends.
const SUBJECTS: [&str; 2] = [CTL, A1];
/// The reads of one step over [`SUBJECTS`]: the notice stream, each subject, the notice stream again.
const READS: usize = 4;

/// What one step tried, as the failing journal and the watching provider saw it: which reads fail
/// (from 0), how many were made, whether one has failed, everything tried after that failure, and
/// every send made while the notice stream held no `NoticeIssued` for it.
#[derive(Default)]
struct Log {
    fail: BTreeSet<usize>,
    reads: usize,
    failed: bool,
    late: Vec<&'static str>,
    unissued: Vec<String>,
}

impl Log {
    fn after_failure(&mut self, what: &'static str) {
        if self.failed {
            self.late.push(what);
        }
    }
}

/// The shared journal, failing each read its log names with `JournalUnavailable`.
struct Unreadable(Shared, Rc<RefCell<Log>>);

impl Journal for Unreadable {
    fn committed(&self, stream: &StreamId) -> Result<Vec<StoredEvent>, DispatchError> {
        let mut log = self.1.borrow_mut();
        log.after_failure("read");
        let n = log.reads;
        log.reads = n.wrapping_add(1);
        if log.fail.contains(&n) {
            log.failed = true;
            return Err(DispatchError::JournalUnavailable);
        }
        self.0.committed(stream)
    }
    fn append_notices(
        &mut self,
        writer: &NoticeWriter,
        head: u64,
        at: UtcNanos,
        drafts: &[&[u8]],
    ) -> AppendOutcome {
        self.1.borrow_mut().after_failure("append");
        self.0.append_notices(writer, head, at, drafts)
    }
}

/// The fixture provider, logging a send after a failed read and a send whose notice the notice
/// stream does not yet hold.
struct Watch<'a>(&'a mut FixtureProvider, Shared, Rc<RefCell<Log>>);

impl Provider for Watch<'_> {
    fn send(
        &mut self,
        origin: &Origin,
        notification: &Notification,
        address: &AddressHandle,
        key: &IdempotencyKey,
    ) -> Result<Outcome, NotifyError> {
        let notice = notification.notice.hex()?;
        let issued = payloads(&self.1, "NoticeIssued")
            .iter()
            .any(|p| text(p, "notice") == notice);
        let mut log = self.2.borrow_mut();
        log.after_failure("send");
        if !issued {
            log.unissued.push(notice);
        }
        drop(log);
        self.0.send(origin, notification, address, key)
    }
}

/// A world with [`SUBJECTS`]' alerts, the provider that outlives every restart, and the random
/// source that does too.
struct Dispatcher {
    journal: Shared,
    fx: FixtureProvider,
    random: Counter,
}

impl Dispatcher {
    fn new() -> Result<Self, Box<dyn Error>> {
        let journal = world()?;
        kill_switch(&journal)?;
        alerts(&journal, A1, 1, &[(1, "kill_switch"), (2, "risk_limit")])?;
        let fx = provider(&journal)?.fx;
        Ok(Self {
            journal,
            fx,
            random: Counter(0),
        })
    }

    /// One step by `writer` whose reads numbered in `fail` fail, with the step's answer and its log.
    fn step(
        &mut self,
        writer: &NoticeWriter,
        fail: BTreeSet<usize>,
    ) -> Result<(Result<(), DispatchError>, Log), Box<dyn Error>> {
        let log = Rc::new(RefCell::new(Log {
            fail,
            ..Log::default()
        }));
        let subjects: Vec<StreamId> = SUBJECTS.iter().filter_map(|s| StreamId::parse(s)).collect();
        let origin = Origin::parse("https://app.example.invalid")?;
        let config = Config {
            writer,
            subjects: &subjects,
            audience: &AUDIENCE,
            origin: &origin,
            environment: Environment::Paper,
            build: BUILD,
        };
        let mut journal = Unreadable(self.journal.clone(), Rc::clone(&log));
        let mut sends = Watch(&mut self.fx, self.journal.clone(), Rc::clone(&log));
        let stepped = step(&config, &mut journal, &mut self.random, &mut sends, now()?);
        drop(journal);
        drop(sends);
        let log = Rc::try_unwrap(log)
            .map_err(|_| "the log outlives the step")?
            .into_inner();
        Ok((stepped, log))
    }

    /// A restarted dispatcher's clean step, then NT-8's oracle over the whole run.
    fn settle(&mut self) -> Checked {
        let (stepped, log) = self.step(&writer(&self.journal)?, BTreeSet::new())?;
        stepped?;
        assert_eq!(
            log.unissued,
            Vec::<String>::new(),
            "a send before its notice"
        );
        nt8(&self.journal, &SUBJECTS, self.fx.sent()?)
    }
}

/// Live at the stub and after it: whichever read of a step fails, the step answers an error and
/// tries nothing more, no send is made before its notice commits, and a later clean step makes and
/// records every due send (NT-8).
#[test]
fn a_read_that_fails_ends_the_step_before_any_issue_send_or_append() -> Checked {
    for at in 0..READS {
        let mut dispatcher = Dispatcher::new()?;
        let writer = writer(&dispatcher.journal)?;
        let (stepped, log) = dispatcher.step(&writer, BTreeSet::from([at]))?;
        assert!(stepped.is_err(), "read {at}: {stepped:?}");
        assert_eq!(log.late, Vec::<&str>::new(), "read {at}: tried after it");
        let issued = payloads(&dispatcher.journal, "NoticeIssued").len();
        let before_the_issue = at < READS.wrapping_sub(1);
        assert_eq!(
            issued,
            if before_the_issue { 0 } else { 2 },
            "read {at}: a failed read before the issue batch issues nothing"
        );
        assert_eq!(dispatcher.fx.sent()?.len(), 0, "read {at}: no send");
        assert_eq!(payloads(&dispatcher.journal, "NoticeAttempted").len(), 0);
        dispatcher.settle().map_err(|e| format!("read {at}: {e}"))?;
    }
    Ok(())
}

#[test]
fn a_read_that_fails_is_the_closed_journal_unavailable() -> Checked {
    for at in 0..READS {
        let mut dispatcher = Dispatcher::new()?;
        let writer = writer(&dispatcher.journal)?;
        let (stepped, _) = dispatcher.step(&writer, BTreeSet::from([at]))?;
        assert_eq!(
            stepped,
            Err(DispatchError::JournalUnavailable),
            "read {at} fails"
        );
    }
    Ok(())
}

/// One run: steps in order, each with the reads that fail and whether a restarted dispatcher
/// (a new writer epoch) makes it.
type Run = Vec<(BTreeSet<usize>, bool)>;

fn runs() -> impl Strategy<Value = Run> {
    let fail = prop::collection::btree_set(0..READS, 0..3);
    prop::collection::vec((fail, any::<bool>()), 1..7)
}

/// Every step of `run`: a step whose read failed answers `JournalUnavailable` and tries nothing
/// after; a step whose reads all answered succeeds; no send is ever made before its notice
/// commits; and a clean step after the run leaves NT-8's oracle satisfied.
fn check_run(run: &Run) -> Result<(), String> {
    let fail = |e: Box<dyn Error>| e.to_string();
    let mut dispatcher = Dispatcher::new().map_err(fail)?;
    let mut writer = writer(&dispatcher.journal).map_err(|e| e.to_string())?;
    for (i, (failing, restart)) in run.iter().enumerate() {
        if *restart {
            writer = super::writer(&dispatcher.journal).map_err(|e| e.to_string())?;
        }
        let (stepped, log) = dispatcher.step(&writer, failing.clone()).map_err(fail)?;
        let want = if log.failed {
            Err(DispatchError::JournalUnavailable)
        } else {
            Ok(())
        };
        if stepped != want {
            return Err(format!("step {i} {failing:?}: {stepped:?}, not {want:?}"));
        }
        if !log.late.is_empty() || !log.unissued.is_empty() {
            return Err(format!(
                "step {i} {failing:?}: {:?} after the failed read, {:?} sent before issue",
                log.late, log.unissued
            ));
        }
    }
    dispatcher.settle().map_err(fail)
}

/// The property over random interleavings of failing and answering reads and restarts. A proptest
/// `TestRunner` in a plain function, as a macro-generated pending test is refused.
#[test]
fn random_failing_reads_never_send_without_a_committed_issue_and_answer_journal_unavailable() {
    let checked =
        TestRunner::default().run(&runs(), |run| check_run(&run).map_err(TestCaseError::fail));
    assert_eq!(checked, Ok(()));
}
