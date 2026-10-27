//! The executor's stream tails (DEC-2854): a pass reads each stream only past the head it last
//! read it to, so it costs what was appended since and never the streams' whole length; every
//! minute each stream is read again from its first record; a kill switch is applied on the next
//! pass wherever it lands; a stream that will not read from its first record holds every opening
//! and no exit; and a stream whose history was rewritten stops the process with an alert, and
//! sends nothing more to the broker.

mod support;

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use mandate_canon::Digest;
use mandate_executor::BrokerRequest;
use mandate_executord::establish::{OpeningGate, pass};
use mandate_executord::{Alert, ExecutorProcess};
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId, TrustedStart};
use mandate_process::{Exit, State};
use mandate_store::Store;
use mandate_time::UtcNanos;
use proptest::prelude::*;
use support::{
    ACCOUNT_STREAM, AGENT, Allowing, CLOCK_STREAM, CONNECTION, CONTROL_STREAM, Error, Journal,
    NOON, Paper, TwoPercentStop, agent_stream, at, deployed, kill, kill_command, process_config,
    propose, revoke, start_allowing, tick, wired,
};

const OPEN: &str = "01JINTENT0000000000000000A";
const LATER_OPEN: &str = "01JINTENT0000000000000000D";
const EXIT: &str = "01JINTENT0000000000000000C";
const SWITCH: &str = "01JKILL00000000000000000AA";
const COMMAND: &str = "01JCMD000000000000000000AA";
const REWRITTEN: &str = "journal_rewritten";

/// A hook that runs once, just after the next read of the stream it names is answered.
type After = Option<(String, Box<dyn Fn(&Journal)>)>;

/// A journal read past a head as the journal's own stores read it (`PgJournal::rows_from`): each
/// record chained to the hash of the one before, the records from the start on, and `None` when
/// the first of them does not follow the start; counted as they are handed. A stream named in
/// `whole_down` will not read from its first record, as a chain broken before the head will not.
#[derive(Clone, Default)]
struct Following {
    journal: Journal,
    handed: Rc<Cell<usize>>,
    whole_down: Rc<RefCell<BTreeSet<String>>>,
    after: Rc<RefCell<After>>,
}

impl Following {
    fn over(journal: Journal) -> Self {
        Self {
            journal,
            ..Self::default()
        }
    }
}

impl Store for Following {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        self.journal.take_ownership(stream)
    }

    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        self.rows_from(stream, TrustedStart::GENESIS)
    }

    fn rows_from(&self, stream: &StreamId, start: TrustedStart) -> Option<Vec<StoredEvent>> {
        if start == TrustedStart::GENESIS && self.whole_down.borrow().contains(stream.as_str()) {
            return None;
        }
        let mut prev = Digest::ZERO;
        let since: Vec<StoredEvent> = self
            .journal
            .rows(stream)?
            .into_iter()
            .map(|row| {
                let linked = StoredEvent {
                    prev_hash: prev,
                    ..row
                };
                prev = linked.hash;
                linked
            })
            .filter(|row| row.seq >= start.from_seq)
            .collect();
        if since
            .first()
            .is_some_and(|first| first.seq != start.from_seq || first.prev_hash != start.prev_hash)
        {
            return None;
        }
        self.handed
            .set(self.handed.get().saturating_add(since.len()));
        let due = self
            .after
            .borrow()
            .as_ref()
            .is_some_and(|(named, _)| named == stream.as_str());
        if due {
            let hook = self.after.borrow_mut().take();
            if let Some((_, hook)) = hook {
                hook(&self.journal);
            }
        }
        Some(since)
    }

    fn append(
        &mut self,
        stream: &StreamId,
        head: u64,
        epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        self.journal
            .append(stream, head, epoch, recorded_at, drafts)
    }
}

/// A journal that refuses every read past a head, so each read is read again from the first record:
/// the whole-stream reading the tails replace, through their own fallback.
#[derive(Clone, Default)]
struct Whole(Journal);

impl Store for Whole {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        self.0.take_ownership(stream)
    }

    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        self.0.rows(stream)
    }

    fn rows_from(&self, stream: &StreamId, start: TrustedStart) -> Option<Vec<StoredEvent>> {
        if start == TrustedStart::GENESIS {
            self.0.rows(stream)
        } else {
            None
        }
    }

    fn append(
        &mut self,
        stream: &StreamId,
        head: u64,
        epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        self.0.append(stream, head, epoch, recorded_at, drafts)
    }
}

/// `n` seconds after noon.
fn secs(n: i64) -> Result<UtcNanos, Error> {
    Ok(UtcNanos::from_parts(at(NOON)?.secs().saturating_add(n), 0)?)
}

