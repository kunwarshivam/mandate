//! An executor's lease on its connection's token (infrastructure §5.2; E21-9's renewal rule).
//!
//! The lease lasts [`LEASE_SECS`] and is renewed every [`RENEW_SECS`] (infrastructure §5.2's
//! Proposed defaults: 24 hours, hourly). [`LeaseKeeper`] holds it:
//!
//! - a renewal that succeeds replaces it;
//! - a renewal that fails with an outage ([`SourceError::is_outage`]) keeps it, until it expires;
//! - a renewal refused for any other reason (the token deleted, the policy changed, the entry
//!   tampered with) drops it at once, since that is a revocation, not an outage;
//! - at its expiry it is dropped, whatever happened before; nothing extends it but a renewal.

use std::fmt;

use mandate_connections::record::ConnectionId;
use mandate_time::UtcNanos;
use secrecy::SecretSlice;

use crate::SourceError;

/// How long a lease lasts, in seconds.
pub const LEASE_SECS: i64 = 86_400;
/// How often a lease is renewed, in seconds.
pub const RENEW_SECS: i64 = 3_600;

/// One connection's token, held until `expires_at`.
pub struct Lease {
    pub connection: ConnectionId,
    pub secret: SecretSlice<u8>,
    pub expires_at: UtcNanos,
    pub renew_at: UtcNanos,
}

impl fmt::Debug for Lease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lease")
            .field("connection", &self.connection)
            .field("expires_at", &self.expires_at)
            .field("renew_at", &self.renew_at)
            .finish_non_exhaustive()
    }
}

/// The instants a lease taken at `now` renews at and expires at, each capped by `cap` (the
/// backend's own expiry, such as an OpenBao token's), or `None` when they are out of range.
pub fn lease_times(now: UtcNanos, cap: Option<UtcNanos>) -> Option<(UtcNanos, UtcNanos)> {
    let after = |secs: i64| {
        now.secs()
            .checked_add(secs)
            .and_then(|s| UtcNanos::from_parts(s, now.nanos()).ok())
    };
    let mut expires_at = after(LEASE_SECS)?;
    let mut renew_at = after(RENEW_SECS)?;
    if let Some(cap) = cap {
        expires_at = expires_at.min(cap);
        renew_at = renew_at.min(cap);
    }
    Some((renew_at, expires_at))
}

/// Holds an executor's lease and applies the renewal rule.
#[derive(Debug)]
pub struct LeaseKeeper {
    lease: Option<Lease>,
}

impl LeaseKeeper {
    pub fn new(lease: Lease) -> Self {
        Self { lease: Some(lease) }
    }

    /// Whether a renewal is due at `now`: a lease is held and `now` is at or past its `renew_at`.
    pub fn renewal_due(&self, now: UtcNanos) -> bool {
        self.lease
            .as_ref()
            .is_some_and(|lease| now >= lease.renew_at && now < lease.expires_at)
    }

    /// Applies a renewal's outcome, as the module's rule says. A lease already expired at `now`
    /// stays dropped: only a new lease, not a renewal, brings a credential back.
    pub fn apply(&mut self, outcome: Result<Lease, SourceError>, now: UtcNanos) {
        self.expire(now);
        let Some(held) = self.lease.as_ref() else {
            return;
        };
        match outcome {
            Ok(renewed) if renewed.connection == held.connection => self.lease = Some(renewed),
            Err(error) if error.is_outage() => {}
            Ok(_) | Err(_) => self.lease = None,
        }
    }

    /// The token while the lease holds at `now`; the lease is dropped at its expiry.
    pub fn secret(&mut self, now: UtcNanos) -> Option<&SecretSlice<u8>> {
        self.expire(now);
        self.lease.as_ref().map(|lease| &lease.secret)
    }

    /// Drops the lease once `now` reaches its expiry.
    fn expire(&mut self, now: UtcNanos) {
        if self
            .lease
            .as_ref()
            .is_some_and(|lease| now >= lease.expires_at)
        {
            self.lease = None;
        }
    }

    /// The held lease, if any, without checking its expiry.
    pub fn lease(&self) -> Option<&Lease> {
        self.lease.as_ref()
    }
}
