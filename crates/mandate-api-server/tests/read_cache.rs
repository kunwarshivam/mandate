//! DEC-2145: the API server's journal port reads each stream once, then only past its head, and
//! serves exactly the stream a read from its first record would, for its own workspace alone.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mandate_api_server::cache::{CachedJournal, Limits};
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
        let answer = if self.fails(stream) {
            Err(JournalUnavailable)
        } else {
            Ok(self.whole(stream))
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
        let chains = match tail.first() {
            Some(first) => first.prev_hash == start.prev_hash,
            None => true,
        };
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
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(journal.rows(stream))
}

/// A clock stopped at one instant, or one that cannot be read.
struct Stopped(Option<UtcNanos>);

impl Clock for Stopped {
    fn now(&self) -> Option<UtcNanos> {
        self.0
    }
}

fn cache_on(store: &Arc<Store>, clock: Stopped, limits: Limits) -> CachedJournal {
    let inner: Arc<dyn Journal> = Arc::clone(store) as Arc<dyn Journal>;
    CachedJournal::new(inner, Arc::new(clock), limits)
}

fn cache(store: &Arc<Store>, limits: Limits) -> CachedJournal {
    cache_on(store, Stopped(Some(UtcNanos::EPOCH)), limits)
}

const ROOMY: Limits = Limits {
    idle: Duration::from_secs(3_600),
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
            max_bytes: 1 << 30,
        },
    );
    read(&idle, A_CTL).unwrap();
    read(&idle, A_CTL).unwrap();
    assert_eq!(store.asked().whole, 2, "an idle workspace is dropped whole");
    assert_eq!(
        idle.held_bytes(),
        store
            .whole(A_CTL)
            .iter()
            .map(|r| r.body.len())
            .sum::<usize>()
    );

    let store = Arc::new(Store::default());
    store.append(A_CTL, 20);
    store.append(B_CTL, 20);
    let one_stream = store
        .whole(A_CTL)
        .iter()
        .map(|r| r.body.len())
        .sum::<usize>();
    let tight = cache(
        &store,
        Limits {
            idle: Duration::from_secs(3_600),
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
    let journal = cache_on(&store, Stopped(None), ROOMY);
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

/// A seeded bug: a cache that serves what it holds and never reads past it.
struct Stale {
    store: Arc<Store>,
    held: Mutex<BTreeMap<String, Vec<StoredEvent>>>,
}

impl Journal for Stale {
    fn rows<'a>(&'a self, stream: &'a str) -> Rows<'a> {
        let held = self.held.lock().unwrap().get(stream).cloned();
        let answer = match held {
            Some(rows) => Ok(rows),
            None if self.store.fails(stream) => Err(JournalUnavailable),
            None => {
                let rows = self.store.whole(stream);
                self.held
                    .lock()
                    .unwrap()
                    .insert(stream.to_owned(), rows.clone());
                Ok(rows)
            }
        };
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
fn the_oracle_catches_a_cache_that_never_reads_past_its_head() {
    let stale = |store: &Arc<Store>| -> Box<dyn Journal> {
        Box::new(Stale {
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
        agrees(&stale, &ops)
    });
    assert!(
        matches!(found, Err(TestError::Fail(..))),
        "the oracle must fail a stale cache: {found:?}"
    );
}
