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
//! Money, price, and quantity types for the core (ADR-0001 ES-04).
//!
//! Each type wraps a private `rust_decimal::Decimal` that never leaves this crate. Values enter only
//! as canonical decimal text (the form `mandate_canon::DecStr::as_str` produces: no exponent, no
//! trailing fractional zeros, `0` for zero), and every operation is exact or returns an error. The
//! only roundings are the ones a spec formula names, each done once, on 256-bit intermediates,
//! with the scale and mode the caller passes ([trading-domain spec §2.1]).
//!
//! [trading-domain spec §2.1]: ../../../docs/specs/trading-domain.md#21-numbers

mod exact;

use core::fmt;

use exact::Exact;
use rust_decimal::Decimal;

/// Rounding modes named by the trading-domain spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rounding {
    /// To nearest; ties to the even neighbour.
    HalfEven,
    /// To nearest; ties away from zero.
    HalfUp,
    /// Towards positive infinity.
    Ceiling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NumError {
    #[error("not canonical decimal text")]
    NotCanonical,
    #[error("more fractional digits than the type or the stored decimal allows")]
    TooPrecise,
    #[error("the exact result does not fit")]
    Overflow,
    #[error("the value must not be negative")]
    Negative,
    #[error("the value must be positive")]
    NotPositive,
    #[error("division by zero")]
    DivisionByZero,
    #[error("the value must not exceed one")]
    AboveOne,
}

impl NumError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotCanonical => "not_canonical",
            Self::TooPrecise => "too_precise",
            Self::Overflow => "overflow",
            Self::Negative => "negative",
            Self::NotPositive => "not_positive",
            Self::DivisionByZero => "division_by_zero",
            Self::AboveOne => "above_one",
        }
    }
}

/// Prices and quantities have at most 9 fractional digits (spec §2.1).
const QTY_SCALE: u32 = 9;
/// Reporting marks have at most 12 fractional digits (spec §2.1).
const MARK_SCALE: u32 = 12;
/// A backtest report holds its ratios at up to 24 fractional digits, because an exact sum of squares
/// of 12-place returns needs 24; every figure the report rounds is rounded at 12
/// ([E4-2 task brief](../../../docs/project/tasks/E4-2-backtest-baseline.md), DEC-127).
const RATIO_SCALE: u32 = 24;
/// Money and rates keep full precision up to the stored maximum.
const FULL_SCALE: u32 = 28;
const BPS_PER_UNIT: u64 = 10_000;
/// The `sqrt` impact model takes its root at 18 fractional digits (spec §6.4, DEC-106 item 3).
const ROOT_SCALE: u32 = 18;

fn parse(text: &str, max_scale: u32) -> Result<Decimal, NumError> {
    let value = Exact::parse(text)?.to_decimal(max_scale)?;
    if value.normalize().to_string() == text {
        Ok(value)
    } else {
        Err(NumError::NotCanonical)
    }
}

fn positive(value: Decimal) -> Result<Decimal, NumError> {
    if value > Decimal::ZERO {
        Ok(value)
    } else {
        Err(NumError::NotPositive)
    }
}

fn non_negative(value: Decimal) -> Result<Decimal, NumError> {
    if value < Decimal::ZERO {
        Err(NumError::Negative)
    } else {
        Ok(value)
    }
}

macro_rules! decimal_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(Decimal);

        impl $name {
            fn exact(self) -> Exact {
                Exact::of(self.0)
            }

            /// Whether the value is zero.
            pub fn is_zero(self) -> bool {
                self.0.is_zero()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0.normalize(), f)
            }
        }
    };
}

