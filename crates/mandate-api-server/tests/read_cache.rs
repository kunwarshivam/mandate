//! DEC-2145: the API server's journal port reads each stream once, then only past its head, and
//! serves exactly the stream a read from its first record would, for its own workspace alone.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mandate_api_server::cache::{CachedJournal, Limits, row_bytes};
use mandate_api_server::clock::Clock;
use mandate_api_server::journal::{Answer, Journal, JournalUnavailable, Rows};
use mandate_canon::Digest;
use mandate_journal::{AppendOutcome, StoredEvent, TrustedStart};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestCaseError, TestError, TestRunner};

const A_CTL: &str = "ctl:ws_a";
const A_ACCT: &str = "acct:ws_a:ref_1";
const B_CTL: &str = "ctl:ws_b";
const B_ACCT: &str = "acct:ws_b:ref_1";
const STREAMS: [&str; 4] = [A_CTL, A_ACCT, B_CTL, B_ACCT];

/// What the cache asked of the store: whole reads, tail reads, and the rows the tails carried.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Asked {
    whole: usize,
    tails: usize,
    tail_rows: usize,
}

/// A store whose streams chain, each row's `prev_hash` the hash of the row before, so a tail read
/// from a hash the stream no longer holds fails, as `PgJournal::rows_from`'s check does.
#[derive(Default)]
struct Store {
    streams: Mutex<BTreeMap<String, Vec<StoredEvent>>>,
    failing: Mutex<Vec<String>>,
    asked: Mutex<Asked>,
    nonce: Mutex<u64>,
}

impl Store {
    fn append(&self, stream: &str, count: usize) {
        let mut streams = self.streams.lock().unwrap();
        let mut nonce = self.nonce.lock().unwrap();
        let rows = streams.entry(stream.to_owned()).or_default();
        for _ in 0..count {
            *nonce += 1;
            let seq = rows.last().map_or(1, |row| row.seq + 1);
            let prev_hash = rows.last().map_or(Digest::ZERO, |row| row.hash);
            let body = format!("{stream}|{seq}|{}|{}", prev_hash.to_hex(), *nonce).into_bytes();
            rows.push(StoredEvent {
                stream_id: stream.to_owned(),
                seq,
                event_id: format!("e{}", *nonce),
                event_type: "Test".to_owned(),
                schema_version: 1,
                environment: "paper".to_owned(),
                recorded_at: "2026-10-27T20:00:00.000000000Z".to_owned(),
                prev_hash,
                hash: Digest::of(&body),
                body,
            });
        }
    }

    /// Keeps the first `keep` rows of `stream`, as a restore to an older head would.
    fn truncate(&self, stream: &str, keep: usize) {
        if let Some(rows) = self.streams.lock().unwrap().get_mut(stream) {
            rows.truncate(keep);
        }
    }

    /// Replaces `stream` from its row `keep` on with as many new rows, as rewritten history would.
    fn rewrite(&self, stream: &str, keep: usize) {
        let gone = {
            let mut streams = self.streams.lock().unwrap();
            let Some(rows) = streams.get_mut(stream) else {
                return;
            };
            let gone = rows.len().saturating_sub(keep);
            rows.truncate(keep);
            gone
        };
        self.append(stream, gone);
    }

    /// Changes the body and hash of row `seq` of `stream` without re-chaining the rows after it,
    /// as a rewrite in the hot store would leave it.
    fn tamper(&self, stream: &str, seq: u64) {
        let mut streams = self.streams.lock().unwrap();
        if let Some(row) = streams
            .get_mut(stream)
            .and_then(|rows| rows.iter_mut().find(|row| row.seq == seq))
        {
            row.body.extend_from_slice(b"|tampered");
            row.hash = Digest::of(&row.body);
        }
    }

    /// Whether `rows` are numbered on from `from_seq` and chain from `prev_hash`, each row's hash
    /// its body's, as `PgJournal`'s check reads them.
    fn chains(rows: &[StoredEvent], from_seq: u64, prev_hash: Digest) -> bool {
        let mut expected = (from_seq, prev_hash);
        rows.iter().all(|row| {
            let ok = row.seq == expected.0
                && row.prev_hash == expected.1
                && row.hash == Digest::of(&row.body);
            expected = (row.seq + 1, row.hash);
            ok
        })
    }

