//! The issuer's key set, parsed from the JWKS document the caller fetched (RFC 7517).

use crate::SetupError;

/// The signature algorithms a token may use. Every other `alg`, `none` and the HMAC family
/// included, is refused before a key is looked up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Algorithm {
    /// ECDSA over P-256 with SHA-256, the signature as the fixed 64-byte `r || s` (RFC 7518 §3.4).
    Es256,
    /// RSASSA-PKCS1-v1_5 with SHA-256, over a modulus of 2048 to 8192 bits.
    Rs256,
    /// Ed25519 (RFC 8037).
    EdDsa,
}

/// The usable signing keys of one issuer, by `kid`.
///
/// Parsing keeps a key only when it has a `kid`, its `use` (if any) is `sig`, and its type is EC
/// on P-256, RSA, or OKP on Ed25519; any other key, a symmetric `oct` key included, is left out and
/// can never verify a token. A kept key with a missing or wrongly sized member fails the whole set,
/// and so do two kept keys with one `kid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jwks {
    stub: (),
}

impl Jwks {
    /// Parses a JWKS document.
    pub fn parse(_json: &str) -> Result<Self, SetupError> {
        Err(SetupError::Unimplemented { story: "E9-1" })
    }
}
