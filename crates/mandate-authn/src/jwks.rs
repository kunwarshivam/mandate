//! The issuer's key set, parsed from the JWKS document the caller fetched (RFC 7517).

use core::fmt;
use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::signature::{
    ECDSA_P256_SHA256_FIXED, ED25519, RSA_PKCS1_2048_8192_SHA256, RsaPublicKeyComponents,
    UnparsedPublicKey,
};
use serde::Deserialize;
use serde_json::Value;

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

impl Algorithm {
    /// The algorithm a header's `alg` names, compared case-sensitively (RFC 7515 §4.1.1).
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "ES256" => Some(Self::Es256),
            "RS256" => Some(Self::Rs256),
            "EdDSA" => Some(Self::EdDsa),
            _ => None,
        }
    }

    /// The `alg` name of this algorithm.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Es256 => "ES256",
            Self::Rs256 => "RS256",
            Self::EdDsa => "EdDSA",
        }
    }
}

/// The public half of one usable key, in the form ring verifies with.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PublicKey {
    /// The uncompressed P-256 point, `0x04 || x || y`.
    P256(Vec<u8>),
    Rsa {
        n: Vec<u8>,
        e: Vec<u8>,
    },
    Ed25519(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Key {
    public: PublicKey,
    alg: Option<String>,
}

/// Why a key's signature check did not pass.
pub(crate) enum KeyUse {
    /// The key is of another type than the algorithm, or names another `alg` itself.
    Mismatch,
    /// The signature does not verify.
    BadSignature,
}

impl Key {
    /// Verifies `signature` over `message` under `alg`, refusing a key of another type.
    pub(crate) fn verify(
        &self,
        alg: Algorithm,
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), KeyUse> {
        if self.alg.as_deref().is_some_and(|own| own != alg.name()) {
            return Err(KeyUse::Mismatch);
        }
        let verified =
            match (&self.public, alg) {
                (PublicKey::P256(point), Algorithm::Es256) => {
                    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, point)
                        .verify(message, signature)
                }
                (PublicKey::Rsa { n, e }, Algorithm::Rs256) => RsaPublicKeyComponents { n, e }
                    .verify(&RSA_PKCS1_2048_8192_SHA256, message, signature),
                (PublicKey::Ed25519(x), Algorithm::EdDsa) => {
                    UnparsedPublicKey::new(&ED25519, x).verify(message, signature)
                }
                _ => return Err(KeyUse::Mismatch),
            };
        verified.map_err(|_| KeyUse::BadSignature)
    }
}

/// The usable signing keys of one issuer, by `kid`.
///
/// Parsing keeps a key only when it has a non-empty `kid`, its `use` (if any) is `sig`, and its
/// type is EC on P-256, RSA, or OKP on Ed25519; any other key, a symmetric `oct` key included, is
/// left out and can never verify a token, and so is an RSA key whose modulus is under 2048 or over
/// 8192 bits.
/// A kept key with a missing or wrongly sized member fails the whole set, and so do two kept keys
/// with one `kid`, and a member the document or a key repeats. Its `Debug` lists the key ids only.
#[derive(Clone, PartialEq, Eq)]
pub struct Jwks {
    keys: BTreeMap<String, Key>,
}

impl fmt::Debug for Jwks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Jwks")
            .field("kids", &self.keys.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Deserialize)]
struct Document {
    keys: Vec<Jwk>,
}

/// The members of one JWK this parser reads; any other member is ignored. A duplicated member
/// fails the whole set, so no key reads differently to two parsers.
#[derive(Deserialize)]
struct Jwk {
    kid: Option<Value>,
    kty: Option<Value>,
    crv: Option<Value>,
    #[serde(rename = "use")]
    usage: Option<Value>,
    alg: Option<Value>,
    x: Option<Value>,
    y: Option<Value>,
    n: Option<Value>,
    e: Option<Value>,
}

