//! Pacing requests to the data host's rate limit. Each response's `X-Ratelimit-*` headers say how
//! many requests the account has left and when that budget is whole again; the pacer spends it
//! down to a low-water mark, then waits for the reset. Without usable headers a token bucket holds
//! every span of one window to the configured limit. A 429 closes the budget until the last known
//! reset, or for a full window when none is known. Times are integer nanoseconds since the Unix
//! epoch; there is no floating point.

use std::time::Duration;

use mandate_time::UtcNanos;

use crate::client::RateHeaders;

/// How the client paces itself against the host's per-window request limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RatePolicy {
    /// Requests the account may send per window, the floor when no header says how many are left
    /// (Alpaca's basic data plan allows 200 a minute). Values below 2 count as 2.
    pub limit: u32,
    /// The host's rate window.
    pub window: Duration,
    /// Stop spending a header-reported budget at this many remaining requests: the margin for
    /// other clients of the same account and for clock skew against the host.
    pub low_water: u32,
    /// The most added to a wait for a reset, so clients sharing an account do not wake together.
    pub max_jitter: Duration,
}

impl Default for RatePolicy {
    fn default() -> Self {
        Self {
            limit: 200,
            window: Duration::from_secs(60),
            low_water: 5,
            max_jitter: Duration::from_millis(500),
        }
    }
}

/// The shared pacing state of one client: a stub that admits everything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pacer {
    policy: RatePolicy,
}

impl Pacer {
    pub(crate) fn new(policy: RatePolicy) -> Self {
        Self { policy }
    }

    pub(crate) fn admit(&mut self, _now: UtcNanos) -> Option<Duration> {
        None
    }

    pub(crate) fn complete(&mut self, _headers: Option<&RateHeaders>, _now: UtcNanos) {}

    pub(crate) fn rate_limited(&mut self, _now: UtcNanos) {}
}
