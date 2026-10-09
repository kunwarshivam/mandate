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
//! Canonical JSON for everything that gets hashed ([journal spec §4](../../../docs/specs/journal.md#4-canonical-serialization),
//! ADR-0001 ES-07): a strict parser, a value tree sorted by key bytes, a writer, the journal decimal
//! grammar, SHA-256, and the canonical ULID text. `serde_json` never touches bytes that get hashed.

use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::fmt;

use sha2::Digest as _;

mod dec;
mod parse;
mod ulid;
mod write;

pub use ulid::{UlidTextError, decode_ulid, encode_ulid, is_ulid};

/// Largest integer the canonical form admits (journal spec §4.4).
pub const MAX_INT: u64 = (1 << 53) - 1;
/// Deepest nesting of arrays and objects the parser accepts.
pub const MAX_DEPTH: usize = 128;

/// A canonical JSON value. There is no floating-point variant: floats are rejected at ingest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(Int),
    Str(String),
    Array(Vec<Value>),
    Object(Object),
}

/// Members sorted by key bytes, which is the canonical order.
pub type Object = BTreeMap<Key, Value>;

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<u64> {
        match self {
            Self::Int(i) => Some(i.get()),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Self::Object(o) => Some(o),
            _ => None,
        }
    }

    /// Member `key` of an object; `None` for other values or a missing key.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object().and_then(|o| o.get(key))
    }
}

/// An object key matching `^[a-z][a-z0-9_]{0,63}$` (journal spec §4.1), so byte, code-point, and
/// UTF-16 orders agree.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key(String);

impl Key {
    pub fn new(key: &str) -> Result<Self, InvalidKey> {
        let mut bytes = key.bytes();
        let first_ok = bytes.next().is_some_and(|b| b.is_ascii_lowercase());
        let rest_ok = bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if first_ok && rest_ok && key.len() <= 64 {
            Ok(Self(key.to_owned()))
        } else {
            Err(InvalidKey)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for Key {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("object keys must match ^[a-z][a-z0-9_]{{0,63}}$")]
pub struct InvalidKey;

/// An integer in `0 ..= 2^53 − 1` (journal spec §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Int(u64);

impl Int {
    pub fn new(n: u64) -> Option<Self> {
        (n <= MAX_INT).then_some(Self(n))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

/// Parses JSON text strictly: duplicate keys, floats, integers outside `0 ..= 2^53 − 1`, keys
/// outside the key grammar, lone surrogates, invalid UTF-8, and trailing data are all rejected.
/// Insignificant whitespace is accepted; use [`to_canonical`] to compare forms.
pub fn parse(input: &[u8]) -> Result<Value, ParseError> {
    parse::parse(input)
}

/// The canonical bytes of `value` (journal spec §4, RFC 8785).
pub fn to_canonical(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    write::write(value, &mut out);
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{kind} at byte {offset}")]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseErrorKind {
    #[error("not valid JSON")]
    Syntax,
    #[error("floating-point number")]
    Float,
    #[error("integer outside 0 ..= 2^53 - 1")]
    IntegerRange,
    #[error("duplicate object key")]
    DuplicateKey,
    #[error("object key outside ^[a-z][a-z0-9_]{{0,63}}$")]
    InvalidKey,
    #[error("lone UTF-16 surrogate")]
    LoneSurrogate,
    #[error("invalid UTF-8")]
    InvalidUtf8,
    #[error("nesting deeper than MAX_DEPTH")]
    TooDeep,
    #[error("data after the value")]
    TrailingData,
}

impl ParseErrorKind {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Float => "float",
            Self::IntegerRange => "integer_range",
            Self::DuplicateKey => "duplicate_key",
            Self::InvalidKey => "invalid_key",
            Self::LoneSurrogate => "lone_surrogate",
            Self::InvalidUtf8 => "invalid_utf8",
            Self::TooDeep => "too_deep",
            Self::TrailingData => "trailing_data",
        }
    }
}

/// A decimal in the journal grammar (journal spec §4.6): `^-?(0|[1-9][0-9]*)(\.[0-9]*[1-9])?$`,
/// not `-0`, at most 28 fractional digits, absolute value below 7.9 × 10²⁸. This is a text-level
/// type: converting to an arithmetic type is exact or an error (ADR-0001 ES-04).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecStr(String);

impl DecStr {
    /// Normalizes `input` (exponent removed, leading and trailing zeros and a trailing point
    /// removed, `-0` → `0`). Values outside the bounds or not parseable are rejected, never rounded.
    pub fn parse(input: &str) -> Result<Self, DecError> {
        dec::normalize(input).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DecStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DecError {
    #[error("not a decimal number")]
    Syntax,
    #[error("more than 28 fractional digits, or absolute value not below 7.9e28")]
    OutOfRange,
}

impl DecError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::OutOfRange => "out_of_range",
        }
    }
}

/// A SHA-256 digest, written as 64 lowercase hex characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest([u8; 32]);

impl Digest {
    /// 64 zeros: the `prev_hash` of seq 1 (journal spec §3).
    pub const ZERO: Self = Self([0; 32]);

    pub fn of(bytes: &[u8]) -> Self {
        Self::of_parts(&[bytes])
    }

    /// The digest of the concatenation of `parts`.
    pub fn of_parts(parts: &[&[u8]]) -> Self {
        let mut hasher = sha2::Sha256::new();
        for part in parts {
            hasher.update(part);
        }
        Self(hasher.finalize().into())
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Accepts exactly 64 lowercase hex characters.
    pub fn from_hex(hex: &str) -> Option<Self> {
        if hex.len() != 64 {
            return None;
        }
        let mut out = [0u8; 32];
        let (pairs, _) = hex.as_bytes().as_chunks::<2>();
        for (byte, [hi, lo]) in out.iter_mut().zip(pairs) {
            *byte = hex_value(*hi)?
                .checked_mul(16)?
                .checked_add(hex_value(*lo)?)?;
        }
        Some(Self(out))
    }

    pub fn to_hex(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => c.checked_sub(b'0'),
        b'a'..=b'f' => c.checked_sub(b'a')?.checked_add(10),
        _ => None,
    }
}
