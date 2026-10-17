//! The bounded hot cache (spec §5.1): per subscribed instrument, the quote that may mark now and
//! the one that last did. It is memory only and starts empty, so a restart has no mark until live
//! quotes arrive (spec §7, "Restart").

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use mandate_domain::AssetId;
use mandate_risk::SaneQuote;
use mandate_time::UtcNanos;

use crate::quote::{Feed, Freshness, Quote};

/// What [`HotCache::intake`] did with a quote. Only [`Intake::Marked`] makes it the instrument's
/// mark; every other outcome is counted by the caller and stored, never marked (spec §3.3, §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intake {
    /// In order and sane: the instrument's mark now.
    Marked,
    /// In order but crossed or too wide: the instrument has no mark until a later sane quote.
    NotSane,
    /// Stamped more than `future_skew` after its receipt: the instrument has no mark until a later
    /// sane quote, and the order watermark does not move (DEC-994 item 3).
    FutureStamped,
    /// Stamped at or before the last in-order quote (DEC-433 item 8): the mark is unchanged.
    OutOfOrder,
    /// The same message as the last in-order quote: dropped from the live path.
    Duplicate,
}

/// Why [`HotCache::intake`] refused a quote outright (spec §3.1: an unknown instrument is refused
/// and counted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The instrument is not in the subscription set.
    NotSubscribed,
    /// The instrument is subscribed on another feed.
    OtherFeed,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = f;
        todo!()
    }
}

impl Error for Refused {}

/// The cache already holds its capacity of instruments (spec §3.2: the overflow is left out, so it
/// has no fresh quote and openings in it are denied).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheFull;

impl fmt::Display for CacheFull {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = f;
        todo!()
    }
}

impl Error for CacheFull {}

/// One subscribed instrument's state.
#[derive(Debug, Clone)]
struct Entry {
    feed: Feed,
    /// The last in-order quote, sane or not: the order watermark (DEC-994 item 2).
    watermark: Option<Quote>,
    /// The quote that marks now, if any.
    mark: Option<Quote>,
    /// The last quote that marked, kept for exit pricing however old (trading spec §8.2).
    last_marked: Option<Quote>,
}

/// The bounded hot cache of one workspace's quotes.
#[derive(Debug, Clone)]
pub struct HotCache {
    freshness: Freshness,
    capacity: usize,
    clock_in_tolerance: bool,
    entries: BTreeMap<AssetId, Entry>,
}

impl HotCache {
    /// An empty cache holding at most `capacity` instruments. The host clock starts not known to
    /// be in tolerance, so nothing is fresh for an opening until [`Self::set_clock_in_tolerance`]
    /// is told otherwise (DEC-994 item 7).
    pub fn new(freshness: Freshness, capacity: usize) -> Self {
        let _ = (freshness, capacity);
        todo!()
    }

    /// Adds `instrument` on `feed` to the subscription set. Subscribing it again on the same feed
    /// changes nothing; on another feed it starts over with no quote. [`CacheFull`] when a new
    /// instrument would exceed the capacity.
    pub fn subscribe(&mut self, instrument: AssetId, feed: Feed) -> Result<(), CacheFull> {
        let _ = (instrument, feed);
        todo!()
    }

    /// Removes `instrument` and everything held for it.
    pub fn unsubscribe(&mut self, instrument: &AssetId) {
        let _ = instrument;
        todo!()
    }

    /// Takes one received quote. Checked in this order: subscribed on its feed, else
    /// [`Refused`]; stamped too far in the future ([`Intake::FutureStamped`]); the same message as
    /// the watermark ([`Intake::Duplicate`]); stamped at or before the watermark
    /// ([`Intake::OutOfOrder`]); then it becomes the watermark, and marks if sane.
    pub fn intake(&mut self, quote: Quote) -> Result<Intake, Refused> {
        let _ = quote;
        todo!()
    }

    /// The connection carrying every subscribed instrument was lost: no instrument has a mark
    /// until a quote received after this one is taken (DP-7, DEC-994 item 5). Watermarks stay, so
    /// a replay of an earlier quote never marks.
    pub fn connection_lost(&mut self) {
        todo!()
    }

    /// Whether the host clock is in tolerance: false while the scheduler's
    /// `ClockToleranceExceeded` stands, true after a later in-tolerance `ClockOffsetRecorded`
    /// (journal spec §5.4, DEC-433 item 5).
    pub fn set_clock_in_tolerance(&mut self, in_tolerance: bool) {
        let _ = in_tolerance;
        todo!()
    }

    /// The quote an opening or increasing order in `instrument` may use at `now`: the mark, if
    /// the clock is in tolerance and it is fresh at `now`, with `at` its age basis. `None` denies
    /// the opening (`stale_mark`, mandate spec §5.2) and never touches an exit.
    pub fn opening_quote(&self, instrument: &AssetId, now: UtcNanos) -> Option<SaneQuote> {
        let _ = (instrument, now);
        todo!()
    }

    /// The last quote that marked `instrument`, however old: exits take any mark source (trading
    /// spec §8.2) and price from it as §5.6 says.
    pub fn last_marked(&self, instrument: &AssetId) -> Option<&Quote> {
        let _ = instrument;
        todo!()
    }
}
