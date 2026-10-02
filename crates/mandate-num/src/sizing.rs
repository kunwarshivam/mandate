//! The exact arithmetic of the order builder ([mandate spec §8.3], DEC-130 item 7).
//!
//! §5.2 says comparisons are exact and only reported ratios round, and §8.3 names exactly three
//! roundings — the `round₁₂` of step 1. Everything else in the chain is exact, so the types here
//! either stay inside a fixed-width integer or say they cannot: an input too wide is `too_precise`
//! and an intermediate beyond 256 bits is `overflow`, and either way the builder proposes nothing
//! (DEC-130 item 8). An approximated size would be an order nobody specified.
//!
//! The four newtypes are bounded where the digit budget needs them to be, not where they look
//! tidy. [`SizeFraction`] stops at 12 fractional places because that is what keeps
//! `b × cap × factor` and the `shares_at` division inside 256 bits; [`Unit`] and [`Conviction`]
//! carry 18 because `MC-B13` already commits a 13-place confidence, and a confidence enters only
//! the combine step, where the budget affords it. [`Fraction`](crate::Fraction) is not reused for
//! either: its 9-place ceiling is part of the backtest fill model's contract (E4-1), and widening
//! it would let a volume cap carry 12 places.
//!
//! [mandate spec §8.3]: ../../../docs/specs/mandate.md#83-order-builder-conviction_linear-dec-47-dec-60

use core::cmp::Ordering;
use core::fmt;

use rust_decimal::Decimal;

use crate::exact::Exact;
use crate::{
    FULL_SCALE, FeeRate, MarkPrice, NegExact, NumError, Price, QTY_SCALE, Qty, Ratio, Rounding,
    Usd, non_negative, parse,
};

/// The sizing and limit fractions of a mandate hold at most 12 fractional digits (DEC-130 item 7).
const SIZE_FRACTION_SCALE: u32 = 12;
/// A model output's confidence and conviction, the combined score, and the two P&L fractions hold
/// at most 18 (DEC-130 item 7; `MC-B13` carries 13).
const OUTPUT_SCALE: u32 = 18;
/// The scale of the three `round₁₂` quotients of §8.3 step 1.
pub const COMBINE_SCALE: u32 = 12;

fn at_most_one(value: Decimal) -> Result<Decimal, NumError> {
    if value > Decimal::ONE {
        Err(NumError::AboveOne)
    } else {
        Ok(value)
    }
}

fn within_unit_interval(value: Decimal) -> Result<Decimal, NumError> {
    if value > Decimal::ONE || value < Decimal::ONE.neg_exact() {
        Err(NumError::AboveOne)
    } else {
        Ok(value)
    }
}

/// A non-negative fraction of one, at most one, with at most 12 fractional digits: a signal model's
/// `weight`, `max_position_fraction`, `entry_threshold`, `exit_threshold`, `rebalance_band`, and a
/// drawdown-ladder rung's size factor.
///
/// The 12-place bound is load-bearing rather than cosmetic: the digit budget of the E6-2 task brief
/// puts `T = b × cap × factor` at 57 places and the `shares_at` division at 10⁶⁹ over a price scaled
/// to 10⁵⁴, and 18-place fractions would carry that past the 256-bit ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SizeFraction(Decimal);

/// A non-negative value in the closed unit interval with at most 18 fractional digits: a model
/// output's `confidence`, the combined score, `thesis_confidence`, `drawdown`, and the unit-typed
/// condition values (§6.3, §8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Unit(Decimal);

/// A signed value in [−1, 1] with at most 18 fractional digits: a model output's `conviction` and
/// the two combined convictions of §8.3 step 1.
///
/// The two combined values are `round₁₂` results and use 12 of the 18. Their bound is a consequence
/// of the formula (|F| ≤ W and F − M ≥ −W) rather than an assumption, so it is checked here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Conviction(Decimal);

/// A signed value with at most 18 fractional digits and no interval bound: `daily_pnl_fraction` and
/// `position_pnl_fraction`, which §5.2 defines at 12 places, and the condition values compared
/// against them, which the schema does not bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Signed(Decimal);

macro_rules! display_by_value {
    ($name:ident) => {
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0.normalize(), f)
            }
        }
    };
}

display_by_value!(SizeFraction);
display_by_value!(Unit);
display_by_value!(Conviction);
display_by_value!(Signed);