decimal_type!(
    /// A non-negative quantity with at most 9 fractional digits.
    Qty
);
decimal_type!(
    /// A signed quantity (long positive, short negative) with at most 9 fractional digits.
    SignedQty
);
decimal_type!(
    /// A positive price with at most 9 fractional digits.
    Price
);
decimal_type!(
    /// A positive reporting mark with at most 12 fractional digits. A mark arrives as a [`Price`]
    /// (9 places); a split replaces it with `round(mark × old ÷ new, 12, half_even)` (spec §2.1,
    /// §8.5), which a `Price` cannot hold.
    MarkPrice
);
decimal_type!(
    /// A signed USD amount at full precision.
    Usd
);
decimal_type!(
    /// A signed cost basis (long: paid; short: received, negative), spec §8.1.
    CostBasis
);
decimal_type!(
    /// A non-negative fee rate applied to an amount of money (SEC Section 31).
    FeeRate
);
decimal_type!(
    /// A non-negative USD fee per share (FINRA TAF, CAT).
    FeePerShare
);
decimal_type!(
    /// A non-negative number of basis points.
    Bps
);
decimal_type!(
    /// A signed ratio with at most 24 fractional digits: a backtest report's returns, variance,
    /// squared Sharpe, drawdown, and turnover (DEC-127). Every figure the report rounds is rounded
    /// at 12 places; the extra digits exist because an exact sum of squares of 12-place returns
    /// needs 24, and because a squared Sharpe is unbounded above
    /// ([E4-2 task brief](../../../docs/project/tasks/E4-2-backtest-baseline.md)).
    Ratio
);
/// A non-negative fraction of one, at most one, with at most 9 fractional digits: the backtest
/// volume-cap fraction ([trading-domain spec §6.4] rule 3). The upper bound is part of the type
/// because a fraction above one would let a bar fill more than it traded.
///
/// [trading-domain spec §6.4]: ../../../docs/specs/trading-domain.md#64-backtest-fill-model
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fraction(Decimal);

impl fmt::Display for Fraction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.normalize(), f)
    }
}

impl Qty {
    pub const ZERO: Self = Self(Decimal::ZERO);

    pub fn parse(text: &str) -> Result<Self, NumError> {
        parse(text, QTY_SCALE).and_then(non_negative).map(Self)
    }

    /// `self − other`; an error if the result would be negative.
    pub fn checked_sub(self, other: Self) -> Result<Self, NumError> {
        let value = self.exact().sub(other.exact())?.to_decimal(QTY_SCALE)?;
        non_negative(value).map(Self)
    }

    /// `self × price`, exact.
    pub fn notional(self, price: Price) -> Result<Usd, NumError> {
        self.exact()
            .mul(price.exact())?
            .to_decimal(FULL_SCALE)
            .map(Usd)
    }

    /// `self × per_share`, exact.
    pub fn times_per_share(self, per_share: FeePerShare) -> Result<Usd, NumError> {
        self.exact()
            .mul(per_share.exact())?
            .to_decimal(FULL_SCALE)
            .map(Usd)
    }

    /// `round(self × bps ÷ 10000, 9, mode)`: one rounding.
    pub fn times_bps(self, bps: Bps, mode: Rounding) -> Result<Self, NumError> {
        self.exact()
            .mul(bps.exact())?
            .div(Exact::integer(BPS_PER_UNIT), QTY_SCALE, mode)?
            .to_decimal(QTY_SCALE)
            .map(Self)
    }

    /// `self + other`, exact.
    pub fn checked_add(self, other: Self) -> Result<Self, NumError> {
        self.exact()
            .add(other.exact())?
            .to_decimal(QTY_SCALE)
            .map(Self)
    }

    /// `truncate(fraction × self, increment)`: a bar's volume cap (spec §6.4 rule 3), truncated
    /// toward zero to the instrument's quantity increment, so a cap never reaches past the volume
    /// its reference bar traded. Truncation, not rounding: the cap is an upper bound.
    pub fn portion(self, fraction: Fraction, increment: ShareIncrement) -> Result<Self, NumError> {
        self.exact()
            .mul(fraction.exact())?
            .div_toward_zero(Exact::integer(1), increment.places())?
            .to_decimal(QTY_SCALE)
            .map(Self)
    }
}

impl SignedQty {
    pub const ZERO: Self = Self(Decimal::ZERO);

    pub fn parse(text: &str) -> Result<Self, NumError> {
        parse(text, QTY_SCALE).map(Self)
    }

    pub fn checked_add(self, other: Self) -> Result<Self, NumError> {
        self.exact()
            .add(other.exact())?
            .to_decimal(QTY_SCALE)
            .map(Self)
    }

    pub fn negated(self) -> Self {
        Self(self.0.neg_exact())
    }

    pub fn abs(self) -> Qty {
        Qty(self.0.abs())
    }

    pub fn is_negative(self) -> bool {
        self.0 < Decimal::ZERO
    }

    /// `self × price`, exact and signed.
    pub fn value_at(self, price: Price) -> Result<Usd, NumError> {
        self.exact()
            .mul(price.exact())?
            .to_decimal(FULL_SCALE)
            .map(Usd)
    }

    /// `self × mark`, exact and signed.
    pub fn value_at_mark(self, mark: MarkPrice) -> Result<Usd, NumError> {
        self.exact()
            .mul(mark.exact())?
            .to_decimal(FULL_SCALE)
            .map(Usd)
    }
}

