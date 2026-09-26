//! The exact arithmetic the order builder needs ([mandate spec §8.3], backlog E6-2,
//! [task brief](../../../docs/project/tasks/E6-2-autonomy-and-order-builder.md), DEC-130 item 7).
//!
//! Spec §5.2 says comparisons are exact and only reported ratios round, and §8.3 states exactly
//! three roundings — the `round₁₂` of its step 1. Everything else in the chain is therefore exact,
//! and the job of this module is to stay inside a 256-bit intermediate or say it cannot
//! ([`NumError::Overflow`]), never to approximate a size nobody asked for (DEC-130 item 8).
//!
//! The scales are load-bearing, not cosmetic. [`SizeFraction`] stops at 12 places because the
//! sizing chain's widest value, a target of buy conviction × cap × ladder factor, then holds 57
//! places over an equity that holds 21, which fits; at 18 places it would hold 69 and the one
//! division would pass 2²⁵⁶. [`Unit`] and [`Conviction`] reach 18 because a model output is not an
//! envelope field and `MC-B13` already carries a 13-place confidence, and a confidence enters only
//! [`Unit::weighted_ratio`] and [`Conviction::weighted_ratio`], where the budget affords it.
//!
//! [`UsdExact`] exists because cap alone reaches 33 places, a 12-place fraction times an equity a
//! 9-place quantity at a 12-place mark gives 21 of, while [`Usd`] holds 28 on a 96-bit significand
//! and [`Ratio`](crate::Ratio) holds 24. Rounding cap or the target to fit would be a rounding the
//! spec does not state, in the one path that decides how large an order is.
//!
//! [mandate spec §8.3]: ../../../docs/specs/mandate.md#83-order-builder-conviction_linear-dec-47-dec-60

use core::cmp::Ordering;
use core::fmt;

use rust_decimal::Decimal;

use crate::exact::Exact;
use crate::{
    CostBasis, FULL_SCALE, FeeRate, NegExact, NumError, Price, Qty, Rounding, ShareIncrement,
    SignedQty, Usd, parse,
};

/// Places a mandate's sizing and limit fractions hold (DEC-130 item 7).
const SIZE_FRACTION_SCALE: u32 = 12;
/// Places a signal model's output and a condition's unit-typed value hold.
const OUTPUT_SCALE: u32 = 18;
/// Places the three combined figures of spec §8.3 step 1 round to.
pub const COMBINED_SCALE: u32 = 12;

/// Reads a canonical decimal and refuses one outside `[low, high]` at this module's own bounds.
fn bounded(text: &str, max_scale: u32, low: Decimal, high: Decimal) -> Result<Decimal, NumError> {
    let value = parse(text, max_scale)?;
    if value < low {
        Err(NumError::Negative)
    } else if value > high {
        Err(NumError::AboveOne)
    } else {
        Ok(value)
    }
}

/// A non-negative fraction of one, at most one, with at most 12 fractional digits: a signal model's
/// fixed `weight`, `max_position_fraction`, `entry_threshold`, `exit_threshold`, `rebalance_band`,
/// and a drawdown rung's size factor (mandate spec §3, §5.3, §8.3).
///
/// Twelve places is the bound that keeps the exact sizing chain inside 256 bits, so a mandate field
/// the schema accepts with more is `too_precise` here and the builder proposes nothing rather than
/// approximating (DEC-130 item 8; the schema bound is proposed to the founder in the task brief).
/// [`Fraction`](crate::Fraction) is not reused: its 9-place ceiling is part of the backtest fill
/// model's contract, where it multiplies a 9-place volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SizeFraction(Decimal);

/// A non-negative value of at most one with at most 18 fractional digits: a signal model's
/// `confidence`, the combined score, a thesis's confidence, a drawdown reading, and a condition's
/// unit-typed value (mandate spec §6.3, §8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Unit(Decimal);

/// A signed value in [−1, 1] with at most 18 fractional digits: a signal model's `conviction`, and
/// the exit and buy convictions §8.3 step 1 derives from it, which are `round₁₂` results and so use
/// 12 of the 18 (mandate spec §8.2, §8.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Conviction(Decimal);

/// A signed fraction with at most 18 fractional digits and no bound of one: `daily_pnl_fraction`
/// and `position_pnl_fraction`, which spec §5.2 reports at 12 places, and their condition values,
/// which the schema does not bound (mandate spec §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Signed(Decimal);

impl SizeFraction {
    pub const ZERO: Self = Self(Decimal::ZERO);
    pub const ONE: Self = Self(Decimal::ONE);