    fn set_failing(&self, stream: &str, failing: bool) {
        let mut set = self.failing.lock().unwrap();
        set.retain(|s| s != stream);
        if failing {
            set.push(stream.to_owned());
        }
    }

    fn fails(&self, stream: &str) -> bool {
        self.failing.lock().unwrap().iter().any(|s| s == stream)
    }

    fn whole(&self, stream: &str) -> Vec<StoredEvent> {
        self.streams
            .lock()
            .unwrap()
            .get(stream)
            .cloned()
            .unwrap_or_default()
    }

    fn asked(&self) -> Asked {
        self.asked.lock().unwrap().clone()
    }
}

impl Journal for Store {
    fn rows<'a>(&'a self, stream: &'a str) -> Rows<'a> {
        self.asked.lock().unwrap().whole += 1;
        let rows = self.whole(stream);
        let answer = if self.fails(stream) || !Self::chains(&rows, 1, Digest::ZERO) {
            Err(JournalUnavailable)
        } else {
            Ok(rows)
        };
        Box::pin(async move { answer })
    }

    fn rows_from<'a>(&'a self, stream: &'a str, start: TrustedStart) -> Rows<'a> {
        let rows = self.whole(stream);
        let tail: Vec<StoredEvent> = rows
            .iter()
            .filter(|row| row.seq >= start.from_seq)
            .cloned()
            .collect();
        let chains = Self::chains(&tail, start.from_seq, start.prev_hash);
        let answer = if self.fails(stream) || !chains {
            Err(JournalUnavailable)
        } else {
            let mut asked = self.asked.lock().unwrap();
            asked.tails += 1;
            asked.tail_rows += tail.len();
            Ok(tail)
        };
        Box::pin(async move { answer })
    }

    fn take_ownership<'a>(&'a self, _: &'a str) -> Answer<'a, u64> {
        Box::pin(async { Err(JournalUnavailable) })
    }

    fn head<'a>(&'a self, stream: &'a str) -> Answer<'a, u64> {
        let head = self.whole(stream).last().map_or(0, |row| row.seq);
        let answer = if self.fails(stream) {
            Err(JournalUnavailable)
        } else {
            Ok(head)
        };
        Box::pin(async move { answer })
    }

    fn event<'a>(&'a self, _: &'a str) -> Answer<'a, Option<StoredEvent>> {
        Box::pin(async { Ok(None) })
    }

    fn append<'a>(
        &'a self,
        _: &'a str,
        _: u64,
        _: u64,
        _: UtcNanos,
        _: &'a [&'a [u8]],
    ) -> Answer<'a, AppendOutcome> {
        Box::pin(async { Err(JournalUnavailable) })
    }
}

fn read(journal: &dyn Journal, stream: &str) -> Result<Vec<StoredEvent>, JournalUnavailable> {
    block(journal.rows(stream))
}

/// A clock set by hand, in nanoseconds since the epoch, or one that cannot be read.
struct Hand(Mutex<Option<u64>>);

impl Hand {
    fn at(nanos: u64) -> Arc<Self> {
        Arc::new(Self(Mutex::new(Some(nanos))))
    }

    fn set(&self, nanos: u64) {
        *self.0.lock().unwrap() = Some(nanos);
    }
}

impl Clock for Hand {
    fn now(&self) -> Option<UtcNanos> {
        let nanos = (*self.0.lock().unwrap())?;
        UtcNanos::from_parts(
            i64::try_from(nanos / 1_000_000_000).unwrap(),
            u32::try_from(nanos % 1_000_000_000).unwrap(),
        )
        .ok()
    }
}

fn cache_on(store: &Arc<Store>, clock: &Arc<Hand>, limits: Limits) -> CachedJournal {
    let inner: Arc<dyn Journal> = Arc::clone(store) as Arc<dyn Journal>;
    CachedJournal::new(inner, Arc::clone(clock) as Arc<dyn Clock>, limits)
}

fn cache(store: &Arc<Store>, limits: Limits) -> CachedJournal {
    cache_on(store, &Hand::at(0), limits)
}

fn held_by_rows(rows: &[StoredEvent]) -> usize {
    rows.iter().map(row_bytes).sum()
}