impl SizeFraction {
    pub const ZERO: Self = Self(Decimal::ZERO);
    pub const ONE: Self = Self(Decimal::ONE);

    /// Canonical text of a value from zero to one inclusive with at most 12 fractional digits.
    /// `negative` below zero, `above_one` above one, `too_precise` beyond 12 places.
    pub fn parse(text: &str) -> Result<Self, NumError> {
        parse(text, SIZE_FRACTION_SCALE)
            .and_then(non_negative)
            .and_then(at_most_one)
            .map(Self)
    }

    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }
}

impl Unit {
    pub const ZERO: Self = Self(Decimal::ZERO);
    pub const ONE: Self = Self(Decimal::ONE);

    /// Canonical text of a value from zero to one inclusive with at most 18 fractional digits.
    pub fn parse(text: &str) -> Result<Self, NumError> {
        parse(text, OUTPUT_SCALE)
            .and_then(non_negative)
            .and_then(at_most_one)
            .map(Self)
    }

    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    /// `round₁₂(Σ wᵢ · uᵢ ÷ Σ weights)`: the combined score of §8.3 step 1 as **one** formula on
    /// 256-bit intermediates — the numerator terms summed exactly, the denominator summed exactly,
    /// and one half-even division at 12 places.
    ///
    /// `terms` are the fresh models' (weight, confidence) pairs and `weights` the weights of **all**
    /// configured models, which is why the two are separate arguments: §8.3's W counts every
    /// configured model, and passing the fresh weights would silently rescale the score.
    /// `division_by_zero` when the weights sum to zero.
    pub fn weighted_ratio(
        terms: &[(SizeFraction, Unit)],
        weights: &[SizeFraction],
    ) -> Result<Self, NumError> {
        let numerator = terms
            .iter()
            .try_fold(Exact::integer(0), |sum, (weight, unit)| {
                sum.add(Exact::of(weight.0).mul(Exact::of(unit.0))?)
            })?;
        combined_ratio(numerator, weights)
            .and_then(non_negative)
            .and_then(at_most_one)
            .map(Self)
    }
}

impl Conviction {
    pub const ZERO: Self = Self(Decimal::ZERO);
    /// The buy conviction of an agent with no fresh output at all (§8.3 step 1, MC-B20).
    pub const MINUS_ONE: Self = Self(Decimal::NEGATIVE_ONE);

    /// Canonical text of a value in [−1, 1] with at most 18 fractional digits.
    pub fn parse(text: &str) -> Result<Self, NumError> {
        parse(text, OUTPUT_SCALE)
            .and_then(within_unit_interval)
            .map(Self)
    }

    pub fn is_negative(self) -> bool {
        self.0 < Decimal::ZERO
    }

    pub fn negated(self) -> Self {
        Self(self.0.neg_exact())
    }

    /// `round₁₂((Σ wᵢ · convictionᵢ · confidenceᵢ − Σ missing) ÷ Σ weights)`: both convictions of
    /// §8.3 step 1 as **one** formula each on 256-bit intermediates.
    ///
    /// `missing` is the weights counted as −1, which is what makes a model without a fresh output
    /// fully bearish for the buy conviction (MI-10); the exit conviction passes an empty slice, so
    /// a missing model counts as zero there and an outage can never force a sell. `weights` is
    /// every configured model's weight, never the fresh ones. `division_by_zero` when the weights
    /// sum to zero.
    pub fn weighted_ratio(
        terms: &[(SizeFraction, Conviction, Unit)],
        missing: &[SizeFraction],
        weights: &[SizeFraction],
    ) -> Result<Self, NumError> {
        let fresh = terms.iter().try_fold(
            Exact::integer(0),
            |sum, (weight, conviction, confidence)| {
                sum.add(
                    Exact::of(weight.0)
                        .mul(Exact::of(conviction.0))?
                        .mul(Exact::of(confidence.0))?,
                )
            },
        )?;
        combined_ratio(fresh.sub(sum_of(missing)?)?, weights)
            .and_then(within_unit_interval)
            .map(Self)
    }
}

fn sum_of(weights: &[SizeFraction]) -> Result<Exact, NumError> {
    weights.iter().try_fold(Exact::integer(0), |sum, weight| {
        sum.add(Exact::of(weight.0))
    })
}

