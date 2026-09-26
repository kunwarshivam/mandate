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
}

impl From<Qty> for SignedQty {
    fn from(qty: Qty) -> Self {
        Self(qty.0)
    }
}

impl Price {
    pub fn parse(text: &str) -> Result<Self, NumError> {
        let value = parse(text, QTY_SCALE)?;
        if value > Decimal::ZERO {
            Ok(Self(value))
        } else {
            Err(NumError::NotPositive)
        }
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
