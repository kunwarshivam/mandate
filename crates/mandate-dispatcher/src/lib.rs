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
//! The notification dispatcher ([notifications spec](../../../docs/specs/notifications.md) §5,
//! backlog E8-10, DEC-701 item 2): the process that turns committed causes into sends through the
//! channel adapters and records every outcome on the notice stream. It is safety-critical, sits
//! above `mandate-journal`, and its `forbidden_internal` list keeps every crate that writes an
//! agent, account, or control stream out of its graph (NT-3 at rung 1, DEC-701 item 3).
//!
//! **Web push, one attempt at a time** (spec §4.6, E8-14, DEC-726 item 3, DEC-727). [`prepare`]
//! turns one attempt into a direct [`PushRequest`] or a [`Relayed`] send. Its VAPID header is
//! built for that attempt with that attempt's time, so its `exp`, a fixed 12 hours on (DEC-790
//! item 4), never lapses inside the 24-hour `safety` retry window, and no header outlives its
//! attempt. A relayed send checks the configured subject with
//! [`VapidSubject::check_relayed`] before it builds any header and is refused when the subject is
//! not a role mailbox. [`relay_refusal`] maps every relay refusal to spec §5.2's permanent
//! `provider_error`, which marks no address and is not retried (DEC-728).
//!
//! Nothing here keeps, journals, or logs a header: [`PushRequest`] and [`Relayed`] have no
//! `Debug`, and an [`Outcome`] or a [`DispatchError`] holds no input (NT-2).
//!
//! Every entry point returns a `Result`, so a stub reports [`DispatchError::Unimplemented`]
//! (DEC-77).

use mandate_notify::Outcome;
use mandate_push_relay::{RelayError, RelayRequest};
use mandate_webpush::{
    NoticeClass, PushAllowlist, PushRequest, VapidSigner, VapidSubject, WebPushError,
};

/// Every way the dispatcher can fail to form a send. None of them is ever a reason to send more,
/// and none carries an input.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DispatchError {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// The push request could not be built; the web-push error's closed code says why.
    #[error("the push request could not be built: {0}")]
    WebPush(#[from] WebPushError),
}

/// How a deployment reaches the push service (spec §4.6, HLD §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route<'a> {
    /// A managed deployment posts to the push service itself.
    Direct,
    /// A hybrid deployment sends through the relay, under the relay id it minted for this send
    /// (DEC-724 item 2).
    Relayed { relay_id: &'a str },
}

/// One web-push attempt: the subscription's endpoint as the vault holds it, the deployment's
/// allowlist, the `aes128gcm` body `mandate_webpush::encrypt` sealed, the notice's class, the
/// application server's signer, the deployment's one subject, this attempt's time, and the route.
#[derive(Clone, Copy)]
pub struct Attempt<'a> {
    pub endpoint: &'a str,
    pub allowlist: &'a PushAllowlist,
    pub body: &'a [u8],
    pub class: NoticeClass,
    pub signer: &'a dyn VapidSigner,
    pub subject: &'a VapidSubject,
    pub now_unix_s: u64,
    pub route: Route<'a>,
}

/// A send through the relay: the relay id and the request the relay forwards. No `Debug`: it
/// holds an address and a header (NT-2).
pub struct Relayed {
    pub relay_id: String,
    pub push: PushRequest,
}

impl Relayed {
    /// The relay's request, `{relay_id, endpoint, urgency, ttl_s, authorization, ciphertext}`,
    /// borrowed from this send (DEC-726 item 7).
    ///
    /// # Errors
    /// Never once implemented: every field is already formed.
    pub fn request(&self) -> Result<RelayRequest<'_>, DispatchError> {
        Err(DispatchError::Unimplemented { story: "E8-14" })
    }
}

/// What one attempt becomes. No `Debug`: a request holds an address and a header (NT-2).
pub enum Prepared {
    Direct(PushRequest),
    Relayed(Relayed),
    /// Nothing was built or sent, and this is the attempt's outcome.
    Refused(Outcome),
}

/// One attempt's request, built with `attempt.now_unix_s` and nothing older. A relayed attempt
/// whose subject fails [`VapidSubject::check_relayed`] builds nothing and is refused
/// `permanent { provider_error }`: spec §5.2 names no reason for a deployment's own configuration
/// fault, and DEC-728 item 2 takes `provider_error`.
///
/// # Errors
/// [`DispatchError::WebPush`] when the endpoint is not allowlisted or the header cannot be signed.
pub fn prepare(attempt: &Attempt<'_>) -> Result<Prepared, DispatchError> {
    let _ = attempt;
    Err(DispatchError::Unimplemented { story: "E8-14" })
}

/// The outcome a relay refusal is journaled as: `permanent { provider_error }` for every one,
/// never `auth_failed` or `address_rejected`, so no address is marked `unreachable` and nothing is
/// retried (DEC-728 item 1).
///
/// # Errors
/// Never once implemented: every refusal has its outcome.
pub fn relay_refusal(refusal: RelayError) -> Result<Outcome, DispatchError> {
    let _ = refusal;
    Err(DispatchError::Unimplemented { story: "E8-14" })
}

#[cfg(test)]
mod tests;