/// `round₁₂(numerator ÷ Σ weights)`, half-even: the one rounding each §8.3 step 1 figure takes.
fn combined_ratio(numerator: Exact, weights: &[SizeFraction]) -> Result<Decimal, NumError> {
    numerator
        .div(sum_of(weights)?, COMBINE_SCALE, Rounding::HalfEven)?
        .to_decimal(OUTPUT_SCALE)
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

/// A signed USD amount carried exactly on 256-bit intermediates, wider than [`Usd`], converted back
/// only by an explicit rounding.
///
/// `cap` reaches 33 fractional places (a 12-place fraction times an equity that holds 21, from a
/// 9-place quantity at a 12-place mark) and the target 57, while `Usd` holds 28 places on a 96-bit
/// significand and [`Ratio`](crate::Ratio) 24. Neither can carry the §8.3 chain, and §5.2 forbids
/// rounding a value a comparison uses, so the chain needs a type of its own. It is also the
/// dimensionless carrier of the accumulate clips' numerators and denominators, which are per-unit
/// costs and quantities rather than amounts of money.
///
/// Equality is **by value**, not by representation: `0.10` and `0.1` are one amount, and deriving
/// equality over a sign-magnitude representation would make them differ while comparing equal
/// (the lesson DEC-128 item 24 records for `SchemaDec`). There is no `Hash`: ES-21 bans `HashMap`
/// and `HashSet`, so a hash of a sizing intermediate has no legitimate caller.
#[derive(Debug, Clone, Copy)]
pub struct UsdExact(Exact);

impl PartialEq for UsdExact {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_canonical_string() == other.0.to_canonical_string()
    }
}

impl Eq for UsdExact {}

impl fmt::Display for UsdExact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl UsdExact {
    /// Canonical decimal text, signed, at any scale the 256-bit magnitude holds.
    pub fn parse(text: &str) -> Result<Self, NumError> {
        let value = Exact::parse(text)?;
        if value.to_canonical_string() == text {
            Ok(Self(value))
        } else {
            Err(NumError::NotCanonical)
        }
    }

    pub fn zero() -> Self {
        Self(Exact::integer(0))
    }

    /// One, the multiplicative identity the accumulate clips add a fee rate to.
    pub fn one() -> Self {
        Self(Exact::integer(1))
    }

    pub fn of(amount: Usd) -> Self {
        Self(amount.exact())
    }

    pub fn of_qty(quantity: Qty) -> Self {
        Self(quantity.exact())
    }

    pub fn of_price(price: Price) -> Self {
        Self(price.exact())
    }

    pub fn of_mark(mark: MarkPrice) -> Self {
        Self(mark.exact())
    }

    pub fn of_fee_rate(rate: FeeRate) -> Self {
        Self(rate.exact())
    }

    /// The risk state's size factor, which mandate §5.5 multiplies the position cap by.
    pub fn of_ratio(ratio: Ratio) -> Self {
        Self(ratio.exact())
    }

    /// `self + other`, exact.
    pub fn checked_add(self, other: Self) -> Result<Self, NumError> {
        Ok(Self(self.0.add(other.0)?))
    }

    /// `self − other`, exact: Delta, the cap headroom, and the gross headroom of §8.3 step 3.
    pub fn checked_sub(self, other: Self) -> Result<Self, NumError> {
        Ok(Self(self.0.sub(other.0)?))
    }

    /// `self × other`, exact: MV at the risk mark, and the accumulate clips' per-unit terms.
    pub fn checked_mul(self, other: Self) -> Result<Self, NumError> {
        Ok(Self(self.0.mul(other.0)?))
    }

    /// `self × fraction`, exact: `max_position_fraction × E`, `rebalance_band × cap`, and the
    /// ladder size factor applied to the target.
    pub fn times_size_fraction(self, fraction: SizeFraction) -> Result<Self, NumError> {
        Ok(Self(self.0.mul(Exact::of(fraction.0))?))
    }

    /// `self × conviction`, exact: `b × cap` of §8.3 step 2.
    pub fn times_conviction(self, conviction: Conviction) -> Result<Self, NumError> {
        Ok(Self(self.0.mul(Exact::of(conviction.0))?))
    }

    /// `self × rate`, exact: the cash fee inside the accumulate per-unit cost `a`.
    pub fn times_fee_rate(self, rate: FeeRate) -> Result<Self, NumError> {
        Ok(Self(self.0.mul(rate.exact())?))
    }

