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
//! Web push for opaque notices ([notifications spec §4.6](../../../docs/specs/notifications.md),
//! backlog E8-14, slice S8a, DEC-438 item 13, DEC-790).
//!
//! **It sends nothing.** It turns one opaque notice into the parts of one push request: the body,
//! encrypted to the subscribing browser with RFC 8291 and the `aes128gcm` coding of RFC 8188; the
//! RFC 8292 VAPID `Authorization` header; and the `TTL` and `Urgency` fixed per class. A channel
//! adapter or the relay (S8b) does the I/O.
//!
//! **Only the closed payload is encrypted** (NT-1, `AGENTS.md` rule 6). [`PushPlaintext`] is
//! `{"notice","text"}` and nothing else, and its constructor is private to this crate. The
//! dispatcher's `Notification` (slice S1, `mandate-notify`) is the one way in once it merges.
//!
//! **Every body is the same size.** The plaintext is padded to [`PADDED_RECORD_LEN`] octets, so a
//! push service learns not even which text key a notice carries (DEC-790 item 2), and no body can
//! exceed the relay's cap, [`MAX_BODY_LEN`].
//!
//! **No key, clock, or randomness of its own.** The application server's key stays behind
//! [`VapidSigner`], the ephemeral key and salt come from [`SecureRandom`], and the time is an
//! argument. It depends on no workspace crate, so it cannot reach the journal or the control
//! stream (NT-3).

mod payload;

use aes_gcm::Aes128Gcm;
use aes_gcm::aead::{Aead, KeyInit};
use base64ct::{Base64UrlUnpadded, Encoding};
use hkdf::Hkdf;
use p256::elliptic_curve::sec1::ToSec1Point;
use p256::{PublicKey, SecretKey};
use sha2::Sha256;

pub use payload::{PushPlaintext, PushText};

/// The relay's cap on a ciphertext (spec §4.6). A body over it is refused, never sent.
pub const MAX_BODY_LEN: usize = 512;

/// The octets every record holds: the plaintext, its delimiter `0x02`, then zeros (DEC-790 item 2).
pub const PADDED_RECORD_LEN: usize = 128;

/// The record size written in the header, RFC 8291 §4's 4096.
const RECORD_SIZE: u32 = 4096;

/// How far after `now` the VAPID token expires: 12 hours, inside RFC 8292's 24 (DEC-790 item 4).
pub const VAPID_LIFETIME_S: u64 = 43_200;

const KEY_INFO: &[u8] = b"WebPush: info\0";
const CEK_INFO: &[u8] = b"Content-Encoding: aes128gcm\0";
const NONCE_INFO: &[u8] = b"Content-Encoding: nonce\0";
const JWT_HEADER: &str = r#"{"typ":"JWT","alg":"ES256"}"#;

/// Why no request could be built. `code()` is stable and carries no input.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WebPushError {
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    #[error("the push endpoint is not an https URL with a host")]
    InvalidEndpoint,
    #[error("the subscription's p256dh key is not an uncompressed P-256 point")]
    InvalidKey,
    #[error("the subscription's auth secret is not 16 octets")]
    InvalidAuthSecret,
    #[error("the VAPID subject is not a mailto: or https: URI")]
    InvalidSubject,
    #[error("the secure random source failed or gave no valid key")]
    Random,
    #[error("the application server key could not sign")]
    Signer,
    #[error("the ciphertext is over the relay's cap")]
    TooLarge,
    #[error("the time is out of range")]
    Clock,
}

impl WebPushError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::InvalidEndpoint => "invalid_endpoint",
            Self::InvalidKey => "invalid_key",
            Self::InvalidAuthSecret => "invalid_auth_secret",
            Self::InvalidSubject => "invalid_subject",
            Self::Random => "random",
            Self::Signer => "signer",
            Self::TooLarge => "too_large",
            Self::Clock => "clock",
        }
    }
}

/// The operating system's secure random source, injected so tests are deterministic.
pub trait SecureRandom {
    /// Fills `out` entirely, or fails.
    fn fill(&mut self, out: &mut [u8]) -> Result<(), WebPushError>;
}

/// The application server's signing key, held elsewhere (the vault). This crate never sees it.
pub trait VapidSigner {
    /// The key's public half, an uncompressed SEC1 point.
    fn public_key(&self) -> [u8; 65];
    /// An ES256 signature, `r || s`, over `message`.
    fn sign_es256(&self, message: &[u8]) -> Result<[u8; 64], WebPushError>;
}

