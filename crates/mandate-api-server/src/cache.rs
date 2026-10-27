//! [`CachedJournal`]: the [`Journal`] port with each stream read once and then only past its head
//! ([DEC-2145](../../../docs/project/decisions/DEC-2145.md)), as the executor's deployment reads its
//! hot streams ([DEC-2847](../../../docs/project/decisions/DEC-2847.md)).
//!
//! Every read the server serves calls [`Journal::rows`], so this one place keeps a page's cost from
//! growing with each stream's length: a stream already read is read again by
//! [`Journal::rows_from`] from its last held row on, its chain checked by the store from that row's
//! `prev_hash`, and the first row read must be the held row, hash for hash. Every row's hash chains
//! from the one before, so a stream changed anywhere at or before the held head fails that check,
//! and the rows served are exactly the stream's, as a read from its first record would give them.
//!
//! - **Tenancy.** Rows are held per workspace, by the workspace segment every stream id carries
//!   (journal spec §2), and per stream id within it. A read returns only the rows held under its
//!   own stream id, so one workspace's rows never answer another's read.
//! - **Exactly the stream.** A tail the store refuses, one that does not begin with the held head
//!   row, or a stream cut below the held head, drops what is held and reads the stream from its
//!   first record again. A read that fails answers [`JournalUnavailable`] and serves nothing held.
//!   So, unlike DEC-2847 item 4, history the server has read cannot be rewritten under it unseen:
//!   the held head row is read again, and its hash carries every earlier row's.
//! - **Bounded memory.** A workspace no read touched for [`Limits::idle`] on the server's [`Clock`]
//!   is dropped whole, and when the rows held pass [`Limits::max_bytes`] the least recently read
//!   other workspaces are dropped first; a stream larger than the bound by itself is served and not
//!   held. A read while the clock cannot be read goes to the store and holds nothing.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mandate_journal::{AppendOutcome, StoredEvent, StreamId, TrustedStart};
use mandate_time::UtcNanos;

use crate::clock::Clock;
use crate::journal::{Answer, Journal, JournalUnavailable, Rows};

/// How long and how much the cache holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// A workspace no read touched for this long is dropped whole at the next read.
    pub idle: Duration,
    /// The most row-body bytes held across every workspace.
    pub max_bytes: usize,
}

impl Limits {
    /// Ten idle minutes, and 256 MiB of row bodies.
    pub const SERVER: Self = Self {
        idle: Duration::from_secs(600),
        max_bytes: 256 * 1024 * 1024,
    };
}

/// One workspace's held streams, and when a read last touched it.
#[derive(Debug)]
struct Held {
    used: UtcNanos,
    streams: BTreeMap<String, Vec<StoredEvent>>,
}

impl Held {
    fn bytes(&self) -> usize {
        self.streams.values().map(|rows| bytes(rows)).sum()
    }
}

fn bytes(rows: &[StoredEvent]) -> usize {
    rows.iter().map(|row| row.body.len()).sum()
}

/// How long after `then` it is at `now`; zero when `now` is not later.
fn elapsed(then: UtcNanos, now: UtcNanos) -> Duration {
    let nanos = |at: UtcNanos| {
        i128::from(at.secs())
            .saturating_mul(1_000_000_000)
            .saturating_add(i128::from(at.nanos()))
    };
    u64::try_from(nanos(now).saturating_sub(nanos(then)))
        .map_or(Duration::ZERO, Duration::from_nanos)
}

/// Where a held stream's next read starts: its last held row, chained from that row's
/// `prev_hash`, so the read returns that row first and the store checks it is still the stream's.
fn start_at_head(rows: &[StoredEvent]) -> TrustedStart {
    rows.last()
        .map_or(TrustedStart::GENESIS, |last| TrustedStart {
            from_seq: last.seq,
            prev_hash: last.prev_hash,
        })
}

/// The [`Journal`] port over `inner`, holding each stream's verified rows (module docs).
pub struct CachedJournal {
    inner: Arc<dyn Journal>,
    clock: Arc<dyn Clock>,
    limits: Limits,
    held: Mutex<BTreeMap<String, Held>>,
}

impl CachedJournal {
    pub fn new(inner: Arc<dyn Journal>, clock: Arc<dyn Clock>, limits: Limits) -> Self {
        Self {
            inner,
            clock,
            limits,
            held: Mutex::new(BTreeMap::new()),
        }
    }

    /// The row-body bytes held now, across every workspace.
    pub fn held_bytes(&self) -> usize {
        self.held
            .lock()
            .map_or(0, |held| held.values().map(Held::bytes).sum())
    }

