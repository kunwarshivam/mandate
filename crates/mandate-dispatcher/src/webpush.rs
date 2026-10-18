//! The web-push channel adapter (notifications spec §4.6, §5.2, §5.9; E8-10 slice D10b, E8-14;
//! DEC-2500 items 6 and 9, DEC-2502, DEC-2503, DEC-2504, DEC-2506).
//!
//! **One call, one address.** A dispatcher's send on `(recipient, web_push)` fans out to each of
//! the recipient's active addresses, one send, one key, and one record each (DEC-2502 item 4). A
//! [`mandate_notify::AddressHandle`] names no address, so the [`Provider`] is [`Addressed`]: the
//! provider seen through one `address_ref`, which the dispatcher's fan-out takes from its own fold
//! of `NotificationAddressChanged` (DEC-2506 item 1).
//!
//! **Every edge is a port**, so each outcome is pinned without I/O: [`AddressBook`] reads one vault
//! entry, [`PushPost`] makes one HTTPS POST, [`Clock`] gives the attempt's time, and
//! [`mandate_webpush::SecureRandom`] draws each body's ephemeral key and salt and each attempt's
//! message id. The VAPID keys are [`RingVapidSigner`]s, one per `key_id`, held in this process's
//! memory (DEC-2503).
//!
//! **One deadline per send** ([`SEND_DEADLINE`], DEC-2500 item 6): the vault read and the POST share
//! it, so a send settles within 10 seconds of its start (spec §4.6, §5.9).
//!
//! **What each answer is** (spec §5.2, DEC-729, DEC-2502 item 7, DEC-2506 item 3). The status alone
//! decides, through [`crate::push_status`]; a push service that did not answer is
//! [`crate::relay_refusal`]'s `unreachable`, and the deadline is `retryable { timeout }`. No push
//! service's header is read: a [`PushAnswer`] is its status and nothing else, so the `Location`
//! header, which names a resource under the endpoint, cannot reach a record (DEC-2500 item 9).
//!
//! **Off until DEC-2504 is decided.** [`WebPushSwitch`] is the process configuration's switch
//! (DEC-2500 item 7), `Off` by default: while it is off, no `(recipient, web_push)` pair reaches a
//! send, so the dispatcher makes no web-push attempt and journals none (spec §4.6). This provider
//! has no "off" outcome: spec §5.2 gives none, and an outcome would be journaled as an attempt.
//!
//! **Nothing here prints an address or a key** (NT-2, DEC-2502 item 8, DEC-2503 item 4). No type
//! that holds an endpoint, a subscription key, a VAPID key, or a signed header has a `Debug` or a
//! `Display`, and every error and outcome is closed.
//!
//! Every entry point returns a `Result`, so a stub reports `Unimplemented` (DEC-77, DEC-137).

use std::time::Duration;

use mandate_notify::{
    AddressHandle, IdempotencyKey, Notification, NotifyError, Origin, Outcome, Provider,
    PushChannel, Reason, Recipient, TextKey,
};
use mandate_push_relay::RelayError;
use mandate_time::UtcNanos;
use mandate_webpush::{
    NoticeClass, PushAllowlist, PushEndpoint, PushPlaintext, PushRequest, SecureRandom,
    Subscription, VapidSigner, VapidSubject, WebPushError, encrypt,
};
use ring::digest::{SHA256, digest};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use secrecy::{ExposeSecret, SecretSlice, SecretString};

use crate::{Attempt, DispatchError, Prepared, Route, prepare, push_status, relay_refusal};

/// One send's whole budget, its vault read and its POST together (DEC-2500 item 6, DEC-2502 item
/// 4). Past it a send is `retryable`: `timeout` for the POST, `provider_error` for a vault read that
/// has not answered (DEC-2502 item 7).
pub const SEND_DEADLINE: Duration = Duration::from_secs(10);

/// The opaque id of one web-push address: the `added` event's own id, never derived from the
/// address (DEC-2502 item 1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AddressRef(String);

impl AddressRef {
    /// The address that `NotificationAddressChanged` event `added_event_id` added.
    #[must_use]
    pub fn new(added_event_id: &str) -> Self {
        Self(added_event_id.to_owned())
    }
}

/// The id of a VAPID key: the first 16 lowercase hex digits of the SHA-256 of its public point
/// (DEC-2503 item 1). Public, so it may print.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId(String);

