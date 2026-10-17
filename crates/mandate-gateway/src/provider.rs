//! The seams the gateway calls through: the clock, the transport, the provider adapter, the
//! secret source, and event ids. The crate is pure: it opens no socket and reads no clock or vault
//! itself, so every test runs on recorded responses with no network (DEC-997 item 4).

use core::fmt;

use mandate_canon::encode_ulid;
use mandate_time::UtcNanos;

use crate::money::Usage;
use crate::registry::{Endpoint, PinnedContent, Reported};

/// Time, as an input (ADR-0001 ES-05).
pub trait Clock {
    fn now(&mut self) -> UtcNanos;
    /// Waits until `at`; a test clock jumps there.
    fn wait_until(&mut self, at: UtcNanos);
}

/// A clock that moves only when told: the default everywhere but a real cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManualClock {
    now: UtcNanos,
}

impl ManualClock {
    pub fn at(now: UtcNanos) -> Self {
        Self { now }
    }

    /// Moves forward by `ms` milliseconds; a step past the last instant stays there.
    pub fn advance_ms(&mut self, ms: u64) {
        if let Some(later) = after_ms(self.now, ms) {
            self.now = later;
        }
    }
}

impl Clock for ManualClock {
    fn now(&mut self) -> UtcNanos {
        self.now
    }

    fn wait_until(&mut self, at: UtcNanos) {
        self.now = self.now.max(at);
    }
}

const NANOS_PER_SEC: i128 = 1_000_000_000;
const NANOS_PER_MS: i128 = 1_000_000;

fn total_nanos(at: UtcNanos) -> i128 {
    i128::from(at.secs())
        .saturating_mul(NANOS_PER_SEC)
        .saturating_add(i128::from(at.nanos()))
}

fn from_total(nanos: i128) -> Option<UtcNanos> {
    let secs = i64::try_from(nanos.checked_div(NANOS_PER_SEC)?).ok()?;
    let rest = u32::try_from(nanos.checked_rem(NANOS_PER_SEC)?).ok()?;
    UtcNanos::from_parts(secs, rest).ok()
}

/// `at` plus `ms` milliseconds, or `None` past the last representable instant.
pub fn after_ms(at: UtcNanos, ms: u64) -> Option<UtcNanos> {
    from_total(total_nanos(at).checked_add(i128::from(ms).checked_mul(NANOS_PER_MS)?)?)
}

/// `at` minus `ms` milliseconds, or the epoch.
pub fn before_ms(at: UtcNanos, ms: u64) -> UtcNanos {
    i128::from(ms)
        .checked_mul(NANOS_PER_MS)
        .and_then(|d| total_nanos(at).checked_sub(d))
        .and_then(from_total)
        .unwrap_or(UtcNanos::EPOCH)
}

/// A provider key, held only in memory, never printed (INF-14). It has no `Display`, and its
/// `Debug` shows nothing of it.
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderKey(String);

impl ProviderKey {
    pub fn new(secret: String) -> Self {
        Self(secret)
    }

    /// The secret, for the transport's authorization header and nothing else.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ProviderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProviderKey(redacted)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    #[error("no key is held for this provider")]
    Missing,
    #[error("the secret source cannot be reached")]
    Unavailable,
}

/// Where provider keys come from: the vault's `SecretSource` (lane V2, E21-32) in a real cell. The
/// gateway reads a key per attempt and never keeps, logs, or records it (inference spec §8.1).
pub trait SecretSource {
    fn provider_key(&self, upstream: &str) -> Result<ProviderKey, SecretError>;
}

/// The secret source of every test and of any cell without a vaulted key: it holds none. A
/// hosted call through it ends `credential_invalid`, metered and sent nowhere.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoKeys;

impl SecretSource for NoKeys {
    fn provider_key(&self, _upstream: &str) -> Result<ProviderKey, SecretError> {
        Err(SecretError::Missing)
    }
}

/// One HTTPS request: the host is checked against the egress allowlist before it is sent. The
/// key travels beside it, never inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub host: String,
    pub path: String,
    pub body: Vec<u8>,
}

/// What a transport attempt came back with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    Response {
        status: u16,
        body: Vec<u8>,
        retry_after_ms: Option<u64>,
    },
    ConnectFailed,
    TimedOut,
}

/// Sends one attempt. A real transport gives up at `deadline` and answers `TimedOut`; the gateway
/// still treats a response it reads after the deadline as late (INF-3).
pub trait Transport {
    fn send(
        &mut self,
        request: &HttpRequest,
        key: Option<&ProviderKey>,
        deadline: UtcNanos,
        clock: &mut dyn Clock,
    ) -> Delivery;
}

/// The rendered prompt: the pinned system text, and the user text with each slot filled from a
/// typed input inside a delimited data section (inference spec §8.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub system: String,
    pub user: String,
}

/// A provider's response, as the adapter read it. The identity that served it is read apart, by
/// [`ProviderAdapter::reported`], so a body of the wrong shape cannot hide a substitution.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reply {
    pub request_id: Option<String>,
    pub content: Option<String>,
    pub refused: bool,
    pub usage: Usage,
}

/// One provider's wire format.
pub trait ProviderAdapter {
    /// The request for `prompt` to `endpoint`, with the entry's pins and no capability (INF-5).
    fn request(&self, content: &PinnedContent, endpoint: &Endpoint, prompt: &Prompt)
    -> HttpRequest;
    /// The response body read, or `None` when it is not the provider's response shape.
    fn reply(&self, body: &[u8]) -> Option<Reply>;
    /// What the response body says served the call, whatever the rest of its shape: each part
    /// `None` when the body does not say (INF-2).
    fn reported(&self, body: &[u8]) -> Reported;
}

/// Event ids for the meter's records.
pub trait IdSource {
    fn next_id(&mut self) -> String;
}

/// ULIDs counted up from a start: deterministic, for tests and replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequentialIds {
    next: u128,
}

impl SequentialIds {
    pub fn starting_at(next: u128) -> Self {
        Self { next }
    }
}

impl IdSource for SequentialIds {
    fn next_id(&mut self) -> String {
        let id = encode_ulid(self.next);
        self.next = self.next.saturating_add(1);
        id
    }
}
