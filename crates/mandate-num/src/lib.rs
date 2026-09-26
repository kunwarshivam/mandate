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

use core::fmt;

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
        ""
    }
}

macro_rules! decimal_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(Decimal);

        impl $name {
            /// Whether the value is zero.
            pub fn is_zero(self) -> bool {
                false
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
                Ok(())
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

    pub fn parse(_text: &str) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }

    /// `self − other`; an error if the result would be negative.
    pub fn checked_sub(self, _other: Self) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }

    /// `self × price`, exact.
    pub fn notional(self, _price: Price) -> Result<Usd, NumError> {
        Err(NumError::Overflow)
    }

    /// `self × per_share`, exact.
    pub fn times_per_share(self, _per_share: FeePerShare) -> Result<Usd, NumError> {
        Err(NumError::Overflow)
    }

    /// `round(self × bps ÷ 10000, 9, mode)`: one rounding.
    pub fn times_bps(self, _bps: Bps, _mode: Rounding) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }
}

impl SignedQty {
    pub const ZERO: Self = Self(Decimal::ZERO);

    pub fn parse(_text: &str) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }

    pub fn checked_add(self, _other: Self) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }

    pub fn negated(self) -> Self {
        Self::ZERO
    }

    pub fn abs(self) -> Qty {
        Qty::ZERO
    }

    pub fn is_negative(self) -> bool {
        false
    }

    /// `self × price`, exact and signed.
    pub fn value_at(self, _price: Price) -> Result<Usd, NumError> {
        Err(NumError::Overflow)
    }
}

impl From<Qty> for SignedQty {
    fn from(_qty: Qty) -> Self {
        Self::ZERO
    }
}

impl Price {
    pub fn parse(_text: &str) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }
}

impl Usd {
    pub const ZERO: Self = Self(Decimal::ZERO);

    pub fn parse(_text: &str) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }

    pub fn checked_add(self, _other: Self) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }

    pub fn checked_sub(self, _other: Self) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }

    pub fn negated(self) -> Self {
        Self::ZERO
    }

    pub fn is_negative(self) -> bool {
        false
    }

    /// `self × rate`, exact.
    pub fn times_rate(self, _rate: FeeRate) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }

    /// `round(self × bps ÷ 10000, scale, mode)`: one rounding.
    pub fn times_bps(self, _bps: Bps, _scale: u32, _mode: Rounding) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }

    /// `round(self, scale, mode)`.
    pub fn round(self, _scale: u32, _mode: Rounding) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }
}

impl CostBasis {
    pub const ZERO: Self = Self(Decimal::ZERO);

    pub fn parse(_text: &str) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }

    /// `self + amount`, exact.
    pub fn checked_add(self, _amount: Usd) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }

    /// `self − removed`, exact.
    pub fn checked_sub(self, _removed: Self) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }

    /// `round(self × part ÷ whole, scale, mode)`: the basis removed by a reduction (spec §8.1),
    /// with a single rounding.
    pub fn portion(
        self,
        _part: Qty,
        _whole: Qty,
        _scale: u32,
        _mode: Rounding,
    ) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }

    pub fn to_usd(self) -> Usd {
        Usd::ZERO
    }
}

impl FeeRate {
    pub fn parse(_text: &str) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }
}

impl FeePerShare {
    pub fn parse(_text: &str) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }
}

impl Bps {
    pub fn parse(_text: &str) -> Result<Self, NumError> {
        Err(NumError::Overflow)
    }
}
