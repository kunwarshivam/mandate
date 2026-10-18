//! The key made at start and the tokens it signs (DEC-2100 items 4, 6 and 7).

use std::fmt::Write as _;
use std::net::SocketAddr;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::{SecureRandom as _, SystemRandom};
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair as _};

/// How long a token is valid, in seconds.
pub const TOKEN_TTL_S: i64 = 300;

/// The longest subject accepted, in bytes (DEC-992 item 2).
pub const MAX_SUBJECT_BYTES: usize = 1024;

/// The bytes of a `kid` before it is written in hex.
const KID_BYTES: usize = 16;

/// Why a key could not be made or a token signed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueError {
    /// The subject is empty, holds a control character, or is over [`MAX_SUBJECT_BYTES`].
    Subject,
    /// The operating system's random source or the signer failed, or the expiry is out of range.
    Key,
}

/// One issuer: its exact issuer string, its audience, its key's id, and its key set. The key
/// itself is never readable, and `Issuer` has no `Debug` form that could print it.
pub struct Issuer {
    /// `http://<bind>`, exactly.
    pub issuer: String,
    /// The `aud` of every token.
    pub audience: String,
    /// The key's id, 16 random bytes in lowercase hex.
    pub kid: String,
    /// The key set, one EC P-256 key, as `GET /jwks.json` answers it.
    pub jwks: String,
    /// The bind as text, the one `Host` the router answers.
    pub(crate) host: String,
    /// The private key, made here and held in memory only.
    key: EcdsaKeyPair,
    /// The random source ECDSA signing draws its nonces from.
    random: SystemRandom,
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
        let mut id = [0_u8; KID_BYTES];
        random.fill(&mut id).map_err(|_| IssueError::Key)?;
        let kid = id.iter().fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        });
        let (x, y) = coordinates(key.public_key().as_ref()).ok_or(IssueError::Key)?;
        let jwks = serde_json::json!({
            "keys": [{
                "kty": "EC",
                "crv": "P-256",
                "x": URL_SAFE_NO_PAD.encode(x),
                "y": URL_SAFE_NO_PAD.encode(y),
                "kid": kid,
                "alg": "ES256",
                "use": "sig",
            }]
        })
        .to_string();
        Ok(Self {
            issuer: format!("http://{bind}"),
            audience: audience.to_owned(),
            kid,
            jwks,
            host: bind.to_string(),
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
        let expires = now_s.checked_add(TOKEN_TTL_S).ok_or(IssueError::Key)?;
        let header = serde_json::json!({"alg": "ES256", "typ": "JWT", "kid": self.kid});
        let claims = serde_json::json!({
            "iss": self.issuer,
            "sub": subject,
            "aud": self.audience,
            "iat": now_s,
            "nbf": now_s,
            "exp": expires,
        });
        let signed = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let signature = self
            .key
            .sign(&self.random, signed.as_bytes())
            .map_err(|_| IssueError::Key)?;
        Ok(format!(
            "{signed}.{}",
            URL_SAFE_NO_PAD.encode(signature.as_ref())
        ))
    }
}

/// The `x` and `y` of the uncompressed P-256 point `04 || x || y` that `ring` gives.
fn coordinates(point: &[u8]) -> Option<(&[u8], &[u8])> {
    point.get(1..)?.split_at_checked(32)
}
