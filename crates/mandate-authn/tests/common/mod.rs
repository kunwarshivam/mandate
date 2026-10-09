//! The in-memory issuer the tests mint tokens from (identity spec §1.3, §14): keys made in the
//! test process, never a real identity provider's account.
//!
//! ring cannot generate RSA keys, so the RS256 key is the one fixed key, a PKCS#1 DER file made for
//! these tests with `openssl genpkey` (DEC-650 item 8); it signs nothing outside them.

#![allow(
    dead_code,
    reason = "each test binary that includes this module uses a different part of it"
)]

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use mandate_authn::{
    Algorithm, IssuerConfig, Jwks, Refusal, TokenKind, UtcNanos, VerifiedSubject, verify,
};
use ring::hmac;
use ring::rand::SystemRandom;
use ring::rsa::PublicKeyComponents;
use ring::signature::{
    ECDSA_P256_SHA256_ASN1_SIGNING, ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, Ed25519KeyPair,
    KeyPair as _, RSA_PKCS1_SHA256, RsaKeyPair,
};
use serde_json::{Value, json};

pub const ISSUER: &str = "https://tenant-a.auth.test/auth/v1";
pub const OTHER_TENANT: &str = "https://tenant-b.auth.test/auth/v1";
pub const AUDIENCE: &str = "authenticated";
pub const NONCE: &str = "canary-nonce-5d1f0c9e";
pub const SUBJECT: &str = "canary-subject-8a2b";
pub const EMAIL: &str = "invitee@example.test";
/// The test clock, as whole seconds since the Unix epoch.
pub const NOW_S: i64 = 1_800_000_000;
pub const LIFETIME_S: i64 = 300;

const RSA_TEST_KEY: &[u8] = include_bytes!("rs256-test-only.der");

/// Who signs a token, and the `alg` and `kid` its header names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signer {
    Es256,
    Rs256,
    EdDsa,
    /// A second P-256 key in the issuer's set under its own `kid`, so a verifier that ignores
    /// `kid` and tries the first key of the type is caught.
    Es256Second,
    /// A P-256 key outside the issuer's set that claims the ES256 key's `kid`.
    ImpostorEs256,
}

impl Signer {
    pub const ALLOWED: [Signer; 3] = [Signer::Es256, Signer::Rs256, Signer::EdDsa];

    pub fn alg(self) -> &'static str {
        match self {
            Signer::Es256 | Signer::Es256Second | Signer::ImpostorEs256 => "ES256",
            Signer::Rs256 => "RS256",
            Signer::EdDsa => "EdDSA",
        }
    }

    pub fn kid(self) -> &'static str {
        match self {
            Signer::Es256 | Signer::ImpostorEs256 => "es256-1",
            Signer::Es256Second => "es256-2",
            Signer::Rs256 => "rs256-1",
            Signer::EdDsa => "ed25519-1",
        }
    }
}

pub struct TestIssuer {
    es: EcdsaKeyPair,
    es_der: EcdsaKeyPair,
    es_second: EcdsaKeyPair,
    impostor: EcdsaKeyPair,
    rs: RsaKeyPair,
    ed: Ed25519KeyPair,
    rng: SystemRandom,
}

pub fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// A P-256 key pair signing the fixed `r || s` form, and the same key signing DER.
fn p256(rng: &SystemRandom) -> (EcdsaKeyPair, EcdsaKeyPair) {
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, rng).unwrap();
    let fixed = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), rng);
    let der = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), rng);
    (fixed.unwrap(), der.unwrap())
}

/// The DER `SubjectPublicKeyInfo` prefix of an uncompressed P-256 point (RFC 5480).
const P256_SPKI_PREFIX: [u8; 26] = [
    0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a,
    0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
];

impl TestIssuer {
    pub fn new() -> Self {
        let rng = SystemRandom::new();
        let (es, es_der) = p256(&rng);
        TestIssuer {
            es,
            es_der,
            es_second: p256(&rng).0,
            impostor: p256(&rng).0,
            rs: RsaKeyPair::from_der(RSA_TEST_KEY).unwrap(),
            ed: Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap(),
            rng,
        }
    }

    /// The issuer's key set as JSON members, one per allowed signer.
    pub fn jwk(&self, signer: Signer) -> Value {
        match signer {
            Signer::Es256 | Signer::Es256Second | Signer::ImpostorEs256 => {
                let pair = if signer == Signer::Es256Second {
                    &self.es_second
                } else {
                    &self.es
                };
                let point = pair.public_key().as_ref();
                json!({"kty": "EC", "crv": "P-256", "kid": signer.kid(), "use": "sig",
                       "x": b64(&point[1..33]), "y": b64(&point[33..65])})
            }
            Signer::Rs256 => {
                let parts = PublicKeyComponents::<Vec<u8>>::from(self.rs.public());
                json!({"kty": "RSA", "kid": signer.kid(), "alg": "RS256",
                       "n": b64(&parts.n), "e": b64(&parts.e)})
            }
            Signer::EdDsa => json!({"kty": "OKP", "crv": "Ed25519", "kid": signer.kid(),
                                    "x": b64(self.ed.public_key().as_ref())}),
        }
    }

    pub fn jwks_json(&self) -> String {
        let signers = [
            Signer::Es256,
            Signer::Es256Second,
            Signer::Rs256,
            Signer::EdDsa,
        ];
        let keys: Vec<Value> = signers.iter().map(|s| self.jwk(*s)).collect();
        json!({ "keys": keys }).to_string()
    }

    pub fn jwks(&self) -> Jwks {
        Jwks::parse(&self.jwks_json()).unwrap()
    }

