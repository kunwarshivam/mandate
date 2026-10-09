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

/// The default push-service allowlist (DEC-792 item 2): Chrome's, Firefox's, Safari's and Edge's
/// push services. A deployment may configure a shorter list; adding a host is a reviewed change.
pub const DEFAULT_PUSH_ALLOWLIST: [&str; 4] = [
    "fcm.googleapis.com",
    "updates.push.services.mozilla.com",
    "*.push.apple.com",
    "*.notify.windows.com",
];

/// The deployment's push-service allowlist (DEC-792 item 2, spec §4.6). Each entry is one exact
/// host, or `*.` and a domain, which matches a proper subdomain at any depth and never the domain
/// itself. The host or domain names at least two labels (DEC-722 item 1).
#[derive(Debug, Clone)]
pub struct PushAllowlist {
    entries: Vec<String>,
}

impl PushAllowlist {
    /// From the deployment's configured entries, such as [`DEFAULT_PUSH_ALLOWLIST`]. An entry no
    /// accepted endpoint could match is refused with [`WebPushError::InvalidEndpoint`]: a host or
    /// domain that is not lowercase ASCII labels of letters, digits and hyphens, one of a single
    /// label (`com`, `*.com`; DEC-722 item 1), one with an empty label, a label that starts or ends
    /// with a hyphen (DEC-722 item 2), a trailing dot, an `xn--` label, or an all-digit last label
    /// (an IPv4 literal), a port, a scheme, or a `*` anywhere but a leading `*.`. One such entry
    /// refuses the whole list.
    pub fn parse(entries: &[&str]) -> Result<Self, WebPushError> {
        entries
            .iter()
            .map(|entry| {
                let host = entry.strip_prefix(WILDCARD).unwrap_or(entry);
                if is_allowable_host(host) {
                    Ok((*entry).to_owned())
                } else {
                    Err(WebPushError::InvalidEndpoint)
                }
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|entries| Self { entries })
    }

    /// Whether `host` is an exact entry, or a proper subdomain of a wildcard entry's domain: the
    /// domain must follow a dot that something precedes, so the domain itself never matches.
    fn allows(&self, host: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| match entry.strip_prefix(WILDCARD) {
                Some(domain) => host
                    .strip_suffix(domain)
                    .and_then(|sub| sub.strip_suffix('.'))
                    .is_some_and(|sub| !sub.is_empty()),
                None => host == entry,
            })
    }
}

/// A wildcard entry's prefix (DEC-792 item 2).
const WILDCARD: &str = "*.";

/// A host DEC-792 item 1 lets through, as DEC-722 tightens it: at least two labels (DEC-722
/// item 1), each non-empty, of lowercase ASCII letters, digits and hyphens, none starting or ending
/// with a hyphen (DEC-722 item 2, RFC 1123), none an `xn--` (IDN) label, and a last label that is
/// not all digits (an IPv4 literal). A hyphen inside a label, as in `a--b`, stays allowed. A
/// trailing dot is an empty last label; a bracketed IPv6 literal, a percent-encoded or non-ASCII
/// character, a port, a scheme, user information and a `*` are all outside the label characters.
/// The one host check for both an allowlist entry's host or wildcard domain and an endpoint's host.
fn is_allowable_host(host: &str) -> bool {
    let two_labels = host.contains('.');
    let labels_ok = host.split('.').all(|label| {
        !label.is_empty()
            && label.bytes().all(|b| HOST_CHARS.contains(&b))
            && !label.starts_with(HYPHEN)
            && !label.ends_with(HYPHEN)
            && !label.starts_with(IDN_PREFIX)
    });
    let ipv4 = host
        .rsplit('.')
        .next()
        .is_some_and(|last| last.bytes().all(|b| b.is_ascii_digit()));
    two_labels && labels_ok && !ipv4
}

/// The character no host label may start or end with (DEC-722 item 2, RFC 1123 §2.1).
const HYPHEN: char = '-';

