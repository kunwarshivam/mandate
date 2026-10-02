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
//! The backtest fill model ([trading-domain spec §6.4](../../../docs/specs/trading-domain.md#64-backtest-fill-model)):
//! bars and orders in, fills out.
//!
//! [`simulate`] is pure. It reads only its arguments, keeps no state between calls, touches no
//! clock, no filesystem, and no randomness, and returns the same [`SimOutcome`] for the same
//! inputs, so a backtest replays bit for bit (ADR-0001 ES-21). Fees are not computed here: a
//! [`SimFill`] carries the liquidity flag and price that `mandate-accounting` needs to charge it
//! from the pinned fee configuration (spec §6.2, §6.3), and E4-2 wires the two together.
//!
//! The model is deliberately pessimistic wherever spec §6.4 leaves a choice (DEC-106): slippage
//! and rounding move a fill price against the order, a touch is never a fill, and a bar whose
//! reference volume is unknown fills nothing. A backtest that flatters an agent misleads the owner
//! who reads it before going live (DEC-97), which is why this crate is safety-critical.
//!
//! Sessions, session starts, and auction bars arrive on the bar (spec §4.1 makes the session part
//! of a bar): the market-data layer labels them, so this crate converts no time zones and reads no
//! calendar. Everything else the model needs — latencies, slippage, the volume-cap fraction, and
//! the 20-session median for a session's first bar — arrives in [`SimConfig`] and
//! [`FirstBarVolumes`].

mod fill;

pub use fill::{check_sessions_of_asset_class, simulate};

use mandate_accounting::{AssetClass, Liquidity, Side};
use mandate_num::{Bps, Fraction, NumError, Price, Qty, ShareIncrement};
use mandate_time::{Date, TimeError, UtcNanos};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SimError {
    #[error("bar {0} starts at or before the bar before it")]
    BarsOutOfOrder(usize),
    #[error("bar {0} has a high below its low, or an open or close outside them")]
    InconsistentBar(usize),
    #[error("bar {0} starts before the session it is labelled with")]
    BarBeforeItsSession(usize),
    /// The first bar, by index, labelled with a session its instrument's asset class never trades
    /// (spec §4.3: crypto trades the continuous session alone, a US equity the four New York
    /// sessions alone), so a continuous bar for an equity and a regular, extended, or overnight bar
    /// for crypto are both refused (DEC-377).
    #[error("bar {0} is labelled with a session its instrument's asset class never trades")]
    SessionOffAssetClass(usize),
    #[error("an order's quantity must be positive")]
    ZeroQuantity,
    #[error("an order's quantity is not a multiple of the instrument's increment")]
    QuantityOffIncrement,
    #[error("only a limit order may trade the extended sessions")]
    ExtendedHoursNeedsALimit,
    #[error("a stop-limit's limit is on the far side of its stop")]
    StopLimitCrossed,
    #[error("an OCO's take-profit and stop are on the same side of the position")]
    OcoLegsCrossed,
    #[error("protection on a fractional instrument is a stop-limit, not an OCO")]
    OcoOnAFractionalInstrument,
    #[error("a continuous instrument has no trading day to cancel a day order at")]
    DayOrderOnAContinuousInstrument,
    #[error("an order rests from bar {0}, which the bars do not contain")]
    RestingBarOutOfRange(usize),
    /// The story named in the pending tests has not been implemented yet, so the call cannot be
    /// answered at all. A stub says so rather than returning a verdict another error could be
    /// mistaken for (DEC-137).
    #[error("this session check is not implemented yet")]
    Unimplemented,
    #[error(transparent)]
    Num(#[from] NumError),
    #[error(transparent)]
    Time(#[from] TimeError),
}

impl SimError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::BarsOutOfOrder(_) => "bars_out_of_order",
            Self::InconsistentBar(_) => "inconsistent_bar",
            Self::BarBeforeItsSession(_) => "bar_before_its_session",
            Self::SessionOffAssetClass(_) => "session_off_asset_class",
            Self::ZeroQuantity => "zero_quantity",
            Self::QuantityOffIncrement => "quantity_off_increment",
            Self::ExtendedHoursNeedsALimit => "extended_hours_needs_a_limit",
            Self::StopLimitCrossed => "stop_limit_crossed",
            Self::OcoLegsCrossed => "oco_legs_crossed",
            Self::OcoOnAFractionalInstrument => "oco_on_a_fractional_instrument",
            Self::DayOrderOnAContinuousInstrument => "day_order_on_a_continuous_instrument",
            Self::RestingBarOutOfRange(_) => "resting_bar_out_of_range",
            Self::Unimplemented => "unimplemented",
            Self::Num(e) => e.code(),
            Self::Time(e) => e.code(),
        }
    }
}

