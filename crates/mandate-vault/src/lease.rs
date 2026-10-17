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
    let _ = (now, cap);
    None
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
        let _ = now;
        false
    }

    /// Applies a renewal's outcome, as the module's rule says.
    pub fn apply(&mut self, outcome: Result<Lease, SourceError>, now: UtcNanos) {
        let _ = (outcome, now);
    }

    /// The token while the lease holds at `now`; the lease is dropped at its expiry.
    pub fn secret(&mut self, now: UtcNanos) -> Option<&SecretSlice<u8>> {
        let _ = now;
        None
    }

    /// The held lease, if any, without checking its expiry.
    pub fn lease(&self) -> Option<&Lease> {
        self.lease.as_ref()
    }
}
