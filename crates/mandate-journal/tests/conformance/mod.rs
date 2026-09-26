//! The append conformance suite (journal spec §2, §5.1, §11): the tests every journal backend must
//! pass with the same results. `tests/conformance.rs` runs it on `MemoryJournal`, and
//! `mandate-journal-pg` includes this file (with `common`) and runs it on Postgres, so a backend
//! cannot drift from the protocol unnoticed (E5-3, DEC-109).
#![allow(
    dead_code,
    reason = "each test crate that includes this module uses a different subset"
)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use mandate_canon::{Digest, Key, Value, parse, to_canonical};
use mandate_journal::{
    AppendOutcome, Head, InvalidReason, MemoryJournal, StoredEvent, StreamId, TrustedStart,
    Verified, verify_events,
};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};

use super::common::{
    STREAM, T, edit, event_id, mark_draft, mark_draft_at, now, opened_draft, stream,
};

/// A journal backend as the suite drives it. Backends that talk to a database block on each call.
pub trait Backend {
    fn take_ownership(&mut self, stream: &StreamId) -> u64;
    fn head(&mut self, stream: &StreamId) -> Head;
    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[&[u8]],
    ) -> AppendOutcome;
    fn rows(&mut self, stream: &StreamId) -> Vec<StoredEvent>;
    fn event(&mut self, event_id: &str) -> Option<StoredEvent>;
    /// Random sequences per run of the differential property; a database backend runs fewer.
    fn property_cases(&self) -> u32 {
        256
    }
}

impl Backend for MemoryJournal {
    fn take_ownership(&mut self, stream: &StreamId) -> u64 {
        MemoryJournal::take_ownership(self, stream)
    }

    fn head(&mut self, stream: &StreamId) -> Head {
        MemoryJournal::head(self, stream)
    }

    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[&[u8]],
    ) -> AppendOutcome {
        MemoryJournal::append(
            self,
            stream,
            expected_head,
            writer_epoch,
            recorded_at,
            drafts,
        )
    }

    fn rows(&mut self, stream: &StreamId) -> Vec<StoredEvent> {
        MemoryJournal::rows(self, stream).to_vec()
    }

    fn event(&mut self, event_id: &str) -> Option<StoredEvent> {
        MemoryJournal::event(self, event_id).cloned()
    }
}

