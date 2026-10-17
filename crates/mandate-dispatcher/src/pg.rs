//! The dispatcher's [`Journal`] over the Postgres journal (E8-10 slice D10a, DEC-2500 items 3 and
//! 8): it takes its notice stream's writer epoch, which fences every older dispatcher; it reads
//! committed rows in short reads, each bounded by [`READ_DEADLINE`], any failure of which is the
//! closed [`DispatchError::JournalUnavailable`]; and it appends only through its
//! [`NoticeWriter`], at that writer's epoch, so a dispatcher whose epoch was taken is `Fenced` at
//! its next append. It holds no lock but that epoch. It drives the async `PgJournal` from the
//! dispatcher's one thread on a current-thread runtime, as `mandate-scheduler`'s store does.

use std::time::Duration;

use mandate_journal::{AppendOutcome, StoredEvent, StreamId};
use mandate_journal_pg::PgJournal;
use mandate_time::UtcNanos;
use tokio::runtime::Runtime;

use crate::DispatchError;
use crate::step::{Journal, NoticeWriter};

/// How long one read may take before it is [`DispatchError::JournalUnavailable`] (DEC-2500 item
/// 8): well inside the process's 60-second liveness bound, so a database that does not answer
/// stalls one step, never the process.
pub const READ_DEADLINE: Duration = Duration::from_secs(5);

/// The Postgres journal as the dispatcher reads and appends to it.
#[allow(
    dead_code,
    reason = "the tests PR's stubs read no field; the implementation reads both (DEC-77, E8-10 D10a)"
)]
pub struct PgNotices {
    journal: PgJournal,
    runtime: Runtime,
}

impl PgNotices {
    /// Connects to `dsn`, which authenticates as the dispatcher's own role (DEC-2500 item 8).
    /// Nothing is read until the first call.
    ///
    /// # Errors
    /// [`DispatchError::JournalUnavailable`] when the runtime cannot start or the DSN does not
    /// parse; the error never carries the DSN, a credential (`AGENTS.md` rule 7).
    pub fn connect(_dsn: &str) -> Result<Self, DispatchError> {
        Err(DispatchError::Unimplemented { story: "E8-10" })
    }

    /// Takes `stream`'s next writer epoch, fencing every older writer, and returns the one
    /// [`NoticeWriter`] this dispatcher appends through (journal spec §5.1, DEC-704 item 1).
    ///
    /// # Errors
    /// [`DispatchError::NotANoticeStream`] for any stream that is not a notice stream, before
    /// anything is written; [`DispatchError::JournalUnavailable`] when the store cannot answer.
    pub fn take(&mut self, _stream: StreamId) -> Result<NoticeWriter, DispatchError> {
        Err(DispatchError::Unimplemented { story: "E8-10" })
    }
}

impl Journal for PgNotices {
    /// One short read, bounded by [`READ_DEADLINE`]; an unreachable database, a refused statement,
    /// a lapsed deadline, and stored bytes that fail re-verification are each
    /// [`DispatchError::JournalUnavailable`].
    fn committed(&self, _stream: &StreamId) -> Result<Vec<StoredEvent>, DispatchError> {
        Err(DispatchError::Unimplemented { story: "E8-10" })
    }

    /// One append at `writer`'s epoch: `Fenced` once a newer dispatcher took the stream, and
    /// `Unavailable` when the store gave no outcome, so the step sends nothing more.
    fn append_notices(
        &mut self,
        _writer: &NoticeWriter,
        _expected_head: u64,
        _recorded_at: UtcNanos,
        _drafts: &[&[u8]],
    ) -> AppendOutcome {
        AppendOutcome::Unavailable
    }
}