const ROOMY: Limits = Limits {
    idle: Duration::from_secs(3_600),
    reverify: Duration::from_secs(3_600),
    max_bytes: 1 << 30,
};

#[test]
fn a_second_read_past_n_new_rows_reads_exactly_those_n() {
    let store = Arc::new(Store::default());
    store.append(A_ACCT, 40);
    let journal = cache(&store, ROOMY);
    assert_eq!(read(&journal, A_ACCT).unwrap(), store.whole(A_ACCT));
    assert_eq!(
        store.asked(),
        Asked {
            whole: 1,
            tails: 0,
            tail_rows: 0
        }
    );
    store.append(A_ACCT, 7);
    assert_eq!(read(&journal, A_ACCT).unwrap(), store.whole(A_ACCT));
    assert_eq!(
        store.asked(),
        Asked {
            whole: 1,
            tails: 1,
            tail_rows: 8
        },
        "only the 7 new rows are read, after the held head row it checks"
    );
    assert_eq!(read(&journal, A_ACCT).unwrap(), store.whole(A_ACCT));
    assert_eq!(
        store.asked(),
        Asked {
            whole: 1,
            tails: 2,
            tail_rows: 9
        },
        "nothing new reads the held head row alone"
    );
}

#[test]
fn two_workspaces_read_in_turn_each_get_only_their_own_rows() {
    let store = Arc::new(Store::default());
    let journal = cache(&store, ROOMY);
    for round in 0..6 {
        for (i, stream) in STREAMS.iter().enumerate() {
            store.append(stream, round + i + 1);
            let served = read(&journal, stream).unwrap();
            assert_eq!(served, store.whole(stream), "{stream} in round {round}");
            assert!(
                served.iter().all(|row| row.stream_id == *stream),
                "{stream} serves only its own rows"
            );
        }
    }
    let a = read(&journal, A_ACCT).unwrap();
    let b = read(&journal, B_ACCT).unwrap();
    assert!(a.iter().all(|row| row.stream_id == A_ACCT));
    assert!(b.iter().all(|row| row.stream_id == B_ACCT));
    assert_ne!(a, b);
}

#[test]
fn a_stream_cut_below_its_held_head_or_rewritten_is_read_whole_again() {
    let store = Arc::new(Store::default());
    store.append(A_ACCT, 10);
    let journal = cache(&store, ROOMY);
    read(&journal, A_ACCT).unwrap();
    store.truncate(A_ACCT, 6);
    assert_eq!(read(&journal, A_ACCT).unwrap(), store.whole(A_ACCT), "cut");
    assert_eq!(store.asked().whole, 2);
    store.append(A_ACCT, 4);
    read(&journal, A_ACCT).unwrap();
    store.rewrite(A_ACCT, 3);
    assert_eq!(
        read(&journal, A_ACCT).unwrap(),
        store.whole(A_ACCT),
        "rewritten from row 4"
    );
    assert_eq!(store.asked().whole, 3);
    store.rewrite(A_ACCT, 2);
    store.append(A_ACCT, 1);
    assert_eq!(
        read(&journal, A_ACCT).unwrap(),
        store.whole(A_ACCT),
        "rewritten and then grown"
    );
}

#[test]
fn a_read_that_fails_serves_nothing_held_and_the_next_one_reads_whole() {
    let store = Arc::new(Store::default());
    store.append(A_CTL, 5);
    let journal = cache(&store, ROOMY);
    read(&journal, A_CTL).unwrap();
    store.set_failing(A_CTL, true);
    assert_eq!(read(&journal, A_CTL), Err(JournalUnavailable));
    store.set_failing(A_CTL, false);
    store.append(A_CTL, 2);
    assert_eq!(read(&journal, A_CTL).unwrap(), store.whole(A_CTL));
    assert_eq!(
        store.asked().whole,
        3,
        "the failed tail fell back to a whole read, which failed too, so nothing was held"
    );
}