/// The ACE prefix of an internationalized label (RFC 5890 §2.3.2.5), refused, never decoded.
const IDN_PREFIX: &str = "xn--";

/// The one port a push endpoint may name (DEC-792 item 1), and the default the origin omits.
const DEFAULT_PORT_SUFFIX: &str = ":443";

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
    /// `https`, a host of lowercase letters, digits, dots and hyphens in non-empty labels, an
    /// optional port written as a nonzero number with no sign or leading zero, and a path, query
    /// and fragment of URI characters only: no user information, space, quote, or backslash.
    ///
    /// Syntax only, with no allowlist, so it is private to this crate: consumers use
    /// [`PushEndpoint::parse_allowed`], the one parser DEC-792 item 3 names.
    pub(crate) fn parse(url: &str) -> Result<Self, WebPushError> {
        let rest = url
            .strip_prefix(HTTPS)
            .ok_or(WebPushError::InvalidEndpoint)?;
        let authority_len = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(authority_len);
        let (host, port) = match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        };
        let host_ok = host
            .split('.')
            .all(|label| !label.is_empty() && label.bytes().all(|b| HOST_CHARS.contains(&b)));
        let port_ok = port.is_none_or(|p| {
            p.parse::<std::num::NonZeroU16>()
                .is_ok_and(|n| n.to_string() == p)
        });
        let tail_ok = tail.bytes().all(is_uri_char);
        if !(host_ok && port_ok && tail_ok) {
            return Err(WebPushError::InvalidEndpoint);
        }
        let origin_len = HTTPS
            .len()
            .checked_add(authority_len)
            .ok_or(WebPushError::InvalidEndpoint)?;
        Ok(Self {
            url: url.to_owned(),
            origin_len,
        })
    }

    /// The one parser the workspace API, the dispatcher and the relay use (DEC-792, spec §4.6):
    /// `https`, port 443 (none written, or `:443`), no user information, and a host of lowercase
    /// ASCII labels on `allowlist`. An IP literal, a trailing dot, a non-ASCII or `xn--` label, a
    /// label that starts or ends with a hyphen (DEC-722 item 2), even under a wildcard entry that
    /// would match it, or a percent-encoded character is refused, never normalized into a match;
    /// an accepted endpoint keeps its address exactly as given. Every refusal is [`WebPushError::InvalidEndpoint`].
    pub fn parse_allowed(url: &str, allowlist: &PushAllowlist) -> Result<Self, WebPushError> {
        let endpoint = Self::parse(url)?;
        let authority = endpoint
            .url
            .get(HTTPS.len()..endpoint.origin_len)
            .ok_or(WebPushError::InvalidEndpoint)?;
        let host = authority
            .strip_suffix(DEFAULT_PORT_SUFFIX)
            .unwrap_or(authority);
        if is_allowable_host(host) && allowlist.allows(host) {
            Ok(endpoint)
        } else {
            Err(WebPushError::InvalidEndpoint)
        }
    }

    /// `https://<host>[:port]`, the VAPID audience (RFC 8292 §2), serialized as RFC 6454 §6.2
    /// says: a written default port, `:443`, is omitted, though the stored address keeps it.
    fn origin(&self) -> Result<&str, WebPushError> {
        let origin = self
            .url
            .get(..self.origin_len)
            .ok_or(WebPushError::InvalidEndpoint)?;
        Ok(origin.strip_suffix(DEFAULT_PORT_SUFFIX).unwrap_or(origin))
    }
}

const HTTPS: &str = "https://";

/// A host label's characters: lowercase ASCII letters, digits, and the hyphen.
const HOST_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789-";

