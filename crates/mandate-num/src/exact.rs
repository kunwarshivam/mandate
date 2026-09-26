//! Sign-and-magnitude decimals on 256-bit integers. Every operation is exact or an error; the only
//! rounding is the one a caller asks for in [`Exact::div`].

use core::cmp::Ordering;

use ruint::aliases::U256;
use rust_decimal::Decimal;

use crate::{NumError, Rounding};

/// The largest scale `rust_decimal` stores.
const MAX_STORED_SCALE: u32 = 28;
/// `rust_decimal` significands have 96 bits.
const SIGNIFICAND_BITS: usize = 96;

/// value = (−1)^`negative` × `magnitude` × 10^−`scale`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Exact {
    negative: bool,
    magnitude: U256,
    scale: u32,
}

fn pow10(exp: u32) -> Result<U256, NumError> {
    (0..exp).try_fold(U256::from(1u8), |acc, _| {
        acc.checked_mul(U256::from(10u8)).ok_or(NumError::Overflow)
    })
}

impl Exact {
    pub(crate) fn integer(n: u64) -> Self {
        Self {
            negative: false,
            magnitude: U256::from(n),
            scale: 0,
        }
    }

    pub(crate) fn of(value: Decimal) -> Self {
        let mantissa = value.mantissa();
        Self {
            negative: mantissa.is_negative(),
            magnitude: U256::from(mantissa.unsigned_abs()),
            scale: value.scale(),
        }
    }

    /// Reads `-?digits(.digits)?`. Canonical form is checked by the caller against the stored value.
    pub(crate) fn parse(text: &str) -> Result<Self, NumError> {
        let (negative, unsigned) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (int, frac) = unsigned.split_once('.').unwrap_or((unsigned, ""));
        if int.is_empty() {
            return Err(NumError::NotCanonical);
        }
        let magnitude = int
            .bytes()
            .chain(frac.bytes())
            .try_fold(U256::ZERO, |acc, byte| {
                let digit = byte
                    .checked_sub(b'0')
                    .filter(|d| *d <= 9)
                    .ok_or(NumError::NotCanonical)?;
                acc.checked_mul(U256::from(10u8))
                    .and_then(|m| m.checked_add(U256::from(digit)))
                    .ok_or(NumError::Overflow)
            })?;
        let scale = u32::try_from(frac.len()).map_err(|_| NumError::TooPrecise)?;
        Ok(Self {
            negative,
            magnitude,
            scale,
        })
    }

    pub(crate) fn neg(self) -> Self {
        Self {
            negative: !self.negative,
            ..self
        }
    }

    /// The magnitude written with `scale` fractional digits; `scale` is at least `self.scale`.
    fn magnitude_at(self, scale: u32) -> Result<U256, NumError> {
        self.magnitude
            .checked_mul(pow10(scale.saturating_sub(self.scale))?)
            .ok_or(NumError::Overflow)
    }

    pub(crate) fn add(self, other: Self) -> Result<Self, NumError> {
        let scale = self.scale.max(other.scale);
        let (a, b) = (self.magnitude_at(scale)?, other.magnitude_at(scale)?);
        let (negative, magnitude) = if self.negative == other.negative {
            (self.negative, a.checked_add(b))
        } else if a >= b {
            (self.negative, a.checked_sub(b))
        } else {
            (other.negative, b.checked_sub(a))
        };
        Ok(Self {
            negative,
            magnitude: magnitude.ok_or(NumError::Overflow)?,
            scale,
        })
    }

    pub(crate) fn sub(self, other: Self) -> Result<Self, NumError> {
        self.add(other.neg())
    }

    pub(crate) fn mul(self, other: Self) -> Result<Self, NumError> {
        Ok(Self {
            negative: self.negative != other.negative,
            magnitude: self
                .magnitude
                .checked_mul(other.magnitude)
                .ok_or(NumError::Overflow)?,
            scale: self
                .scale
                .checked_add(other.scale)
                .ok_or(NumError::Overflow)?,
        })
    }

    /// `self ÷ divisor`, rounded once to `scale` fractional digits.
    pub(crate) fn div(self, divisor: Self, scale: u32, mode: Rounding) -> Result<Self, NumError> {
        if divisor.magnitude.is_zero() {
            return Err(NumError::DivisionByZero);
        }
        let up = scale.checked_add(divisor.scale).ok_or(NumError::Overflow)?;
        let numerator = self
            .magnitude
            .checked_mul(pow10(up.saturating_sub(self.scale))?)
            .ok_or(NumError::Overflow)?;
        let denominator = divisor
            .magnitude
            .checked_mul(pow10(self.scale.saturating_sub(up))?)
            .ok_or(NumError::Overflow)?;
        let negative = self.negative != divisor.negative;
        Ok(Self {
            negative,
            magnitude: round_quotient(numerator, denominator, negative, mode)?,
            scale,
        })
    }

    /// Stores the value, dropping only trailing zeros; anything that would need rounding or more
    /// than 96 significand bits is an error.
    pub(crate) fn to_decimal(self, max_scale: u32) -> Result<Decimal, NumError> {
        let max_scale = max_scale.min(MAX_STORED_SCALE);
        let mut value = self;
        while value.scale > max_scale || value.magnitude.bit_len() > SIGNIFICAND_BITS {
            let ten = U256::from(10u8);
            let remainder = value.magnitude.checked_rem(ten).unwrap_or(U256::ZERO);
            if value.scale == 0 || !remainder.is_zero() {
                return Err(if value.scale > max_scale {
                    NumError::TooPrecise
                } else {
                    NumError::Overflow
                });
            }
            value = Self {
                magnitude: value.magnitude.checked_div(ten).unwrap_or(U256::ZERO),
                scale: value.scale.saturating_sub(1),
                ..value
            };
        }
        let magnitude = i128::try_from(value.magnitude).map_err(|_| NumError::Overflow)?;
        let mantissa = if value.negative {
            magnitude.checked_neg().ok_or(NumError::Overflow)?
        } else {
            magnitude
        };
        Decimal::try_from_i128_with_scale(mantissa, value.scale).map_err(|_| NumError::Overflow)
    }
}

/// `numerator ÷ denominator` rounded to an integer; `negative` is the sign of the true quotient.
fn round_quotient(
    numerator: U256,
    denominator: U256,
    negative: bool,
    mode: Rounding,
) -> Result<U256, NumError> {
    let quotient = numerator
        .checked_div(denominator)
        .ok_or(NumError::DivisionByZero)?;
    let remainder = numerator
        .checked_rem(denominator)
        .ok_or(NumError::DivisionByZero)?;
    let rest = denominator.checked_sub(remainder).unwrap_or(U256::ZERO);
    let half = remainder.cmp(&rest);
    let away_from_zero = match mode {
        Rounding::HalfEven => {
            half == Ordering::Greater || (half == Ordering::Equal && quotient.bit(0))
        }
        Rounding::HalfUp => half != Ordering::Less,
        Rounding::Ceiling => !negative && !remainder.is_zero(),
    };
    if away_from_zero {
        quotient
            .checked_add(U256::from(1u8))
            .ok_or(NumError::Overflow)
    } else {
        Ok(quotient)
    }
}