#[test]
fn memory_is_bounded_by_idle_time_and_by_bytes() {
    let store = Arc::new(Store::default());
    store.append(A_CTL, 3);
    let idle = cache(
        &store,
        Limits {
            idle: Duration::ZERO,
            reverify: Duration::from_secs(3_600),
            max_bytes: 1 << 30,
        },
    );
    read(&idle, A_CTL).unwrap();
    read(&idle, A_CTL).unwrap();
    assert_eq!(store.asked().whole, 2, "an idle workspace is dropped whole");
    assert_eq!(
        idle.held_bytes(),
        store.whole(A_CTL).iter().map(row_bytes).sum::<usize>()
    );

    let store = Arc::new(Store::default());
    store.append(A_CTL, 20);
    store.append(B_CTL, 20);
    let one_stream = store.whole(A_CTL).iter().map(row_bytes).sum::<usize>();
    let tight = cache(
        &store,
        Limits {
            idle: Duration::from_secs(3_600),
            reverify: Duration::from_secs(3_600),
            max_bytes: one_stream + one_stream / 2,
        },
    );
    read(&tight, A_CTL).unwrap();
    read(&tight, B_CTL).unwrap();
    assert!(tight.held_bytes() <= one_stream + one_stream / 2);
    read(&tight, A_CTL).unwrap();
    assert_eq!(
        store.asked().whole,
        3,
        "the least recently read workspace was dropped"
    );

    let store = Arc::new(Store::default());
    store.append(A_ACCT, 50);
    let small = cache(
        &store,
        Limits {
            idle: Duration::from_secs(3_600),
            reverify: Duration::from_secs(3_600),
            max_bytes: 10,
        },
    );
    assert_eq!(read(&small, A_ACCT).unwrap(), store.whole(A_ACCT));
    assert_eq!(
        small.held_bytes(),
        0,
        "a stream over the bound is served, not held"
    );
}

#[test]
fn a_read_while_the_clock_cannot_be_read_is_the_stores_and_holds_nothing() {
    let store = Arc::new(Store::default());
    store.append(A_ACCT, 5);
    let journal = cache_on(&store, &Arc::new(Hand(Mutex::new(None))), ROOMY);
    assert_eq!(read(&journal, A_ACCT).unwrap(), store.whole(A_ACCT));
    assert_eq!(read(&journal, A_ACCT).unwrap(), store.whole(A_ACCT));
    assert_eq!(store.asked().whole, 2);
    assert_eq!(journal.held_bytes(), 0);
}

#[test]
fn a_stream_id_that_names_no_workspace_is_the_stores_to_answer() {
    let store = Arc::new(Store::default());
    let journal = cache(&store, ROOMY);
    assert_eq!(read(&journal, "not a stream"), Ok(Vec::new()));
    assert_eq!(read(&journal, "not a stream"), Ok(Vec::new()));
    assert_eq!(store.asked().whole, 2, "never held");
}

#[derive(Debug, Clone)]
enum Op {
    Append(usize, usize),
    Read(usize),
    Truncate(usize, usize),
    Rewrite(usize, usize),
    Fail(usize, bool),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (0..4usize, 1..6usize).prop_map(|(s, n)| Op::Append(s, n)),
        6 => (0..4usize).prop_map(Op::Read),
        1 => (0..4usize, 0..8usize).prop_map(|(s, k)| Op::Truncate(s, k)),
        1 => (0..4usize, 0..8usize).prop_map(|(s, k)| Op::Rewrite(s, k)),
        1 => (0..4usize, any::<bool>()).prop_map(|(s, f)| Op::Fail(s, f)),
    ]
}

/// The oracle: after any sequence of appends, cuts, rewrites and failures across two workspaces,
/// every read the journal under test answers is exactly the store's stream read whole, and fails
/// exactly when that read fails.
fn agrees(make: &dyn Fn(&Arc<Store>) -> Box<dyn Journal>, ops: &[Op]) -> Result<(), TestCaseError> {
    let store = Arc::new(Store::default());
    let journal = make(&store);
    for op in ops {
        match *op {
            Op::Append(s, n) => store.append(STREAMS[s], n),
            Op::Truncate(s, k) => store.truncate(STREAMS[s], k),
            Op::Rewrite(s, k) => store.rewrite(STREAMS[s], k),
            Op::Fail(s, f) => store.set_failing(STREAMS[s], f),
            Op::Read(s) => {
                let stream = STREAMS[s];
                let expected = if store.fails(stream) {
                    Err(JournalUnavailable)
                } else {
                    Ok(store.whole(stream))
                };
                prop_assert_eq!(read(journal.as_ref(), stream), expected, "{:?}", op);
            }
        }
    }
    Ok(())
}