impl From<Qty> for SignedQty {
    fn from(qty: Qty) -> Self {
        Self(qty.0)
    }
}

impl Price {
    pub fn parse(text: &str) -> Result<Self, NumError> {
        parse(text, QTY_SCALE).and_then(positive).map(Self)
    }

    /// A backtest fill price after slippage (spec §6.4 rules 4 to 7): `self + ceil(self × bps ÷
    /// 10000, 9)` for [`Adverse::Up`], a buy, and `self − ceil(self × bps ÷ 10000, 9)` for
    /// [`Adverse::Down`], a sell. Rounding the slippage amount up, once, at the 9 places a price
    /// holds (spec §2.1) moves the price against the order whichever way it goes, so a fill price
    /// never flatters a backtest (DEC-106 item 2). The result is not tick-rounded (spec §6.4 rule
    /// 9). `not_positive` when the slippage takes a sell price to zero or below.
    pub fn slipped(self, slippage: Bps, adverse: Adverse) -> Result<Self, NumError> {
        let amount = self.exact().mul(slippage.exact())?.div(
            Exact::integer(BPS_PER_UNIT),
            QTY_SCALE,
            Rounding::Ceiling,
        )?;
        let moved = match adverse {
            Adverse::Up => self.exact().add(amount)?,
            Adverse::Down => self.exact().sub(amount)?,
        };
        moved.to_decimal(QTY_SCALE).and_then(positive).map(Self)
    }

    /// The nearest price on `tick`'s grid, moved against the order: down for a buy limit
    /// ([`Adverse::Up`] is the direction slippage moves a buy, so a buy's limit rounds the other
    /// way) and up for a sell limit, which is spec §2.1's rule for limit and stop prices. A price
    /// already on the grid is returned unchanged. `not_positive` when rounding a buy limit down
    /// reaches zero, and `division_by_zero` for an increment of zero.
    pub fn on_tick(self, tick: TickRule, adverse: Adverse) -> Result<Self, NumError> {
        let _ = (tick, adverse);
        Err(NumError::Overflow)
    }
}

/// The price grid an order's limit must sit on (spec §2.1): Reg NMS Rule 612 for US equities, 0.01
/// at or above 1.00 USD and 0.0001 below it, or the venue's own increment for crypto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickRule {
    RegNmsEquity,
    Increment(Price),
}

/// Which way slippage moves a price: against the order, so a buy pays more and a sell receives
/// less (spec §6.4 rules 4 to 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adverse {
    Up,
    Down,
}

impl MarkPrice {
    /// Canonical text of a positive value with at most 12 fractional digits.
    pub fn parse(text: &str) -> Result<Self, NumError> {
        parse(text, MARK_SCALE).and_then(positive).map(Self)
    }
}

impl From<Price> for MarkPrice {
    fn from(price: Price) -> Self {
        Self(price.0)
    }
}

/// The quantity grid a split truncates to (spec §8.5): 10⁻⁹ shares for a fractionable
/// instrument, whole shares otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareIncrement {
    Fractional,
    Whole,
}

impl ShareIncrement {
    /// How many fractional digits a quantity on this grid may have: a quantity is truncated to it
    /// by a split (spec §8.5) and a backtest volume cap (spec §6.4 rule 3).
    fn places(self) -> u32 {
        match self {
            Self::Fractional => QTY_SCALE,
            Self::Whole => 0,
        }
    }
}

/// A split ratio `new:old` of positive integers: `new` shares for every `old` (spec §8.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitRatio {
    new: u64,
    old: u64,
}

impl SplitRatio {
    /// Both terms must be positive (`not_positive`).
    pub fn new(new: u64, old: u64) -> Result<Self, NumError> {
        if new == 0 || old == 0 {
            Err(NumError::NotPositive)
        } else {
            Ok(Self { new, old })
        }
    }

    pub fn new_shares(self) -> u64 {
        self.new
    }

    pub fn old_shares(self) -> u64 {
        self.old
    }

    /// Q' = Q × new ÷ old truncated toward zero to `increment`, with Q kept for the residual
    /// formulas.
    pub fn split(self, qty: SignedQty, increment: ShareIncrement) -> Result<SplitQty, NumError> {
        let after = qty
            .exact()
            .mul(Exact::integer(self.new))?
            .div_toward_zero(Exact::integer(self.old), increment.places())?
            .to_decimal(QTY_SCALE)?;
        Ok(SplitQty {
            ratio: self,
            before: qty,
            after: SignedQty(after),
        })
    }