/// The notice's class, which alone fixes the envelope (DEC-700 item 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeClass {
    Action,
    Safety,
    Info,
}

/// RFC 8030 §5.3 urgency; only the two the relay accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    High,
    Normal,
}

impl Urgency {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Normal => "normal",
        }
    }
}

impl NoticeClass {
    /// `action` and `safety` are `high`; `info` is `normal` (spec §4.6).
    #[must_use]
    pub fn urgency(self) -> Urgency {
        match self {
            Self::Action | Self::Safety => Urgency::High,
            Self::Info => Urgency::Normal,
        }
    }

    /// Fixed per class and never from a deadline (DEC-700 item 3).
    #[must_use]
    pub fn ttl_s(self) -> u32 {
        match self {
            Self::Action => 3_600,
            Self::Safety => 86_400,
            Self::Info => 21_600,
        }
    }
}

/// A push endpoint: `https://<host>[:port]/...`, with no user information. An address (NT-2).
#[derive(Clone, PartialEq, Eq)]
pub struct PushEndpoint {
    url: String,
    origin_len: usize,
}

impl std::fmt::Debug for PushEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PushEndpoint(..)")
    }
}

impl PushEndpoint {
    pub fn parse(url: &str) -> Result<Self, WebPushError> {
        let rest = url
            .strip_prefix("https://")
            .ok_or(WebPushError::InvalidEndpoint)?;
        let host = rest.split('/').next().unwrap_or_default();
        let plain = url
            .bytes()
            .all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\');
        if host.is_empty() || host.contains('@') || !plain {
            return Err(WebPushError::InvalidEndpoint);
        }
        Ok(Self {
            url: url.to_owned(),
            origin_len: "https://".len().saturating_add(host.len()),
        })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.url
    }

    /// `https://<host>[:port]`, the VAPID audience.
    #[must_use]
    pub fn origin(&self) -> &str {
        self.url.get(..self.origin_len).unwrap_or_default()
    }
}

/// A browser's push subscription: where to send, and the keys to encrypt to.
#[derive(Clone)]
pub struct Subscription {
    endpoint: PushEndpoint,
    p256dh: PublicKey,
    auth: [u8; 16],
}

impl Subscription {
    /// From the subscription's endpoint and its decoded `p256dh` and `auth` keys.
    pub fn new(endpoint: PushEndpoint, p256dh: &[u8], auth: &[u8]) -> Result<Self, WebPushError> {
        if p256dh.len() != 65 {
            return Err(WebPushError::InvalidKey);
        }
        let p256dh = PublicKey::from_sec1_bytes(p256dh).map_err(|_| WebPushError::InvalidKey)?;
        let auth = auth
            .try_into()
            .map_err(|_| WebPushError::InvalidAuthSecret)?;
        Ok(Self {
            endpoint,
            p256dh,
            auth,
        })
    }

    #[must_use]
    pub fn endpoint(&self) -> &PushEndpoint {
        &self.endpoint
    }
}

/// The VAPID `sub` claim: one contact URI for the deployment, the same on every notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VapidSubject(String);

impl VapidSubject {
    pub fn parse(uri: &str) -> Result<Self, WebPushError> {
        let rest = uri
            .strip_prefix("mailto:")
            .or_else(|| uri.strip_prefix("https://"))
            .ok_or(WebPushError::InvalidSubject)?;
        let plain = uri
            .bytes()
            .all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\');
        if rest.is_empty() || !plain {
            return Err(WebPushError::InvalidSubject);
        }
        Ok(Self(uri.to_owned()))
    }
}

/// The parts of one push request (RFC 8030 §5, RFC 8291 §4, RFC 8292 §3).
#[derive(Clone, PartialEq, Eq)]
pub struct PushRequest {
    pub endpoint: String,
    pub ttl_s: u32,
    pub urgency: Urgency,
    pub authorization: String,
    pub body: Vec<u8>,
}

impl PushRequest {
    pub const CONTENT_ENCODING: &'static str = "aes128gcm";
}

/// Encrypts `plaintext` to `subscription`, signs, and fixes the envelope by `class`.
pub fn build_request(
    subscription: &Subscription,
    plaintext: &PushPlaintext,
    class: NoticeClass,
    signer: &dyn VapidSigner,
    subject: &VapidSubject,
    now_unix_s: u64,
    random: &mut dyn SecureRandom,
) -> Result<PushRequest, WebPushError> {
    let body = encrypt(subscription, plaintext, random)?;
    let authorization = vapid_authorization(subscription.endpoint(), subject, signer, now_unix_s)?;
    Ok(PushRequest {
        endpoint: subscription.endpoint().as_str().to_owned(),
        ttl_s: class.ttl_s(),
        urgency: class.urgency(),
        authorization,
        body,
    })
}

