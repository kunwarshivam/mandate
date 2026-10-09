//! OIDC token verification against one configured issuer (identity spec §6.1, E9-1).

use core::fmt;
use std::collections::BTreeSet;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::jwks::KeyUse;
use crate::{Algorithm, Jwks, Refusal, SetupError, TokenPart, UtcNanos};

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
    issuer: String,
    audiences: BTreeSet<String>,
    algorithms: BTreeSet<Algorithm>,
}

impl IssuerConfig {
    /// Configures the issuer, compared byte for byte with `iss` (no trailing-slash or case
    /// folding), the audiences, at least one, none empty, and the algorithms, at least one.
    pub fn new(
        issuer: &str,
        audiences: &[&str],
        algorithms: &[Algorithm],
    ) -> Result<Self, SetupError> {
        if issuer.is_empty() {
            return Err(SetupError::EmptyIssuer);
        }
        if audiences.is_empty() || audiences.iter().any(|a| a.is_empty()) {
            return Err(SetupError::EmptyAudience);
        }
        if algorithms.is_empty() {
            return Err(SetupError::NoAlgorithm);
        }
        Ok(Self {
            issuer: issuer.to_owned(),
            audiences: audiences.iter().map(|a| (*a).to_owned()).collect(),
            algorithms: algorithms.iter().copied().collect(),
        })
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
    issuer: String,
    subject: String,
    expires_at: UtcNanos,
    email: Option<String>,
    email_verified: bool,
}

impl fmt::Debug for VerifiedSubject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifiedSubject")
            .field("issuer", &self.issuer)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl VerifiedSubject {
    /// The issuer, equal to the configured one.
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// The issuer's subject identifier (`sub`).
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// When the token stops verifying, without the skew: `exp` as a time.
    pub fn expires_at(&self) -> UtcNanos {
        self.expires_at
    }

    /// Whether `email_verified` is the JSON value `true`; a string `"true"` is not.
    pub fn email_verified(&self) -> bool {
        self.email_verified
    }

    /// Admits this subject to an invitation sent to `invited`: the token's `email` must be
    /// verified and equal `invited` byte for byte.
    pub fn matches_invitation(&self, invited: &str) -> Result<(), Refusal> {
        if !self.email_verified {
            return Err(Refusal::EmailNotVerified);
        }
        if self.email.as_deref() != Some(invited) {
            return Err(Refusal::EmailMismatch);
        }
        Ok(())
    }
}

/// The protected header's members this verifier reads. A duplicated member fails parsing.
#[derive(Deserialize)]
struct Header {
    alg: Option<String>,
    kid: Option<String>,
    crit: Option<Value>,
    jwk: Option<Value>,
    jku: Option<Value>,
    x5u: Option<Value>,
    x5c: Option<Value>,
    x5t: Option<Value>,
    #[serde(rename = "x5t#S256")]
    x5t_s256: Option<Value>,
}

impl Header {
    /// Whether the header carries a key, or a pointer to one, of its own.
    fn names_a_key(&self) -> bool {
        [
            &self.jwk,
            &self.jku,
            &self.x5u,
            &self.x5c,
            &self.x5t,
            &self.x5t_s256,
        ]
        .iter()
        .any(|member| member.is_some())
    }
}

/// The claims this verifier reads; every other claim is ignored. A duplicated claim, or one of
/// another JSON type (a fractional `exp`, a numeric `iss`), fails parsing.
#[derive(Deserialize)]
struct Claims {
    iss: Option<String>,
    sub: Option<String>,
    aud: Option<Audience>,
    azp: Option<String>,
    exp: Option<i64>,
    nbf: Option<i64>,
    nonce: Option<String>,
    email: Option<String>,
    email_verified: Option<Value>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

/// Decodes one base64url segment holding a JSON object.
fn object<T: DeserializeOwned>(segment: &str, part: TokenPart) -> Result<T, Refusal> {
    let malformed = Refusal::Malformed { part };
    let json = URL_SAFE_NO_PAD
        .decode(segment)
        .map_err(|_| malformed.clone())?;
    if json.first() != Some(&b'{') {
        return Err(malformed);
    }
    serde_json::from_slice(&json).map_err(|_| malformed)
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
    token: &str,
    config: &IssuerConfig,
    jwks: &Jwks,
    kind: TokenKind<'_>,
    now: UtcNanos,
) -> Result<VerifiedSubject, Refusal> {
    let shape = Refusal::Malformed {
        part: TokenPart::Token,
    };
    if token.len() > MAX_TOKEN_BYTES {
        return Err(shape);
    }
    let (signed, signature) = token.rsplit_once('.').ok_or(shape.clone())?;
    let (header, payload) = signed.split_once('.').ok_or(shape.clone())?;
    if payload.contains('.') {
        return Err(shape);
    }
    let header: Header = object(header, TokenPart::Header)?;
    let alg = header
        .alg
        .as_deref()
        .and_then(Algorithm::from_name)
        .filter(|alg| config.algorithms.contains(alg))
        .ok_or(Refusal::AlgorithmNotAllowed)?;
    if header.crit.is_some() {
        return Err(Refusal::CriticalHeader);
    }
    if header.names_a_key() {
        return Err(Refusal::HeaderKey);
    }
    let key = header
        .kid
        .as_deref()
        .and_then(|kid| jwks.key(kid))
        .ok_or(Refusal::KeyNotFound)?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| Refusal::Malformed {
            part: TokenPart::Signature,
        })?;
    key.verify(alg, signed.as_bytes(), &signature)
        .map_err(|e| match e {
            KeyUse::Mismatch => Refusal::KeyAlgorithmMismatch,
            KeyUse::BadSignature => Refusal::BadSignature,
        })?;
    let claims: Claims = object(payload, TokenPart::Payload)?;
    subject(claims, config, kind, now)
}

/// The claim checks, run once the signature has verified.
fn subject(
    claims: Claims,
    config: &IssuerConfig,
    kind: TokenKind<'_>,
    now: UtcNanos,
) -> Result<VerifiedSubject, Refusal> {
    let issuer = claims.iss.ok_or(Refusal::MissingClaim { claim: "iss" })?;
    if issuer != config.issuer {
        return Err(Refusal::WrongIssuer);
    }
    let subject = claims
        .sub
        .filter(|s| !s.is_empty())
        .ok_or(Refusal::MissingClaim { claim: "sub" })?;
    let audiences = match claims.aud.ok_or(Refusal::MissingClaim { claim: "aud" })? {
        Audience::One(one) => vec![one],
        Audience::Many(many) => many,
    };
    if !audiences.iter().any(|a| config.audiences.contains(a)) {
        return Err(Refusal::WrongAudience);
    }
    if audiences.len() > 1
        && !claims
            .azp
            .as_ref()
            .is_some_and(|azp| config.audiences.contains(azp))
    {
        return Err(Refusal::WrongAuthorizedParty);
    }
    let payload = Refusal::Malformed {
        part: TokenPart::Payload,
    };
    let exp = claims.exp.ok_or(Refusal::MissingClaim { claim: "exp" })?;
    let expires_at = UtcNanos::from_parts(exp, 0).map_err(|_| payload.clone())?;
    let not_before = claims
        .nbf
        .map(|nbf| UtcNanos::from_parts(nbf, 0))
        .transpose()
        .map_err(|_| payload)?;
    if now.secs().saturating_sub(CLOCK_SKEW_S) >= expires_at.secs() {
        return Err(Refusal::Expired);
    }
    if let Some(not_before) = not_before
        && now.secs().saturating_add(CLOCK_SKEW_S) < not_before.secs()
    {
        return Err(Refusal::NotYetValid);
    }
    if let TokenKind::IdToken { nonce } = kind
        && (nonce.is_empty() || claims.nonce.as_deref() != Some(nonce))
    {
        return Err(Refusal::NonceMismatch);
    }
    Ok(VerifiedSubject {
        issuer,
        subject,
        expires_at,
        email: claims.email,
        email_verified: matches!(claims.email_verified, Some(Value::Bool(true))),
    })
}
