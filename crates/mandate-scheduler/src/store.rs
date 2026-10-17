//! The journal port the scheduler writes through, over the in-memory journal and Postgres.

use std::cell::RefCell;
use std::rc::Rc;

use mandate_journal::{AppendOutcome, MemoryJournal, StoredEvent, StreamId};
use mandate_journal_pg::PgJournal;
use mandate_time::UtcNanos;
use tokio::runtime::Runtime;

/// The three calls the scheduler makes on its stream (journal spec §5.1).
pub trait ClockStore {
    /// Takes a newer writer epoch, fencing every earlier writer. `None` when the store cannot
    /// answer, which the scheduler reads as not holding the stream (`AGENTS.md` rule 3).
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64>;

    /// The stream's records, verified from its first. `None` when the store cannot answer.
    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>>;

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

/// One in-memory journal shared by every scheduler of a test or fixture run, so two can contend
/// for one stream.
#[derive(Debug, Clone, Default)]
pub struct MemoryStore(pub Rc<RefCell<MemoryJournal>>);

impl ClockStore for MemoryStore {
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

/// The Postgres journal, driven from the scheduler's one thread on a current-thread runtime.
pub struct PgStore {
    journal: PgJournal,
    runtime: Runtime,
}

impl PgStore {
    /// Connects to `dsn`.
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
            PgJournal::from_dsn(dsn).map_err(|e| e.code())?
        };
        Ok(Self { journal, runtime })
    }
}

impl ClockStore for PgStore {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        self.runtime
            .block_on(self.journal.take_ownership(stream))
            .ok()
    }

    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        self.runtime.block_on(self.journal.rows(stream)).ok()
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