    /// `round(mark × old ÷ new, scale, mode)`: one rounding; `not_positive` if it rounds to zero,
    /// `too_precise` if `scale` exceeds a mark's 12 places.
    pub fn mark(self, mark: MarkPrice, scale: u32, mode: Rounding) -> Result<MarkPrice, NumError> {
        if scale > MARK_SCALE {
            return Err(NumError::TooPrecise);
        }
        mark.exact()
            .mul(Exact::integer(self.old))?
            .div(Exact::integer(self.new), scale, mode)?
            .to_decimal(MARK_SCALE)
            .and_then(positive)
            .map(MarkPrice)
    }
}

/// A position's quantity before (Q) and after (Q') a split. Only [`SplitRatio::split`] builds one,
/// so Q' is always the truncation of Q_raw = Q × new ÷ old, and the residual f = Q_raw − Q' is
/// always (Q·new − Q'·old) ÷ old.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitQty {
    ratio: SplitRatio,
    before: SignedQty,
    after: SignedQty,
}

impl SplitQty {
    pub fn before(self) -> SignedQty {
        self.before
    }

    pub fn after(self) -> SignedQty {
        self.after
    }

    /// `round(B × (Q·new − Q'·old) ÷ (Q·new), scale, mode)`: the basis of the residual, a single
    /// division of terminating inputs (spec §8.5).
    pub fn residual_basis(
        self,
        basis: CostBasis,
        scale: u32,
        mode: Rounding,
    ) -> Result<CostBasis, NumError> {
        basis
            .exact()
            .mul(self.residual_times_old()?)?
            .div(self.scaled_before()?, scale, mode)?
            .to_decimal(FULL_SCALE)
            .map(CostBasis)
    }

    /// `round(f × price, scale, mode)` with f = (Q·new − Q'·old) ÷ old: cash in lieu of the
    /// residual, signed like Q.
    pub fn cash_in_lieu(self, price: Price, scale: u32, mode: Rounding) -> Result<Usd, NumError> {
        self.residual_times_old()?
            .mul(price.exact())?
            .div(Exact::integer(self.ratio.old), scale, mode)?
            .to_decimal(FULL_SCALE)
            .map(Usd)
    }

    /// Q·new.
    fn scaled_before(self) -> Result<Exact, NumError> {
        self.before.exact().mul(Exact::integer(self.ratio.new))
    }

    /// Q·new − Q'·old = f × old.
    fn residual_times_old(self) -> Result<Exact, NumError> {
        self.scaled_before()?
            .sub(self.after.exact().mul(Exact::integer(self.ratio.old))?)
    }
}

impl Usd {
    pub const ZERO: Self = Self(Decimal::ZERO);

    pub fn parse(text: &str) -> Result<Self, NumError> {
        parse(text, FULL_SCALE).map(Self)
    }

    pub fn checked_add(self, other: Self) -> Result<Self, NumError> {
        self.exact()
            .add(other.exact())?
            .to_decimal(FULL_SCALE)
            .map(Self)
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, NumError> {
        self.exact()
            .sub(other.exact())?
            .to_decimal(FULL_SCALE)
            .map(Self)
    }

    pub fn negated(self) -> Self {
        Self(self.0.neg_exact())
    }

    pub fn is_negative(self) -> bool {
        self.0 < Decimal::ZERO
    }

    /// `self × rate`, exact.
    pub fn times_rate(self, rate: FeeRate) -> Result<Self, NumError> {
        self.exact()
            .mul(rate.exact())?
            .to_decimal(FULL_SCALE)
            .map(Self)
    }

    /// `round(self × bps ÷ 10000, scale, mode)`: one rounding.
    pub fn times_bps(self, bps: Bps, scale: u32, mode: Rounding) -> Result<Self, NumError> {
        self.exact()
            .mul(bps.exact())?
            .div(Exact::integer(BPS_PER_UNIT), scale, mode)?
            .to_decimal(FULL_SCALE)
            .map(Self)
    }

    /// `round(self, scale, mode)`.
    pub fn round(self, scale: u32, mode: Rounding) -> Result<Self, NumError> {
        self.exact()
            .div(Exact::integer(1), scale, mode)?
            .to_decimal(FULL_SCALE)
            .map(Self)
    }

    /// `round(self ÷ denominator, scale, mode)` as a [`Ratio`]: one rounding of one quotient, which
    /// is how a backtest report takes a return, a drawdown rung, and its turnover (DEC-127 items 4,
    /// 9, and 10). `division_by_zero` when `denominator` is zero, which the caller avoids by
    /// checking that the equity it divides by is positive.
    pub fn ratio_to(self, denominator: Usd, scale: u32, mode: Rounding) -> Result<Ratio, NumError> {
        let _ = (denominator, scale, mode);
        Err(NumError::Overflow)
    }

    /// `truncate(self ÷ price, increment)`: the shares this amount of money buys at `price`, never
    /// more (spec §2.1 truncates an order quantity to the increment). `division_by_zero` cannot
    /// happen, because a [`Price`] is positive.
    pub fn shares_at(self, price: Price, increment: ShareIncrement) -> Result<Qty, NumError> {
        let _ = (price, increment);
        Err(NumError::Overflow)
    }
}

impl CostBasis {
    pub const ZERO: Self = Self(Decimal::ZERO);

