//! The key made at start and the tokens it signs (DEC-2100 items 4, 6 and 7).

use std::net::SocketAddr;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use serde_json::json;

/// How long a token is valid, in seconds.
pub const TOKEN_TTL_S: i64 = 300;

/// The longest subject accepted, in bytes (DEC-992 item 2).
pub const MAX_SUBJECT_BYTES: usize = 1024;

/// Why a key could not be made or a token signed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueError {
    /// The subject is empty, holds a control character, or is over [`MAX_SUBJECT_BYTES`].
    Subject,
    /// The operating system's random source or the signer failed.
    Key,
}

/// One issuer: its exact issuer string, its audience, its key's id, and its key set. The key
/// itself is never readable: it is made from the operating system's random source here, held in
/// memory only, and never written or printed: `Issuer` has no `Debug` form (DEC-2100 item 4).
pub struct Issuer {
    /// `http://<bind>`, exactly.
    pub issuer: String,
    /// The `aud` of every token.
    pub audience: String,
    /// The key's id, 16 random bytes in lowercase hex.
    pub kid: String,
    /// The key set, one EC P-256 key, as `GET /jwks.json` answers it.
    pub jwks: String,
    key: EcdsaKeyPair,
    random: SystemRandom,
}

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

impl Issuer {
    /// A new key for the stub bound at `bind`, signing for `audience`.
    ///
    /// # Errors
    /// [`IssueError::Key`].
    pub fn new(bind: SocketAddr, audience: &str) -> Result<Self, IssueError> {
        let random = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &random)
            .map_err(|_| IssueError::Key)?;
        let key =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &random)
                .map_err(|_| IssueError::Key)?;
        let point = key.public_key().as_ref();
        let x = point.get(1..33).ok_or(IssueError::Key)?;
        let y = point.get(33..65).ok_or(IssueError::Key)?;
        let mut id = [0_u8; 16];
        random.fill(&mut id).map_err(|_| IssueError::Key)?;
        let kid: String = id.iter().map(|byte| format!("{byte:02x}")).collect();
        let jwks = json!({"keys": [{
            "kty": "EC",
            "crv": "P-256",
            "x": b64(x),
            "y": b64(y),
            "kid": kid,
            "alg": "ES256",
            "use": "sig",
        }]})
        .to_string();
        Ok(Self {
            issuer: format!("http://{bind}"),
            audience: audience.to_owned(),
            kid,
            jwks,
            key,
            random,
        })
    }

    /// An ES256 access token for `subject`, issued at `now_s`, Unix seconds.
    ///
    /// # Errors
    /// [`IssueError::Subject`] or [`IssueError::Key`].
    pub fn token(&self, subject: &str, now_s: i64) -> Result<String, IssueError> {
        if subject.is_empty()
            || subject.len() > MAX_SUBJECT_BYTES
            || subject.chars().any(char::is_control)
        {
            return Err(IssueError::Subject);
        }
        let exp = now_s.checked_add(TOKEN_TTL_S).ok_or(IssueError::Key)?;
        let header = json!({"alg": "ES256", "typ": "JWT", "kid": self.kid});
        let claims = json!({
            "iss": self.issuer,
            "sub": subject,
            "aud": self.audience,
            "iat": now_s,
            "nbf": now_s,
            "exp": exp,
        });
        let signed = format!(
            "{}.{}",
            b64(header.to_string().as_bytes()),
            b64(claims.to_string().as_bytes())
        );
        let signature = self
            .key
            .sign(&self.random, signed.as_bytes())
            .map_err(|_| IssueError::Key)?;
        Ok(format!("{signed}.{}", b64(signature.as_ref())))
    }
}