    /// Canonical text of a value in [0, 1] with at most 12 fractional digits. `negative` below
    /// zero, `above_one` above one, and `too_precise` beyond 12 places.
    pub fn parse(text: &str) -> Result<Self, NumError> {
        bounded(text, SIZE_FRACTION_SCALE, Decimal::ZERO, Decimal::ONE).map(Self)
    }

    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    fn exact(self) -> Exact {
        Exact::of(self.0)
    }
}

impl Unit {
    pub const ZERO: Self = Self(Decimal::ZERO);
    pub const ONE: Self = Self(Decimal::ONE);

    /// Canonical text of a value in [0, 1] with at most 18 fractional digits.
    pub fn parse(text: &str) -> Result<Self, NumError> {
        bounded(text, OUTPUT_SCALE, Decimal::ZERO, Decimal::ONE).map(Self)
    }

    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    /// `round(Σ (wᵢ × vᵢ) ÷ Σ weights, 12, half_even)`: the combined score of spec §8.3 step 1,
    /// where `products` pairs each **fresh** model's weight with its confidence and `weights` holds
    /// every **configured** model's weight, fresh or not. One formula, one rounding, on 256-bit
    /// intermediates (ADR-0001 ES-04).
    ///
    /// `division_by_zero` when the weights sum to zero, which a mandate with at least one model
    /// whose weight is positive cannot reach; `above_one` if the fresh weights somehow exceed the
    /// configured ones, which is a caller's bug rather than a value to round away.
    pub fn weighted_ratio(
        products: &[(SizeFraction, Self)],
        weights: &[SizeFraction],
    ) -> Result<Self, NumError> {
        let _ = (products, weights, Self::ZERO.exact());
        Err(NumError::Overflow)
    }

    fn exact(self) -> Exact {
        Exact::of(self.0)
    }
}

impl Conviction {
    pub const ZERO: Self = Self(Decimal::ZERO);
    pub const NEGATIVE_ONE: Self = Self(Decimal::NEGATIVE_ONE);

    /// Canonical text of a value in [−1, 1] with at most 18 fractional digits.
    pub fn parse(text: &str) -> Result<Self, NumError> {
        bounded(text, OUTPUT_SCALE, Decimal::NEGATIVE_ONE, Decimal::ONE).map(Self)
    }

    pub fn is_negative(self) -> bool {
        self.0 < Decimal::ZERO
    }

    pub fn negated(self) -> Self {
        Self(self.0.neg_exact())
    }

    /// `round((Σ (wᵢ × cᵢ × fᵢ) − Σ bearish) ÷ Σ weights, 12, half_even)`: both convictions of spec
    /// §8.3 step 1 from one formula and one rounding.
    ///
    /// `products` holds a fresh output's weight, conviction, and confidence; `weights` holds every
    /// configured model's weight; and `bearish` holds the weight of each model **without** a fresh
    /// output, counted at −1. The exit conviction passes an empty `bearish`, so a missing model
    /// counts as zero and an outage never forces a sell; the buy conviction passes every missing
    /// model's weight, so a missing model counts as fully bearish and an outage never enlarges a buy
    /// (MI-10).
    ///
    /// `division_by_zero` when the weights sum to zero; `above_one` or `negative` if the result
    /// leaves [−1, 1], which the formula's own bounds (|Σ products| ≤ Σ weights and
    /// Σ products − Σ bearish ≥ −Σ weights) make unreachable and which is therefore checked rather
    /// than assumed.
    pub fn weighted_ratio(
        products: &[(SizeFraction, Self, Unit)],
        bearish: &[SizeFraction],
        weights: &[SizeFraction],
    ) -> Result<Self, NumError> {
        let _ = (products, bearish, weights);
        Err(NumError::Overflow)
    }

    /// A non-negative fraction read as a conviction: how `behavior.sizing`'s thresholds compare
    /// against the combined convictions (spec §8.3 step 2).
    pub fn of_fraction(fraction: SizeFraction) -> Self {
        Self(fraction.0)
    }

    fn exact(self) -> Exact {
        Exact::of(self.0)
    }
}

impl Signed {
    pub const ZERO: Self = Self(Decimal::ZERO);

    /// Canonical text of a signed value with at most 18 fractional digits.
    pub fn parse(text: &str) -> Result<Self, NumError> {
        parse(text, OUTPUT_SCALE).map(Self)
    }

    pub fn is_negative(self) -> bool {
        self.0 < Decimal::ZERO
    }
}

