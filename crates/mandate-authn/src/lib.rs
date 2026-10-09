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
//! Pure authentication for workspace services (backlog E9-1, [identity spec §6](../../../docs/specs/identity.md#6-authentication-and-sessions),
//! DEC-650).
//!
//! **It verifies; it fetches nothing.** [`verify`] checks an OIDC ID token or an issuer's access
//! token against the one issuer the workspace is configured for, using a key set ([`Jwks`]) the
//! caller fetched and handed in, at a time ([`UtcNanos`]) the caller read. No clock, no network,
//! no randomness: the same inputs always give the same answer (ES-21).
//!
//! **Only an allowed algorithm reaches a key.** The header's `alg` must be one of the issuer's
//! configured algorithms, drawn from ES256, RS256 and EdDSA ([`Algorithm`]), its `kid` must name a
//! key in the set, and that key must be of the algorithm's type; `none`, every HMAC algorithm, and
//! anything else are refused before a key is looked up, so a public key is never used as an HMAC
//! secret.
//!
//! **A refusal names the check, never the value** (ID-9). [`Refusal`] and [`SetupError`] carry
//! static text only: no token, nonce, claim, or key appears in their `Display` or `Debug`.

mod jwks;
mod oidc;

pub use jwks::{Algorithm, Jwks};
pub use mandate_time::UtcNanos;
pub use oidc::{CLOCK_SKEW_S, IssuerConfig, MAX_TOKEN_BYTES, TokenKind, VerifiedSubject, verify};

/// The part of a compact JWS a [`Refusal::Malformed`] points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenPart {
    /// The token as a whole: its size, or its count of `.`-separated segments.
    Token,
    /// The protected header: its base64url text or its JSON.
    Header,
    /// The claims: their base64url text, their JSON, a duplicated claim, or a claim of the wrong
    /// type (a fractional `exp`, a numeric `iss`).
    Payload,
    /// The signature's base64url text.
    Signature,
}

/// Every way [`verify`] or [`VerifiedSubject::matches_invitation`] refuses, one variant per check,
/// in the order [`verify`] runs them.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// The token is not a well-formed compact JWS within [`MAX_TOKEN_BYTES`].
    #[error("the token's {part:?} is malformed")]
    Malformed { part: TokenPart },
    /// The header's `alg` is not one of the issuer's configured algorithms, which are drawn from
    /// ES256, RS256, and EdDSA (`none` and HMAC never).
    #[error("the token's algorithm is not allowed")]
    AlgorithmNotAllowed,
    /// The header names a `crit` extension, which this verifier understands none of (RFC 7515
    /// §4.1.11).
    #[error("the token names a critical header extension")]
    CriticalHeader,
    /// The header carries a key or a pointer to one (`jwk`, `jku`, `x5u`, `x5c`, `x5t`,
    /// `x5t#S256`); only the issuer's supplied key set is ever used.
    #[error("the token's header names its own key")]
    HeaderKey,
    /// The header has no `kid`, or the key set has no usable key with it.
    #[error("the token's key is not in the issuer's key set")]
    KeyNotFound,
    /// The key the `kid` names is of another type than the algorithm, or its own `alg` differs.
    #[error("the token's key does not fit its algorithm")]
    KeyAlgorithmMismatch,
    /// The signature does not verify under the key: another key's, over other bytes, of the
    /// wrong form or length (an ES256 signature is the raw 64-byte `r || s`), or empty.
    #[error("the token's signature does not verify")]
    BadSignature,
    /// A required claim is absent.
    #[error("the token has no {claim} claim")]
    MissingClaim { claim: &'static str },
    /// `iss` is not byte for byte the configured issuer.
    #[error("the token is from another issuer")]
    WrongIssuer,
    /// `aud` names none of the configured audiences.
    #[error("the token is for another audience")]
    WrongAudience,
    /// `aud` names several audiences and `azp` is absent or not a configured audience (OIDC Core
    /// §3.1.3.7 items 4 and 5).
    #[error("the token's authorized party is not this client")]
    WrongAuthorizedParty,
    /// `now` is at or past `exp` plus [`CLOCK_SKEW_S`].
    #[error("the token has expired")]
    Expired,
    /// `now` is before `nbf` less [`CLOCK_SKEW_S`].
    #[error("the token is not valid yet")]
    NotYetValid,
    /// An ID token's `nonce` is absent or is not the one the sign-in issued.
    #[error("the token's nonce is not the expected one")]
    NonceMismatch,
    /// Matching an invitation needs `email_verified` to be the JSON value `true`.
    #[error("the token's email address is not verified")]
    EmailNotVerified,
    /// The verified address is not byte for byte the invited one.
    #[error("the token's email address is not the invited one")]
    EmailMismatch,
}

impl Refusal {
    /// The stable code an API response carries for this refusal (ES-09), which names the check
    /// and nothing the token held.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::Malformed { .. } => "token_malformed",
            Self::AlgorithmNotAllowed => "algorithm_not_allowed",
            Self::CriticalHeader => "critical_header",
            Self::HeaderKey => "header_key",
            Self::KeyNotFound => "key_not_found",
            Self::KeyAlgorithmMismatch => "key_algorithm_mismatch",
            Self::BadSignature => "bad_signature",
            Self::MissingClaim { .. } => "missing_claim",
            Self::WrongIssuer => "wrong_issuer",
            Self::WrongAudience => "wrong_audience",
            Self::WrongAuthorizedParty => "wrong_authorized_party",
            Self::Expired => "token_expired",
            Self::NotYetValid => "token_not_yet_valid",
            Self::NonceMismatch => "nonce_mismatch",
            Self::EmailNotVerified => "email_not_verified",
            Self::EmailMismatch => "email_mismatch",
        }
    }
}

/// Every way building an [`IssuerConfig`] or parsing a [`Jwks`] fails.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SetupError {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// The issuer is empty.
    #[error("the issuer is empty")]
    EmptyIssuer,
    /// No audience is configured, or one is empty.
    #[error("an audience is missing or empty")]
    EmptyAudience,
    /// No algorithm is configured for the issuer.
    #[error("the issuer has no algorithm")]
    NoAlgorithm,
    /// The key set is not a JSON object with a `keys` array.
    #[error("the key set is malformed")]
    JwksMalformed,
    /// A key of a supported type is missing a member or holds a value of the wrong length.
    #[error("a key in the key set is malformed")]
    KeyMalformed,
    /// Two usable keys share a `kid`.
    #[error("two keys in the key set share a key id")]
    DuplicateKeyId,
}