    /// The lesser of two amounts: the four clips of §8.3 step 3 and the accumulate bounds of step 4.
    ///
    /// Fallible because aligning two scales can exceed 256 bits, and because a stub that answered
    /// "the first one" would size an order off a bound that does not bind.
    pub fn min(self, other: Self) -> Result<Self, NumError> {
        Ok(if other.is_below(self)? { other } else { self })
    }

    /// `self < other`, exact.
    ///
    /// Fallible for the same reason as [`UsdExact::min`], and because a `false` from a stub would
    /// read as "the delta is not inside the band", which is the direction that proposes an order.
    pub fn is_below(self, other: Self) -> Result<bool, NumError> {
        Ok(self.0.sub(other.0)?.sign() == Ordering::Less)
    }

    /// `self > 0`, exact.
    ///
    /// Fallible so that the stub cannot answer "not positive" and hold: a hold is the plausible
    /// do-nothing answer for §8.3 step 3, and a pending test must fail on the stub rather than pass
    /// on it (DEC-110).
    pub fn is_positive(self) -> Result<bool, NumError> {
        Ok(self.0.sign() == Ordering::Greater)
    }

    /// `round(self, scale, mode)` as a [`Usd`]: the one explicit narrowing out of the wide chain.
    ///
    /// It **rounds** in the mode the caller names; it does not truncate. `too_precise` when the
    /// value needs more places than `Usd` stores, rather than silently dropping them.
    pub fn round(self, scale: u32, mode: Rounding) -> Result<Usd, NumError> {
        self.0
            .div(Exact::integer(1), scale, mode)?
            .to_decimal(FULL_SCALE)
            .map(Usd)
    }

    /// `round(self ÷ divisor, scale, mode)` as a [`Usd`]: one rounding of the exact quotient.
    ///
    /// Mandate spec §5.1 scales the high-water mark, the day-start equity, the capital base, and
    /// the inherited loss by `X × (E + Δ) ÷ E`, rounded up at 12 places. The product needs more
    /// places than a `Usd` holds, so it stays exact until this one division (DEC-167 item 5).
    /// `division_by_zero` for a zero divisor; `too_precise` when `scale` is past what a `Usd`
    /// stores.
    pub fn quotient(self, divisor: Self, scale: u32, mode: Rounding) -> Result<Usd, NumError> {
        self.0
            .div(divisor.0, scale, mode)?
            .to_decimal(FULL_SCALE)
            .map(Usd)
    }

    /// `truncate(self ÷ price, increment)`: the shares this amount buys at `price`, never more
    /// (§8.3 step 3's one division, trading spec §2.1's truncation to the increment).
    ///
    /// The increment is a [`Qty`] and not a
    /// [`ShareIncrement`](crate::ShareIncrement), which carries only `Whole` and `Fractional` at
    /// nine places: the mandate reference cases state increments of `1`, `0.0001`, and `0.000001`,
    /// and `MC-B28`'s expected `0.010025` is a 6-place truncation that nine places would round to
    /// `0.010025062`. This is the same correction DEC-128 item 27 made for a goal's increment.
    /// `not_positive` for an increment of zero or below, where neither answer is safe.
    pub fn shares_at(self, price: Price, increment: Qty) -> Result<Qty, NumError> {
        self.truncated_quotient(Self::of_price(price), increment)
    }

    /// `truncate(self ÷ per_unit, increment)`: the three accumulate clips of §8.3 step 4, whose
    /// denominators are the per-unit cost `a`, the quantity received per unit `β`, and
    /// `a − max_avg_price × β` rather than a price.
    pub fn truncated_quotient(self, per_unit: Self, increment: Qty) -> Result<Qty, NumError> {
        if !increment.exact().is_positive() {
            return Err(NumError::NotPositive);
        }
        let count = self
            .0
            .div_toward_zero(per_unit.0.mul(increment.exact())?, 0)?;
        if count.sign() != Ordering::Greater {
            return Ok(Qty::ZERO);
        }
        count.mul(increment.exact())?.to_decimal(QTY_SCALE).map(Qty)
    }

