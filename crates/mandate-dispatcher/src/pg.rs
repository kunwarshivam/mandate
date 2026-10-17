//! The dispatcher's [`Journal`] over the Postgres journal (E8-10 slice D10a, DEC-2500 items 3 and
//! 8): it takes its notice stream's writer epoch, which fences every older dispatcher; it reads
//! committed rows in short reads, each bounded by [`READ_DEADLINE`], any failure of which is the
//! closed [`DispatchError::JournalUnavailable`]; and it appends only through its
//! [`NoticeWriter`], at that writer's epoch, so a dispatcher whose epoch was taken is `Fenced` at
//! its next append. It holds no lock but that epoch.
//!
//! It drives the async `PgJournal` from the dispatcher's one thread on a multi-thread tokio
//! runtime with exactly one worker thread, never a current-thread runtime. An append that returns
//! `Fenced` drops its transaction without an explicit rollback, and sqlx finishes that rollback on
//! a spawned task. A current-thread runtime runs spawned tasks only inside `block_on`, so between
//! calls it would leave the connection idle in a transaction that holds `FOR UPDATE` on the
//! stream's head row, and the newer dispatcher's next append would wait on that lock forever:
//! fencing would deadlock the very writer it hands the stream to (DEC-2500 item 3, NT-8). The one
//! worker keeps driving spawned tasks between calls, so the rollback lands and the lock is freed.

use std::time::Duration;

use mandate_journal::{AppendOutcome, StoredEvent, StreamId};
use mandate_journal_pg::PgJournal;
use mandate_time::UtcNanos;
use tokio::runtime::{Builder, Runtime};
use tokio::time::timeout;

use crate::DispatchError;
use crate::step::{Journal, NoticeWriter};

/// How long one read may take before it is [`DispatchError::JournalUnavailable`] (DEC-2500 item
/// 8): well inside the process's 60-second liveness bound, so a database that does not answer
/// stalls one step, never the process.
pub const READ_DEADLINE: Duration = Duration::from_secs(5);

/// The Postgres journal as the dispatcher reads and appends to it, with the one-worker multi-thread
/// runtime that drives it (see the module docs for why it is never a current-thread runtime).
pub struct PgNotices {
    journal: PgJournal,
    runtime: Runtime,
}

impl PgNotices {
    /// Connects to `dsn`, which authenticates as the dispatcher's own role (DEC-2500 item 8).
    /// Nothing is read until the first call. Starts the runtime as a multi-thread runtime with one
    /// worker thread, so a dropped transaction's rollback completes between calls.
    ///
    /// # Errors
    /// [`DispatchError::JournalUnavailable`] when the runtime cannot start or the DSN does not
    /// parse; the error never carries the DSN, a credential (`AGENTS.md` rule 7).
    pub fn connect(dsn: &str) -> Result<Self, DispatchError> {
        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|_| DispatchError::JournalUnavailable)?;
        let journal = {
            let _entered = runtime.enter();
            PgJournal::from_dsn(dsn).map_err(|_| DispatchError::JournalUnavailable)?
        };
        Ok(Self { journal, runtime })
    }

    /// Takes `stream`'s next writer epoch, fencing every older writer, and returns the one
    /// [`NoticeWriter`] this dispatcher appends through (journal spec §5.1, DEC-704 item 1).
    ///
    /// # Errors
    /// [`DispatchError::NotANoticeStream`] for any stream that is not a notice stream, before
    /// anything is written; [`DispatchError::JournalUnavailable`] when the store cannot answer.
    pub fn take(&mut self, stream: StreamId) -> Result<NoticeWriter, DispatchError> {
        let unwritten = NoticeWriter::new(stream, 0)?;
        let stream = unwritten.stream()?;
        let epoch = self
            .runtime
            .block_on(self.journal.take_ownership(stream))
            .map_err(|_| DispatchError::JournalUnavailable)?;
        NoticeWriter::new(stream.clone(), epoch)
    }
}

impl Journal for PgNotices {
    /// One short read, bounded by [`READ_DEADLINE`]; an unreachable database, a refused statement,
    /// a lapsed deadline, and stored bytes that fail re-verification are each
    /// [`DispatchError::JournalUnavailable`].
    fn committed(&self, stream: &StreamId) -> Result<Vec<StoredEvent>, DispatchError> {
        let bounded = async { timeout(READ_DEADLINE, self.journal.rows(stream)).await };
        match self.runtime.block_on(bounded) {
            Ok(Ok(rows)) => Ok(rows),
            Ok(Err(_)) | Err(_) => Err(DispatchError::JournalUnavailable),
        }
    }

    /// One append at `writer`'s epoch: `Fenced` once a newer dispatcher took the stream, and
    /// `Unavailable` when the store gave no outcome, so the step sends nothing more. A `Fenced`
    /// append's transaction is rolled back by the runtime's worker after this returns, which is
    /// why that runtime must keep driving between calls.
    fn append_notices(
        &mut self,
        writer: &NoticeWriter,
        expected_head: u64,
        recorded_at: UtcNanos,
        drafts: &[&[u8]],
    ) -> AppendOutcome {
        let (Ok(stream), Ok(epoch)) = (writer.stream(), writer.epoch()) else {
            return AppendOutcome::Unavailable;
        };
        self.runtime
            .block_on(
                self.journal
                    .append(stream, expected_head, epoch, recorded_at, drafts),
            )
            .unwrap_or(AppendOutcome::Unavailable)
    }
}