/// Nanoseconds in a millisecond, the unit spec §6.4 configures latencies in.
const NANOS_PER_MILLI: u64 = 1_000_000;
/// Nanoseconds in a second, the unit [`UtcNanos`] splits an instant into.
const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// A non-negative duration in nanoseconds. Spec §6.4 configures latencies in milliseconds; the
/// type keeps the unit explicit so a millisecond figure can never be added to an instant as if it
/// were nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Nanos(u64);

impl Nanos {
    pub const ZERO: Self = Self(0);

    /// `millis` milliseconds; `overflow` beyond what a duration in nanoseconds can hold.
    pub fn from_millis(millis: u64) -> Result<Self, SimError> {
        millis
            .checked_mul(NANOS_PER_MILLI)
            .map(Self)
            .ok_or(SimError::Num(NumError::Overflow))
    }

    pub fn nanos(self) -> u64 {
        self.0
    }

    /// `at` moved on by this duration: the instant an order becomes eligible (spec §6.4 rule 1).
    /// `out_of_range` beyond the instants [`UtcNanos`] holds.
    fn after(self, at: UtcNanos) -> Result<UtcNanos, SimError> {
        let total = u64::from(at.nanos())
            .checked_add(self.0)
            .ok_or(NumError::Overflow)?;
        let whole_seconds = total
            .checked_div(NANOS_PER_SECOND)
            .and_then(|seconds| i64::try_from(seconds).ok())
            .ok_or(NumError::Overflow)?;
        let rest = total
            .checked_rem(NANOS_PER_SECOND)
            .and_then(|nanos| u32::try_from(nanos).ok())
            .ok_or(NumError::Overflow)?;
        let secs = at
            .secs()
            .checked_add(whole_seconds)
            .ok_or(NumError::Overflow)?;
        Ok(UtcNanos::from_parts(secs, rest)?)
    }
}

/// The session a bar belongs to (spec §4.3). US equities trade four sessions in New York time;
/// crypto trades continuously (spec §4.3, last line), so a crypto bar is always
/// [`Session::Continuous`]. No order trades overnight (DEC-30).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    Overnight,
    PreMarket,
    Regular,
    AfterHours,
    Continuous,
}

/// One bar of the instrument being simulated (spec §4.1), with the session labels the fill model
/// reads instead of converting time zones itself.
///
/// `trade_date` and `session` name the session this bar belongs to: the volume-cap reference is the
/// most recent earlier bar of the same pair (spec §6.4 rule 3), and a day order's last eligible
/// session is the last one of its `trade_date` that it may trade (rule 2). `session_start` is that
/// session's first instant, which decides whether the bar can be its session's first *covered* bar
/// (DEC-106 item 4). `auction` marks a bar covering an auction: the first regular-session bar of a
/// covered trading day, or the first bar after a halt reopens (spec §6.4 rule 5, §4.4).
///
/// A missing minute is no bar at all, not a gap (spec §4.2), so the previous bar of a session may be
/// several minutes earlier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimBar {
    pub start: UtcNanos,
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
    pub volume: Qty,
    pub trade_date: Date,
    pub session: Session,
    pub session_start: UtcNanos,
    pub auction: bool,
}

/// What the fill model needs to know about the instrument: its asset class, which decides whether
/// stops may trigger only in the regular session (spec §6.4 rule 6), and the quantity increment
/// every fill and every volume cap is truncated to (spec §2.1, §6.4 rule 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instrument {
    pub asset_class: AssetClass,
    pub increment: ShareIncrement,
}

/// Slippage `s = half-spread + impact` (spec §6.4). `Fixed` states the impact in basis points;
/// `Sqrt` derives it from the fill's share of the bar's reference volume, `coefficient_bps ×
/// sqrt(fill qty ÷ reference volume)` (DEC-106 item 3). The fill quantity is decided by the cap
/// and the remaining quantity before the price is computed, so the `Sqrt` model is not circular.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slippage {
    Fixed {
        half_spread_bps: Bps,
        impact_bps: Bps,
    },
    Sqrt {
        half_spread_bps: Bps,
        coefficient_bps: Bps,
    },
}

/// The backtest configuration of spec §6.4, pinned for a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimConfig {
    pub decision_latency: Nanos,
    pub approval_latency: Nanos,
    pub slippage: Slippage,
    pub volume_cap_fraction: Fraction,
}

/// An order's place in submission order, which decides how a bar's volume cap is shared (spec §6.4
/// rule 3). [`simulate`] assigns it from the order's index in the submitted slice, so a fill points
/// back to its order without this crate carrying broker identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OrderRef(usize);

