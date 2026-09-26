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
        }
    }
}

/// Prices and quantities have at most 9 fractional digits (spec §2.1).
const QTY_SCALE: u32 = 9;
/// Reporting marks have at most 12 fractional digits (spec §2.1).
const MARK_SCALE: u32 = 12;
/// Money and rates keep full precision up to the stored maximum.
const FULL_SCALE: u32 = 28;
const BPS_PER_UNIT: u64 = 10_000;

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
        let grid = match increment {
            ShareIncrement::Fractional => QTY_SCALE,
            ShareIncrement::Whole => 0,
        };
        let after = qty
            .exact()
            .mul(Exact::integer(self.new))?
            .div_toward_zero(Exact::integer(self.old), grid)?
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
    pub fn parse(text: &str) -> Result<Self, NumError> {
        parse(text, FULL_SCALE).and_then(non_negative).map(Self)
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
