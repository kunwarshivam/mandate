//! OIDC token verification against one configured issuer (identity spec §6.1, E9-1).

use core::fmt;

use crate::{Algorithm, Jwks, Refusal, SetupError, UtcNanos};

/// The clock skew allowed on each side of `exp` and `nbf`, in seconds. It is a constant, not a
/// setting, so no configuration can widen it.
pub const CLOCK_SKEW_S: i64 = 60;

/// The largest token [`verify`] reads, in bytes; a longer one is refused before it is decoded.
pub const MAX_TOKEN_BYTES: usize = 16_384;

/// The one issuer a workspace accepts tokens from, the audiences that name this workspace's
/// client, and the algorithms that issuer signs with: the managed Supabase issuer ES256 only
/// (DEC-820), another SSO issuer whichever of [`Algorithm`]'s three it uses (DEC-650 item 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuerConfig {
    stub: (),
}

impl IssuerConfig {
    /// Configures the issuer, compared byte for byte with `iss` (no trailing-slash or case
    /// folding), the audiences, at least one, none empty, and the algorithms, at least one.
    pub fn new(
        _issuer: &str,
        _audiences: &[&str],
        _algorithms: &[Algorithm],
    ) -> Result<Self, SetupError> {
        Err(SetupError::Unimplemented { story: "E9-1" })
    }
}

/// Which token [`verify`] is reading. Its `Debug` never prints the nonce (ID-9).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TokenKind<'a> {
    /// An ID token from the authorization code flow, whose `nonce` must equal the one this
    /// sign-in issued.
    IdToken { nonce: &'a str },
    /// The issuer's access token (Supabase Auth's, DEC-211), which carries no nonce.
    AccessToken,
}

impl fmt::Debug for TokenKind<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdToken { .. } => f.write_str("IdToken { nonce: .. }"),
            Self::AccessToken => f.write_str("AccessToken"),
        }
    }
}

/// The subject a token proved, constructed only by [`verify`]. Its `Debug` shows the issuer and
/// the expiry only, never the subject or the address, which belong in the personal-data vault
/// (identity spec §3.1).
#[derive(Clone, PartialEq, Eq)]
pub struct VerifiedSubject {
    stub: (),
}

#[expect(
    clippy::todo,
    reason = "formatting has no error to carry, so its stub is todo!(), the other form DEC-137 names"
)]
impl fmt::Debug for VerifiedSubject {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

#[expect(
    clippy::todo,
    reason = "a getter has no error to carry, so its stub is todo!(), the other form DEC-137 names"
)]
impl VerifiedSubject {
    /// The issuer, equal to the configured one.
    pub fn issuer(&self) -> &str {
        todo!()
    }

    /// The issuer's subject identifier (`sub`).
    pub fn subject(&self) -> &str {
        todo!()
    }

    /// When the token stops verifying, without the skew: `exp` as a time.
    pub fn expires_at(&self) -> UtcNanos {
        todo!()
    }

    /// Whether `email_verified` is the JSON value `true`; a string `"true"` is not.
    pub fn email_verified(&self) -> bool {
        todo!()
    }

    /// Admits this subject to an invitation sent to `invited`: the token's `email` must be
    /// verified and equal `invited` byte for byte.
    pub fn matches_invitation(&self, _invited: &str) -> Result<(), Refusal> {
        Err(Refusal::Unimplemented { story: "E9-1" })
    }
}

/// Verifies a compact JWS `token` from `config`'s issuer under `jwks` at `now`.
///
/// The checks run in this order, and the first that fails is the refusal: size and shape, the
/// header (`alg` allowed for this issuer, no `crit`, no key of its own in `jwk`, `jku`, `x5u`,
/// `x5c`, `x5t`, or `x5t#S256`), the key (`kid` found, of the algorithm's type), the signature,
/// then the claims (`iss`, `sub`, `aud`, `azp`, `exp`, `nbf`, and for an ID token `nonce`). No
/// claim is read before the signature verifies. An empty signature segment decodes to no bytes,
/// which never verify: it is [`Refusal::BadSignature`], not a malformed token.
pub fn verify(
    _token: &str,
    _config: &IssuerConfig,
    _jwks: &Jwks,
    _kind: TokenKind<'_>,
    _now: UtcNanos,
) -> Result<VerifiedSubject, Refusal> {
    Err(Refusal::Unimplemented { story: "E9-1" })
}