impl Jwk {
    fn get(&self, member: &str) -> Option<&Value> {
        match member {
            "kid" => self.kid.as_ref(),
            "kty" => self.kty.as_ref(),
            "crv" => self.crv.as_ref(),
            "use" => self.usage.as_ref(),
            "alg" => self.alg.as_ref(),
            "x" => self.x.as_ref(),
            "y" => self.y.as_ref(),
            "n" => self.n.as_ref(),
            "e" => self.e.as_ref(),
            _ => None,
        }
    }
}

const P256_COORDINATE_BYTES: usize = 32;
const ED25519_KEY_BYTES: usize = 32;
const UNCOMPRESSED_POINT_TAG: u8 = 0x04;
/// The RSA moduli ring verifies under RS256, in bits; a key outside them is left out (DEC-650
/// item 5).
const RSA_MIN_BITS: usize = 2048;
const RSA_MAX_BITS: usize = 8192;

/// The number of significant bits in a big-endian unsigned integer.
fn bit_length(be: &[u8]) -> usize {
    let digits = be.iter().skip_while(|b| **b == 0).collect::<Vec<_>>();
    let Some(top) = digits.first() else {
        return 0;
    };
    let top_bits = usize::try_from(u8::BITS.saturating_sub(top.leading_zeros())).unwrap_or(0);
    digits
        .len()
        .saturating_sub(1)
        .saturating_mul(8)
        .saturating_add(top_bits)
}

impl Jwks {
    /// Parses a JWKS document.
    pub fn parse(json: &str) -> Result<Self, SetupError> {
        let document: Document =
            serde_json::from_str(json).map_err(|_| SetupError::JwksMalformed)?;
        let mut keys = BTreeMap::new();
        for jwk in &document.keys {
            let Some((kid, key)) = usable(jwk)? else {
                continue;
            };
            if keys.insert(kid, key).is_some() {
                return Err(SetupError::DuplicateKeyId);
            }
        }
        Ok(Self { keys })
    }

    pub(crate) fn key(&self, kid: &str) -> Option<&Key> {
        self.keys.get(kid)
    }
}

fn text<'a>(jwk: &'a Jwk, member: &str) -> Option<&'a str> {
    jwk.get(member).and_then(Value::as_str)
}

fn bytes(jwk: &Jwk, member: &str) -> Result<Vec<u8>, SetupError> {
    let encoded = text(jwk, member).ok_or(SetupError::KeyMalformed)?;
    URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| SetupError::KeyMalformed)
}

fn sized(jwk: &Jwk, member: &str, len: usize) -> Result<Vec<u8>, SetupError> {
    let value = bytes(jwk, member)?;
    if value.len() == len {
        Ok(value)
    } else {
        Err(SetupError::KeyMalformed)
    }
}

/// The key `jwk` describes with its `kid`, or `None` for a key no token may use.
fn usable(jwk: &Jwk) -> Result<Option<(String, Key)>, SetupError> {
    let Some(kid) = text(jwk, "kid").filter(|kid| !kid.is_empty()) else {
        return Ok(None);
    };
    if jwk.get("use").is_some_and(|u| u.as_str() != Some("sig")) {
        return Ok(None);
    }
    let public = match (text(jwk, "kty"), text(jwk, "crv")) {
        (Some("EC"), Some("P-256")) => {
            let mut point = vec![UNCOMPRESSED_POINT_TAG];
            point.extend(sized(jwk, "x", P256_COORDINATE_BYTES)?);
            point.extend(sized(jwk, "y", P256_COORDINATE_BYTES)?);
            PublicKey::P256(point)
        }
        (Some("RSA"), _) => {
            let n = bytes(jwk, "n")?;
            if !(RSA_MIN_BITS..=RSA_MAX_BITS).contains(&bit_length(&n)) {
                return Ok(None);
            }
            PublicKey::Rsa {
                n,
                e: bytes(jwk, "e")?,
            }
        }
        (Some("OKP"), Some("Ed25519")) => PublicKey::Ed25519(sized(jwk, "x", ED25519_KEY_BYTES)?),
        _ => return Ok(None),
    };
    let alg = match jwk.get("alg") {
        None => None,
        Some(Value::String(alg)) => Some(alg.clone()),
        Some(_) => return Err(SetupError::KeyMalformed),
    };
    Ok(Some((kid.to_owned(), Key { public, alg })))
}