    /// The held rows of `stream` in `workspace`, after dropping every idle workspace, or `None`.
    fn take_held(&self, workspace: &str, stream: &str, now: UtcNanos) -> Option<Vec<StoredEvent>> {
        let mut held = self.held.lock().ok()?;
        held.retain(|_, space| elapsed(space.used, now) < self.limits.idle);
        let space = held.get_mut(workspace)?;
        space.used = now;
        space.streams.get(stream).cloned()
    }

    /// Holds `rows` as `stream`'s in `workspace`, then drops the least recently read other
    /// workspaces while the bound is passed; a stream over the bound by itself is not held.
    fn hold(&self, workspace: &str, stream: &str, rows: &[StoredEvent], now: UtcNanos) {
        let Ok(mut held) = self.held.lock() else {
            return;
        };
        if bytes(rows) > self.limits.max_bytes {
            if let Some(space) = held.get_mut(workspace) {
                space.streams.remove(stream);
            }
            return;
        }
        let space = held.entry(workspace.to_owned()).or_insert_with(|| Held {
            used: now,
            streams: BTreeMap::new(),
        });
        space.used = now;
        space.streams.insert(stream.to_owned(), rows.to_vec());
        while held.values().map(Held::bytes).sum::<usize>() > self.limits.max_bytes {
            let oldest = held
                .iter()
                .filter(|(name, _)| name.as_str() != workspace)
                .min_by_key(|(_, space)| space.used)
                .map(|(name, _)| name.clone());
            match oldest {
                Some(name) => {
                    held.remove(&name);
                }
                None => {
                    if let Some(space) = held.get_mut(workspace) {
                        space.streams.retain(|name, _| name == stream);
                    }
                    break;
                }
            }
        }
    }

    /// Drops what is held of `stream` in `workspace`.
    fn drop_held(&self, workspace: &str, stream: &str) {
        if let Ok(mut held) = self.held.lock()
            && let Some(space) = held.get_mut(workspace)
        {
            space.streams.remove(stream);
        }
    }

    /// `stream` read from its first record, then held.
    async fn read_whole(
        &self,
        workspace: &str,
        stream: &str,
        now: UtcNanos,
    ) -> Result<Vec<StoredEvent>, JournalUnavailable> {
        self.drop_held(workspace, stream);
        let rows = self.inner.rows(stream).await?;
        self.hold(workspace, stream, &rows, now);
        Ok(rows)
    }

    async fn cached_rows(&self, stream: &str) -> Result<Vec<StoredEvent>, JournalUnavailable> {
        let Some(workspace) =
            StreamId::parse(stream).and_then(|id| id.as_str().split(':').nth(1).map(str::to_owned))
        else {
            return self.inner.rows(stream).await;
        };
        let Some(now) = self.clock.now() else {
            return self.inner.rows(stream).await;
        };
        let Some(mut rows) = self.take_held(&workspace, stream, now) else {
            return self.read_whole(&workspace, stream, now).await;
        };
        let Ok(mut tail) = self.inner.rows_from(stream, start_at_head(&rows)).await else {
            return self.read_whole(&workspace, stream, now).await;
        };
        match (rows.last(), tail.first()) {
            (Some(held), Some(read)) if read.seq == held.seq && read.hash == held.hash => {
                tail.remove(0);
            }
            (None, None) => {}
            (None, Some(read)) if read.seq == 1 => {}
            _ => return self.read_whole(&workspace, stream, now).await,
        }
        if tail.is_empty() {
            return Ok(rows);
        }
        rows.extend(tail);
        self.hold(&workspace, stream, &rows, now);
        Ok(rows)
    }
}

impl Journal for CachedJournal {
    fn rows<'a>(&'a self, stream: &'a str) -> Rows<'a> {
        Box::pin(self.cached_rows(stream))
    }

    fn rows_from<'a>(&'a self, stream: &'a str, start: TrustedStart) -> Rows<'a> {
        self.inner.rows_from(stream, start)
    }

    fn take_ownership<'a>(&'a self, stream: &'a str) -> Answer<'a, u64> {
        self.inner.take_ownership(stream)
    }

    fn head<'a>(&'a self, stream: &'a str) -> Answer<'a, u64> {
        self.inner.head(stream)
    }

    fn event<'a>(&'a self, event_id: &'a str) -> Answer<'a, Option<StoredEvent>> {
        self.inner.event(event_id)
    }

    fn append<'a>(
        &'a self,
        stream: &'a str,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &'a [&'a [u8]],
    ) -> Answer<'a, AppendOutcome> {
        self.inner
            .append(stream, expected_head, writer_epoch, recorded_at, drafts)
    }
}