impl KeyId {
    /// The key id as the vault entry and the key's path write it.
    #[must_use]
    pub fn new(id: &str) -> Self {
        Self(id.to_owned())
    }
}

/// One address's vault entry, read for one send and dropped when it returns (DEC-2502 items 1 and
/// 8): the endpoint, the subscription's decoded `p256dh` and `auth`, and the key it was made under.
/// No `Debug`: it holds an address (NT-2).
pub struct StoredSubscription {
    pub endpoint: SecretString,
    pub p256dh: SecretSlice<u8>,
    pub auth: SecretSlice<u8>,
    pub key_id: KeyId,
}

/// The vault did not answer: unreachable, sealed, refusing the dispatcher's identity, or past its
/// deadline. Closed: it holds nothing the vault said (DEC-2502 item 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the vault did not answer")]
pub struct VaultUnavailable;

/// The workspace's vault namespace, as the adapter reads it (DEC-2502 item 1, DEC-712): the only
/// path to an address (NT-2).
pub trait AddressBook {
    /// The entry `address` names, if it is `recipient`'s, read within `deadline`.
    ///
    /// # Errors
    /// [`VaultUnavailable`] for anything but the vault's definite answer. `Ok(None)` is that answer:
    /// no entry exists, `address_missing` (spec §5.2).
    fn entry(
        &mut self,
        recipient: &Recipient,
        address: &AddressRef,
        deadline: Duration,
    ) -> Result<Option<StoredSubscription>, VaultUnavailable>;
}

/// A push service's answer: its status, and nothing else. The adapter behind [`PushPost`] reads at
/// most a small fixed number of octets of a body and discards them, and reads no header (spec §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushAnswer {
    pub status: u16,
}

/// Why a POST has no answer. Closed: the HTTP client's error, which can carry the request's URL,
/// is never formatted or kept (DEC-2502 item 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PostFailure {
    /// The deadline passed first.
    #[error("the push service did not answer in time")]
    Timeout,
    /// No answer came: the connection was refused or reset, or TLS failed.
    #[error("the push service could not be reached")]
    Unreachable,
}

/// One HTTPS POST of a push request (RFC 8030 §5): `TTL`, `Urgency`, `Content-Encoding:
/// aes128gcm`, the VAPID `Authorization`, and the body, over TLS, following no redirect (spec
/// §4.6), and given up at `deadline`.
pub trait PushPost {
    /// Posts `request` once.
    ///
    /// # Errors
    /// [`PostFailure`] when no status came back.
    fn post(
        &mut self,
        request: &PushRequest,
        deadline: Duration,
    ) -> Result<PushAnswer, PostFailure>;
}

/// The process's clock.
pub trait Clock {
    /// Now.
    fn now(&self) -> UtcNanos;
}

/// The ports one provider sends through. No `Debug`.
pub struct Ports {
    pub post: Box<dyn PushPost>,
    pub book: Box<dyn AddressBook>,
    pub clock: Box<dyn Clock>,
    pub random: Box<dyn SecureRandom>,
}

/// One VAPID key, held in this process's memory as `ring`'s key pair and signing with
/// `ECDSA_P256_SHA256_FIXED_SIGNING` (DEC-2503 items 3 and 4). No `Debug`: it holds a key.
pub struct RingVapidSigner {
    key_id: KeyId,
    public: [u8; UNCOMPRESSED_POINT_LEN],
    pair: EcdsaKeyPair,
    random: SystemRandom,
}

/// The length of an uncompressed P-256 point: `0x04`, then `x` and `y`, 32 bytes each.
const UNCOMPRESSED_POINT_LEN: usize = 65;

/// The length of a fixed ES256 signature, `r || s`, 32 bytes each.
const FIXED_SIGNATURE_LEN: usize = 64;

/// How many leading bytes of the public point's SHA-256 a key id writes: 16 hex digits
/// (DEC-2503 item 1).
const KEY_ID_DIGEST_BYTES: usize = 8;

