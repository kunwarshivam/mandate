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
//! attempt. A relayed send checks the configured subject with [`VapidSubject::check_relayed`]
//! before it builds any header. [`relay_refusal`] gives each relay refusal its §5.2 outcome, and
//! [`push_status`] each push service's status (DEC-729).
//!
//! Nothing here keeps, journals, or logs a header: [`PushRequest`] and [`Relayed`] have no
//! `Debug`, and an [`Outcome`] or a [`DispatchError`] holds no input (NT-2).
//!
//! Every entry point returns a `Result`, so a stub reports [`DispatchError::Unimplemented`]
//! (DEC-77); [`push_status`] does until its tests are live.

use mandate_notify::{Outcome, Reason};
use mandate_push_relay::{RelayError, RelayRequest};
use mandate_webpush::{
    NoticeClass, PushAllowlist, PushEndpoint, PushRequest, VapidSigner, VapidSubject, WebPushError,
    vapid_authorization,
};

/// Every way the dispatcher can fail to form a send. None of them is ever a reason to send more,
/// and none carries an input.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DispatchError {
    /// The body of a stub in a tests PR (DEC-77).
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
    /// Never: every field is already formed.
    pub fn request(&self) -> Result<RelayRequest<'_>, DispatchError> {
        Ok(RelayRequest {
            relay_id: &self.relay_id,
            endpoint: &self.push.endpoint,
            urgency: self.push.urgency.as_str(),
            ttl_s: self.push.ttl_s,
            authorization: &self.push.authorization,
            ciphertext: &self.push.body,
        })
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
/// The endpoint passes [`PushEndpoint::parse_allowed`], which keeps the address exactly as given,
/// and the header comes from [`vapid_authorization`] on every call: nothing is cached, so no
/// header outlives its attempt (DEC-726 item 3).
///
/// # Errors
/// [`DispatchError::WebPush`] when the endpoint is not allowlisted or the header cannot be signed.
pub fn prepare(attempt: &Attempt<'_>) -> Result<Prepared, DispatchError> {
    if let Route::Relayed { .. } = attempt.route
        && attempt.subject.check_relayed().is_err()
    {
        return Ok(Prepared::Refused(Outcome::Permanent {
            reason: Reason::ProviderError,
        }));
    }
    let endpoint = PushEndpoint::parse_allowed(attempt.endpoint, attempt.allowlist)?;
    let authorization = vapid_authorization(
        &endpoint,
        attempt.subject,
        attempt.signer,
        attempt.now_unix_s,
    )?;
    let push = PushRequest {
        endpoint: attempt.endpoint.to_owned(),
        ttl_s: attempt.class.ttl_s(),
        urgency: attempt.class.urgency(),
        authorization,
        body: attempt.body.to_vec(),
    };
    Ok(match attempt.route {
        Route::Direct => Prepared::Direct(push),
        Route::Relayed { relay_id } => Prepared::Relayed(Relayed {
            relay_id: relay_id.to_owned(),
            push,
        }),
    })
}

/// The outcome a relay refusal is journaled as (spec §5.2, §5.3): `address_rejected` is
/// `permanent { address_rejected }`, which marks the address (DEC-724 item 4); `unreachable`, a push
/// service that did not answer, is `retryable { timeout }`; and every other refusal, a fault of the
/// deployment's own request that no retry heals, is `permanent { provider_error }`, never
/// `auth_failed` (DEC-728 item 1).
///
/// The relay's own `unimplemented` code is a fault on the deployment's request path too, so it is
/// `provider_error` as well. The `match` names every refusal, so a new one cannot reach an outcome
/// unread.
///
/// # Errors
/// Never: every refusal has its outcome.
pub fn relay_refusal(refusal: RelayError) -> Result<Outcome, DispatchError> {
    Ok(match refusal {
        RelayError::AddressRejected => Outcome::Permanent {
            reason: Reason::AddressRejected,
        },
        RelayError::Unreachable => Outcome::Retryable {
            reason: Reason::Timeout,
        },
        RelayError::Unimplemented { .. }
        | RelayError::InvalidRelayId
        | RelayError::InvalidEnvelope
        | RelayError::TooLarge
        | RelayError::InvalidAuthorization => Outcome::Permanent {
            reason: Reason::ProviderError,
        },
    })
}

/// The outcome a push service's HTTP status is journaled as, on either route: the relay returns
/// the status unchanged (DEC-724 item 6), and this is where it is read (spec §4.6, §5.2, §5.3,
/// DEC-729).
///
/// - Any `2xx` is `accepted`, carrying `message_id`, the id the caller journals as
///   `provider_message_id`, unchanged.
/// - Any `3xx` is `permanent { address_rejected }`: no redirect is followed (§4.6).
/// - `404` and `410`, a subscription that is gone, are `permanent { address_rejected }`.
/// - `400`, `401` and `403`, faults of the deployment's own request or VAPID header, are
///   `permanent { provider_error }`, never `auth_failed` (DEC-728's reading).
/// - `413` is `permanent { too_large }`.
/// - `429` is `retryable { rate_limited }` (§5.3) and any `5xx` is `retryable { provider_error }`.
/// - Every other status, one outside `100..=599` included, is `retryable { provider_error }`: an
///   unknown answer is retried inside the class's window and marks no address.
///
/// # Errors
/// Never, once implemented; the stub reports [`DispatchError::Unimplemented`] (DEC-77).
pub fn push_status(status: u16, message_id: &str) -> Result<Outcome, DispatchError> {
    let _ = (status, message_id);
    Err(DispatchError::Unimplemented { story: "E8-14" })
}

#[cfg(test)]
mod tests;
