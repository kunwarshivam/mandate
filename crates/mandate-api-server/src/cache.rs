//! [`CachedJournal`]: the [`Journal`] port with each stream read whole now and then and, between,
//! only from its held head row on ([DEC-2145](../../../docs/project/decisions/DEC-2145.md)), as the
//! executor's deployment reads its hot streams ([DEC-2847](../../../docs/project/decisions/DEC-2847.md)).
//!
//! Every read the server serves calls [`Journal::rows`], so this one place keeps the store's read and
//! chain check from growing with each stream's length; the rows handed to the caller are still a
//! copy of the whole stream. A stream read whole is held. A later read asks the store, by
//! [`Journal::rows_from`], for the rows from the held head row on, chain-checked from that row's
//! `prev_hash`, and the first row read must be the held head row, seq and hash alike; the rows after
//! it are appended.
//!
//! - **What a read past the head sees.** Appends, and any change that reaches the held head row or
//!   cuts the stream below it, are seen at the next read: the stream is read whole again. A change
//!   to rows before the head that leaves the head row's hash as it was breaks the stored chain, so a
//!   whole read fails; the cache sees it at its next read of the stream once its last whole read is
//!   [`Limits::reverify`] old, and until then serves the rows it verified. A clock read earlier
//!   than a workspace's last read drops the workspace (below), so its streams are read whole.
//! - **Exactly the stream otherwise.** A tail the store refuses, or one that does not begin with the
//!   held head row, drops what is held and reads the stream whole. A whole read that fails drops
//!   every stream held for that workspace and answers [`JournalUnavailable`], as the store's own read
//!   does; nothing held is served in its place. A read already in flight answers as it would have,
//!   but holds nothing once any workspace has been dropped since it began.
//! - **Tenancy.** Rows are held per stream id, under the id's second segment, the workspace for
//!   every workspace stream (journal spec §2). A read returns only the rows held under its own
//!   stream id, so one workspace's rows never answer another's read.
//! - **Bounded memory.** A workspace no read touched for [`Limits::idle`] on the server's [`Clock`]
//!   is dropped whole at the next read, as is one whose last read the clock now reads as later than
//!   now, and when the rows held pass [`Limits::max_bytes`] the least
//!   recently read other workspaces are dropped first; a stream larger than the bound by itself is
//!   served and not held. A read while the clock cannot be read goes to the store and holds nothing.

use std::collections::BTreeMap;
use std::mem::size_of;
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
    /// A stream last read whole this long ago is read whole again, so a change to rows before its
    /// held head is seen within this long.
    pub reverify: Duration,
    /// The most bytes of rows held across every workspace, counted by [`row_bytes`].
    pub max_bytes: usize,
}

impl Limits {
    /// Ten idle minutes, a whole read of each stream at least once a minute, and 256 MiB of rows.
    pub const SERVER: Self = Self {
        idle: Duration::from_secs(600),
        reverify: Duration::from_secs(60),
        max_bytes: 256 * 1024 * 1024,
    };
}

/// The bytes a held row takes: the row itself and every string and body it owns.
pub fn row_bytes(row: &StoredEvent) -> usize {
    size_of::<StoredEvent>()
        .saturating_add(row.stream_id.len())
        .saturating_add(row.event_id.len())
        .saturating_add(row.event_type.len())
        .saturating_add(row.environment.len())
        .saturating_add(row.recorded_at.len())
        .saturating_add(row.body.len())
}

fn rows_bytes(rows: &[StoredEvent]) -> usize {
    rows.iter().map(row_bytes).fold(0, usize::saturating_add)
}

/// One stream's held rows, their bytes, and when they were last read whole.
#[derive(Debug, Clone)]
struct Stream {
    rows: Arc<Vec<StoredEvent>>,
    bytes: usize,
    whole_at: UtcNanos,
}

/// One workspace's held streams, their bytes, and when a read last touched it.
#[derive(Debug)]
struct Held {
    used: UtcNanos,
    bytes: usize,
    streams: BTreeMap<String, Stream>,
}

/// Every workspace held, their bytes together, and how many workspaces a failed whole read has
/// dropped, so a read that began before a drop does not hold its rows again after it.
#[derive(Debug, Default)]
struct State {
    bytes: usize,
    spaces: BTreeMap<String, Held>,
    drops: u64,
}

impl State {
    fn remove_stream(&mut self, workspace: &str, stream: &str) {
        let Some(space) = self.spaces.get_mut(workspace) else {
            return;
        };
        if let Some(gone) = space.streams.remove(stream) {
            space.bytes = space.bytes.saturating_sub(gone.bytes);
            self.bytes = self.bytes.saturating_sub(gone.bytes);
        }
        if space.streams.is_empty() {
            self.spaces.remove(workspace);
        }
    }

    fn remove_space(&mut self, workspace: &str) {
        if let Some(gone) = self.spaces.remove(workspace) {
            self.bytes = self.bytes.saturating_sub(gone.bytes);
        }
    }
}

/// How long after `then` it is at `now`; [`Duration::MAX`] when the clock has stepped back past
/// `then`, so a time it can no longer measure from counts as expired.
fn elapsed(then: UtcNanos, now: UtcNanos) -> Duration {
    let nanos = |at: UtcNanos| {
        i128::from(at.secs())
            .saturating_mul(1_000_000_000)
            .saturating_add(i128::from(at.nanos()))
    };
    u64::try_from(nanos(now).saturating_sub(nanos(then)))
        .map_or(Duration::MAX, Duration::from_nanos)
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
    held: Mutex<State>,
}

