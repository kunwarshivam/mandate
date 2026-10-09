//! The canonical text of a ULID, as journal spec §3 validates an `event_id`: 26 digits of
//! uppercase Crockford base 32, `0123456789ABCDEFGHJKMNPQRSTVWXYZ`, most significant first, the
//! first at most `7` so the value fits 128 bits. One codec for every crate that spells or checks an
//! id (#765); only the canonical spelling is accepted, so lowercase and the aliases `I`, `L`, `O`
//! and `U` are refused rather than read as `1`, `1`, `0` and nothing.

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
#[expect(
    clippy::todo,
    reason = "an infallible encoder has no error to carry, so its stub is todo!(), the other form DEC-137 names"
)]
pub fn encode_ulid(value: u128) -> String {
    let _ = value;
    todo!()
}

/// The value of a canonical ULID text.
///
/// # Errors
/// [`UlidTextError`] when `text` is not exactly 26 characters of the uppercase Crockford alphabet
/// whose first is at most `7`.
#[expect(
    clippy::todo,
    reason = "mandate-canon has no Unimplemented error convention, so its stub is todo!(), the other form DEC-137 names"
)]
pub fn decode_ulid(text: &str) -> Result<u128, UlidTextError> {
    let _ = text;
    todo!()
}

/// Whether `text` is a canonical ULID, that is, whether [`decode_ulid`] accepts it.
#[expect(
    clippy::todo,
    reason = "a predicate has no error to carry, so its stub is todo!(), the other form DEC-137 names"
)]
pub fn is_ulid(text: &str) -> bool {
    let _ = text;
    todo!()
}