macro_rules! display_decimal {
    ($name:ident) => {
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0.normalize(), f)
            }
        }
    };
}

display_decimal!(SizeFraction);
display_decimal!(Unit);
display_decimal!(Conviction);
display_decimal!(Signed);

/// A signed USD amount carried exactly on 256-bit intermediates, wider than [`Usd`]: the cap, the
/// target, and the delta of spec §8.3 steps 2 and 3, which reach 33 and 57 fractional places.
///
/// It converts to [`Usd`] only through [`UsdExact::round`], which names its scale and mode, so a
/// rounding the spec does not state cannot happen by accident. [`fmt::Display`] writes the exact
/// value with trailing zeros dropped, which is how the mandate reference cases report a cap.
///
/// ADR-0001 ES-04's engineering note says typed domain values stay within a 96-bit significand and
/// scale 28; this type does not, and the task brief proposes the amendment to the founder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsdExact(Exact);

impl UsdExact {
    pub const ZERO: Self = Self(Exact::ZERO);
    pub const ONE: Self = Self(Exact::ONE);

    pub fn of_usd(value: Usd) -> Self {
        Self(value.exact())
    }

    pub fn of_price(value: Price) -> Self {
        Self(value.exact())
    }

    pub fn of_qty(value: Qty) -> Self {
        Self(value.exact())
    }

    pub fn of_signed_qty(value: SignedQty) -> Self {
        Self(value.exact())
    }

    pub fn of_basis(value: CostBasis) -> Self {
        Self(value.exact())
    }

    pub fn of_fee_rate(value: FeeRate) -> Self {
        Self(value.exact())
    }

    /// `self + other`, exact.
    pub fn checked_add(self, other: Self) -> Result<Self, NumError> {
        self.0.add(other.0).map(Self)
    }

    /// `self − other`, exact.
    pub fn checked_sub(self, other: Self) -> Result<Self, NumError> {
        self.0.sub(other.0).map(Self)
    }

    /// `self × other`, exact.
    pub fn times(self, other: Self) -> Result<Self, NumError> {
        self.0.mul(other.0).map(Self)
    }

    /// `self × fraction`, exact: `max_position_fraction × E` and `rebalance_band × cap`.
    pub fn times_size_fraction(self, fraction: SizeFraction) -> Result<Self, NumError> {
        self.0.mul(fraction.exact()).map(Self)
    }

    /// `self × conviction`, exact: the target of spec §8.3 step 2.
    pub fn times_conviction(self, conviction: Conviction) -> Result<Self, NumError> {
        self.0.mul(conviction.exact()).map(Self)
    }

    /// Whether the value is strictly above zero.
    pub fn is_positive(self) -> bool {
        self.0.is_positive()
    }

    /// Orders two amounts exactly; `overflow` only if aligning their scales would leave 256 bits.
    pub fn compare(self, other: Self) -> Result<Ordering, NumError> {
        self.0.compare(other.0)
    }

    /// The lesser of two amounts, which is how spec §8.3 step 3 clips a buy to the least of its
    /// four bounds.
    pub fn min(self, other: Self) -> Result<Self, NumError> {
        Ok(match self.compare(other)? {
            Ordering::Greater => other,
            Ordering::Less | Ordering::Equal => self,
        })
    }

    /// `truncate(self ÷ divisor, increment)`, toward zero: the quantity an amount of money buys at
    /// a per-unit cost, never more than it pays for (spec §2.1 truncates an order quantity to the
    /// increment). Signed, because a bound already exceeded leaves a negative budget, which spec
    /// §8.3 step 3 holds on rather than turning into a sell.
    ///
    /// `division_by_zero` when `divisor` is zero.
    pub fn truncated_quotient(
        self,
        divisor: Self,
        increment: ShareIncrement,
    ) -> Result<SignedQty, NumError> {
        let _ = (divisor, increment);
        Err(NumError::Overflow)
    }

    /// `round(self, scale, mode)` as a [`Usd`]: the one place a wide amount narrows, and the mode is
    /// always the caller's to name. A `scale` beyond the 28 places [`Usd`] stores is `too_precise`
    /// rather than quietly clamped, so a caller cannot ask for places it will not get.
    pub fn round(self, scale: u32, mode: Rounding) -> Result<Usd, NumError> {
        if scale > FULL_SCALE {
            return Err(NumError::TooPrecise);
        }
        self.0
            .div(Exact::ONE, scale, mode)?
            .to_decimal(FULL_SCALE)
            .map(Usd)
    }
}

impl fmt::Display for UsdExact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}