/// RFC 3986's unreserved and reserved characters and `%`; never a space, quote, or backslash.
fn is_uri_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-._~:/?#[]@!$&'()*+,;=%".contains(&b)
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
    /// `p256dh` must be an uncompressed point on the curve (RFC 8291 §3.1); `auth` 16 octets.
    pub fn new(endpoint: PushEndpoint, p256dh: &[u8], auth: &[u8]) -> Result<Self, WebPushError> {
        if p256dh.len() != UNCOMPRESSED_POINT_LEN || p256dh.first() != Some(&UNCOMPRESSED_TAG) {
            return Err(WebPushError::InvalidKey);
        }
        let p256dh = PublicKey::from_sec1_bytes(p256dh).map_err(|_| WebPushError::InvalidKey)?;
        let auth = <[u8; 16]>::try_from(auth).map_err(|_| WebPushError::InvalidAuthSecret)?;
        Ok(Self {
            endpoint,
            p256dh,
            auth,
        })
    }
}

/// DEC-727 item 2's role mailboxes, the only local parts a relayed subject may have.
const RELAYED_ROLES: [&str; 5] = ["push", "notifications", "postmaster", "abuse", "security"];

/// One byte of a relayed subject's domain: a lowercase ASCII letter, a digit, `.` or `-`
/// (DEC-727 item 2).
fn is_domain_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
}

/// The VAPID `sub` claim: one contact URI for the deployment, the same on every notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VapidSubject(String);

impl VapidSubject {
    /// `mailto:` or `https://` and something after it, all printable ASCII with no space, quote,
    /// or backslash, so the claim needs no escaping (DEC-790 item 4).
    pub fn parse(uri: &str) -> Result<Self, WebPushError> {
        let rest = uri
            .strip_prefix("mailto:")
            .or_else(|| uri.strip_prefix(HTTPS))
            .ok_or(WebPushError::InvalidSubject)?;
        let printable = uri
            .bytes()
            .all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\');
        if rest.is_empty() || !printable {
            return Err(WebPushError::InvalidSubject);
        }
        Ok(Self(uri.to_owned()))
    }

