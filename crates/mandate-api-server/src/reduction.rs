//! Route 2's request keys (workspace API spec §4.10 route 2 item 3,
//! [DEC-2117](../../../docs/project/decisions/DEC-2117.md),
//! [DEC-2119](../../../docs/project/decisions/DEC-2119.md)): what the challenge and opening routes
//! count a request against, and the values they name.
//!
//! The address is the TCP peer, or, only from a peer `MANDATE_API_INGRESS_PEERS` lists, the first
//! `X-Forwarded-For` entry from the right that is not itself a listed peer; an IPv4-mapped address
//! counts as its IPv4 address and a native IPv6 address as its /64. The device is the
//! `__Host-mandate_device` cookie, keyed only by its SHA-256. [`limits`] are DEC-2117 item 3's
//! numbers for `mandate_authn`'s gate (DEC-653).

use std::net::IpAddr;

use axum::http::HeaderMap;
use mandate_authn::ReductionLimits;
use mandate_time::UtcNanos;

use crate::config::StartRefusal;

/// The device cookie's name (§4.10 route 2 item 3).
pub const DEVICE_COOKIE: &str = "__Host-mandate_device";

/// The setting naming the ingress's own addresses (§3.11 item 2).
pub const INGRESS_PEERS: &str = "MANDATE_API_INGRESS_PEERS";

/// DEC-2117 item 3's numbers: 4 challenges per device and 32 per address outstanding, and 10
/// failures per device and 100 per address kept.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn limits() -> ReductionLimits {
    todo!()
}

/// The ingress peers `setting` lists, comma-separated IP addresses; none when it is absent or
/// empty, and [`StartRefusal::Setting`] naming [`INGRESS_PEERS`] for any entry that is not an
/// address.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn ingress_peers(setting: Option<&str>) -> Result<Vec<IpAddr>, StartRefusal> {
    let _ = (setting, INGRESS_PEERS);
    todo!()
}

/// The address a request counts against: `peer`, or, when `peer` is one of `ingress`, the first
/// `X-Forwarded-For` entry from the right that is not itself a listed peer, its lines read as one
/// list; `peer` again when that entry is missing or not an address. An IPv4-mapped address, the
/// peer's, a listed one or a forwarded one, is read as its IPv4 address throughout, so it counts
/// as that address; only a native IPv6 address counts as its /64, as `<prefix>::/64`.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn address(peer: IpAddr, headers: &HeaderMap, ingress: &[IpAddr]) -> String {
    let _ = (peer, headers, ingress);
    todo!()
}

/// The device key of a request carrying exactly one well-formed device cookie (32 bytes as
/// unpadded base64url): the lowercase hex SHA-256 of its value, never the value itself.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn device(headers: &HeaderMap) -> Option<String> {
    let _ = (headers, DEVICE_COOKIE);
    todo!()
}

/// The WebAuthn RP ID of `app_origin`: its host, when it is an `https` origin whose host has a dot.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn rp_id(app_origin: &str) -> Option<String> {
    let _ = app_origin;
    todo!()
}

/// A challenge's id: the ULID whose time is `issued_at`'s millisecond and whose randomness is the
/// challenge's first ten bytes.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn challenge_id(issued_at: UtcNanos, bytes: &[u8; 32]) -> String {
    let _ = (issued_at, bytes);
    todo!()
}
