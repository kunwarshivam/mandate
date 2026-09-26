//! Vendor numbers, exactly (ADR-0001 ES-23): JSON number tokens arrive as raw text, become
//! [`DecStr`] without passing through `f64`, and convert to and from the integer units of a
//! `Decimal128(38, scale)` column exactly or with an error, never rounded.

use mandate_canon::{DecError, DecStr};

/// Significant digits a `Decimal128` column holds.
pub const PRECISION: u8 = 38;
/// Largest scale a column may use: [`DecStr`] holds at most 28 fractional digits.
pub const MAX_SCALE: u8 = 28;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NumberError {
    #[error("not a JSON number token")]
    NotANumber,
    #[error("not an unsigned 64-bit integer token")]
    NotAnInteger,
    #[error("outside the journal decimal grammar: {0}")]
    Decimal(DecError),
    #[error("needs more than {scale} fractional digits")]
    Scale { scale: u8 },
    #[error("needs more than 38 significant digits at scale {scale}")]
    Precision { scale: u8 },
    #[error("scale {scale} is above the supported maximum of 28")]
    UnsupportedScale { scale: u8 },
}

impl NumberError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::NotANumber => "not_a_number",
            Self::NotAnInteger => "not_an_integer",
            Self::Decimal(_) => "decimal",
            Self::Scale { .. } => "scale",
            Self::Precision { .. } => "precision",
            Self::UnsupportedScale { .. } => "unsupported_scale",
        }
    }
}

/// A JSON number token (RFC 8259 §6, nothing around it) as an exact decimal.
pub fn decimal_from_json(raw: &str) -> Result<DecStr, NumberError> {
    if !is_json_number(raw.as_bytes()) {
        return Err(NumberError::NotANumber);
    }
    DecStr::parse(raw).map_err(NumberError::Decimal)
}

/// A JSON integer token without sign, fraction, or exponent, as a `u64`.
pub fn unsigned_from_json(raw: &str) -> Result<u64, NumberError> {
    let canonical_digits = match raw.as_bytes() {
        [b'0'] => true,
        [first, rest @ ..] => (b'1'..=b'9').contains(first) && rest.iter().all(u8::is_ascii_digit),
        [] => false,
    };
    if !canonical_digits {
        return Err(NumberError::NotAnInteger);
    }
    raw.parse().map_err(|_| NumberError::NotAnInteger)
}

/// `value × 10^scale` as an integer, if that is exact and fits 38 digits.
pub fn to_units(value: &DecStr, scale: u8) -> Result<i128, NumberError> {
    check_scale(scale)?;
    let text = value.as_str();
    let (negative, magnitude) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (int, frac) = magnitude.split_once('.').unwrap_or((magnitude, ""));
    let padding = usize::from(scale)
        .checked_sub(frac.len())
        .ok_or(NumberError::Scale { scale })?;
    let digits: String = int
        .chars()
        .chain(frac.chars())
        .chain(std::iter::repeat_n('0', padding))
        .skip_while(|c| *c == '0')
        .collect();
    if digits.len() > usize::from(PRECISION) {
        return Err(NumberError::Precision { scale });
    }
    let units = digits
        .bytes()
        .try_fold(0i128, |acc, d| {
            acc.checked_mul(10)?
                .checked_add(i128::from(d.checked_sub(b'0')?))
        })
        .ok_or(NumberError::Precision { scale })?;
    if negative {
        units.checked_neg().ok_or(NumberError::Precision { scale })
    } else {
        Ok(units)
    }
}

/// `units × 10^-scale` as a decimal; the inverse of [`to_units`].
pub fn from_units(units: i128, scale: u8) -> Result<DecStr, NumberError> {
    check_scale(scale)?;
    let digits = units.unsigned_abs().to_string();
    if digits.len() > usize::from(PRECISION) {
        return Err(NumberError::Precision { scale });
    }
    let width = usize::from(scale).saturating_add(1);
    let padded = format!("{digits:0>width$}");
    let split = padded.len().saturating_sub(usize::from(scale));
    let (int, frac) = padded
        .split_at_checked(split)
        .ok_or(NumberError::Precision { scale })?;
    let sign = if units < 0 { "-" } else { "" };
    DecStr::parse(&format!("{sign}{int}.{frac}")).map_err(NumberError::Decimal)
}

fn check_scale(scale: u8) -> Result<(), NumberError> {
    if scale > MAX_SCALE {
        return Err(NumberError::UnsupportedScale { scale });
    }
    Ok(())
}

/// `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`, the whole input.
fn is_json_number(raw: &[u8]) -> bool {
    let mut rest = raw;
    let mut eat = |pred: &dyn Fn(u8) -> bool| match rest.split_first() {
        Some((b, tail)) if pred(*b) => {
            rest = tail;
            true
        }
        _ => false,
    };
    let digit = |b: u8| b.is_ascii_digit();
    eat(&|b| b == b'-');
    if eat(&|b| b == b'0') {
    } else if eat(&|b| (b'1'..=b'9').contains(&b)) {
        while eat(&digit) {}
    } else {
        return false;
    }
    if eat(&|b| b == b'.') {
        if !eat(&digit) {
            return false;
        }
        while eat(&digit) {}
    }
    if eat(&|b| b == b'e' || b == b'E') {
        eat(&|b| b == b'+' || b == b'-');
        if !eat(&digit) {
            return false;
        }
        while eat(&digit) {}
    }
    rest.is_empty()
}