impl RingVapidSigner {
    /// The signer of the PKCS #8 document the vault holds, whose bytes are dropped once parsed; its
    /// key id is computed from its public point, never read from anywhere (DEC-2503 items 1, 3).
    ///
    /// # Errors
    /// [`DispatchError::WebPush`] with [`WebPushError::Signer`] for a document that is not a P-256
    /// key; it holds nothing of the document.
    pub fn from_pkcs8(pkcs8: SecretSlice<u8>) -> Result<Self, DispatchError> {
        let random = SystemRandom::new();
        let pair = EcdsaKeyPair::from_pkcs8(
            &ECDSA_P256_SHA256_FIXED_SIGNING,
            pkcs8.expose_secret(),
            &random,
        )
        .map_err(|_| DispatchError::WebPush(WebPushError::Signer))?;
        let public: [u8; UNCOMPRESSED_POINT_LEN] = pair
            .public_key()
            .as_ref()
            .try_into()
            .map_err(|_| DispatchError::WebPush(WebPushError::Signer))?;
        Ok(Self {
            key_id: key_id_of(&public),
            public,
            pair,
            random,
        })
    }

    /// The key's id (DEC-2503 item 1).
    ///
    /// # Errors
    /// Never, once built.
    pub fn key_id(&self) -> Result<&KeyId, DispatchError> {
        Ok(&self.key_id)
    }
}

/// The first 16 lowercase hex digits of the SHA-256 of `public` (DEC-2503 item 1).
fn key_id_of(public: &[u8]) -> KeyId {
    let hash = digest(&SHA256, public);
    let id: String = hash
        .as_ref()
        .iter()
        .take(KEY_ID_DIGEST_BYTES)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    KeyId(id)
}

impl VapidSigner for RingVapidSigner {
    /// The key's uncompressed public point.
    fn public_key(&self) -> [u8; 65] {
        self.public
    }

    /// An ES256 `r || s` over `message`, from `ring` with the process's secure random source.
    fn sign_es256(&self, message: &[u8]) -> Result<[u8; 64], WebPushError> {
        let signature = self
            .pair
            .sign(&self.random, message)
            .map_err(|_| WebPushError::Signer)?;
        let fixed: [u8; FIXED_SIGNATURE_LEN] = signature
            .as_ref()
            .try_into()
            .map_err(|_| WebPushError::Signer)?;
        Ok(fixed)
    }
}

/// The web-push provider of a managed deployment, sending directly (spec §4.6). No `Debug`.
pub struct WebPushProvider {
    ports: Ports,
    allowlist: PushAllowlist,
    subject: VapidSubject,
    keys: Vec<(KeyId, Box<dyn VapidSigner>)>,
}

impl WebPushProvider {
    /// A provider over `ports`, sending only to `allowlist`'s hosts, signing each address with the
    /// key its entry names among `keys`, under `subject`, which must be a role mailbox on this
    /// direct route too (DEC-2503 item 6).
    ///
    /// # Errors
    /// [`DispatchError::WebPush`] with [`WebPushError::InvalidSubject`] for a subject that fails
    /// [`VapidSubject::check_relayed`]: a person's address is never given to a push service.
    pub fn new(
        ports: Ports,
        allowlist: PushAllowlist,
        subject: VapidSubject,
        keys: Vec<(KeyId, Box<dyn VapidSigner>)>,
    ) -> Result<Self, DispatchError> {
        subject.check_relayed()?;
        Ok(Self {
            ports,
            allowlist,
            subject,
            keys,
        })
    }

    /// This provider seen through `address`, as the dispatcher's fan-out calls it.
    ///
    /// # Errors
    /// Never.
    pub fn to<'a>(&'a mut self, address: &'a AddressRef) -> Result<Addressed<'a>, DispatchError> {
        Ok(Addressed {
            provider: self,
            address,
        })
    }