/// One `#[test]` per suite function, each on a backend from `fresh`. `fresh` returns `None` when
/// the backend is not available in this environment (Postgres without `MANDATE_PG_URL`); the test
/// then passes after `fresh` has said why. Attributes before `fresh` go on every test.
macro_rules! conformance_tests {
    ($(#[$attr:meta])* fresh = $fresh:expr) => {
        $(#[$attr])*
        #[test]
        fn new_streams_start_empty() {
            $crate::conformance::run($fresh, $crate::conformance::new_streams_start_empty);
        }

        $(#[$attr])*
        #[test]
        fn stream_rules() {
            $crate::conformance::run($fresh, $crate::conformance::stream_rules);
        }

        $(#[$attr])*
        #[test]
        fn ownership_and_fencing() {
            $crate::conformance::run($fresh, $crate::conformance::ownership_and_fencing);
        }

        $(#[$attr])*
        #[test]
        fn risk_clock_never_decreases_along_a_stream() {
            $crate::conformance::run(
                $fresh,
                $crate::conformance::risk_clock_never_decreases_along_a_stream,
            );
        }

        $(#[$attr])*
        #[test]
        fn identical_retries_return_the_stored_events() {
            $crate::conformance::run(
                $fresh,
                $crate::conformance::identical_retries_return_the_stored_events,
            );
        }

        $(#[$attr])*
        #[test]
        fn events_with_artifact_references_are_stored_and_read_back() {
            $crate::conformance::run(
                $fresh,
                $crate::conformance::events_with_artifact_references_are_stored_and_read_back,
            );
        }

        $(#[$attr])*
        #[test]
        fn rejected_batches_write_nothing_and_use_no_seq() {
            $crate::conformance::run(
                $fresh,
                $crate::conformance::rejected_batches_write_nothing_and_use_no_seq,
            );
        }

        $(#[$attr])*
        #[test]
        fn chain_vectors_byte_for_byte() {
            $crate::conformance::run($fresh, $crate::conformance::chain_vectors_byte_for_byte);
        }

        $(#[$attr])*
        #[test]
        fn append_vectors() {
            $crate::conformance::append_vectors($fresh);
        }

        $(#[$attr])*
        #[test]
        fn matches_the_memory_journal_on_random_sequences() {
            $crate::conformance::run(
                $fresh,
                $crate::conformance::matches_the_memory_journal_on_random_sequences,
            );
        }
    };
}
pub(crate) use conformance_tests;

pub fn run<B: Backend>(fresh: impl Fn() -> Option<B>, test: impl FnOnce(&mut B)) {
    if let Some(mut backend) = fresh() {
        test(&mut backend);
    }
}

fn committed(outcome: AppendOutcome) -> Vec<StoredEvent> {
    match outcome {
        AppendOutcome::Committed(rows) => rows,
        other => panic!("expected Committed, got {other:?}"),
    }
}

fn invalid(outcome: AppendOutcome) -> (usize, InvalidReason, String) {
    match outcome {
        AppendOutcome::Invalid { draft, error } => (draft, error.reason, error.path),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

/// Opens the suite's stream under a new writer epoch and appends `marks` MarkUpdated events;
/// returns the epoch.
fn opened_with_marks<B: Backend>(b: &mut B, marks: u64) -> u64 {
    let s = stream();
    let epoch = b.take_ownership(&s);
    committed(b.append(&s, 0, epoch, now(), &[&opened_draft("paper")]));
    for n in 1..=marks {
        committed(b.append(&s, n, epoch, now(), &[&mark_draft(n, "1")]));
    }
    epoch
}

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse(text).unwrap()
}

pub fn new_streams_start_empty<B: Backend>(b: &mut B) {
    let s = stream();
    assert_eq!(
        b.head(&s),
        Head {
            seq: 0,
            hash: Digest::ZERO,
            writer_epoch: 0
        }
    );
    assert!(b.rows(&s).is_empty());
    assert_eq!(b.event(&event_id(1)), None);
}

pub fn stream_rules<B: Backend>(b: &mut B) {
    let s = stream();
    assert_eq!(
        invalid(b.append(&s, 0, 0, now(), &[])),
        (0, InvalidReason::EmptyBatch, String::new())
    );
    assert_eq!(
        invalid(b.append(&s, 0, 0, now(), &[&mark_draft(1, "1")])).1,
        InvalidReason::NotStreamOpened
    );
    let other = StreamId::parse("acct:ws_1:OTHER").unwrap();
    assert_eq!(
        invalid(b.append(&other, 0, 0, now(), &[&opened_draft("paper")])).1,
        InvalidReason::StreamMismatch
    );
    assert_eq!(
        invalid(b.append(
            &s,
            0,
            0,
            now(),
            &[
                &opened_draft("paper"),
                &mark_draft(1, "1"),
                &mark_draft(1, "2")
            ]
        ))
        .0,
        2,
        "a batch may not repeat an event_id"
    );
    assert_eq!(
        invalid(b.append(
            &s,
            0,
            0,
            now(),
            &[
                &opened_draft("paper"),
                &edit(&mark_draft(1, "1"), "environment", Some("\"backtest\""))
            ]
        )),
        (
            1,
            InvalidReason::EnvironmentMismatch,
            "environment".to_owned()
        )
    );
    assert!(b.rows(&s).is_empty(), "rejected batches write nothing");
    assert_eq!(b.head(&s).seq, 0);

    let rows = committed(b.append(
        &s,
        0,
        0,
        now(),
        &[
            &opened_draft("paper"),
            &mark_draft(1, "1"),
            &mark_draft(2, "2"),
        ],
    ));
    assert_eq!(rows.iter().map(|r| r.seq).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(rows[0].prev_hash, Digest::ZERO);
    assert_eq!(rows[1].prev_hash, rows[0].hash);
    assert_eq!(rows[2].prev_hash, rows[1].hash);
    assert!(rows.iter().all(|r| r.hash == Digest::of(&r.body)
        && r.recorded_at == T
        && r.environment == "paper"
        && r.stream_id == STREAM));
    assert_eq!(b.rows(&s), rows);
    assert_eq!(
        b.head(&s),
        Head {
            seq: 3,
            hash: rows[2].hash,
            writer_epoch: 0
        }
    );
    assert_eq!(b.event(&event_id(2)), Some(rows[2].clone()));
    assert_eq!(b.event(&event_id(7)), None);

    let reopened = edit(
        &opened_draft("paper"),
        "event_id",
        Some(&format!("\"{}\"", event_id(50))),
    );
    assert_eq!(
        invalid(b.append(&s, 3, 0, now(), &[&reopened])).1,
        InvalidReason::StreamAlreadyOpened
    );
    let backtest = edit(&mark_draft(3, "1"), "environment", Some("\"backtest\""));
    assert_eq!(
        invalid(b.append(&s, 3, 0, now(), &[&backtest])).1,
        InvalidReason::EnvironmentMismatch,
        "the stream's environment is fixed by its StreamOpened"
    );

    let elsewhere = edit(
        &opened_draft("paper"),
        "stream_id",
        Some("\"acct:ws_1:OTHER\""),
    );
    let elsewhere = edit(&elsewhere, "payload.account_ref", Some("\"OTHER\""));
    assert_eq!(
        b.append(&other, 0, 0, now(), &[&elsewhere]),
        AppendOutcome::IdempotencyConflict { stored_seq: 1 },
        "event IDs are global: the same event_id in another stream is a conflict"
    );
    assert!(b.rows(&other).is_empty());
    assert_eq!(b.rows(&s), rows);
}

pub fn ownership_and_fencing<B: Backend>(b: &mut B) {
    let s = stream();
    let epoch = opened_with_marks(b, 2);
    assert_eq!(epoch, 1, "the first owner of a stream gets epoch 1");
    assert_eq!(b.head(&s).writer_epoch, 1);
    assert_eq!(b.take_ownership(&s), 2);
    assert_eq!(b.head(&s).writer_epoch, 2);
    assert_eq!(
        b.append(&s, 3, 1, now(), &[&mark_draft(3, "1")]),
        AppendOutcome::Fenced { current_epoch: 2 },
        "the previous writer is fenced out"
    );
    assert_eq!(
        b.append(&s, 3, 3, now(), &[&mark_draft(3, "1")]),
        AppendOutcome::Fenced { current_epoch: 2 },
        "an epoch nobody took is fenced too"
    );
    let head = b.head(&s);
    assert_eq!(
        b.append(&s, 2, 2, now(), &[&mark_draft(3, "1")]),
        AppendOutcome::HeadMismatch {
            actual_seq: 3,
            actual_hash: head.hash
        }
    );
    assert_eq!(b.head(&s), head, "fenced and stale appends change nothing");
    let rows = committed(b.append(&s, 3, 2, now(), &[&mark_draft(3, "1")]));
    assert_eq!(rows[0].seq, 4);
    assert_eq!(rows[0].prev_hash, head.hash);
    assert_eq!(b.take_ownership(&s), 3);
    assert_eq!(
        b.head(&s),
        Head {
            seq: 4,
            hash: rows[0].hash,
            writer_epoch: 3
        },
        "taking ownership keeps the head"
    );
}

pub fn risk_clock_never_decreases_along_a_stream<B: Backend>(b: &mut B) {
    let s = stream();
    let epoch = opened_with_marks(b, 0);
    let mark = |n, clock| mark_draft_at(n, "1", clock);
    let regressed = |draft| {
        (
            draft,
            InvalidReason::RiskClockRegressed,
            "payload.risk_clock".to_owned(),
        )
    };
    committed(b.append(
        &s,
        1,
        epoch,
        now(),
        &[&mark(1, "2026-09-21T14:00:05.000000000Z")],
    ));
    assert_eq!(
        invalid(b.append(
            &s,
            2,
            epoch,
            now(),
            &[&mark(2, "2026-09-21T14:00:04.000000000Z")]
        )),
        regressed(0),
        "a later append may not undercut the stream's last risk_clock"
    );
    assert_eq!(
        invalid(b.append(
            &s,
            2,
            epoch,
            now(),
            &[
                &mark(2, "2026-09-21T14:00:07.000000000Z"),
                &mark(3, "2026-09-21T14:00:06.000000000Z")
            ]
        )),
        regressed(1),
        "a batch may not go backwards either"
    );
    committed(b.append(
        &s,
        2,
        epoch,
        now(),
        &[
            &mark(2, "2026-09-21T14:00:05.000000000Z"),
            &mark(3, "2026-09-21T14:00:06.000000000Z"),
        ],
    ));
    assert_eq!(
        invalid(b.append(
            &s,
            4,
            epoch,
            now(),
            &[&mark(4, "2026-09-21T14:00:05.000000000Z")]
        )),
        regressed(0),
        "the committed batch moved the stream's clock to its last draft"
    );
    let other = StreamId::parse("acct:ws_1:OTHER").unwrap();
    let opened = edit(
        &opened_draft("paper"),
        "stream_id",
        Some("\"acct:ws_1:OTHER\""),
    );
    let opened = edit(&opened, "payload.account_ref", Some("\"OTHER\""));
    let opened = edit(&opened, "event_id", Some(&format!("\"{}\"", event_id(100))));
    let early = edit(
        &edit(
            &mark(101, "2026-09-21T13:00:00.000000000Z"),
            "stream_id",
            Some("\"acct:ws_1:OTHER\""),
        ),
        "event_id",
        Some(&format!("\"{}\"", event_id(101))),
    );
    committed(b.append(&other, 0, 0, now(), &[&opened, &early]));
}

pub fn identical_retries_return_the_stored_events<B: Backend>(b: &mut B) {
    let s = stream();
    let epoch = opened_with_marks(b, 0);
    let batch = [mark_draft(1, "150.010"), mark_draft(2, "2")];
    let refs: Vec<&[u8]> = batch.iter().map(Vec::as_slice).collect();
    let rows = committed(b.append(&s, 1, epoch, now(), &refs));
    let later = at("2026-09-21T15:00:00.000000000Z");
    assert_eq!(
        b.append(&s, 0, epoch.saturating_add(5), later, &refs),
        AppendOutcome::AlreadyCommitted(rows.clone()),
        "a retry returns the stored events whatever its head, epoch, and clock"
    );
    assert_eq!(
        b.append(&s, 3, epoch, later, &[&batch[1], &batch[0]]),
        AppendOutcome::AlreadyCommitted(vec![rows[1].clone(), rows[0].clone()]),
        "in batch order"
    );
    assert_eq!(
        b.append(&s, 1, epoch, later, &[&mark_draft(1, "150.01")]),
        AppendOutcome::AlreadyCommitted(vec![rows[0].clone()]),
        "an equivalent decimal is the same canonical draft"
    );
    assert_eq!(
        b.append(&s, 3, epoch, later, &[&mark_draft(1, "151")]),
        AppendOutcome::IdempotencyConflict { stored_seq: 2 }
    );
    assert_eq!(
        b.append(
            &s,
            3,
            epoch,
            later,
            &[&mark_draft(3, "1"), &mark_draft(2, "2")]
        ),
        AppendOutcome::IdempotencyConflict { stored_seq: 3 },
        "a batch that partly overlaps stored events is a conflict"
    );
    assert_eq!(b.event(&event_id(3)), None);
    let next = committed(b.append(&s, 3, epoch, later, &[&mark_draft(3, "1")]));
    assert_eq!(next[0].seq, 4);
    assert_eq!(
        b.append(&s, 1, epoch, now(), &refs),
        AppendOutcome::AlreadyCommitted(rows.clone()),
        "still after the stream has moved on"
    );
    let mut all = vec![b.rows(&s)[0].clone()];
    all.extend(rows);
    all.extend(next);
    assert_eq!(b.rows(&s), all, "retries stored nothing");
}

/// A `MarkUpdated` draft whose `payload.source` is the artifact reference `sha256:<fill>…`.
pub fn artifact_mark_draft(n: u64, fill: char) -> Vec<u8> {
    let reference = format!("\"sha256:{}\"", fill.to_string().repeat(64));
    let draft = edit(&mark_draft(n, "1"), "payload.source", Some(&reference));
    edit(&draft, "artifact_refs", Some(&format!("[{reference}]")))
}

pub fn events_with_artifact_references_are_stored_and_read_back<B: Backend>(b: &mut B) {
    let s = stream();
    let epoch = opened_with_marks(b, 0);
    let batch = [artifact_mark_draft(1, 'a'), artifact_mark_draft(2, 'b')];
    let refs: Vec<&[u8]> = batch.iter().map(Vec::as_slice).collect();
    let rows = committed(b.append(&s, 1, epoch, now(), &refs));
    let next = committed(b.append(&s, 3, epoch, now(), &[&mark_draft(3, "1")]));
    assert_eq!(next[0].prev_hash, rows[1].hash);
    assert_eq!(
        b.append(&s, 0, epoch, now(), &refs),
        AppendOutcome::AlreadyCommitted(rows.clone()),
        "artifact bytes are not needed to recognise a retry"
    );
    assert_eq!(b.event(&event_id(1)).as_ref(), Some(&rows[0]));
    let mut all = vec![b.rows(&s)[0].clone()];
    all.extend(rows);
    all.extend(next);
    assert_eq!(
        b.rows(&s),
        all,
        "reads do not need the artifact store (§11 checks 6 and 7)"
    );
}

pub fn rejected_batches_write_nothing_and_use_no_seq<B: Backend>(b: &mut B) {
    let s = stream();
    let epoch = opened_with_marks(b, 1);
    let before = b.head(&s);
    let stored = b.rows(&s);
    let regressing = mark_draft_at(4, "1", "2026-09-21T13:59:59.000000000Z");
    let rejected: [(Vec<Vec<u8>>, u64, u64); 4] = [
        (
            vec![mark_draft(2, "1"), mark_draft(3, "1"), regressing],
            2,
            epoch,
        ),
        (vec![mark_draft(2, "1"), b"{\"a\":1.5}".to_vec()], 2, epoch),
        (vec![mark_draft(2, "1"), mark_draft(3, "1")], 1, epoch),
        (
            vec![mark_draft(2, "1"), mark_draft(3, "1")],
            2,
            epoch.saturating_add(1),
        ),
    ];
    for (drafts, expected_head, writer_epoch) in rejected {
        let refs: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
        let outcome = b.append(&s, expected_head, writer_epoch, now(), &refs);
        assert!(
            !matches!(outcome, AppendOutcome::Committed(_)),
            "{outcome:?}"
        );
        assert_eq!(b.head(&s), before, "after {outcome:?}");
        assert_eq!(b.rows(&s), stored, "after {outcome:?}");
        assert_eq!(b.event(&event_id(2)), None, "after {outcome:?}");
    }
    let rows = committed(b.append(
        &s,
        2,
        epoch,
        now(),
        &[&mark_draft(2, "1"), &mark_draft(3, "1")],
    ));
    assert_eq!(
        rows.iter()
            .map(|r| (r.seq, r.prev_hash))
            .collect::<Vec<_>>(),
        [(3, before.hash), (4, rows[0].hash)],
        "rejected appends used no seq"
    );
}

/// `fixtures/refcases/journal.json`, from either crate's directory.
pub fn fixture() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/refcases/journal.json"
    );
    parse(&std::fs::read(path).unwrap()).unwrap()
}

pub fn get<'a>(v: &'a Value, path: &str) -> &'a Value {
    path.split('.').fold(v, |v, key| {
        v.get(key).unwrap_or_else(|| panic!("no `{path}`"))
    })
}

pub fn text<'a>(v: &'a Value, path: &str) -> &'a str {
    get(v, path).as_str().unwrap()
}

pub fn int(v: &Value, path: &str) -> u64 {
    get(v, path).as_int().unwrap()
}

pub fn list<'a>(v: &'a Value, path: &str) -> &'a [Value] {
    get(v, path).as_array().unwrap()
}

fn object(v: &mut Value) -> &mut BTreeMap<Key, Value> {
    match v {
        Value::Object(members) => members,
        other => panic!("not an object: {other:?}"),
    }
}

fn set(v: &mut Value, key: &str, value: Value) {
    object(v).insert(Key::new(key).unwrap(), value);
}

fn string(s: &str) -> Value {
    Value::Str(s.to_owned())
}

/// The writer's draft inside a chain body: the body without the journal-assigned fields.
fn draft_of(body: &Value) -> Vec<u8> {
    let mut draft = body.clone();
    for field in ["seq", "prev_hash", "recorded_at"] {
        object(&mut draft).remove(field).unwrap();
    }
    to_canonical(&draft)
}

/// Appends the fixture's chain through the protocol, one draft per append, as the stream's first
/// owner; checks every stored body and hash byte for byte and returns the stream.
pub fn append_chain<B: Backend>(b: &mut B, fx: &Value) -> StreamId {
    let chain = list(fx, "chain");
    let s = StreamId::parse(text(&chain[0], "body.stream_id")).unwrap();
    let epoch = b.take_ownership(&s);
    for (head, entry) in (0u64..).zip(chain) {
        let body = get(entry, "body");
        let rows = committed(b.append(
            &s,
            head,
            epoch,
            at(text(body, "recorded_at")),
            &[&draft_of(body)],
        ));
        let [row] = rows.as_slice() else {
            panic!("one row per append, got {rows:?}")
        };
        assert_eq!(
            String::from_utf8_lossy(&row.body),
            text(entry, "canonical"),
            "stored body of seq {}",
            head + 1
        );
        assert_eq!(row.hash.to_hex(), text(entry, "hash"));
        assert_eq!(b.event(&row.event_id), Some(row.clone()));
    }
    s
}

pub fn chain_vectors_byte_for_byte<B: Backend>(b: &mut B) {
    let fx = fixture();
    let s = append_chain(b, &fx);
    let last = list(&fx, "chain").last().unwrap();
    let last_hash = Digest::from_hex(text(last, "hash")).unwrap();
    let rows = b.rows(&s);
    assert_eq!(rows.len(), list(&fx, "chain").len());
    assert_eq!(
        verify_events(&rows, TrustedStart::GENESIS, &BTreeMap::new()),
        Ok(Verified {
            next_seq: int(last, "seq") + 1,
            last_hash
        })
    );
    assert_eq!(
        b.head(&s),
        Head {
            seq: int(last, "seq"),
            hash: last_hash,
            writer_epoch: 1
        }
    );
}

const APPEND_TIME: &str = "2026-09-21T14:00:02.000000000Z";
const DRAFT_F_ID: &str = "01J8Z3M4F0000000000000000F";
const NEW_GATE_ID: &str = "01J8Z3M4G0000000000000000G";

fn chain_body(fx: &Value, index: usize) -> Value {
    get(&list(fx, "chain")[index], "body").clone()
}

fn with_payload(mut body: Value, field: &str, value: Value) -> Value {
    let payload = object(&mut body).get_mut("payload").unwrap();
    assert!(payload.get(field).is_some(), "payload has no `{field}`");
    set(payload, field, value);
    body
}

/// Draft F: a new `MarkUpdated` in the chain's stream, caused by the seq 5 fill.
fn draft_f(fx: &Value) -> Value {
    let fill = chain_body(fx, 4);
    let opened = chain_body(fx, 0);
    let mut body = fill.clone();
    let mut payload = Value::Object(BTreeMap::new());
    for (field, value) in [
        ("instrument_id", get(&fill, "payload.instrument_id").clone()),
        ("price", string("150.01")),
        ("source", string("quote")),
        ("feed", string("iex")),
        ("risk_clock", string(APPEND_TIME)),
    ] {
        set(&mut payload, field, value);
    }
    for (field, value) in [
        ("event_id", string(DRAFT_F_ID)),
        ("event_type", string("MarkUpdated")),
        (
            "schema_version",
            Value::Int(mandate_canon::Int::new(1).unwrap()),
        ),
        ("event_time", string(APPEND_TIME)),
        ("clock_source", string("local")),
        ("causation_id", get(&fill, "event_id").clone()),
        ("actor", get(&opened, "actor").clone()),
        ("config_refs", Value::Object(BTreeMap::new())),
        ("payload", payload),
    ] {
        set(&mut body, field, value);
    }
    body
}

type Drafts = fn(&Value) -> Vec<Vec<u8>>;

/// The journal vectors' append cases: (name, the vector's draft descriptions, the drafts they
/// describe). A case fails when its prose changes, so a changed vector is re-read.
const APPENDS: &[(&str, &[&str], Drafts)] = &[
    (
        "identical_retry_with_stale_head",
        &["draft of seq 5 (event_id 01J8Z3M3T0000000000000000E), unchanged"],
        |fx| vec![draft_of(&chain_body(fx, 4))],
    ),
    (
        "retry_with_equivalent_decimal",
        &["draft of seq 5 with payload.price '150.00'"],
        |fx| {
            vec![draft_of(&with_payload(
                chain_body(fx, 4),
                "price",
                string("150.00"),
            ))]
        },
    ),
    (
        "same_event_id_different_content",
        &["draft of seq 5 with payload.qty_gross '11'"],
        |fx| {
            vec![draft_of(&with_payload(
                chain_body(fx, 4),
                "qty_gross",
                string("11"),
            ))]
        },
    ),
    (
        "partial_overlap_batch",
        &["draft of seq 5, unchanged", "new draft F"],
        |fx| vec![draft_of(&chain_body(fx, 4)), draft_of(&draft_f(fx))],
    ),
    ("stale_head", &["new draft F"], |fx| {
        vec![draft_of(&draft_f(fx))]
    }),
    ("fenced_writer", &["new draft F"], |fx| {
        vec![draft_of(&draft_f(fx))]
    }),
    ("committed", &["new draft F"], |fx| {
        vec![draft_of(&draft_f(fx))]
    }),
    (
        "float_rejected",
        &["new draft with payload.price as the JSON number 150.0"],
        |fx| {
            let draft = String::from_utf8(draft_of(&draft_f(fx))).unwrap();
            let floated = draft.replacen(r#""price":"150.01""#, r#""price":150.0"#, 1);
            assert_ne!(floated, draft, "draft F has no price to replace");
            vec![floated.into_bytes()]
        },
    ),
    (
        "missing_required_config_ref",
        &["new GateDecided draft without config_refs.rule_set"],
        |fx| {
            let mut body = chain_body(fx, 2);
            set(&mut body, "event_id", string(NEW_GATE_ID));
            let refs = object(&mut body).get_mut("config_refs").unwrap();
            object(refs).remove("rule_set").unwrap();
            vec![draft_of(&body)]
        },
    ),
];

/// Every append case of the journal vectors, each on a fresh backend holding the vectors' chain.
pub fn append_vectors<B: Backend>(fresh: impl Fn() -> Option<B>) {
    let fx = fixture();
    let cases = list(&fx, "append_cases.cases");
    assert_eq!(
        cases.len(),
        APPENDS.len(),
        "every append case is interpreted"
    );
    for case in cases {
        let name = text(case, "name");
        let (_, descriptions, build) = APPENDS
            .iter()
            .find(|(n, _, _)| *n == name)
            .unwrap_or_else(|| panic!("no interpretation for append case `{name}`"));
        let described: Vec<&str> = list(case, "request.drafts")
            .iter()
            .map(|d| d.as_str().unwrap())
            .collect();
        assert_eq!(
            described.as_slice(),
            *descriptions,
            "{name}: the vector changed; re-read it"
        );
        let Some(mut b) = fresh() else { return };
        let s = append_chain(&mut b, &fx);
        let before = b.head(&s);
        assert_eq!(
            (before.seq, before.hash.to_hex(), before.writer_epoch),
            (
                int(&fx, "append_cases.stream_state.head_seq"),
                text(&fx, "append_cases.stream_state.head_hash").to_owned(),
                int(&fx, "append_cases.stream_state.writer_epoch")
            )
        );
        let drafts = build(&fx);
        let refs: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
        let outcome = b.append(
            &s,
            int(case, "request.expected_head"),
            int(case, "request.writer_epoch"),
            at(APPEND_TIME),
            &refs,
        );
        for (key, want) in get(case, "expect").as_object().unwrap() {
            let got = match (key.as_str(), &outcome) {
                ("outcome", o) => string(o.name()),
                ("returns_seq", AppendOutcome::AlreadyCommitted(rows))
                | ("seq", AppendOutcome::Committed(rows)) => {
                    let [row] = rows.as_slice() else {
                        panic!("{name}: one row expected, got {rows:?}")
                    };
                    Value::Int(mandate_canon::Int::new(row.seq).unwrap())
                }
                ("stored_seq", AppendOutcome::IdempotencyConflict { stored_seq }) => {
                    Value::Int(mandate_canon::Int::new(*stored_seq).unwrap())
                }
                ("actual_seq", AppendOutcome::HeadMismatch { actual_seq, .. }) => {
                    Value::Int(mandate_canon::Int::new(*actual_seq).unwrap())
                }
                ("actual_hash", AppendOutcome::HeadMismatch { actual_hash, .. }) => {
                    string(&actual_hash.to_hex())
                }
                ("current_epoch", AppendOutcome::Fenced { current_epoch }) => {
                    Value::Int(mandate_canon::Int::new(*current_epoch).unwrap())
                }
                ("prev_hash", AppendOutcome::Committed(rows)) => {
                    string(&rows[0].prev_hash.to_hex())
                }
                ("reason", AppendOutcome::Invalid { error, .. }) => string(error.reason.code()),
                _ => panic!("{name}: cannot check expect.{key} against {outcome:?}"),
            };
            assert_eq!(&got, want, "{name}: expect.{key}");
        }
        let rows = b.rows(&s);
        if !matches!(outcome, AppendOutcome::Committed(_)) {
            assert_eq!(
                b.head(&s),
                before,
                "{name}: head after a non-committing append"
            );
            assert_eq!(rows.len(), 5, "{name}: rows after a non-committing append");
        }
        assert!(
            verify_events(&rows, TrustedStart::GENESIS, &BTreeMap::new()).is_ok(),
            "{name}: the stream fails verification after the append"
        );
    }
}

/// One step of a random writer session against a single stream.
#[derive(Debug, Clone)]
enum Step {
    TakeOwnership,
    /// `head_offset` and `epoch_offset` are added to the true head and epoch (−1, 0, or +1).
    Append {
        head_offset: i8,
        epoch_offset: i8,
        drafts: Vec<DraftKind>,
    },
}

#[derive(Debug, Clone)]
enum DraftKind {
    Opened,
    /// A new mark whose `risk_clock` is `seconds` after 14:00:00.
    Mark {
        seconds: u8,
    },
    /// An earlier draft of this session, unchanged or with a different price.
    Resend {
        back: u8,
        changed: bool,
    },
    /// A new mark in the `backtest` environment.
    Backtest,
}

fn step() -> impl Strategy<Value = Step> {
    let draft = prop_oneof![
        1 => Just(DraftKind::Opened),
        6 => (0u8..30).prop_map(|seconds| DraftKind::Mark { seconds }),
        2 => (0u8..8, any::<bool>()).prop_map(|(back, changed)| DraftKind::Resend { back, changed }),
        1 => Just(DraftKind::Backtest),
    ];
    prop_oneof![
        1 => Just(Step::TakeOwnership),
        8 => (
            prop_oneof![1 => Just(-1i8), 6 => Just(0i8), 1 => Just(1i8)],
            prop_oneof![1 => Just(-1i8), 6 => Just(0i8), 1 => Just(1i8)],
            proptest::collection::vec(draft, 0..4)
        )
            .prop_map(|(head_offset, epoch_offset, drafts)| Step::Append {
                head_offset,
                epoch_offset,
                drafts
            }),
    ]
}

/// Distinguishes the streams and event IDs of each random session, so sessions can share one
/// backend without touching each other.
static SESSION: AtomicU64 = AtomicU64::new(0);

/// A session's draft builder: its own stream and event ID range, and the drafts made so far.
struct Session {
    number: u64,
    made: Vec<Vec<u8>>,
}

impl Session {
    fn new() -> Self {
        Self {
            number: SESSION.fetch_add(1, Ordering::Relaxed),
            made: Vec::new(),
        }
    }

    fn account(&self) -> String {
        format!("P{}", self.number)
    }

    fn stream(&self) -> StreamId {
        StreamId::parse(&format!("acct:ws_1:{}", self.account())).unwrap()
    }

    fn next_id(&self) -> String {
        format!(
            "\"{}\"",
            event_id(1_000_000 + self.number * 1000 + self.made.len() as u64)
        )
    }

    fn in_stream(&self, draft: &[u8]) -> Vec<u8> {
        let draft = edit(draft, "stream_id", Some(&format!("\"{}\"", self.stream())));
        edit(&draft, "event_id", Some(&self.next_id()))
    }

    fn draft(&mut self, kind: &DraftKind) -> Vec<u8> {
        let draft = match kind {
            DraftKind::Opened => edit(
                &self.in_stream(&opened_draft("paper")),
                "payload.account_ref",
                Some(&format!("\"{}\"", self.account())),
            ),
            DraftKind::Mark { seconds } => self.in_stream(&mark_draft_at(
                0,
                "1",
                &format!("2026-09-21T14:00:{seconds:02}.000000000Z"),
            )),
            DraftKind::Resend { back, changed } => {
                let Some(earlier) = self
                    .made
                    .len()
                    .checked_sub(usize::from(*back) + 1)
                    .map(|i| self.made[i].clone())
                else {
                    return self.draft(&DraftKind::Mark { seconds: 0 });
                };
                if *changed {
                    edit(
                        &earlier,
                        "event_time",
                        Some("\"2026-09-21T14:30:00.000000000Z\""),
                    )
                } else {
                    return earlier;
                }
            }
            DraftKind::Backtest => edit(
                &self.in_stream(&mark_draft(0, "1")),
                "environment",
                Some("\"backtest\""),
            ),
        };
        self.made.push(draft.clone());
        draft
    }
}

fn offset(value: u64, by: i8) -> u64 {
    match by {
        -1 => value.saturating_sub(1),
        1 => value.saturating_add(1),
        _ => value,
    }
}

/// Differential property: random writer sessions (valid appends, stale heads, wrong epochs,
/// retries, conflicting resends, reopenings, environment changes, regressing risk clocks, and
/// ownership changes) give exactly the outcomes, heads, and rows of a fresh `MemoryJournal`, and
/// every stored stream stays a verified chain.
pub fn matches_the_memory_journal_on_random_sequences<B: Backend>(b: &mut B) {
    let config = Config {
        cases: b.property_cases(),
        failure_persistence: None,
        ..Config::default()
    };
    let backend = RefCell::new(b);
    let mut runner = TestRunner::new(config);
    let result = runner.run(&proptest::collection::vec(step(), 1..12), |steps| {
        let mut b = backend.borrow_mut();
        let mut oracle = MemoryJournal::new();
        let mut session = Session::new();
        let s = session.stream();
        for (i, step) in (0u64..).zip(&steps) {
            let recorded_at = UtcNanos::from_parts(now().secs() + i as i64, 0).unwrap();
            match step {
                Step::TakeOwnership => {
                    let want = oracle.take_ownership(&s);
                    prop_assert_eq!(b.take_ownership(&s), want);
                }
                Step::Append {
                    head_offset,
                    epoch_offset,
                    drafts,
                } => {
                    let truth = oracle.head(&s);
                    let expected_head = offset(truth.seq, *head_offset);
                    let epoch = offset(truth.writer_epoch, *epoch_offset);
                    let drafts: Vec<Vec<u8>> = drafts.iter().map(|k| session.draft(k)).collect();
                    let refs: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
                    let want = oracle.append(&s, expected_head, epoch, recorded_at, &refs);
                    let got = b.append(&s, expected_head, epoch, recorded_at, &refs);
                    prop_assert_eq!(got, want, "step {}", i);
                }
            }
            prop_assert_eq!(b.head(&s), oracle.head(&s), "head after step {}", i);
            let rows = b.rows(&s);
            prop_assert_eq!(&rows, oracle.rows(&s), "rows after step {}", i);
            prop_assert!(verify_events(&rows, TrustedStart::GENESIS, &BTreeMap::new()).is_ok());
        }
        Ok(())
    });
    if let Err(failure) = result {
        panic!("{failure}");
    }
}