impl CachedJournal {
    pub fn new(inner: Arc<dyn Journal>, clock: Arc<dyn Clock>, limits: Limits) -> Self {
        Self {
            inner,
            clock,
            limits,
            held: Mutex::new(State::default()),
        }
    }

    /// The bytes of rows held now, across every workspace.
    pub fn held_bytes(&self) -> usize {
        self.held.lock().map_or(0, |held| held.bytes)
    }

    /// The failed whole reads so far, read before a read begins and checked again by [`Self::hold`].
    fn drops(&self) -> Option<u64> {
        self.held.lock().ok().map(|held| held.drops)
    }

    /// The held rows of `stream` in `workspace` if they were read whole within
    /// [`Limits::reverify`], with [`Self::drops`] as it stood when they were taken, after dropping
    /// every idle workspace, or `None`.
    fn take_held(&self, workspace: &str, stream: &str, now: UtcNanos) -> Option<(Stream, u64)> {
        let mut held = self.held.lock().ok()?;
        let drops = held.drops;
        let idle: Vec<String> = held
            .spaces
            .iter()
            .filter(|(_, space)| elapsed(space.used, now) >= self.limits.idle)
            .map(|(name, _)| name.clone())
            .collect();
        for name in idle {
            held.remove_space(&name);
        }
        let space = held.spaces.get_mut(workspace)?;
        space.used = now;
        let found = space.streams.get(stream)?;
        if elapsed(found.whole_at, now) >= self.limits.reverify {
            return None;
        }
        Some((found.clone(), drops))
    }

    /// Holds `rows` as `stream`'s in `workspace`, then drops the least recently read other
    /// workspaces while the bound is passed; a stream over the bound by itself is not held, and
    /// nothing is held when a workspace was dropped since `drops` was read.
    fn hold(&self, workspace: &str, stream: &str, rows: Stream, now: UtcNanos, drops: u64) {
        let Ok(mut held) = self.held.lock() else {
            return;
        };
        if held.drops != drops {
            return;
        }
        held.remove_stream(workspace, stream);
        let bytes = rows.bytes;
        if bytes > self.limits.max_bytes {
            return;
        }
        let space = held
            .spaces
            .entry(workspace.to_owned())
            .or_insert_with(|| Held {
                used: now,
                bytes: 0,
                streams: BTreeMap::new(),
            });
        space.used = now;
        space.bytes = space.bytes.saturating_add(bytes);
        space.streams.insert(stream.to_owned(), rows);
        held.bytes = held.bytes.saturating_add(bytes);
        while held.bytes > self.limits.max_bytes {
            let oldest = held
                .spaces
                .iter()
                .filter(|(name, _)| name.as_str() != workspace)
                .min_by_key(|(_, space)| space.used)
                .map(|(name, _)| name.clone());
            match oldest {
                Some(name) => held.remove_space(&name),
                None => {
                    let others: Vec<String> = held
                        .spaces
                        .get(workspace)
                        .map(|space| {
                            space
                                .streams
                                .keys()
                                .filter(|name| name.as_str() != stream)
                                .cloned()
                                .collect()
                        })
                        .unwrap_or_default();
                    for name in others {
                        held.remove_stream(workspace, &name);
                    }
                    break;
                }
            }
        }
    }

    /// Drops what is held of `stream` in `workspace`.
    fn drop_held(&self, workspace: &str, stream: &str) {
        if let Ok(mut held) = self.held.lock() {
            held.remove_stream(workspace, stream);
        }
    }

    /// Drops every stream held for `workspace`, and counts the drop.
    fn drop_workspace(&self, workspace: &str) {
        if let Ok(mut held) = self.held.lock() {
            held.remove_space(workspace);
            held.drops = held.drops.saturating_add(1);
        }
    }

    /// `stream` read from its first record, then held; a read that fails drops the workspace.
    async fn read_whole(
        &self,
        workspace: &str,
        stream: &str,
        now: UtcNanos,
    ) -> Result<Vec<StoredEvent>, JournalUnavailable> {
        self.drop_held(workspace, stream);
        let drops = self.drops();
        let Ok(rows) = self.inner.rows(stream).await else {
            self.drop_workspace(workspace);
            return Err(JournalUnavailable);
        };
        if let Some(drops) = drops {
            let whole = Stream {
                bytes: rows_bytes(&rows),
                rows: Arc::new(rows.clone()),
                whole_at: now,
            };
            self.hold(workspace, stream, whole, now, drops);
        }
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
        let Some((held, drops)) = self.take_held(&workspace, stream, now) else {
            return self.read_whole(&workspace, stream, now).await;
        };
        let Ok(tail) = self
            .inner
            .rows_from(stream, start_at_head(&held.rows))
            .await
        else {
            return self.read_whole(&workspace, stream, now).await;
        };
        let new = match (held.rows.last(), tail.split_first()) {
            (Some(head), Some((read, new))) if read.seq == head.seq && read.hash == head.hash => {
                new
            }
            (None, None) => &[],
            (None, Some((read, _))) if read.seq == 1 => tail.as_slice(),
            _ => return self.read_whole(&workspace, stream, now).await,
        };
        if new.is_empty() {
            return Ok(held.rows.as_ref().clone());
        }
        let mut rows = Vec::with_capacity(held.rows.len().saturating_add(new.len()));
        rows.extend_from_slice(&held.rows);
        rows.extend_from_slice(new);
        let rows = Arc::new(rows);
        let grown = Stream {
            rows: Arc::clone(&rows),
            bytes: held.bytes.saturating_add(rows_bytes(new)),
            whole_at: held.whole_at,
        };
        self.hold(&workspace, stream, grown, now, drops);
        Ok(rows.as_ref().clone())
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