    /// One attempt to `address_ref`, `handle`'s address on web push (DEC-2506 item 2):
    ///
    /// 1. a handle on any other channel is `permanent { provider_error }`, with no vault read;
    /// 2. the entry is read within [`SEND_DEADLINE`]: a vault that does not answer is
    ///    `retryable { provider_error }`, no entry `permanent { address_missing }`;
    /// 3. a key id this process does not hold is `permanent { provider_error }` (DEC-2503 item 5),
    ///    an endpoint off the allowlist `permanent { address_rejected }` (DEC-792 item 4), and keys
    ///    that do not parse `permanent { provider_error }`, each with no POST;
    /// 4. the message id is drawn, then the body is sealed from [`mandate_webpush::PushPlaintext::of`]
    ///    and the request built by [`crate::prepare`] with this attempt's time, its class the one
    ///    `notification`'s text key belongs to;
    /// 5. one POST, within what is left of the deadline, whose answer [`crate::push_status`] reads.
    ///
    /// The origin is not read: a push carries no link, only the closed pair (NT-1). The idempotency
    /// key is not read either: a push service keeps no key, and the message id is a fresh draw
    /// (DEC-2506 item 5).
    ///
    /// # Errors
    /// [`NotifyError::EntropyUnavailable`] when the random source or the signer fails, before any
    /// POST: nothing is sent and nothing is journaled, and the next step tries again.
    /// [`NotifyError::Unrepresentable`] for a request that cannot be formed for any other reason,
    /// such as a clock before the Unix epoch; nothing is posted.
    pub fn send_to(
        &mut self,
        _origin: &Origin,
        notification: &Notification,
        handle: &AddressHandle,
        address_ref: &AddressRef,
        _key: &IdempotencyKey,
    ) -> Result<Outcome, NotifyError> {
        if handle.channel != PushChannel::WebPush {
            return Ok(PROVIDER_FAULT);
        }
        let started = self.ports.clock.now();
        let entry = match self
            .ports
            .book
            .entry(&handle.recipient, address_ref, SEND_DEADLINE)
        {
            Ok(Some(entry)) => entry,
            Ok(None) => {
                return Ok(Outcome::Permanent {
                    reason: Reason::AddressMissing,
                });
            }
            Err(VaultUnavailable) => return Ok(VAULT_SILENT),
        };
        let now = self.ports.clock.now();
        let Some(left) = left_of(started, now) else {
            return Ok(VAULT_SILENT);
        };
        let Some(signer) = self
            .keys
            .iter()
            .find(|(id, _)| *id == entry.key_id)
            .map(|(_, signer)| signer.as_ref())
        else {
            return Ok(PROVIDER_FAULT);
        };
        let endpoint = entry.endpoint.expose_secret();
        let Ok(allowed) = PushEndpoint::parse_allowed(endpoint, &self.allowlist) else {
            return Ok(Outcome::Permanent {
                reason: Reason::AddressRejected,
            });
        };
        let Ok(subscription) = Subscription::new(
            allowed,
            entry.p256dh.expose_secret(),
            entry.auth.expose_secret(),
        ) else {
            return Ok(PROVIDER_FAULT);
        };
        let mut drawn = [0u8; MESSAGE_ID_BYTES];
        self.ports.random.fill(&mut drawn).map_err(unformed)?;
        let message_id: String = drawn.iter().map(|byte| format!("{byte:02x}")).collect();
        let plaintext = PushPlaintext::of(notification).map_err(unformed)?;
        let body =
            encrypt(&subscription, &plaintext, self.ports.random.as_mut()).map_err(unformed)?;
        let now_unix_s = u64::try_from(now.secs()).map_err(|_| unformed(WebPushError::Clock))?;
        let attempt = Attempt {
            endpoint,
            allowlist: &self.allowlist,
            body: &body,
            class: class_of(notification.text),
            signer,
            subject: &self.subject,
            now_unix_s,
            route: Route::Direct,
        };
        let request = match prepare(&attempt).map_err(not_formed)? {
            Prepared::Direct(request) => request,
            Prepared::Refused(outcome) => return Ok(outcome),
            Prepared::Relayed(_) => {
                return Err(NotifyError::Unrepresentable {
                    what: DIRECT_REQUEST,
                });
            }
        };
        match self.ports.post.post(&request, left) {
            Ok(PushAnswer { status }) => push_status(status, &message_id).map_err(not_formed),
            Err(PostFailure::Timeout) => Ok(Outcome::Retryable {
                reason: Reason::Timeout,
            }),
            Err(PostFailure::Unreachable) => {
                relay_refusal(RelayError::Unreachable).map_err(not_formed)
            }
        }
    }
}

/// The octets of a message id's draw: 128 bits, written as 32 lowercase hex digits (DEC-2500 item
/// 9, DEC-2506 item 5).
const MESSAGE_ID_BYTES: usize = 16;

/// What a request that cannot be formed is named as, in [`NotifyError::Unrepresentable`].
const DIRECT_REQUEST: &str = "direct push request";

/// A fault of the deployment's own wiring or keys: retried by no step, marking no address
/// (DEC-728's reading, DEC-2503 item 5, DEC-2506 items 2 and 3).
const PROVIDER_FAULT: Outcome = Outcome::Permanent {
    reason: Reason::ProviderError,
};