    /// DEC-727: the subject a deployment signs with when it sends through the relay is
    /// `mailto:<role>@<domain>`, `<role>` exactly one of DEC-727 item 2's role mailboxes and
    /// `<domain>` a non-empty run of lowercase letters, digits, `.` and `-` with nothing after it,
    /// never a person's address; anything else, an `https:` subject included, is
    /// [`WebPushError::InvalidSubject`]. A direct send keeps [`VapidSubject::parse`]'s rule.
    pub fn check_relayed(&self) -> Result<(), WebPushError> {
        let domain = self
            .0
            .strip_prefix("mailto:")
            .and_then(|rest| rest.split_once('@'))
            .filter(|(role, _)| RELAYED_ROLES.contains(role))
            .map(|(_, domain)| domain);
        match domain {
            Some(domain) if !domain.is_empty() && domain.bytes().all(is_domain_byte) => Ok(()),
            _ => Err(WebPushError::InvalidSubject),
        }
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
    let authorization = vapid_authorization(&subscription.endpoint, subject, signer, now_unix_s)?;
    Ok(PushRequest {
        endpoint: subscription.endpoint.url.clone(),
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
/// A key draw that is not a valid scalar is refused, never retried (DEC-790 item 3).
fn seal(
    subscription: &Subscription,
    plaintext: &[u8],
    pad_to: Option<usize>,
    random: &mut dyn SecureRandom,
) -> Result<Vec<u8>, WebPushError> {
    let mut drawn = [0u8; 32];
    random.fill(&mut drawn)?;
    let ephemeral = SecretKey::from_slice(&drawn).map_err(|_| WebPushError::Random);
    drawn.fill(0);
    let ephemeral = ephemeral?;
    let mut salt = <[u8; 16]>::default();
    random.fill(&mut salt)?;
    let as_public = ephemeral.public_key().to_sec1_point(false);
    let ua_public = subscription.p256dh.to_sec1_point(false);
    let shared = ephemeral.diffie_hellman(&subscription.p256dh);
    let info = [
        b"WebPush: info\0".as_slice(),
        ua_public.as_bytes(),
        as_public.as_bytes(),
    ]
    .concat();
    let mut ikm = [0u8; 32];
    let combined = Hkdf::<Sha256>::new(Some(&subscription.auth), shared.raw_secret_bytes())
        .expand(&info, &mut ikm)
        .map_err(|_| WebPushError::TooLarge);
    let body =
        combined.and_then(|()| aes128gcm(&ikm, &salt, as_public.as_bytes(), plaintext, pad_to));
    ikm.fill(0);
    body
}

/// RFC 8291 §4: one record of at most this size.
const RECORD_SIZE: u32 = 4096;

/// The header's `rs` (4 octets) and `idlen` (1 octet), around the salt and the `keyid`.
const RS_AND_IDLEN_LEN: usize = 5;

/// The 16-octet tag AES-GCM appends.
const TAG_LEN: usize = 16;

const UNCOMPRESSED_POINT_LEN: usize = 65;
const UNCOMPRESSED_TAG: u8 = 0x04;

/// RFC 8188 §2: the last record's delimiter.
const LAST_RECORD_DELIMITER: u8 = 0x02;

/// RFC 8188 §2: one record, the last, so its delimiter is `0x02`.
/// The header is `salt || rs || idlen || keyid`; a body over [`MAX_BODY_LEN`], or a plaintext
/// with no room for its delimiter inside `pad_to`, is refused before anything is encrypted.
fn aes128gcm(
    ikm: &[u8],
    salt: &[u8],
    keyid: &[u8],
    plaintext: &[u8],
    pad_to: Option<usize>,
) -> Result<Vec<u8>, WebPushError> {
    let idlen = u8::try_from(keyid.len()).map_err(|_| WebPushError::TooLarge)?;
    let unpadded = plaintext
        .len()
        .checked_add(1)
        .ok_or(WebPushError::TooLarge)?;
    let record_len = match pad_to {
        Some(pad_to) if unpadded > pad_to => return Err(WebPushError::TooLarge),
        Some(pad_to) => pad_to,
        None => unpadded,
    };
    let body_len = [
        salt.len(),
        RS_AND_IDLEN_LEN,
        keyid.len(),
        record_len,
        TAG_LEN,
    ]
    .into_iter()
    .try_fold(0usize, usize::checked_add)
    .ok_or(WebPushError::TooLarge)?;
    if body_len > MAX_BODY_LEN {
        return Err(WebPushError::TooLarge);
    }
    let mut record = Vec::with_capacity(record_len);
    record.extend_from_slice(plaintext);
    record.push(LAST_RECORD_DELIMITER);
    record.resize(record_len, 0);
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
    let derived = hk
        .expand(b"Content-Encoding: aes128gcm\0", &mut cek)
        .and_then(|()| hk.expand(b"Content-Encoding: nonce\0", &mut nonce))
        .map_err(|_| WebPushError::TooLarge);
    let sealed = derived.and_then(|()| {
        Aes128Gcm::new_from_slice(&cek)
            .map_err(|_| WebPushError::TooLarge)?
            .encrypt(&nonce.into(), record.as_slice())
            .map_err(|_| WebPushError::TooLarge)
    });
    cek.fill(0);
    record.fill(0);
    let sealed = sealed?;
    let mut body = Vec::with_capacity(body_len);
    body.extend_from_slice(salt);
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(idlen);
    body.extend_from_slice(keyid);
    body.extend_from_slice(&sealed);
    Ok(body)
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
    let header = Base64UrlUnpadded::encode_string(br#"{"typ":"JWT","alg":"ES256"}"#);
    let claims = format!(
        r#"{{"aud":"{}","exp":{exp},"sub":"{}"}}"#,
        endpoint.origin()?,
        subject.0
    );
    let signed = format!(
        "{header}.{}",
        Base64UrlUnpadded::encode_string(claims.as_bytes())
    );
    let signature = signer
        .sign_es256(signed.as_bytes())
        .map_err(|_| WebPushError::Signer)?;
    Ok(format!(
        "vapid t={signed}.{}, k={}",
        Base64UrlUnpadded::encode_string(&signature),
        Base64UrlUnpadded::encode_string(&signer.public_key())
    ))
}

#[cfg(test)]
mod tests;
