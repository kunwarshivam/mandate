#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The journal port a long-running process writes through (journal spec §5.1): it takes a newer
//! writer epoch on its own stream, reads every stream it follows, and appends under its epoch,
//! over the in-memory journal (tests and fixtures, shared by every process of one run) and
//! Postgres. The scheduler (E21-29) and the runtime and executor processes (E21-30) share it
//! (DEC-1012).

pub mod envelope;
pub mod meter;

use std::cell::RefCell;
use std::rc::Rc;

use mandate_journal::{AppendOutcome, MemoryJournal, StoredEvent, StreamId, TrustedStart};
use mandate_journal_pg::PgJournal;
use mandate_time::UtcNanos;
use tokio::runtime::Runtime;

/// The three calls a writer makes (journal spec §5.1).
pub trait Store {
    /// Takes a newer writer epoch, fencing every earlier writer. `None` when the store cannot
    /// answer, which a writer reads as not holding the stream (`AGENTS.md` rule 3).
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64>;

    /// The stream's records, verified from its first. `None` when the store cannot answer.
    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>>;

    /// The stream's records from `start.from_seq` on, for a reader that holds the stream up to
    /// the seq before it with that record's hash (DEC-2847): `None` when the store cannot answer
    /// or the first of them does not follow `start`, as a reader whose held head is not this
    /// journal's must read the whole stream again. A store that reads only whole streams answers
    /// from [`Self::rows`], whose records are verified from their first, so a read from the first
    /// record is that read; the journal's own stores read only what follows.
    fn rows_from(&self, stream: &StreamId, start: TrustedStart) -> Option<Vec<StoredEvent>> {
        if start == TrustedStart::GENESIS {
            return self.rows(stream);
        }
        let since: Vec<StoredEvent> = self
            .rows(stream)?
            .into_iter()
            .filter(|row| row.seq >= start.from_seq)
            .collect();
        match since.first() {
            Some(first) if first.seq != start.from_seq || first.prev_hash != start.prev_hash => {
                None
            }
            _ => Some(since),
        }
    }

    /// The store's own clock, a second host's that a process may bound its own against
    /// (DEC-2292); `None` when the store has none or cannot answer, which a caller reads as no
    /// bound it can trust (`AGENTS.md` rule 3). The in-memory store has none.
    fn clock(&self) -> Option<UtcNanos> {
        None
    }

    /// Appends `drafts` at `expected_head` under `epoch`, recorded at `recorded_at`.
    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome;
}

/// One in-memory journal shared by every process of a test or fixture run, so two can contend
/// for one stream.
#[derive(Debug, Clone, Default)]
pub struct MemoryStore(pub Rc<RefCell<MemoryJournal>>);

impl Store for MemoryStore {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        Some(self.0.borrow_mut().take_ownership(stream))
    }

    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        Some(self.0.borrow().rows(stream).to_vec())
    }

    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        let drafts: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
        self.0
            .borrow_mut()
            .append(stream, expected_head, epoch, recorded_at, &drafts)
    }
}

/// The Postgres journal, driven from the writer's one thread on a current-thread runtime.
pub struct PgStore {
    journal: PgJournal,
    runtime: Runtime,
}

impl PgStore {
    /// Connects to `dsn` over one connection ([`PgJournal::single_writer`]).
    ///
    /// # Errors
    /// The runtime's or the connection's, as a stable code only: a DSN is a credential and never
    /// appears in an error (`AGENTS.md` rule 7).
    pub fn connect(dsn: &str) -> Result<Self, &'static str> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "runtime_unavailable")?;
        let journal = {
            let _entered = runtime.enter();
            PgJournal::single_writer(dsn).map_err(|e| e.code())?
        };
        Ok(Self { journal, runtime })
    }
}

impl Store for PgStore {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        self.runtime
            .block_on(self.journal.take_ownership(stream))
            .ok()
    }

    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        self.runtime.block_on(self.journal.rows(stream)).ok()
    }

    /// The stream's records from `start.from_seq` on, their chain checked from `start`
    /// ([`PgJournal::rows_from`]). `None` when the store cannot answer or the chain fails.
    /// The database server's `clock_timestamp()` ([`PgJournal::clock`]).
    fn clock(&self) -> Option<UtcNanos> {
        self.runtime.block_on(self.journal.clock()).ok()
    }

    fn rows_from(&self, stream: &StreamId, start: TrustedStart) -> Option<Vec<StoredEvent>> {
        self.runtime
            .block_on(self.journal.rows_from(stream, start))
            .ok()
    }

    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        let drafts: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
        self.runtime
            .block_on(
                self.journal
                    .append(stream, expected_head, epoch, recorded_at, &drafts),
            )
            .unwrap_or(AppendOutcome::Unavailable)
    }
}
