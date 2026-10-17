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
    /// sane quote, and the order watermark does not move (DEC-1008 item 3).
    FutureStamped,
    /// Received at or before the instrument's subscription or the last connection loss, so not a
    /// live quote since resync (DP-7, DEC-1008 item 5): the mark and the watermark are unchanged.
    /// A quote that is also stamped too far in the future is [`Intake::FutureStamped`] instead,
    /// and withdraws the mark.
    BeforeResync,
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
        f.write_str(match self {
            Self::NotSubscribed => "quote for an instrument not subscribed",
            Self::OtherFeed => "quote from a feed the instrument is not subscribed on",
        })
    }
}

impl Error for Refused {}

/// The cache already holds its capacity of instruments (spec §3.2: the overflow is left out, so it
/// has no fresh quote and openings in it are denied).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheFull;

impl fmt::Display for CacheFull {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the hot cache holds its capacity of instruments")
    }
}

impl Error for CacheFull {}

/// One subscribed instrument's state.
#[derive(Debug, Clone)]
struct Entry {
    feed: Feed,
    /// The subscription's instant, or the last connection loss after it: a quote received at or
    /// before it never marks.
    resync_from: UtcNanos,
    /// The last in-order quote, sane or not: the order watermark (DEC-1008 item 2).
    watermark: Option<Quote>,
    /// The quote that marks now, if any.
    mark: Option<Quote>,
    /// The last quote that marked, kept for exit pricing however old (trading spec §8.2).
    last_marked: Option<Quote>,
}

impl Entry {
    fn empty(feed: Feed, resync_from: UtcNanos) -> Self {
        Self {
            feed,
            resync_from,
            watermark: None,
            mark: None,
            last_marked: None,
        }
    }
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
    /// is told otherwise (DEC-1008 item 7).
    pub fn new(freshness: Freshness, capacity: usize) -> Self {
        Self {
            freshness,
            capacity,
            clock_in_tolerance: false,
            entries: BTreeMap::new(),
        }
    }

    /// Adds `instrument` on `feed` to the subscription set at `at`, the host clock's time of the
    /// subscription: only a quote received after `at` can mark it (spec §3.2, `resyncing`).
    /// Subscribing it again on the same feed changes nothing; on another feed it starts over with
    /// no quote, still taking nothing received at or before an earlier connection loss.
    /// [`CacheFull`] when a new instrument would exceed the capacity.
    pub fn subscribe(
        &mut self,
        instrument: AssetId,
        feed: Feed,
        at: UtcNanos,
    ) -> Result<(), CacheFull> {
        if let Some(entry) = self.entries.get_mut(&instrument) {
            if entry.feed != feed {
                *entry = Entry::empty(feed, entry.resync_from.max(at));
            }
            return Ok(());
        }
        if self.entries.len() >= self.capacity {
            return Err(CacheFull);
        }
        self.entries.insert(instrument, Entry::empty(feed, at));
        Ok(())
    }

    /// Removes `instrument` and everything held for it.
    pub fn unsubscribe(&mut self, instrument: &AssetId) {
        self.entries.remove(instrument);
    }

    /// Takes one received quote. Checked in this order: subscribed on its feed, else
    /// [`Refused`]; stamped too far in the future ([`Intake::FutureStamped`]); received at or
    /// before the subscription or the last connection loss ([`Intake::BeforeResync`]); the same
    /// message as the watermark ([`Intake::Duplicate`]); stamped at or before the watermark
    /// ([`Intake::OutOfOrder`]); then it becomes the watermark, and marks if sane.
    pub fn intake(&mut self, quote: Quote) -> Result<Intake, Refused> {
        let entry = self
            .entries
            .get_mut(&quote.instrument)
            .ok_or(Refused::NotSubscribed)?;
        if entry.feed != quote.feed {
            return Err(Refused::OtherFeed);
        }
        if self.freshness.future_stamped(&quote) {
            entry.mark = None;
            return Ok(Intake::FutureStamped);
        }
        if quote.received_at <= entry.resync_from {
            return Ok(Intake::BeforeResync);
        }
        if let Some(watermark) = &entry.watermark {
            if watermark.same_message(&quote) {
                return Ok(Intake::Duplicate);
            }
            if quote.vendor_time <= watermark.vendor_time {
                return Ok(Intake::OutOfOrder);
            }
        }
        let sane = self.freshness.sane(&quote);
        entry.mark = sane.then(|| quote.clone());
        if sane {
            entry.last_marked = Some(quote.clone());
        }
        entry.watermark = Some(quote);
        Ok(if sane {
            Intake::Marked
        } else {
            Intake::NotSane
        })
    }

    /// The connection carrying every subscribed instrument was lost at `at`, by the host clock:
    /// no instrument has a mark until a quote received after `at` marks it, however late a quote
    /// received before it is taken (DP-7, DEC-1008 item 5). Watermarks stay, so a replay of an
    /// earlier quote never marks either.
    pub fn connection_lost(&mut self, at: UtcNanos) {
        for entry in self.entries.values_mut() {
            entry.mark = None;
            entry.resync_from = entry.resync_from.max(at);
        }
    }

    /// Whether the host clock is in tolerance: false while the scheduler's
    /// `ClockToleranceExceeded` stands, true after a later in-tolerance `ClockOffsetRecorded`
    /// (journal spec §5.4, DEC-433 item 5).
    pub fn set_clock_in_tolerance(&mut self, in_tolerance: bool) {
        self.clock_in_tolerance = in_tolerance;
    }

    /// The quote an opening or increasing order in `instrument` may use at `now`: the mark, if
    /// the clock is in tolerance and it is fresh at `now`, with `at` its age basis. `None` denies
    /// the opening (`stale_mark`, mandate spec §5.2) and never touches an exit.
    pub fn opening_quote(&self, instrument: &AssetId, now: UtcNanos) -> Option<SaneQuote> {
        let mark = self.entries.get(instrument)?.mark.as_ref()?;
        (self.clock_in_tolerance && self.freshness.fresh_at(mark, now)).then(|| SaneQuote {
            bid: mark.bid,
            ask: mark.ask,
            at: mark.age_basis(),
        })
    }

    /// The last quote that marked `instrument`, however old: exits take any mark source (trading
    /// spec §8.2) and price from it as §5.6 says.
    pub fn last_marked(&self, instrument: &AssetId) -> Option<&Quote> {
        self.entries.get(instrument)?.last_marked.as_ref()
    }
}