    pub fn parse(text: &str) -> Result<Self, NumError> {
        parse(text, FULL_SCALE).map(Self)
    }

    /// `self + amount`, exact.
    pub fn checked_add(self, amount: Usd) -> Result<Self, NumError> {
        self.exact()
            .add(amount.exact())?
            .to_decimal(FULL_SCALE)
            .map(Self)
    }

    /// `self − removed`, exact.
    pub fn checked_sub(self, removed: Self) -> Result<Self, NumError> {
        self.exact()
            .sub(removed.exact())?
            .to_decimal(FULL_SCALE)
            .map(Self)
    }

    /// `round(self × part ÷ whole, scale, mode)`: the basis removed by a reduction (spec §8.1),
    /// with a single rounding.
    pub fn portion(
        self,
        part: Qty,
        whole: Qty,
        scale: u32,
        mode: Rounding,
    ) -> Result<Self, NumError> {
        self.exact()
            .mul(part.exact())?
            .div(whole.exact(), scale, mode)?
            .to_decimal(FULL_SCALE)
            .map(Self)
    }

    pub fn to_usd(self) -> Usd {
        Usd(self.0)
    }
}

impl FeeRate {
    pub fn parse(text: &str) -> Result<Self, NumError> {
        parse(text, FULL_SCALE).and_then(non_negative).map(Self)
    }
}

impl FeePerShare {
    pub fn parse(text: &str) -> Result<Self, NumError> {
        parse(text, FULL_SCALE).and_then(non_negative).map(Self)
    }
}

impl Bps {
    pub const ZERO: Self = Self(Decimal::ZERO);

    pub fn parse(text: &str) -> Result<Self, NumError> {
        parse(text, FULL_SCALE).and_then(non_negative).map(Self)
    }

    /// `self + other`, exact: the half-spread plus the impact of spec §6.4's slippage.
    pub fn checked_add(self, other: Self) -> Result<Self, NumError> {
        self.exact()
            .add(other.exact())?
            .to_decimal(FULL_SCALE)
            .map(Self)
    }

    /// `coefficient × sqrt(fill ÷ reference)` in basis points: the `sqrt` impact model of spec
    /// §6.4. The root is `ceil(sqrt(fill ÷ reference), 18)`, the smallest value with 18 fractional
    /// digits whose square is at or above the ratio, so the impact is never understated
    /// (DEC-106 item 3); the product is then exact. `division_by_zero` when `reference` is zero,
    /// which the caller avoids by capping the bar at zero instead (spec §6.4 rule 3).
    pub fn sqrt_impact(coefficient: Bps, fill: Qty, reference: Qty) -> Result<Self, NumError> {
        let root = fill
            .exact()
            .ceiling_root_of_ratio(reference.exact(), ROOT_SCALE)?;
        coefficient
            .exact()
            .mul(root)?
            .to_decimal(FULL_SCALE)
            .map(Self)
    }
}

impl Ratio {
    pub const ZERO: Self = Self(Decimal::ZERO);

    /// Canonical text of a signed value with at most 24 fractional digits.
    pub fn parse(text: &str) -> Result<Self, NumError> {
        parse(text, RATIO_SCALE).map(Self)
    }

    /// Whether the value is below zero.
    pub fn is_negative(self) -> bool {
        self.0 < Decimal::ZERO
    }

    /// The same magnitude with the opposite sign.
    pub fn negated(self) -> Self {
        Self(self.0.neg_exact())
    }