/// The scheduler's tick at `n` seconds after noon.
fn ticked(journal: &Journal, n: i64) -> Result<(), Error> {
    let clock = format!("2026-09-21T15:{:02}:{:02}Z", n / 60, n % 60);
    tick(journal, &format!("t{n:05}"), &clock)
}

fn opening(journal: &Journal, id: &str) -> Result<(), Error> {
    propose(journal, AGENT, id, "buy", "10", "150", "open")
}

fn exit(journal: &Journal, id: &str) -> Result<(), Error> {
    propose(journal, AGENT, id, "sell", "10", "150", "risk_exit")
}

fn started<S: Store + Clone>(
    store: &S,
    broker: &Paper,
) -> Result<ExecutorProcess<S, Paper>, Error> {
    Ok(ExecutorProcess::start(
        store.clone(),
        broker.clone(),
        wired(Box::new(Allowing), Box::new(TwoPercentStop))?,
        &process_config(Environment::Paper),
        at(NOON)?,
    ))
}

/// How many kill switches the account stream records as applied.
fn switches(journal: &Journal) -> usize {
    journal.payloads("KillSwitchActivated").len()
}

/// The requests that change the broker's orders: every submission and cancel, never a read.
fn orders(broker: &Paper) -> Vec<BrokerRequest> {
    broker
        .seen()
        .into_iter()
        .map(|seen| seen.request)
        .filter(|request| {
            matches!(
                request,
                BrokerRequest::Submit(_)
                    | BrokerRequest::Cancel { .. }
                    | BrokerRequest::CancelAll(_)
                    | BrokerRequest::ClosePosition(..)
            )
        })
        .collect()
}

fn alerted(alerts: &[Alert], key: &str) -> bool {
    alerts.iter().any(|alert| alert.key == key)
}

/// Whether the pass at `n` seconds reads every stream again from its first record, the first pass
/// having been at one second (DEC-2854 item 3).
fn rechecks(n: i64) -> bool {
    n > 1 && (n - 1) % 60 == 0
}

/// DEC-2854 item 5: an owner's kill switch is applied on the next pass wherever it lands: before a
/// restart, so the first pass of the new process reads it with nothing cached; between the pass's
/// read of the agent stream and its fold, so the next pass reads it past the head; on the pass a
/// check reads every stream from its first record; after many passes, read past the head; and, for
/// an agent whose connection a compromise revoked at or after it, as the owner's command on the
/// control stream itself (DEC-2343). No pass after its landing passes without it.
#[test]
fn a_kill_switch_is_applied_on_the_next_pass_wherever_it_lands() -> Result<(), Error> {
    let agent = agent_stream(AGENT);
    for landing in ["restart", "between", "check", "tail", "revoked"] {
        let store = Following::over(deployed()?);
        let broker = Paper::new(&store.journal);
        let mut process = started(&store, &broker)?;
        for n in 1..=30 {
            ticked(&store.journal, n)?;
            process.step(secs(n)?);
        }
        let mut next = 31;
        match landing {
            "restart" => {
                kill(&store.journal, AGENT, SWITCH)?;
                process = started(&store, &broker)?;
            }
            "between" => {
                *store.after.borrow_mut() = Some((
                    agent.clone(),
                    Box::new(|journal: &Journal| {
                        assert!(kill(journal, AGENT, SWITCH).is_ok());
                    }),
                ));
                process.step(secs(31)?);
                assert_eq!(
                    switches(&store.journal),
                    0,
                    "the pass whose read came first"
                );
                next = 32;
            }
            "check" => {
                kill(&store.journal, AGENT, SWITCH)?;
                next = 61;
            }
            "revoked" => {
                kill_command(&store.journal, COMMAND, "connection", CONNECTION)?;
                revoke(&store.journal, "r1", CONNECTION, true)?;
            }
            _ => kill(&store.journal, AGENT, SWITCH)?,
        }
        process.step(secs(next)?);
        assert_eq!(switches(&store.journal), 1, "landing {landing}");
    }
    Ok(())
}

/// DEC-2854 item 2: over a long run a pass reads only what its streams gained since the pass before
/// read them, never their whole length, the establishment's reads included; a check pass reads each
/// stream whole once.
#[test]
fn a_long_run_reads_only_what_each_pass_appended() -> Result<(), Error> {
    let store = Following::over(deployed()?);
    let broker = Paper::new(&store.journal);
    let gate = OpeningGate::default();
    let mut process = started(&store, &broker)?;
    let agent = agent_stream(AGENT);
    let total = || {
        [CLOCK_STREAM, CONTROL_STREAM, agent.as_str(), ACCOUNT_STREAM]
            .iter()
            .map(|stream| store.journal.rows_of(stream).len())
            .sum::<usize>()
    };
    let mut grown = [0, total()];
    for n in 1..=600 {
        ticked(&store.journal, n)?;
        let before = store.handed.get();
        process.step(secs(n)?);
        let _connecting = pass(&mut process, &gate, secs(n)?);
        let read = store.handed.get().saturating_sub(before);
        let now = total();
        let appended = now.saturating_sub(grown[0]);
        grown = [grown[1], now];
        if n == 1 {
            continue;
        }
        if rechecks(n) {
            assert!(
                read >= now.saturating_sub(4),
                "check pass {n} read {read} of {now}"
            );
        } else {
            assert!(
                read <= appended,
                "pass {n} read {read}, more than the {appended} appended"
            );
        }
    }
    assert!(total() > 600, "the streams grew to {}", total());
    assert!(!matches!(process.lifecycle().state(), State::Exited(_)));
    Ok(())
}

