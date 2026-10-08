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

use p256::PublicKey;

pub use payload::{PushPlaintext, PushText};

/// The relay's cap on a ciphertext (spec §4.6). A body over it is refused, never sent.
pub const MAX_BODY_LEN: usize = 512;

/// The octets every record holds: the plaintext, its delimiter `0x02`, then zeros (DEC-790 item 2).
pub const PADDED_RECORD_LEN: usize = 128;

/// How far after `now` the VAPID token expires: 12 hours, inside RFC 8292's 24 (DEC-790 item 4).
pub const VAPID_LIFETIME_S: u64 = 43_200;

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

/// Prints no part of the address (NT-2, DEC-790 item 6).
impl std::fmt::Debug for PushEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PushEndpoint(..)")
    }
}

impl PushEndpoint {
    pub fn parse(url: &str) -> Result<Self, WebPushError> {
        let _ = url;
        Err(WebPushError::Unimplemented { story: "E8-14" })
    }
}

/// A browser's push subscription: where to send, and the keys to encrypt to.
#[derive(Clone)]
#[expect(dead_code, reason = "E8-14 builds it")]
pub struct Subscription {
    endpoint: PushEndpoint,
    p256dh: PublicKey,
    auth: [u8; 16],
}

impl Subscription {
    /// From the subscription's endpoint and its decoded `p256dh` and `auth` keys.
    pub fn new(endpoint: PushEndpoint, p256dh: &[u8], auth: &[u8]) -> Result<Self, WebPushError> {
        let _ = (endpoint, p256dh, auth);
        Err(WebPushError::Unimplemented { story: "E8-14" })
    }
}

/// The VAPID `sub` claim: one contact URI for the deployment, the same on every notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VapidSubject(String);

impl VapidSubject {
    pub fn parse(uri: &str) -> Result<Self, WebPushError> {
        let _ = uri;
        Err(WebPushError::Unimplemented { story: "E8-14" })
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
    let _ = (
        subscription,
        plaintext,
        class,
        signer,
        subject,
        now_unix_s,
        random,
    );
    Err(WebPushError::Unimplemented { story: "E8-14" })
}

/// The `aes128gcm` body for `plaintext`, padded to [`PADDED_RECORD_LEN`].
pub fn encrypt(
    subscription: &Subscription,
    plaintext: &PushPlaintext,
    random: &mut dyn SecureRandom,
) -> Result<Vec<u8>, WebPushError> {
    let _ = (subscription, plaintext, random);
    Err(WebPushError::Unimplemented { story: "E8-14" })
}

/// RFC 8291 §3: an ephemeral key, then a salt, both drawn in that order from `random`.
#[cfg_attr(not(test), expect(dead_code, reason = "E8-14's encrypt calls it"))]
fn seal(
    subscription: &Subscription,
    plaintext: &[u8],
    pad_to: Option<usize>,
    random: &mut dyn SecureRandom,
) -> Result<Vec<u8>, WebPushError> {
    let _ = (subscription, plaintext, pad_to, random);
    Err(WebPushError::Unimplemented { story: "E8-14" })
}

/// RFC 8188 §2: one record, the last, so its delimiter is `0x02`.
#[cfg_attr(not(test), expect(dead_code, reason = "E8-14's seal calls it"))]
fn aes128gcm(
    ikm: &[u8],
    salt: &[u8],
    keyid: &[u8],
    plaintext: &[u8],
    pad_to: Option<usize>,
) -> Result<Vec<u8>, WebPushError> {
    let _ = (ikm, salt, keyid, plaintext, pad_to);
    Err(WebPushError::Unimplemented { story: "E8-14" })
}

/// `vapid t=<JWT>, k=<public key>` (RFC 8292 §3), with `aud` the endpoint's origin.
pub fn vapid_authorization(
    endpoint: &PushEndpoint,
    subject: &VapidSubject,
    signer: &dyn VapidSigner,
    now_unix_s: u64,
) -> Result<String, WebPushError> {
    let _ = (endpoint, subject, signer, now_unix_s);
    Err(WebPushError::Unimplemented { story: "E8-14" })
}

#[cfg(test)]
mod tests;
