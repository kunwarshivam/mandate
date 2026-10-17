//! Reading the journal (§6.1): the server reads streams and never writes an agent or account
//! stream (§1.4). The one port is [`Journal`]; [`Postgres`] serves it from the hot store.

use std::future::Future;
use std::pin::Pin;

use mandate_journal::{StoredEvent, StreamId};
use mandate_journal_pg::PgJournal;

/// The journal could not answer; the read reports `journal_unavailable` and claims nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalUnavailable;

/// A stream's rows, in `seq` order, or the failure.
pub type Rows<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<StoredEvent>, JournalUnavailable>> + Send + 'a>>;

/// The journal, as the server reads it.
pub trait Journal: Send + Sync {
    /// Every row of `stream`; an empty list when the stream has none.
    fn rows<'a>(&'a self, stream: &'a str) -> Rows<'a>;
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
}