/// A vault that did not answer within the send's budget: retried, marking nothing (DEC-2502 item
/// 7).
const VAULT_SILENT: Outcome = Outcome::Retryable {
    reason: Reason::ProviderError,
};

/// What is left of [`SEND_DEADLINE`] for the POST once the vault read, begun at `started`, has
/// answered at `now`, or `None` when nothing is left (DEC-2506 item 4). A clock that moved back
/// spent nothing.
fn left_of(started: UtcNanos, now: UtcNanos) -> Option<Duration> {
    let spent = since_epoch(now)?.saturating_sub(since_epoch(started)?);
    SEND_DEADLINE
        .checked_sub(spent)
        .filter(|left| !left.is_zero())
}

/// `at` as the time since the Unix epoch, or `None` before it.
fn since_epoch(at: UtcNanos) -> Option<Duration> {
    let secs = u64::try_from(at.secs()).ok()?;
    Duration::from_secs(secs).checked_add(Duration::from_nanos(u64::from(at.nanos())))
}

/// Spec §3.2's class of each text key, which partition the classes (DEC-2506 item 6). The match
/// names every key, so a new key fails to compile until it has its class.
fn class_of(text: TextKey) -> NoticeClass {
    match text {
        TextKey::ApprovalNeeded => NoticeClass::Action,
        TextKey::AttentionNeeded | TextKey::AccountChanged => NoticeClass::Safety,
        TextKey::BriefReady => NoticeClass::Info,
    }
}

/// Why a send was not formed: a random source or a signer that failed is
/// [`NotifyError::EntropyUnavailable`], so the next step tries again (DEC-2506 item 3); anything
/// else, which no well-formed entry reaches, is [`NotifyError::Unrepresentable`]. Closed: neither
/// holds an input. The match names every code, so a new one cannot fall through.
fn unformed(error: WebPushError) -> NotifyError {
    match error {
        WebPushError::Random | WebPushError::Signer => NotifyError::EntropyUnavailable,
        WebPushError::Unimplemented { .. }
        | WebPushError::InvalidEndpoint
        | WebPushError::InvalidKey
        | WebPushError::InvalidAuthSecret
        | WebPushError::InvalidSubject
        | WebPushError::TooLarge
        | WebPushError::Clock
        | WebPushError::InvalidNotice => NotifyError::Unrepresentable {
            what: DIRECT_REQUEST,
        },
    }
}

/// [`unformed`] for the dispatcher's own errors. The match names every variant.
fn not_formed(error: DispatchError) -> NotifyError {
    match error {
        DispatchError::WebPush(error) => unformed(error),
        DispatchError::Notify(error) => error,
        DispatchError::Unimplemented { .. }
        | DispatchError::NoMessageId
        | DispatchError::NotANoticeStream
        | DispatchError::Fenced { .. }
        | DispatchError::NotCommitted { .. }
        | DispatchError::JournalUnavailable => NotifyError::Unrepresentable {
            what: DIRECT_REQUEST,
        },
    }
}

/// The provider through one address. No `Debug`.
pub struct Addressed<'a> {
    provider: &'a mut WebPushProvider,
    address: &'a AddressRef,
}

impl Provider for Addressed<'_> {
    fn send(
        &mut self,
        origin: &Origin,
        notification: &Notification,
        address: &AddressHandle,
        key: &IdempotencyKey,
    ) -> Result<Outcome, NotifyError> {
        self.provider
            .send_to(origin, notification, address, self.address, key)
    }
}

/// Whether web push is on: the process configuration's switch (DEC-2500 item 7), off by default
/// and off in the managed cell until the founder decides DEC-2504.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WebPushSwitch {
    #[default]
    Off,
    On,
}

impl WebPushSwitch {
    /// The `(recipient, channel)` pairs a step may send on, from the configured audience in its
    /// order: all of them when on, and every pair but the `web_push` ones when off, so no web-push
    /// attempt is made or journaled (spec §4.6, DEC-2504 item 4).
    ///
    /// # Errors
    /// Never.
    pub fn sends<'a>(
        self,
        audience: &[(&'a str, PushChannel)],
    ) -> Result<Vec<(&'a str, PushChannel)>, DispatchError> {
        Ok(audience
            .iter()
            .copied()
            .filter(|(_, channel)| self == Self::On || *channel != PushChannel::WebPush)
            .collect())
    }
}

#[cfg(test)]
mod tests;