/// DEC-2854 item 4: a record at a held head that no longer reads as it did, as a rewrite that
/// hashes its chain again leaves it, is found by the next check, at most `60` s of passes later,
/// on every stream the process reads; sooner on a stream that gains a record after it, whose read
/// past the head no longer follows it, as the account stream does when the process writes to it.
/// An opening proposed for the check's pass is never sent. The process stops with an alert, or the establishment's
/// code, and sends nothing more: the order resting at the broker is never cancelled, and no other
/// is sent.
#[test]
fn a_rewritten_head_stops_the_process_at_the_next_check_and_sends_nothing() -> Result<(), Error> {
    let agent = agent_stream(AGENT);
    for stream in [CONTROL_STREAM, CLOCK_STREAM, agent.as_str(), ACCOUNT_STREAM] {
        let store = Following::over(deployed()?);
        opening(&store.journal, OPEN)?;
        let broker = Paper::new(&store.journal);
        let gate = OpeningGate::default();
        let mut process = started(&store, &broker)?;
        for n in 1..=10 {
            ticked(&store.journal, n)?;
            process.step(secs(n)?);
            let _connecting = pass(&mut process, &gate, secs(n)?);
        }
        assert_eq!(store.journal.received(), [OPEN], "{stream}");
        let sent = orders(&broker);
        store.journal.rewrite_head(stream);
        let mut found = None;
        for n in 11..=61 {
            if n == 61 {
                opening(&store.journal, LATER_OPEN)?;
            }
            let alerts = process.step(secs(n)?);
            let code = pass(&mut process, &gate, secs(n)?);
            if alerted(&alerts, REWRITTEN) || code == Err(REWRITTEN) {
                found = Some(n);
                break;
            }
        }
        let gains = stream == ACCOUNT_STREAM;
        assert!(
            found.is_some_and(|n| n == 61 || gains),
            "{stream} found at {found:?}"
        );
        assert_eq!(
            process.lifecycle().state(),
            State::Exited(Exit::CycleFailed),
            "{stream}"
        );
        assert_eq!(orders(&broker), sent, "{stream}");
    }
    Ok(())
}

/// DEC-2854 items 4 and 5, the kill path: a control stream rewritten under the process, and then an
/// owner's kill switch (the owner's command, and the runtime's copy on the agent stream) and a
/// later opening. The read past the head no longer follows it, and the
/// read from the first record no longer holds it, so the process stops with an alert on that very
/// pass. It sends nothing: no new opening, and the order resting at the broker is never
/// cancelled. The restart reads the stream from its first record and applies the kill switch.
#[test]
fn a_kill_after_a_rewritten_control_stream_reaches_the_account_through_the_restart()
-> Result<(), Error> {
    let journal = deployed()?;
    opening(&journal, OPEN)?;
    let broker = Paper::new(&journal);
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.received(), [OPEN]);
    let sent = orders(&broker);
    journal.rewrite_head(CONTROL_STREAM);
    kill_command(&journal, COMMAND, "agent", AGENT)?;
    kill(&journal, AGENT, SWITCH)?;
    opening(&journal, LATER_OPEN)?;
    let alerts = process.step(at(NOON)?);
    assert!(alerted(&alerts, REWRITTEN), "{alerts:?}");
    assert_eq!(
        process.lifecycle().state(),
        State::Exited(Exit::CycleFailed)
    );
    assert_eq!(orders(&broker), sent);
    assert_eq!(switches(&journal), 0);
    let mut restarted = start_allowing(&journal, &broker)?;
    restarted.step(at(NOON)?);
    assert_eq!(switches(&journal), 1);
    assert!(
        !broker.submitted().contains(&format!("md-{LATER_OPEN}")),
        "{:?}",
        broker.submitted()
    );
    Ok(())
}

