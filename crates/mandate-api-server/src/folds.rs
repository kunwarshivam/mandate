//! [`Folds`]: each account read's fold held per stream and agent, and resumed past the last row it
//! folded ([DEC-2145](../../../docs/project/decisions/DEC-2145.md) item 10), so a page's fold costs
//! the rows appended since the last read, not the stream's length.
//!
//! - **Exactly the stream's fold.** A held fold is resumed only when the rows read now hold, at
//!   the held fold's last seq, the row it last folded, hash for hash. The rows come from the
//!   [`crate::journal::Journal`] port with their chain verified (journal spec §11; DEC-2145 item 1),
//!   and every row's hash chains from the row before, so that one row's hash fixes every row the
//!   fold has folded: the held fold is the fold of the stream's prefix. Any other rows, a stream
//!   cut below the held seq or changed at or before it, fold from the first row again.
//! - **Nothing from part of the rows.** A row a fold cannot read drops what is held and serves
//!   nothing, as the fold of the whole stream does.
//! - **Tenancy.** A fold is held under the stream id its rows carry, which names the workspace
//!   (journal spec §2), and the agent it folds for; it answers only that stream and agent.
//! - **Bounded memory.** At most [`HELD`] folds of each kind are held; past it the least recently
//!   read is dropped.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use mandate_canon::{Digest, Value};
use mandate_journal::StoredEvent;

use crate::reads::{AccountFold, HoldingsFold, OrdersFold, PositionsFold};

/// The most folds of one kind held at once.
pub const HELD: usize = 1024;

/// One fold, the seq and hash of the last row it folded, and when it was last read.
#[derive(Debug, Clone)]
struct Held<F> {
    fold: F,
    seq: u64,
    hash: Digest,
    used: u64,
}

/// The stream id and the agent a fold is held for.
type Key = (String, String);

/// The folds of one kind held, and a counter that orders their reads.
#[derive(Debug)]
struct Table<F> {
    tick: u64,
    folds: BTreeMap<Key, Held<F>>,
}

/// One kind of fold, held per stream and agent.
#[derive(Debug)]
struct Resumed<F> {
    held: Mutex<Table<F>>,
    folded: AtomicU64,
}

impl<F> Default for Resumed<F> {
    fn default() -> Self {
        Self {
            held: Mutex::new(Table {
                tick: 0,
                folds: BTreeMap::new(),
            }),
            folded: AtomicU64::new(0),
        }
    }
}

impl<F: AccountFold> Resumed<F> {
    /// `rows`' fold for `agent`, resumed from a held fold when `rows` still hold its last row,
    /// else begun by `fresh`; `None` when a row cannot be read.
    fn read(&self, rows: &[StoredEvent], agent: &str, fresh: impl FnOnce() -> F) -> Option<F> {
        let Some(last) = rows.last() else {
            return Some(fresh());
        };
        let key = (last.stream_id.clone(), agent.to_owned());
        let held = self
            .held
            .lock()
            .ok()
            .and_then(|held| held.folds.get(&key).cloned());
        let resumed = held.and_then(|held| {
            let at = usize::try_from(held.seq).ok()?.checked_sub(1)?;
            let row = rows.get(at)?;
            (row.seq == held.seq && row.hash == held.hash)
                .then(|| (held.fold, at.saturating_add(1)))
        });
        let (mut fold, from) = resumed.unwrap_or_else(|| (fresh(), 0));
        let tail = rows.get(from..).unwrap_or_default();
        for row in tail {
            if fold.fold(row).is_none() {
                self.drop_held(&key);
                return None;
            }
        }
        self.folded.fetch_add(
            u64::try_from(tail.len()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        self.hold(key, &fold, last);
        Some(fold)
    }

    fn hold(&self, key: Key, fold: &F, last: &StoredEvent) {
        let Ok(mut held) = self.held.lock() else {
            return;
        };
        let Table { tick, folds } = &mut *held;
        *tick = tick.saturating_add(1);
        folds.insert(
            key,
            Held {
                fold: fold.clone(),
                seq: last.seq,
                hash: last.hash,
                used: *tick,
            },
        );
        while folds.len() > HELD {
            let Some(oldest) = folds
                .iter()
                .min_by_key(|(_, held)| held.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            folds.remove(&oldest);
        }
    }

    fn drop_held(&self, key: &Key) {
        if let Ok(mut held) = self.held.lock() {
            held.folds.remove(key);
        }
    }

    fn len(&self) -> usize {
        self.held.lock().map_or(0, |held| held.folds.len())
    }
}

/// The account reads' folds, held per stream and agent (module docs).
#[derive(Debug, Default)]
pub struct Folds {
    orders: Resumed<OrdersFold>,
    positions: Resumed<PositionsFold>,
    holdings: Resumed<HoldingsFold>,
}

impl Folds {
    /// [`crate::reads::agent_orders`] of `rows` for `agent`.
    pub fn orders(&self, rows: &[StoredEvent], agent: &str) -> Vec<Value> {
        self.orders
            .read(rows, agent, || OrdersFold::new(agent))
            .map_or_else(Vec::new, |fold| fold.finish())
    }

    /// [`crate::reads::agent_positions`] of `rows` for `agent`.
    pub fn positions(&self, rows: &[StoredEvent], agent: &str) -> Option<Vec<Value>> {
        self.positions
            .read(rows, agent, || PositionsFold::new(agent))?
            .finish()
    }

    /// [`crate::reads::account_positions`] of `rows`.
    pub fn holdings(&self, rows: &[StoredEvent]) -> Vec<Value> {
        self.holdings
            .read(rows, "", HoldingsFold::default)
            .map_or_else(Vec::new, |fold| fold.finish())
    }

    /// How many rows the folds have folded, for each of orders, positions and holdings.
    pub fn folded(&self) -> [u64; 3] {
        [
            self.orders.folded.load(Ordering::Relaxed),
            self.positions.folded.load(Ordering::Relaxed),
            self.holdings.folded.load(Ordering::Relaxed),
        ]
    }

    /// How many folds are held, for each of orders, positions and holdings.
    pub fn held(&self) -> [usize; 3] {
        [self.orders.len(), self.positions.len(), self.holdings.len()]
    }
}