/// The `aes128gcm` body for `plaintext`, padded to [`PADDED_RECORD_LEN`].
pub fn encrypt(
    subscription: &Subscription,
    plaintext: &PushPlaintext,
    random: &mut dyn SecureRandom,
) -> Result<Vec<u8>, WebPushError> {
    seal(
        subscription,
        plaintext.as_bytes(),
        Some(PADDED_RECORD_LEN),
        random,
    )
}

/// RFC 8291 §3: an ephemeral key, then a salt, both drawn in that order from `random`.
fn seal(
    subscription: &Subscription,
    plaintext: &[u8],
    pad_to: Option<usize>,
    random: &mut dyn SecureRandom,
) -> Result<Vec<u8>, WebPushError> {
    let mut scalar = [0u8; 32];
    random.fill(&mut scalar)?;
    let mut salt = [0u8; 16];
    random.fill(&mut salt)?;
    let ephemeral = SecretKey::from_slice(&scalar).map_err(|_| WebPushError::Random)?;
    let as_public = sec1(&ephemeral.public_key());
    let ua_public = sec1(&subscription.p256dh);
    let shared = p256::ecdh::diffie_hellman(
        ephemeral.to_nonzero_scalar(),
        subscription.p256dh.as_affine(),
    );
    let key_info = [KEY_INFO, &ua_public, &as_public].concat();
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&subscription.auth), shared.raw_secret_bytes())
        .expand(&key_info, &mut ikm)
        .map_err(|_| WebPushError::Random)?;
    aes128gcm(&ikm, &salt, &as_public, plaintext, pad_to)
}

/// RFC 8188 §2: one record, the last, so its delimiter is `0x02`.
fn aes128gcm(
    ikm: &[u8],
    salt: &[u8],
    keyid: &[u8],
    plaintext: &[u8],
    pad_to: Option<usize>,
) -> Result<Vec<u8>, WebPushError> {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut cek = [0u8; 16];
    let mut nonce = [0u8; 12];
    hk.expand(CEK_INFO, &mut cek)
        .map_err(|_| WebPushError::Random)?;
    hk.expand(NONCE_INFO, &mut nonce)
        .map_err(|_| WebPushError::Random)?;
    let mut record = plaintext.to_vec();
    record.push(2);
    if let Some(len) = pad_to {
        if record.len() > len {
            return Err(WebPushError::TooLarge);
        }
        record.resize(len, 0);
    }
    let sealed = Aes128Gcm::new_from_slice(&cek)
        .map_err(|_| WebPushError::Random)?
        .encrypt(&nonce.into(), record.as_slice())
        .map_err(|_| WebPushError::TooLarge)?;
    let idlen = u8::try_from(keyid.len()).map_err(|_| WebPushError::InvalidKey)?;
    let body = [salt, &RECORD_SIZE.to_be_bytes(), &[idlen], keyid, &sealed].concat();
    if body.len() > MAX_BODY_LEN {
        return Err(WebPushError::TooLarge);
    }
    Ok(body)
}

fn sec1(key: &PublicKey) -> Vec<u8> {
    key.to_sec1_point(false).as_bytes().to_vec()
}

/// `vapid t=<JWT>, k=<public key>` (RFC 8292 §3), with `aud` the endpoint's origin.
pub fn vapid_authorization(
    endpoint: &PushEndpoint,
    subject: &VapidSubject,
    signer: &dyn VapidSigner,
    now_unix_s: u64,
) -> Result<String, WebPushError> {
    let exp = now_unix_s
        .checked_add(VAPID_LIFETIME_S)
        .ok_or(WebPushError::Clock)?;
    let claims = format!(
        r#"{{"aud":"{}","exp":{exp},"sub":"{}"}}"#,
        endpoint.origin(),
        subject.0
    );
    let signing_input = format!(
        "{}.{}",
        Base64UrlUnpadded::encode_string(JWT_HEADER.as_bytes()),
        Base64UrlUnpadded::encode_string(claims.as_bytes())
    );
    let signature = signer.sign_es256(signing_input.as_bytes())?;
    Ok(format!(
        "vapid t={signing_input}.{}, k={}",
        Base64UrlUnpadded::encode_string(&signature),
        Base64UrlUnpadded::encode_string(&signer.public_key())
    ))
}

#[cfg(test)]
mod tests;