fn the_cache(store: &Arc<Store>) -> Box<dyn Journal> {
    Box::new(cache(store, ROOMY))
}

proptest! {
    #![proptest_config(Config { cases: 300, ..Config::default() })]
    #[test]
    fn every_cached_read_is_the_stream_read_whole(ops in prop::collection::vec(op(), 1..60)) {
        agrees(&the_cache, &ops)?;
    }
}

/// The seeded bugs the oracle must catch.
#[derive(Debug, Clone, Copy)]
enum Bug {
    /// Serves what it holds and never reads past it.
    Stale,
    /// Reads past the held head from the row after it, without reading the head row again.
    NoHeadCheck,
    /// Holds rows by the stream id's last segment, so two workspaces' `ref_1` share them.
    KeyedByRef,
}

/// A cache with one seeded bug.
struct Seeded {
    bug: Bug,
    store: Arc<Store>,
    held: Mutex<BTreeMap<String, Vec<StoredEvent>>>,
}

impl Seeded {
    fn answer(&self, stream: &str) -> Result<Vec<StoredEvent>, JournalUnavailable> {
        let key = match self.bug {
            Bug::KeyedByRef => stream.rsplit(':').next().unwrap_or(stream).to_owned(),
            Bug::Stale | Bug::NoHeadCheck => stream.to_owned(),
        };
        let held = self.held.lock().unwrap().get(&key).cloned();
        let whole = || -> Result<Vec<StoredEvent>, JournalUnavailable> {
            let rows = read(self.store.as_ref(), stream)?;
            self.held.lock().unwrap().insert(key.clone(), rows.clone());
            Ok(rows)
        };
        match (self.bug, held) {
            (_, None) => whole(),
            (Bug::Stale, Some(rows)) => Ok(rows),
            (Bug::KeyedByRef, Some(rows)) => {
                let head = rows.last().map_or(0, |row| row.seq);
                if read_head(self.store.as_ref(), stream) == Ok(head) {
                    Ok(rows)
                } else {
                    whole()
                }
            }
            (Bug::NoHeadCheck, Some(mut rows)) => {
                let start = rows
                    .last()
                    .map_or(TrustedStart::GENESIS, |last| TrustedStart {
                        from_seq: last.seq + 1,
                        prev_hash: last.hash,
                    });
                let Ok(tail) = block(self.store.rows_from(stream, start)) else {
                    return whole();
                };
                rows.extend(tail);
                self.held.lock().unwrap().insert(key, rows.clone());
                Ok(rows)
            }
        }
    }
}

fn block<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(future)
}

fn read_head(journal: &dyn Journal, stream: &str) -> Result<u64, JournalUnavailable> {
    block(journal.head(stream))
}

impl Journal for Seeded {
    fn rows<'a>(&'a self, stream: &'a str) -> Rows<'a> {
        let answer = self.answer(stream);
        Box::pin(async move { answer })
    }

    fn rows_from<'a>(&'a self, stream: &'a str, start: TrustedStart) -> Rows<'a> {
        self.store.rows_from(stream, start)
    }

    fn take_ownership<'a>(&'a self, stream: &'a str) -> Answer<'a, u64> {
        self.store.take_ownership(stream)
    }

    fn head<'a>(&'a self, stream: &'a str) -> Answer<'a, u64> {
        self.store.head(stream)
    }

    fn event<'a>(&'a self, event_id: &'a str) -> Answer<'a, Option<StoredEvent>> {
        self.store.event(event_id)
    }

    fn append<'a>(
        &'a self,
        stream: &'a str,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &'a [&'a [u8]],
    ) -> Answer<'a, AppendOutcome> {
        Journal::append(
            self.store.as_ref(),
            stream,
            expected_head,
            writer_epoch,
            recorded_at,
            drafts,
        )
    }
}

#[test]
fn the_oracle_catches_each_seeded_bug() {
    for bug in [Bug::Stale, Bug::NoHeadCheck, Bug::KeyedByRef] {
        let seeded = move |store: &Arc<Store>| -> Box<dyn Journal> {
            Box::new(Seeded {
                bug,
                store: Arc::clone(store),
                held: Mutex::new(BTreeMap::new()),
            })
        };
        let mut runner = TestRunner::new(Config {
            cases: 300,
            failure_persistence: None,
            ..Config::default()
        });
        let found = runner.run(&prop::collection::vec(op(), 1..60), |ops| {
            agrees(&seeded, &ops)
        });
        assert!(
            matches!(found, Err(TestError::Fail(..))),
            "the oracle must fail the {bug:?} cache: {found:?}"
        );
    }
}

