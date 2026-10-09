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
//! The web-push relay's pure core ([notifications spec §4.6](../../../docs/specs/notifications.md),
//! backlog E8-14, slice S8b, DEC-438 item 13, DEC-724).
//!
//! [`relay`] refuses a [`RelayRequest`] with a closed [`RelayError`] or posts it once through the
//! injected [`PushService`]. It keeps no state, and each request leaves one [`LogEntry`] that
//! cannot hold an endpoint or a ciphertext (NT-2, rule 6). It reaches only `mandate-webpush` (NT-9).

use mandate_webpush::Urgency;

/// Why a request was not forwarded or answered. `code()` is stable and carries no input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RelayError {
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    #[error("the relay id is not 32 lowercase hex digits")]
    InvalidRelayId,
    #[error("the endpoint is not a push service on the relay's allowlist")]
    AddressRejected,
    #[error("the urgency and TTL are not one class's fixed pair")]
    InvalidEnvelope,
    #[error("the ciphertext is over the relay's cap")]
    TooLarge,
    #[error("the push service did not answer")]
    Unreachable,
}

impl RelayError {
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::InvalidRelayId => "invalid_relay_id",
            Self::AddressRejected => "address_rejected",
            Self::InvalidEnvelope => "invalid_envelope",
            Self::TooLarge => "too_large",
            Self::Unreachable => "unreachable",
        }
    }
}

/// The relay's own id for one send (DEC-724 item 2), the only part of a request it logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayId(String);

impl RelayId {
    /// Exactly 32 characters of `0-9a-f`; anything else is [`RelayError::InvalidRelayId`].
    pub fn parse(text: &str) -> Result<Self, RelayError> {
        let _ = text;
        Err(RelayError::Unimplemented { story: "E8-14" })
    }
}

/// One request from a deployment (spec §4.6). No `Debug`: the endpoint is an address (NT-2).
pub struct RelayRequest<'a> {
    pub relay_id: &'a str,
    pub endpoint: &'a str,
    pub urgency: &'a str,
    pub ttl_s: u32,
    pub ciphertext: &'a [u8],
}

/// What the push service is sent; only [`relay`] builds one, without the relay id (DEC-724 item 5).
#[non_exhaustive]
pub struct Forward<'a> {
    pub endpoint: &'a str,
    pub urgency: Urgency,
    pub ttl_s: u32,
    pub body: &'a [u8],
}

/// What the push service answered: its HTTP status, or nothing at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushAnswer {
    Status(u16),
    Unreachable,
}

/// The one `POST` to a push service. It follows no redirect: a `3xx` is returned (spec §4.6).
pub trait PushService {
    fn post(&mut self, forward: &Forward<'_>) -> PushAnswer;
}

/// The one record each request leaves: its relay id, when it parsed, and the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub relay_id: Option<RelayId>,
    pub answer: Result<u16, RelayError>,
}

/// Where the relay's opaque log goes.
pub trait RelayLog {
    fn record(&mut self, entry: LogEntry);
}

/// Checks, in order, the relay id, the endpoint (on `allowlist` and at most 2 048 octets), the
/// urgency and TTL of one class, and the ciphertext (at most `mandate_webpush::MAX_BODY_LEN`
/// octets); posts once through `push`, answering its status; and records one [`LogEntry`] in `log`.
pub fn relay(
    request: &RelayRequest<'_>,
    allowlist: &mandate_webpush::PushAllowlist,
    push: &mut dyn PushService,
    log: &mut dyn RelayLog,
) -> Result<u16, RelayError> {
    let _ = (request, allowlist, push, log);
    Err(RelayError::Unimplemented { story: "E8-14" })
}

#[cfg(test)]
mod tests;
