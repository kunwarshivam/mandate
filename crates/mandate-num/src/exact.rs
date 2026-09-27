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
        let (numerator, denominator, negative) = self.quotient_terms(divisor, scale)?;
        Ok(Self {
            negative,
            magnitude: round_quotient(numerator, denominator, negative, mode)?,
            scale,
        })
    }

    /// `self ÷ divisor`, truncated toward zero to `scale` fractional digits.
    pub(crate) fn div_toward_zero(self, divisor: Self, scale: u32) -> Result<Self, NumError> {
        let (numerator, denominator, negative) = self.quotient_terms(divisor, scale)?;
        Ok(Self {
            negative,
            magnitude: numerator
                .checked_div(denominator)
                .ok_or(NumError::DivisionByZero)?,
            scale,
        })
    }

    /// Integer magnitudes whose quotient is |self ÷ divisor| × 10^`scale`, and the quotient's sign.
    fn quotient_terms(self, divisor: Self, scale: u32) -> Result<(U256, U256, bool), NumError> {
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
        Ok((numerator, denominator, self.negative != divisor.negative))
    }

    /// `ceil(sqrt(self ÷ divisor), scale)`: the smallest value with `scale` fractional digits
    /// whose square is at or above the ratio, so an impact built on it is never understated
    /// ([trading-domain spec §6.4](../../../docs/specs/trading-domain.md#64-backtest-fill-model),
    /// DEC-106 item 3). The ratio is taken at twice `scale`, rounded up, which an integer root
    /// then turns into the ceiling of the root itself: for an integer n, n² ≥ ratio × 10^2·scale
    /// exactly when n² ≥ ceil(ratio × 10^2·scale). `division_by_zero` when `divisor` is zero.
    pub(crate) fn ceiling_root_of_ratio(self, divisor: Self, scale: u32) -> Result<Self, NumError> {
        let squared_scale = scale.checked_mul(2).ok_or(NumError::Overflow)?;
        let (numerator, denominator, _) = self.quotient_terms(divisor, squared_scale)?;
        let squared = round_quotient(numerator, denominator, false, Rounding::Ceiling)?;
        Ok(Self {
            negative: false,
            magnitude: ceiling_sqrt(squared)?,
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

impl Exact {
    /// The canonical decimal text of the value: no exponent, no trailing fractional zeros, `0` for
    /// zero, and a leading `-` only for a non-zero negative. This is the form
    /// [`crate::parse`] accepts and the form the reference cases write, so a value that round-trips
    /// through it is the same value.
    pub(crate) fn to_canonical_string(self) -> String {
        let digits = self.magnitude.to_string();
        let scale = usize::try_from(self.scale).unwrap_or(usize::MAX);
        let padded = if digits.len() <= scale {
            format!(
                "{}{digits}",
                "0".repeat(scale.saturating_sub(digits.len()).saturating_add(1))
            )
        } else {
            digits
        };
        let point = padded.len().saturating_sub(scale);
        let (int, frac) = padded.split_at(point);
        let frac = frac.trim_end_matches('0');
        let magnitude = if frac.is_empty() {
            int.to_owned()
        } else {
            format!("{int}.{frac}")
        };
        if self.negative && !self.magnitude.is_zero() {
            format!("-{magnitude}")
        } else {
            magnitude
        }
    }
}

impl core::fmt::Display for Exact {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.to_canonical_string())
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

/// `ceil(sqrt(value))`, from the floor and one comparison.
fn ceiling_sqrt(value: U256) -> Result<U256, NumError> {
    let floor = floor_sqrt(value)?;
    let squared = floor.checked_mul(floor).ok_or(NumError::Overflow)?;
    if squared < value {
        floor.checked_add(U256::from(1u8)).ok_or(NumError::Overflow)
    } else {
        Ok(floor)
    }
}

/// `floor(sqrt(value))` by Newton's method on integers. The first guess, 2^ceil(bits ÷ 2), is at or
/// above the root, and each step stays at or above it while falling strictly until it reaches it, so
/// the loop ends there and ends at the root. Zero alone is returned without iterating, because it is
/// the one value whose root is below one and so the one guess the steps could fall to and then
/// divide by. Integers throughout: no float reaches a price (ADR-0001 ES-04), and a backtest that
/// uses the root replays bit for bit (ES-21).
fn floor_sqrt(value: U256) -> Result<U256, NumError> {
    if value.is_zero() {
        return Ok(U256::ZERO);
    }
    let two = U256::from(2u8);
    let halved_bits = value
        .bit_len()
        .checked_add(1)
        .and_then(|bits| bits.checked_div(2))
        .ok_or(NumError::Overflow)?;
    let mut guess = U256::from(1u8)
        .checked_shl(halved_bits)
        .ok_or(NumError::Overflow)?;
    loop {
        let next = value
            .checked_div(guess)
            .ok_or(NumError::DivisionByZero)?
            .checked_add(guess)
            .ok_or(NumError::Overflow)?
            .checked_div(two)
            .ok_or(NumError::DivisionByZero)?;
        if next >= guess {
            return Ok(guess);
        }
        guess = next;
    }
}