const SECOND: u64 = 1_000_000_000;

#[test]
fn a_change_below_the_held_head_is_seen_at_the_next_whole_read_within_reverify() {
    let store = Arc::new(Store::default());
    store.append(A_ACCT, 10);
    let clock = Hand::at(0);
    let journal = cache_on(
        &store,
        &clock,
        Limits {
            idle: Duration::from_secs(3_600),
            reverify: Duration::from_secs(60),
            max_bytes: 1 << 30,
        },
    );
    let verified = read(&journal, A_ACCT).unwrap();
    store.tamper(A_ACCT, 4);
    assert_eq!(
        read(store.as_ref(), A_ACCT),
        Err(JournalUnavailable),
        "the store's own whole read fails on the broken chain"
    );
    clock.set(60 * SECOND - 1);
    assert_eq!(
        read(&journal, A_ACCT),
        Ok(verified),
        "within reverify the rows verified are served"
    );
    clock.set(60 * SECOND);
    assert_eq!(
        read(&journal, A_ACCT),
        Err(JournalUnavailable),
        "at reverify the stream is read whole, and the broken chain fails it"
    );
}

#[test]
fn a_change_at_the_held_head_is_seen_at_the_next_read() {
    let store = Arc::new(Store::default());
    store.append(A_ACCT, 10);
    let journal = cache(&store, ROOMY);
    let before = read(&journal, A_ACCT).unwrap();
    store.tamper(A_ACCT, 10);
    let after = read(&journal, A_ACCT).unwrap();
    assert_ne!(after, before);
    assert_eq!(after, store.whole(A_ACCT), "the changed head row is served");
    assert_eq!(
        store.asked().whole,
        2,
        "read whole again, from its first record"
    );
}

#[test]
fn a_workspace_is_dropped_once_idle_for_exactly_its_limit_and_not_before() {
    let store = Arc::new(Store::default());
    store.append(A_CTL, 3);
    let clock = Hand::at(5 * SECOND);
    let journal = cache_on(
        &store,
        &clock,
        Limits {
            idle: Duration::from_secs(10),
            reverify: Duration::from_secs(3_600),
            max_bytes: 1 << 30,
        },
    );
    read(&journal, A_CTL).unwrap();
    clock.set(15 * SECOND - 1);
    read(&journal, A_CTL).unwrap();
    assert_eq!(store.asked().whole, 1, "a read before the limit keeps it");
    clock.set(25 * SECOND - 2);
    read(&journal, A_CTL).unwrap();
    assert_eq!(store.asked().whole, 1, "each read restarts the idle time");
    clock.set(35 * SECOND - 2);
    read(&journal, A_CTL).unwrap();
    assert_eq!(
        store.asked().whole,
        2,
        "idle for exactly the limit drops it"
    );
    clock.set(0);
    read(&journal, A_CTL).unwrap();
    assert_eq!(
        store.asked().whole,
        3,
        "a clock that steps back past the last read drops the workspace"
    );
    assert_eq!(journal.held_bytes(), held_by_rows(&store.whole(A_CTL)));
}

fn reverify_in_a_minute() -> Limits {
    Limits {
        idle: Duration::from_secs(3_600),
        reverify: Duration::from_secs(60),
        max_bytes: 1 << 30,
    }
}

#[test]
fn reverify_counts_from_the_last_whole_read_not_from_a_read_past_the_head() {
    let store = Arc::new(Store::default());
    store.append(A_ACCT, 10);
    let clock = Hand::at(0);
    let journal = cache_on(&store, &clock, reverify_in_a_minute());
    read(&journal, A_ACCT).unwrap();
    store.append(A_ACCT, 2);
    clock.set(30 * SECOND);
    read(&journal, A_ACCT).unwrap();
    assert_eq!(store.asked().whole, 1, "grown by a read past the head");
    store.tamper(A_ACCT, 4);
    clock.set(60 * SECOND - 1);
    assert!(read(&journal, A_ACCT).is_ok());
    clock.set(60 * SECOND);
    assert_eq!(read(&journal, A_ACCT), Err(JournalUnavailable));
}

