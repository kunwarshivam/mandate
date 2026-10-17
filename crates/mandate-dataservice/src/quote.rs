//! Spec §3.1's normalized quote, and the profile's freshness and sanity bounds (§3.4, trading
//! spec §8.2).

use std::time::Duration;

use mandate_domain::AssetId;
use mandate_num::{Adverse, Fraction, Price, Qty};
use mandate_time::UtcNanos;

/// The data profile a quote came from (trading-domain spec §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feed {
    /// Consolidated quotes, the live equity profile.
    Sip,
    /// IEX quotes, the paper equity profile.
    Iex,
    /// The broker's crypto feed.
    Crypto,
}

/// One top-of-book quote as spec §3.1 normalizes it. Both sides are priced: a side with no price
/// is refused at normalization and never reaches this type, since [`Price`] holds no zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
    pub instrument: AssetId,
    pub feed: Feed,
    pub bid: Price,
    pub bid_size: Qty,
    pub ask: Price,
    pub ask_size: Qty,
    /// The time the vendor stamped on the message.
    pub vendor_time: UtcNanos,
    /// When the data service first received it, from the host clock (journal spec §5.4).
    pub received_at: UtcNanos,
}

impl Quote {
    /// The instant the quote's age counts from: the earlier of [`Self::vendor_time`] and
    /// [`Self::received_at`] (DEC-433 item 5), so neither a vendor clock ahead of ours nor a slow
    /// delivery makes a quote look younger than it is.
    pub fn age_basis(&self) -> UtcNanos {
        self.vendor_time.min(self.received_at)
    }

    /// Whether `other` is the same vendor message: the same feed, instrument, vendor time and
    /// content (spec §3.3), whenever it was received.
    pub fn same_message(&self, other: &Self) -> bool {
        Self {
            received_at: other.received_at,
            ..self.clone()
        } == *other
    }
}

/// The data profile's bounds on a quote that may mark (trading spec §4.2 and §8.2, spec §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Freshness {
    /// The profile's staleness threshold: a quote older than this is not fresh. An age of exactly
    /// this is fresh.
    pub max_age: Duration,
    /// `future_skew_ms`: a quote whose vendor time is later than its receive time by more than
    /// this is not sane (DEC-433 item 5).
    pub future_skew: Duration,
    /// The profile's spread limit, as a fraction of the bid: a quote whose ask is above
    /// `bid × (1 + max_spread)` is not sane (DEC-1008 item 6).
    pub max_spread: Fraction,
}

impl Freshness {
    /// Spec §3.4's default `future_skew_ms`.
    pub const DEFAULT_FUTURE_SKEW: Duration = Duration::from_millis(1_000);

    /// Whether `quote`'s vendor time is later than its receive time by more than
    /// [`Self::future_skew`].
    pub fn future_stamped(&self, quote: &Quote) -> bool {
        !span(quote.vendor_time, quote.received_at)
            .is_some_and(|ahead| within(ahead, self.future_skew))
    }

    /// Trading spec §8.2's sane quote: `0 < bid ≤ ask` (a locked quote is sane), and the ask no
    /// higher than `bid × (1 + max_spread)` rounded down, so rounding never admits a wider quote.
    pub fn sane(&self, quote: &Quote) -> bool {
        quote.bid <= quote.ask
            && quote
                .bid
                .collar_bound(self.max_spread, Adverse::Up)
                .is_ok_and(|widest| quote.ask <= widest)
    }

    /// Whether `quote` is fresh at `now`: its [`Quote::age_basis`] is at or before `now` and at
    /// most [`Self::max_age`] earlier, counted in UTC nanoseconds (DEC-1008 item 4).
    /// The receive time may be after `now` when the host clock stepped back; the clock-tolerance
    /// check, not this one, keeps such a quote from an opening (DEC-433 item 5).
    pub fn fresh_at(&self, quote: &Quote, now: UtcNanos) -> bool {
        let basis = quote.age_basis();
        basis <= now && span(now, basis).is_some_and(|age| within(age, self.max_age))
    }
}

/// `later − earlier` in whole nanoseconds, negative when `later` is the earlier instant. `None`
/// only if the arithmetic overflowed, which no two [`UtcNanos`] can make it do.
fn span(later: UtcNanos, earlier: UtcNanos) -> Option<i128> {
    nanos(later)?.checked_sub(nanos(earlier)?)
}

fn nanos(at: UtcNanos) -> Option<i128> {
    i128::from(at.secs())
        .checked_mul(1_000_000_000)?
        .checked_add(i128::from(at.nanos()))
}

/// Whether a span of `nanos` is at most `bound`. A bound too large for `i128` holds every span.
fn within(nanos: i128, bound: Duration) -> bool {
    i128::try_from(bound.as_nanos()).map_or(true, |bound| nanos <= bound)
}