    /// `self + other`, exact.
    pub fn checked_add(self, other: Self) -> Result<Self, NumError> {
        let _ = (self.exact(), other.exact());
        Err(NumError::Overflow)
    }

    /// `self − other`, exact: a backtest report's excess return over its benchmark (DEC-127).
    pub fn checked_sub(self, other: Self) -> Result<Self, NumError> {
        let _ = other;
        Err(NumError::Overflow)
    }

    /// `self × factor`, exact: annualizing a variance or a squared Sharpe by the period count
    /// (DEC-127 items 6 and 7), which is exact because the factor is an integer.
    pub fn times_int(self, factor: u32) -> Result<Self, NumError> {
        let _ = factor;
        Err(NumError::Overflow)
    }

    /// `Σ values`, exact: a backtest report's `return_sum`.
    pub fn sum(values: &[Self]) -> Result<Self, NumError> {
        let _ = values;
        Err(NumError::Overflow)
    }

    /// `Σ values²`, exact: a backtest report's `return_sum_of_squares`, which needs 24 places when
    /// the values hold 12.
    pub fn sum_of_squares(values: &[Self]) -> Result<Self, NumError> {
        let _ = values;
        Err(NumError::Overflow)
    }

    /// `round(Σ values ÷ n, 12, half_even)`: the mean of a period-return series (DEC-127 item 4).
    /// `division_by_zero` for an empty series.
    pub fn mean(values: &[Self]) -> Result<Self, NumError> {
        let _ = values;
        Err(NumError::Overflow)
    }

    /// `round((n × Σr² − (Σr)²) ÷ (n × (n − 1)), 12, half_even)`, the sample variance of a period
    /// return series as one rounding of one formula (DEC-127 item 5). The caller passes the sums the
    /// report shows, so the figure recomputes from the report. `division_by_zero` below two periods,
    /// which the caller reports as absent instead.
    pub fn sample_variance(sum: Self, sum_of_squares: Self, count: u32) -> Result<Self, NumError> {
        let _ = (sum, sum_of_squares, count);
        Err(NumError::Overflow)
    }

    /// `round(numerator² ÷ denominator, 12, half_even)`: a squared Sharpe from an excess mean and a
    /// variance (DEC-127 item 7). `division_by_zero` for a zero denominator, which the caller
    /// reports as absent instead.
    pub fn squared_quotient(numerator: Self, denominator: Self) -> Result<Self, NumError> {
        let _ = (numerator, denominator);
        Err(NumError::Overflow)
    }

    /// The greatest value with 12 fractional digits whose square is at or below `self`, so a root
    /// taken this way never overstates the figure it stands for: a report's Sharpe with a
    /// non-negative sign (DEC-127 item 7). `negative` below zero.
    pub fn root_floor(self) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }

    /// The least value with 12 fractional digits whose square is at or above `self`, so a root taken
    /// this way never understates the figure it stands for: a report's volatility, and the magnitude
    /// of a negative Sharpe (DEC-127 items 6 and 7). `negative` below zero.
    pub fn root_ceiling(self) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }
}

impl Fraction {
    pub const ZERO: Self = Self(Decimal::ZERO);
    /// The whole of a quantity: `Qty::portion(Fraction::ONE, increment)` truncates to the increment.
    pub const ONE: Self = Self(Decimal::ONE);

    /// Canonical text of a value from zero to one inclusive: `negative` below zero, `above_one`
    /// above one, `too_precise` beyond 9 places.
    pub fn parse(text: &str) -> Result<Self, NumError> {
        let value = parse(text, QTY_SCALE).and_then(non_negative)?;
        if value > Decimal::ONE {
            Err(NumError::AboveOne)
        } else {
            Ok(Self(value))
        }
    }

    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    fn exact(self) -> Exact {
        Exact::of(self.0)
    }
}

/// A non-negative USD cap on a fee (FINRA TAF), at full precision. A negative cap cannot be
/// represented, so a capped fee can never turn into a credit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FeeCap(Decimal);

impl FeeCap {
    pub fn parse(text: &str) -> Result<Self, NumError> {
        parse(text, FULL_SCALE).and_then(non_negative).map(Self)
    }

    pub fn to_usd(self) -> Usd {
        Usd(self.0)
    }
}

trait NegExact {
    fn neg_exact(self) -> Self;
}

impl NegExact for Decimal {
    fn neg_exact(self) -> Self {
        let mut negated = self;
        if !self.is_zero() {
            negated.set_sign_negative(!self.is_sign_negative());
        }
        negated
    }
}
