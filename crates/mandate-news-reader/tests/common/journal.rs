//! A journal whose news stream is the real in-memory journal, so every record the reader writes
//! passes the journal's own rules, and whose other streams are the rows a test gives; an artifact
//! shelf; and the control stream's instrument snapshot registrations.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use mandate_canon::{Digest, Value, to_canonical};
use mandate_journal::{
    AppendOutcome, ArtifactError, ArtifactRef, ArtifactSource, ArtifactStore, StoredEvent,
    StreamId, StreamType,
};
use mandate_store::{MemoryStore, Store};
use mandate_time::UtcNanos;

use super::fixtures::{WS, object, s};

/// The journal: the news stream is the real in-memory journal; every other stream answers the
/// rows a test set, or nothing when `others_down` is set.
#[derive(Clone, Default)]
pub struct Journal {
    pub memory: MemoryStore,
    pub others: Rc<RefCell<BTreeMap<String, Vec<StoredEvent>>>>,
    pub others_down: Rc<Cell<bool>>,
    /// Answers every append `Unavailable` while set.
    pub appends_down: Rc<Cell<bool>>,
    /// Answers every append `HeadMismatch` while set: a refusal that is not a fence.
    pub appends_refused: Rc<Cell<bool>>,
    /// The news stream's row bodies a test replaces, by `seq`, as a store that cannot be trusted
    /// would answer them: what the reader reads at start, never what the journal holds.
    pub news_bodies: Rc<RefCell<BTreeMap<u64, Vec<u8>>>>,
    /// Run once, on the journal, as the next `take_ownership` begins: another writer's append
    /// landing between a start's first read of its stream and the epoch it takes.
    pub before_ownership: Rc<RefCell<Option<BeforeOwnership>>>,
    /// What the store's clock answers once a test sets it, `None` for a store that cannot answer;
    /// unset, it answers a time far past every test's, so it bounds nothing (DEC-2292).
    pub store_clock: Rc<Cell<Option<Option<UtcNanos>>>>,
}

/// What [`Journal::before_ownership`] runs.
pub type BeforeOwnership = Box<dyn FnOnce(&mut MemoryStore)>;

impl Store for Journal {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        let hook = self.before_ownership.borrow_mut().take();
        if let Some(hook) = hook {
            hook(&mut self.memory);
        }
        self.memory.take_ownership(stream)
    }

    fn clock(&self) -> Option<UtcNanos> {
        self.store_clock
            .get()
            .unwrap_or_else(|| UtcNanos::from_parts(253_402_300_799, 0).ok())
    }

    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        if stream.stream_type() == StreamType::News {
            let replaced = self.news_bodies.borrow();
            return self.memory.rows(stream).map(|rows| {
                rows.into_iter()
                    .map(|mut row| {
                        if let Some(body) = replaced.get(&row.seq) {
                            row.body.clone_from(body);
                        }
                        row
                    })
                    .collect()
            });
        }
        if self.others_down.get() {
            return None;
        }
        Some(
            self.others
                .borrow()
                .get(stream.as_str())
                .cloned()
                .unwrap_or_default(),
        )
    }

    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        if self.appends_down.get() {
            return AppendOutcome::Unavailable;
        }
        if self.appends_refused.get() {
            return AppendOutcome::HeadMismatch {
                actual_seq: 99,
                actual_hash: Digest::of(b"other"),
            };
        }
        self.memory
            .append(stream, expected_head, epoch, recorded_at, drafts)
    }
}

impl Journal {
    /// Appends a row of `event_type` with `payload` to `stream`, outside the journal's checks: the
    /// streams the reader only reads.
    pub fn push(&self, stream: &str, event_type: &str, payload: Value) {
        let mut others = self.others.borrow_mut();
        let rows = others.entry(stream.to_owned()).or_default();
        let seq = u64::try_from(rows.len()).unwrap() + 1;
        let body = mandate_canon::Value::Object(
            [
                ("event_type".to_owned(), Value::Str(event_type.to_owned())),
                ("payload".to_owned(), payload),
            ]
            .into_iter()
            .map(|(k, v)| (mandate_canon::Key::new(&k).unwrap(), v))
            .collect(),
        );
        rows.push(StoredEvent {
            stream_id: stream.to_owned(),
            seq,
            event_id: format!("e{seq}"),
            event_type: event_type.to_owned(),
            schema_version: 1,
            environment: "paper".to_owned(),
            recorded_at: "2026-10-26T14:00:00.000000000Z".to_owned(),
            prev_hash: Digest::of(b""),
            hash: Digest::of(b""),
            body: to_canonical(&body),
        });
    }

    /// The news stream's rows, as the journal stored them.
    pub fn news(&self) -> Vec<StoredEvent> {
        self.memory
            .rows(&StreamId::parse(&format!("news:{WS}")).unwrap())
            .unwrap()
    }
}

/// An artifact store shared with the test.
#[derive(Clone, Default)]
pub struct Shelf {
    pub objects: Rc<RefCell<BTreeMap<Digest, Vec<u8>>>>,
    pub down: Rc<Cell<bool>>,
}

impl ArtifactSource for Shelf {
    fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        self.objects.borrow().read_artifact(reference)
    }
}

impl ArtifactStore for Shelf {
    fn put_artifact(&mut self, bytes: &[u8]) -> Result<ArtifactRef, ArtifactError> {
        if self.down.get() {
            return Err(ArtifactError::Unavailable);
        }
        self.objects.borrow_mut().put_artifact(bytes)
    }
}

/// Registers `asset`'s instrument snapshot naming `symbol` on the control stream, its artifact on
/// the shelf; returns the artifact's reference.
pub fn register(journal: &Journal, shelf: &Shelf, asset: &str, symbol: &str) -> ArtifactRef {
    let snapshot = object(&[
        ("asset_class", s("us_equity")),
        ("etp", s("plain")),
        ("etp_classified_at", s("2026-10-01T00:00:00Z")),
        ("etp_source", s("nasdaq_trader_symbol_directory")),
        ("exchange", s("arca")),
        ("increment", s("whole")),
        ("instrument_id", s(asset)),
        ("symbol", s(symbol)),
    ]);
    let reference = shelf
        .clone()
        .put_artifact(&to_canonical(&snapshot))
        .unwrap();
    register_ref(journal, &reference);
    reference
}

/// A registration naming `reference`, whatever the shelf holds under it.
pub fn register_ref(journal: &Journal, reference: &ArtifactRef) {
    journal.push(
        &format!("ctl:{WS}"),
        "ConfigSnapshotRegistered",
        object(&[
            ("kind", s("instrument_snapshot")),
            ("content_hash", s(&reference.to_string())),
        ]),
    );
}