/// DEC-2854 item 4: once a check is due, a stream that will not read from its first record holds
/// every opening, and never an exit, with the stream's alert raised, until it reads; then the
/// opening goes. Nothing past its head is read meanwhile. The exit's account holds the position the
/// journal does not, so it is not ready and takes no opening: an exit goes while it is not.
#[test]
fn a_check_that_will_not_read_holds_openings_and_no_exit_until_it_does() -> Result<(), Error> {
    for proposed in [OPEN, EXIT] {
        let store = Following::over(deployed()?);
        let broker = Paper::new(&store.journal);
        if proposed == EXIT {
            broker.holding("AAPL", "10")?;
        }
        let mut process = started(&store, &broker)?;
        for n in 1..=60 {
            process.step(secs(n)?);
        }
        store
            .whole_down
            .borrow_mut()
            .insert(CLOCK_STREAM.to_owned());
        if proposed == EXIT {
            exit(&store.journal, EXIT)?;
            let alerts = process.step(secs(61)?);
            assert!(alerted(&alerts, "stream_unreadable"), "{alerts:?}");
            assert_eq!(store.journal.received(), [EXIT]);
            continue;
        }
        opening(&store.journal, OPEN)?;
        let alerts = process.step(secs(61)?);
        assert!(alerted(&alerts, "stream_unreadable"), "{alerts:?}");
        for n in 62..=70 {
            process.step(secs(n)?);
        }
        assert!(process.ready());
        assert!(store.journal.received().is_empty());
        store.whole_down.borrow_mut().clear();
        process.step(secs(71)?);
        assert_eq!(store.journal.received(), [OPEN]);
    }
    Ok(())
}

/// One thing that can happen between passes.
#[derive(Debug, Clone, Copy)]
enum Op {
    Tick,
    Open,
    Exit,
    Command,
    Copy,
    Restart,
    /// A minute passes, so the next pass is a check.
    Minute,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => Just(Op::Tick),
        2 => Just(Op::Open),
        2 => Just(Op::Exit),
        1 => Just(Op::Command),
        1 => Just(Op::Copy),
        1 => Just(Op::Restart),
        1 => Just(Op::Minute),
    ]
}

fn apply(journal: &Journal, op: Op, n: usize, second: i64) -> Result<(), Error> {
    match op {
        Op::Tick | Op::Minute => ticked(journal, second),
        Op::Open => opening(journal, &format!("01JINTENT{n:017}")),
        Op::Exit => propose(
            journal,
            AGENT,
            &format!("01JINTENT{n:017}"),
            "sell",
            "1",
            "150",
            "risk_exit",
        ),
        Op::Command => kill_command(journal, &format!("01JCMD{n:020}"), "connection", CONNECTION),
        Op::Copy => kill(journal, AGENT, &format!("01JKILL{n:019}")),
        Op::Restart => Ok(()),
    }
}

/// `result`, its error as the test case's failure.
fn fail<T>(result: Result<T, Error>) -> Result<T, TestCaseError> {
    result.map_err(|error| TestCaseError::fail(error.to_string()))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// DEC-2854 item 2: a process that reads each stream only past its head journals and sends
    /// exactly what one reading every stream from its first record at every pass does, whatever
    /// the interleaving of ticks, openings, exits, kill switches, restarts and checks.
    #[test]
    fn reading_past_the_head_journals_what_reading_whole_streams_does(
        ops in proptest::collection::vec(op(), 1..32),
    ) {
        let (by_head, by_whole) = (Following::over(fail(deployed())?), Whole(fail(deployed())?));
        let (head_broker, whole_broker) =
            (Paper::new(&by_head.journal), Paper::new(&by_whole.0));
        for broker in [&head_broker, &whole_broker] {
            fail(broker.holding("AAPL", "100"))?;
        }
        let (head_gate, whole_gate) = (OpeningGate::default(), OpeningGate::default());
        let mut head = fail(started(&by_head, &head_broker))?;
        let mut whole = fail(started(&by_whole, &whole_broker))?;
        let mut second = 0;
        for (n, op) in ops.iter().enumerate() {
            second += if matches!(op, Op::Minute) { 60 } else { 1 };
            for journal in [&by_head.journal, &by_whole.0] {
                fail(apply(journal, *op, n, second))?;
            }
            if matches!(op, Op::Restart) {
                head = fail(started(&by_head, &head_broker))?;
                whole = fail(started(&by_whole, &whole_broker))?;
            }
            let now = fail(secs(second))?;
            let alerts = (head.step(now), whole.step(now));
            prop_assert_eq!(&alerts.0, &alerts.1, "{:?}", op);
            prop_assert_eq!(
                pass(&mut head, &head_gate, now),
                pass(&mut whole, &whole_gate, now)
            );
            prop_assert_eq!(head.lifecycle().state(), whole.lifecycle().state());
            let bodies = |journal: &Journal| -> Vec<Vec<u8>> {
                journal
                    .rows_of(ACCOUNT_STREAM)
                    .into_iter()
                    .map(|row| row.body)
                    .collect()
            };
            prop_assert_eq!(bodies(&by_head.journal), bodies(&by_whole.0));
            prop_assert_eq!(head_broker.seen(), whole_broker.seen());
        }
    }
}
