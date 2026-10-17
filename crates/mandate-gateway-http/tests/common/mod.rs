//! Fixtures shared by the transport's tests: the local HTTPS server, a transport that trusts only
//! its test authority, requests to it, and deadlines on the system clock.

#![allow(
    dead_code,
    reason = "each test binary uses a different part of the shared fixtures"
)]

pub mod fixtures;
pub mod server;

use std::time::Duration;

use mandate_gateway::provider::{Clock, Delivery, HttpRequest, ProviderKey, Transport};
use mandate_gateway_http::{HttpsTransport, SystemClock, TlsRoots};
use mandate_time::UtcNanos;

use server::Pki;

pub const PATH: &str = "/api/v1/chat/completions";

/// A transport that trusts only `pki`'s authority.
pub fn trusting(pki: &Pki) -> HttpsTransport {
    HttpsTransport::new(TlsRoots::Only(vec![pki.ca_der.clone()])).unwrap()
}

pub fn request(port: u16, body: &[u8]) -> HttpRequest {
    HttpRequest {
        host: format!("localhost:{port}"),
        path: PATH.to_owned(),
        body: body.to_vec(),
    }
}

/// The system time `ms` milliseconds from now.
pub fn in_ms(ms: u64) -> UtcNanos {
    after(SystemClock.now(), Duration::from_millis(ms))
}

pub fn after(at: UtcNanos, by: Duration) -> UtcNanos {
    let nanos = u64::from(at.nanos()) + u64::from(by.subsec_nanos());
    let secs = at.secs()
        + i64::try_from(by.as_secs()).unwrap()
        + i64::try_from(nanos / 1_000_000_000).unwrap();
    UtcNanos::from_parts(secs, u32::try_from(nanos % 1_000_000_000).unwrap()).unwrap()
}

/// One attempt with ten seconds to spare.
pub fn send(
    transport: &mut HttpsTransport,
    request: &HttpRequest,
    key: Option<&ProviderKey>,
) -> Delivery {
    transport.send(request, key, in_ms(10_000), &mut SystemClock)
}

pub fn status_of(delivery: &Delivery) -> Option<u16> {
    match delivery {
        Delivery::Response { status, .. } => Some(*status),
        _ => None,
    }
}