#[test]
fn a_clock_that_steps_back_reads_the_stream_whole() {
    let store = Arc::new(Store::default());
    store.append(A_ACCT, 10);
    let clock = Hand::at(100 * SECOND);
    let journal = cache_on(&store, &clock, reverify_in_a_minute());
    read(&journal, A_ACCT).unwrap();
    store.tamper(A_ACCT, 4);
    clock.set(50 * SECOND);
    assert_eq!(
        read(&journal, A_ACCT),
        Err(JournalUnavailable),
        "a time before the last whole read cannot vouch for it"
    );
}

#[test]
fn a_whole_read_that_fails_drops_its_workspace_and_no_other() {
    let store = Arc::new(Store::default());
    store.append(A_ACCT, 10);
    store.append(A_CTL, 4);
    store.append(B_CTL, 4);
    let clock = Hand::at(0);
    let journal = cache_on(&store, &clock, reverify_in_a_minute());
    read(&journal, A_ACCT).unwrap();
    clock.set(50 * SECOND);
    read(&journal, A_CTL).unwrap();
    read(&journal, B_CTL).unwrap();
    assert_eq!(store.asked().whole, 3);
    store.tamper(A_ACCT, 4);
    clock.set(60 * SECOND);
    assert_eq!(read(&journal, A_ACCT), Err(JournalUnavailable));
    assert_eq!(store.asked().whole, 4, "reverify read it whole, and failed");
    assert_eq!(
        journal.held_bytes(),
        held_by_rows(&store.whole(B_CTL)),
        "only the other workspace is still held"
    );
    assert_eq!(read(&journal, A_CTL).unwrap(), store.whole(A_CTL));
    assert_eq!(
        store.asked().whole,
        5,
        "the failed workspace's other stream is read whole"
    );
    assert_eq!(read(&journal, B_CTL).unwrap(), store.whole(B_CTL));
    assert_eq!(store.asked().whole, 5, "the other workspace is kept");
}

#[test]
fn held_bytes_are_exactly_the_rows_held_after_growth_and_rewrite() {
    let store = Arc::new(Store::default());
    store.append(A_ACCT, 10);
    store.append(B_CTL, 3);
    let journal = cache(&store, ROOMY);
    read(&journal, A_ACCT).unwrap();
    assert_eq!(journal.held_bytes(), held_by_rows(&store.whole(A_ACCT)));
    store.append(A_ACCT, 5);
    read(&journal, A_ACCT).unwrap();
    assert_eq!(journal.held_bytes(), held_by_rows(&store.whole(A_ACCT)));
    store.rewrite(A_ACCT, 3);
    store.append(A_ACCT, 2);
    read(&journal, A_ACCT).unwrap();
    assert_eq!(journal.held_bytes(), held_by_rows(&store.whole(A_ACCT)));
    read(&journal, B_CTL).unwrap();
    assert_eq!(
        journal.held_bytes(),
        held_by_rows(&store.whole(A_ACCT)) + held_by_rows(&store.whole(B_CTL))
    );
    store.truncate(A_ACCT, 2);
    read(&journal, A_ACCT).unwrap();
    assert_eq!(
        journal.held_bytes(),
        held_by_rows(&store.whole(A_ACCT)) + held_by_rows(&store.whole(B_CTL))
    );
}

#[test]
fn over_the_bound_with_one_workspace_its_other_streams_are_dropped() {
    let store = Arc::new(Store::default());
    store.append(A_CTL, 20);
    store.append(A_ACCT, 20);
    let one = held_by_rows(&store.whole(A_ACCT));
    let journal = cache(
        &store,
        Limits {
            idle: Duration::from_secs(3_600),
            reverify: Duration::from_secs(3_600),
            max_bytes: one + one / 2,
        },
    );
    read(&journal, A_CTL).unwrap();
    read(&journal, A_ACCT).unwrap();
    assert_eq!(journal.held_bytes(), one, "the stream just read is kept");
    read(&journal, A_ACCT).unwrap();
    assert_eq!(store.asked().whole, 2);
    read(&journal, A_CTL).unwrap();
    assert_eq!(store.asked().whole, 3, "the other stream was dropped");
}
