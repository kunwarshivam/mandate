//! Shared fixtures. No network, no clock.

use mandate_time::UtcNanos;

use crate::start::ClientId;

pub const VERIFIER: &str = "verifier-canary-5d0e";

pub fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

pub fn client() -> ClientId {
    ClientId("client-1".to_owned())
}
