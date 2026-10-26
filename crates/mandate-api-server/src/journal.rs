//! The journal (§6.1): the server reads streams, and appends only owner commands to the workspace's
//! control stream; it never writes an agent or account stream (§1.4). The one port is [`Journal`];
//! [`Postgres`] serves it from the hot store.

use std::future::Future;
use std::pin::Pin;

use mandate_journal::{AppendOutcome, StoredEvent, StreamId};
use mandate_journal_pg::PgJournal;
use mandate_time::UtcNanos;

/// The journal could not answer; the read reports `journal_unavailable` and claims nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalUnavailable;

/// One journal call's answer, or the failure.
pub type Answer<'a, T> = Pin<Box<dyn Future<Output = Result<T, JournalUnavailable>> + Send + 'a>>;

/// A stream's rows, in `seq` order, or the failure.
pub type Rows<'a> = Answer<'a, Vec<StoredEvent>>;

/// The journal, as the server reads and appends to it (journal spec §5.1).
pub trait Journal: Send + Sync {
    /// Every row of `stream`; an empty list when the stream has none.
    fn rows<'a>(&'a self, stream: &'a str) -> Rows<'a>;
    /// A new writer epoch for `stream`, fencing any other writer.
    fn take_ownership<'a>(&'a self, stream: &'a str) -> Answer<'a, u64>;
    /// The `seq` of `stream`'s last row, 0 for none.
    fn head<'a>(&'a self, stream: &'a str) -> Answer<'a, u64>;
    /// The event stored under `event_id` in any stream, or `None` when no stream holds it. The
    /// caller scopes it to a workspace (§4.8.1).
    fn event<'a>(&'a self, event_id: &'a str) -> Answer<'a, Option<StoredEvent>>;
    /// Appends `drafts`, in order and all or none, at `expected_head` under `writer_epoch`. A
    /// failure that wrote nothing is [`JournalUnavailable`].
    fn append<'a>(
        &'a self,
        stream: &'a str,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &'a [&'a [u8]],
    ) -> Answer<'a, AppendOutcome>;
}

/// The Postgres hot store (`mandate-journal-pg`), which re-verifies each stream's chain on read.
pub struct Postgres(pub PgJournal);

impl Journal for Postgres {
    fn rows<'a>(&'a self, stream: &'a str) -> Rows<'a> {
        Box::pin(async move {
            let id = StreamId::parse(stream).ok_or(JournalUnavailable)?;
            self.0.rows(&id).await.map_err(|_| JournalUnavailable)
        })
    }

    fn take_ownership<'a>(&'a self, stream: &'a str) -> Answer<'a, u64> {
        Box::pin(async move {
            let id = StreamId::parse(stream).ok_or(JournalUnavailable)?;
            self.0
                .take_ownership(&id)
                .await
                .map_err(|_| JournalUnavailable)
        })
    }

    fn head<'a>(&'a self, stream: &'a str) -> Answer<'a, u64> {
        Box::pin(async move {
            let id = StreamId::parse(stream).ok_or(JournalUnavailable)?;
            let head = self.0.head(&id).await.map_err(|_| JournalUnavailable)?;
            Ok(head.seq)
        })
    }

    fn event<'a>(&'a self, event_id: &'a str) -> Answer<'a, Option<StoredEvent>> {
        Box::pin(async move { self.0.event(event_id).await.map_err(|_| JournalUnavailable) })
    }

    fn append<'a>(
        &'a self,
        stream: &'a str,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &'a [&'a [u8]],
    ) -> Answer<'a, AppendOutcome> {
        Box::pin(async move {
            let id = StreamId::parse(stream).ok_or(JournalUnavailable)?;
            self.0
                .append(&id, expected_head, writer_epoch, recorded_at, drafts)
                .await
                .map_err(|_| JournalUnavailable)
        })
    }
}
