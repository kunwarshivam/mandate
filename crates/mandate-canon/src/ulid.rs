//! The canonical text of a ULID, as journal spec §3 validates an `event_id`: 26 digits of
//! uppercase Crockford base 32, `0123456789ABCDEFGHJKMNPQRSTVWXYZ`, most significant first, the
//! first at most `7` so the value fits 128 bits. One codec for every crate that spells or checks an
//! id (#765); only the canonical spelling is accepted, so lowercase and the aliases `I`, `L`, `O`
//! and `U` are refused rather than read as `1`, `1`, `0` and nothing.

/// The digits in ascending value, so a digit's position is its value.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// Digits in a canonical text.
const DIGITS: usize = 26;
/// Bits one digit carries.
const BITS_PER_DIGIT: u32 = 5;
/// The base, 2^5.
const RADIX: u128 = 32;
/// The low five bits, one digit's worth.
const DIGIT_MASK: u128 = 0x1f;

/// Why a text is not a canonical ULID. When several reasons apply, the first in declaration order
/// is reported: the length is judged on characters, then every character against the alphabet,
/// then the first digit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UlidTextError {
    #[error("not 26 characters long")]
    Length,
    #[error("a character outside uppercase Crockford base32")]
    NotCrockford,
    #[error("first digit above 7, so the value does not fit 128 bits")]
    Overflow,
}

impl UlidTextError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::Length => "length",
            Self::NotCrockford => "not_crockford",
            Self::Overflow => "overflow",
        }
    }
}

/// The 26 uppercase Crockford base32 digits of `value`, most significant first. Every `u128` has
/// one, the first digit at most `7`.
pub fn encode_ulid(value: u128) -> String {
    (0..DIGITS)
        .rev()
        .filter_map(|place| digit_at(value, place))
        .collect()
}

/// The digit `place` positions from the least significant end of `value`; `None` only for a
/// place past the 26th, which [`encode_ulid`] never asks for.
fn digit_at(value: u128, place: usize) -> Option<char> {
    let shift = u32::try_from(place).ok()?.checked_mul(BITS_PER_DIGIT)?;
    let index = usize::try_from(value.checked_shr(shift)? & DIGIT_MASK).ok()?;
    ALPHABET.get(index).copied().map(char::from)
}

/// The value of one digit, or `None` for a character outside the canonical alphabet.
fn digit_value(ch: char) -> Option<u128> {
    ALPHABET
        .iter()
        .position(|&digit| char::from(digit) == ch)
        .and_then(|position| u128::try_from(position).ok())
}

/// The value of a canonical ULID text.
///
/// # Errors
/// [`UlidTextError`] when `text` is not exactly 26 characters of the uppercase Crockford alphabet
/// whose first is at most `7`. A first digit above `7` is exactly the case where reading the
/// digits most significant first overflows `u128`, since 32^25 is 2^125, so the checked
/// arithmetic is the overflow check.
pub fn decode_ulid(text: &str) -> Result<u128, UlidTextError> {
    if text.chars().count() != DIGITS {
        return Err(UlidTextError::Length);
    }
    let digits = text
        .chars()
        .map(digit_value)
        .collect::<Option<Vec<u128>>>()
        .ok_or(UlidTextError::NotCrockford)?;
    digits
        .into_iter()
        .try_fold(0_u128, |acc, digit| {
            acc.checked_mul(RADIX)?.checked_add(digit)
        })
        .ok_or(UlidTextError::Overflow)
}

/// Whether `text` is a canonical ULID, that is, whether [`decode_ulid`] accepts it.
pub fn is_ulid(text: &str) -> bool {
    decode_ulid(text).is_ok()
}