    /// Signs `header` and `payload`, both given as raw JSON text so a test can write duplicate
    /// members or odd types, with `signer`'s key whatever `alg` the header names.
    pub fn sign_raw(&self, signer: Signer, header: &str, payload: &str) -> String {
        let input = format!("{}.{}", b64(header.as_bytes()), b64(payload.as_bytes()));
        self.sign_input(signer, &input)
    }

    /// Signs the signing input `input` exactly as given, so a test can sign segment text that is
    /// not canonical base64url, and appends the signature.
    pub fn sign_input(&self, signer: Signer, input: &str) -> String {
        let sig = match signer {
            Signer::Es256 => self
                .es
                .sign(&self.rng, input.as_bytes())
                .unwrap()
                .as_ref()
                .to_vec(),
            Signer::Es256Second => self
                .es_second
                .sign(&self.rng, input.as_bytes())
                .unwrap()
                .as_ref()
                .to_vec(),
            Signer::ImpostorEs256 => self
                .impostor
                .sign(&self.rng, input.as_bytes())
                .unwrap()
                .as_ref()
                .to_vec(),
            Signer::Rs256 => {
                let mut out = vec![0; self.rs.public().modulus_len()];
                self.rs
                    .sign(&RSA_PKCS1_SHA256, &self.rng, input.as_bytes(), &mut out)
                    .unwrap();
                out
            }
            Signer::EdDsa => self.ed.sign(input.as_bytes()).as_ref().to_vec(),
        };
        format!("{input}.{}", b64(&sig))
    }

    /// An HS256 token keyed with `secret`: the algorithm-confusion attack when `secret` is a public
    /// key the verifier holds.
    pub fn sign_hs256(&self, header: &str, payload: &str, secret: &[u8]) -> String {
        let input = format!("{}.{}", b64(header.as_bytes()), b64(payload.as_bytes()));
        let tag = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, secret), input.as_bytes());
        format!("{input}.{}", b64(tag.as_ref()))
    }

    /// An ES256 token whose signature is the ES256 key's DER (ASN.1) form, not the fixed one.
    pub fn sign_es256_der(&self, header: &str, payload: &str) -> String {
        let input = format!("{}.{}", b64(header.as_bytes()), b64(payload.as_bytes()));
        let sig = self.es_der.sign(&self.rng, input.as_bytes()).unwrap();
        format!("{input}.{}", b64(sig.as_ref()))
    }

    /// Every encoding of the ES256 public key an attacker might try as an HMAC secret: the raw
    /// point, the DER `SubjectPublicKeyInfo`, the JWK's `x` and `y` text, and the whole JWKS.
    pub fn es256_public_encodings(&self) -> Vec<Vec<u8>> {
        let point = self.es.public_key().as_ref().to_vec();
        let spki = [P256_SPKI_PREFIX.as_slice(), &point].concat();
        let jwk = self.jwk(Signer::Es256);
        vec![
            point,
            spki,
            jwk["x"].as_str().unwrap().as_bytes().to_vec(),
            jwk["y"].as_str().unwrap().as_bytes().to_vec(),
            self.jwks_json().into_bytes(),
        ]
    }

    /// A token with the usual header for `signer` over `claims`.
    pub fn token(&self, signer: Signer, claims: &Value) -> String {
        let header = json!({"alg": signer.alg(), "kid": signer.kid(), "typ": "JWT"});
        self.sign_raw(signer, &header.to_string(), &claims.to_string())
    }
}

/// Claims every check accepts at [`now`] for an ID token with [`NONCE`].
pub fn claims() -> Value {
    json!({
        "iss": ISSUER, "sub": SUBJECT, "aud": AUDIENCE,
        "iat": NOW_S, "exp": NOW_S + LIFETIME_S, "nonce": NONCE,
        "email": EMAIL, "email_verified": true,
    })
}

/// `claims()` with each `key` set to its value, or removed when the value is null.
pub fn with_all(changes: &[(&str, Value)]) -> Value {
    let mut c = claims();
    let map = c.as_object_mut().unwrap();
    for (key, value) in changes {
        if value.is_null() {
            map.remove(*key);
        } else {
            map.insert((*key).to_owned(), value.clone());
        }
    }
    c
}

/// `claims()` with one change, as [`with_all`].
pub fn with(key: &str, value: Value) -> Value {
    with_all(&[(key, value)])
}

pub fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

pub fn now() -> UtcNanos {
    at(NOW_S)
}

/// An SSO issuer that may sign with each allowed algorithm.
pub fn config() -> IssuerConfig {
    let algorithms = [Algorithm::Es256, Algorithm::Rs256, Algorithm::EdDsa];
    IssuerConfig::new(ISSUER, &[AUDIENCE], &algorithms).unwrap()
}

/// The managed issuer's shape: ES256 only (DEC-820).
pub fn es256_only() -> IssuerConfig {
    IssuerConfig::new(ISSUER, &[AUDIENCE], &[Algorithm::Es256]).unwrap()
}

/// The base64url alphabet in index order, so `index ^ 1` flips a character's lowest bit.
pub const BASE64URL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// An ID token sign-in that issued [`NONCE`].
pub const ID: TokenKind<'static> = TokenKind::IdToken { nonce: NONCE };

/// `token` verified as an ID token from [`ISSUER`] at [`now`].
pub fn check(issuer: &TestIssuer, token: &str) -> Result<VerifiedSubject, Refusal> {
    verify(token, &config(), &issuer.jwks(), ID, now())
}

/// The refusal [`check`] gives.
pub fn refused(issuer: &TestIssuer, token: &str) -> Refusal {
    check(issuer, token).unwrap_err()
}

/// Decodes base64url text written by [`b64`].
pub fn unb64(text: &str) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(text).unwrap()
}