    /// `ceiling(self ÷ per_unit, increment)`: the fewest whole increments whose cost at `per_unit`
    /// covers `self`, which is mandate §5.5's trim "rounded up to the increment", so a trim never
    /// leaves a position above its target. Zero when `self` is zero or below.
    /// `not_positive` for an increment of zero or below, and `division_by_zero` for a zero
    /// `per_unit`.
    pub fn ceiled_quotient(self, per_unit: Self, increment: Qty) -> Result<Qty, NumError> {
        if !increment.exact().is_positive() {
            return Err(NumError::NotPositive);
        }
        let count = self
            .0
            .div(per_unit.0.mul(increment.exact())?, 0, Rounding::Ceiling)?;
        if count.sign() != Ordering::Greater {
            return Ok(Qty::ZERO);
        }
        count.mul(increment.exact())?.to_decimal(QTY_SCALE).map(Qty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exact(text: &str) -> Result<UsdExact, NumError> {
        UsdExact::parse(text)
    }

    /// §5.1's scaling: 10000 × 4500 ÷ 9500 is 4736.842105263157894…, which rounds up at 12 places
    /// to …158 and half to even to …158 too, while 9500 ÷ 4500 × the scaled mark rounds up past
    /// 10000 where half to even returns to it. One rounding of the exact quotient, in the mode named.
    #[test]
    fn a_quotient_is_rounded_once_in_the_mode_named() -> Result<(), NumError> {
        let scaled = exact("45000000")?.quotient(exact("9500")?, 12, Rounding::Ceiling)?;
        assert_eq!(scaled, Usd::parse("4736.842105263158")?);
        let back = UsdExact::of(scaled).checked_mul(exact("9500")?)?.quotient(
            exact("4500")?,
            12,
            Rounding::Ceiling,
        )?;
        assert_eq!(back, Usd::parse("10000.000000000001")?);
        let even = UsdExact::of(scaled).checked_mul(exact("9500")?)?.quotient(
            exact("4500")?,
            12,
            Rounding::HalfEven,
        )?;
        assert_eq!(even, Usd::parse("10000")?);
        assert_eq!(
            exact("-1")?.quotient(exact("3")?, 2, Rounding::Ceiling)?,
            Usd::parse("-0.33")?,
            "a ceiling of a negative quotient rounds toward zero"
        );
        Ok(())
    }

    #[test]
    fn a_quotient_by_zero_or_past_a_usd_is_an_error() -> Result<(), NumError> {
        assert_eq!(
            exact("1")?.quotient(UsdExact::zero(), 12, Rounding::Ceiling),
            Err(NumError::DivisionByZero)
        );
        assert!(
            exact("1")?
                .quotient(exact("3")?, 29, Rounding::Ceiling)
                .is_err(),
            "29 places is past the 28 a Usd stores"
        );
        assert_eq!(
            exact("1")?.quotient(exact("3")?, 28, Rounding::HalfEven)?,
            Usd::parse("0.3333333333333333333333333333")?
        );
        Ok(())
    }

    fn ceiled(amount: &str, per_unit: &str, increment: &str) -> Result<Qty, NumError> {
        exact(amount)?.ceiled_quotient(exact(per_unit)?, Qty::parse(increment)?)
    }

    /// Mandate §5.5's trim rounds **up**: 250 over at 100 a share is 2.5 shares, so 3; an exact
    /// quotient stays as it is; a finer increment rounds up to its own grid; nothing over is zero.
    #[test]
    fn a_ceiled_quotient_rounds_up_to_the_increment() -> Result<(), NumError> {
        let cases = [
            ("250", "100", "1", "3"),
            ("200", "100", "1", "2"),
            ("200.000000001", "100", "1", "3"),
            ("120", "100", "0.5", "1.5"),
            ("1", "3", "0.000000001", "0.333333334"),
            ("0", "100", "1", "0"),
            ("-250", "100", "1", "0"),
        ];
        for (amount, per_unit, increment, expected) in cases {
            assert_eq!(
                ceiled(amount, per_unit, increment)?,
                Qty::parse(expected)?,
                "ceiling({amount} ÷ {per_unit}) on a grid of {increment}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_ceiled_quotient_refuses_a_grid_or_a_divisor_it_cannot_use() -> Result<(), NumError> {
        assert_eq!(ceiled("250", "100", "0"), Err(NumError::NotPositive));
        assert_eq!(ceiled("250", "0", "1"), Err(NumError::DivisionByZero));
        Ok(())
    }

    /// The size factor enters exactly, at every one of a ratio's places.
    #[test]
    fn a_ratio_enters_the_chain_exactly() -> Result<(), NumError> {
        let product = exact("1500")?.checked_mul(UsdExact::of_ratio(Ratio::parse(
            "0.200000000000000000000001",
        )?))?;
        assert_eq!(product, exact("300.0000000000000000000015")?);
        Ok(())
    }
}