impl OrderRef {
    pub fn new(index: usize) -> Self {
        Self(index)
    }

    pub fn index(self) -> usize {
        self.0
    }
}

/// The order types v1 simulates (spec §5.1, §5.2). `Oco` is a protective pair: its `limit` leg is
/// the take-profit and its `stop` leg the protective stop, and the first fill of either cancels the
/// other (spec §6.4 rule 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderKind {
    Market,
    Limit { limit: Price },
    Stop { stop: Price },
    StopLimit { stop: Price, limit: Price },
    Oco { limit: Price, stop: Price },
}

/// Which leg of an [`OrderKind::Oco`] a fill or a cancellation belongs to. Both legs of a protective
/// pair are working orders away from the market, so neither is ever marketable on arrival: the
/// take-profit leg rests at its limit and its remainder keeps resting there, while a stop leg that
/// fills leaves a market-order remainder (DEC-106 items 8 and 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcoLeg {
    Limit,
    Stop,
}

/// Day orders lose their remainder at the end of their last eligible session (spec §6.4 rule 2);
/// a GTC order keeps working while bars remain. GTC's 90-day expiry is the runner's (E4-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeInForce {
    Day,
    Gtc,
}

/// When an order starts being simulated (spec §6.4 rule 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eligibility {
    /// Decided at `at`: eligible from the first bar starting at or after `at` plus the decision
    /// latency, plus the approval latency when `approval_required`. The bar that produced the
    /// decision closed at or before `at`, so it starts strictly earlier and never fills.
    DecidedAt {
        at: UtcNanos,
        approval_required: bool,
    },
    /// Already resting at the start of bar `from_bar`: it became eligible before that bar, so the
    /// marketable-on-arrival test of spec §6.4 rule 5 does not apply to it.
    Resting { from_bar: usize },
}

/// One order submitted to the model. `extended_hours` is the flag the gate sets for an exit that
/// may trade pre-market or after hours (spec §4.3, §5.2, DEC-37); without it the order trades only
/// the regular session, and only a [`OrderKind::Limit`] may carry it, because the extended sessions
/// take limit orders alone. Nothing ever trades overnight (DEC-30).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimOrder {
    pub side: Side,
    pub qty: Qty,
    pub kind: OrderKind,
    pub tif: TimeInForce,
    pub extended_hours: bool,
    pub eligible_from: Eligibility,
}

/// The reference volume for a bar that is the first of its session: the median volume of that
/// minute over the prior 20 sessions (spec §6.4 rule 3). `None` when it is unavailable — fewer than
/// 20 prior sessions, or no dataset — and the bar's cap is then 0.
///
/// The E4-2 runner computes it from the dataset, counting a prior session with no bar at that
/// minute as volume 0 (a missing bar is no trade, spec §4.2); the reference-case harness reads the
/// case's `first_bar_reference_volume`.
pub trait FirstBarVolumes {
    fn median_at(&self, bar_start: UtcNanos) -> Option<Qty>;
}

/// One simulated fill. `price` is not tick-rounded (spec §6.4 rule 9). `liquidity` is `None` for an
/// auction fill, which is neither maker nor taker (DEC-106 item 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimFill {
    pub order: OrderRef,
    pub leg: Option<OcoLeg>,
    pub bar: usize,
    pub qty: Qty,
    pub price: Price,
    pub liquidity: Option<Liquidity>,
}

/// The leg an OCO lost because its other leg filled first (spec §6.4 rule 8), at the bar of that
/// first fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanceledLeg {
    pub order: OrderRef,
    pub leg: OcoLeg,
    pub bar: usize,
}

/// How an order stood when the bars ran out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderEnd {
    /// Every submitted share filled.
    Filled,
    /// A day order's remainder was canceled after `at_bar`, the last bar of its last eligible
    /// session (spec §6.4 rule 2).
    Expired { at_bar: usize },
    /// Still working, in part or in whole.
    Open,
}

/// What [`simulate`] produces: the fills in bar order and, within a bar, submission order; the OCO
/// legs canceled along the way; and one end state per order, at the index of its [`OrderRef`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimOutcome {
    pub fills: Vec<SimFill>,
    pub canceled_legs: Vec<CanceledLeg>,
    pub ends: Vec<OrderEnd>,
}

impl SimOutcome {
    /// The fills of one order, in bar order.
    pub fn fills_of(&self, order: OrderRef) -> impl Iterator<Item = &SimFill> {
        self.fills.iter().filter(move |f| f.order == order)
    }

    /// How `order` ended, or `None` when it was never submitted.
    pub fn end_of(&self, order: OrderRef) -> Option<OrderEnd> {
        self.ends.get(order.index()).copied()
    }
}
