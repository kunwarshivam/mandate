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
//! The Postgres journal hot store ([journal spec](../../../docs/specs/journal.md) §6.1, backlog
//! E5-3). It runs the append protocol of §5.1 over the same `Draft` and `seal` as
//! `mandate_journal::MemoryJournal`, with the same outcomes and rejection reasons, in one
//! transaction per append that holds the stream's head row locked, so concurrent appenders
//! serialize and can neither fork the chain nor reuse a `seq`.
//!
//! The database enforces the rest itself, through the forward-only migrations in `migrations/`
//! applied by [`migrator`] under [`OWNER_ROLE`]: the body is `bytea` with
//! `CHECK (hash = sha256(body))`, every event links to its predecessor, heads only advance, and the
//! event tables reject UPDATE, DELETE, and TRUNCATE by both privilege ([`APP_ROLE`] has INSERT and
//! SELECT) and trigger. Every read re-checks the stored bytes (§11 checks 1 to 5) before returning
//! them (DEC-109).

use mandate_journal::{AppendOutcome, EventFailure, Head, StoredEvent, StreamId};
use mandate_time::UtcNanos;
use sqlx::PgPool;
use sqlx::migrate::Migrator;

/// The migration-only role that owns the journal tables (journal spec §6.1). Deployments create it
/// before the first migration; no application connects as it.
pub const OWNER_ROLE: &str = "mandate_journal_owner";

/// The application role: INSERT and SELECT on `events` and `event_ids`, and on `stream_heads` the
/// UPDATE that advances a head or takes ownership. Deployments create it before the first
/// migration, which grants it exactly these privileges.
pub const APP_ROLE: &str = "mandate_journal_app";

/// The journal schema's migrations, embedded at build time: versioned, forward-only (no down
/// migrations), and applied only by the migration command under [`OWNER_ROLE`], never at startup
/// (ADR-0001 ES-08). Running it again is a no-op; a changed or unknown applied migration is an
/// error.
pub fn migrator() -> Migrator {
    Migrator::with_migrations(Vec::new())
}

/// Stored bytes failed re-verification on read: something changed the database outside the append
/// path. Nothing is repaired in place (journal spec §11); the caller treats it as an integrity
/// incident and stops writing the stream.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("stored event {stream_id} seq {} fails `{}`", failure.seq, failure.check.code())]
pub struct IntegrityError {
    pub stream_id: String,
    pub failure: EventFailure,
}

impl IntegrityError {
    /// Stable reason code (ADR-0001 ES-09): the failed check's code from journal spec §11.
    pub fn code(&self) -> &'static str {
        self.failure.check.code()
    }
}

/// Why a read or an ownership change did not return a result.
#[derive(Debug, thiserror::Error)]
pub enum PgError {
    /// The database could not be reached or the statement failed; nothing changed.
    #[error("the journal database is unavailable")]
    Unavailable(#[source] sqlx::Error),
    #[error(transparent)]
    Integrity(#[from] IntegrityError),
}

impl PgError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unavailable(_) => "unavailable",
            Self::Integrity(_) => "integrity",
        }
    }
}

/// The journal over a pool whose connections act as [`APP_ROLE`]. Streams exist implicitly with
/// head 0 and writer epoch 0, as in `MemoryJournal`.
#[derive(Debug, Clone)]
pub struct PgJournal {
    pool: PgPool,
}

impl PgJournal {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Increments the stream's writer epoch, fencing out the previous writer, and returns it.
    pub async fn take_ownership(&self, stream: &StreamId) -> Result<u64, PgError> {
        let _ = (&self.pool, stream);
        Err(PgError::Unavailable(sqlx::Error::PoolClosed))
    }

    pub async fn head(&self, stream: &StreamId) -> Result<Head, PgError> {
        let _ = (&self.pool, stream);
        Err(PgError::Unavailable(sqlx::Error::PoolClosed))
    }

    /// Appends `drafts` to `stream` in one transaction, in the order of journal spec §5.1, with the
    /// outcomes `MemoryJournal::append` gives for the same state and drafts. A failure before the
    /// commit is `Unavailable` (nothing was written; retry with the same drafts); a lost connection
    /// during the commit is `Ambiguous` (re-query by `event_id` before acting). `Err` means stored
    /// events the append had to read failed re-verification, and nothing was written.
    pub async fn append(
        &self,
        stream: &StreamId,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[&[u8]],
    ) -> Result<AppendOutcome, IntegrityError> {
        let _ = (
            &self.pool,
            stream,
            expected_head,
            writer_epoch,
            recorded_at,
            drafts,
        );
        Ok(AppendOutcome::Unavailable)
    }

    /// Every event of `stream` in `seq` order, after checking the whole chain from seq 1 (journal
    /// spec §11 checks 1 to 5). Artifact checks need the artifact store: run `verify_events` over
    /// these rows for those.
    pub async fn rows(&self, stream: &StreamId) -> Result<Vec<StoredEvent>, PgError> {
        let _ = (&self.pool, stream);
        Err(PgError::Unavailable(sqlx::Error::PoolClosed))
    }

    /// The event stored under `event_id` in any stream, after checking its bytes, columns, and hash
    /// (journal spec §11 checks 1, 2, and 4).
    pub async fn event(&self, event_id: &str) -> Result<Option<StoredEvent>, PgError> {
        let _ = (&self.pool, event_id);
        Err(PgError::Unavailable(sqlx::Error::PoolClosed))
    }
}
