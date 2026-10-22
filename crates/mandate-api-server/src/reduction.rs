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

use std::net::{IpAddr, Ipv6Addr};
use std::num::NonZeroU32;

use axum::http::HeaderMap;
use axum::http::header::COOKIE;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use mandate_authn::ReductionLimits;
use mandate_canon::encode_ulid;
use mandate_time::UtcNanos;
use ring::digest::{SHA256, digest};

use crate::config::StartRefusal;

/// The device cookie's name (§4.10 route 2 item 3).
pub const DEVICE_COOKIE: &str = "__Host-mandate_device";

/// The setting naming the ingress's own addresses (§3.11 item 2).
pub const INGRESS_PEERS: &str = "MANDATE_API_INGRESS_PEERS";

/// The forwarded-address header the ingress appends to.
const FORWARDED_FOR: &str = "x-forwarded-for";

/// DEC-2117 item 3's numbers: 4 challenges per device and 32 per address outstanding, and 10
/// failures per device and 100 per address kept; DEC-2119's bounds in all: 4,096 outstanding, the
/// reserve from 3,072, and 65,536 failure keys.
pub fn limits() -> ReductionLimits {
    ReductionLimits {
        challenges_per_address: NonZeroU32::MIN.saturating_add(31),
        challenges_per_device: NonZeroU32::MIN.saturating_add(3),
        failures_per_address: NonZeroU32::MIN.saturating_add(99),
        failures_per_device: NonZeroU32::MIN.saturating_add(9),
        max_outstanding: NonZeroU32::MIN.saturating_add(4_095),
        reserve: NonZeroU32::MIN.saturating_add(3_071),
        max_failure_keys: NonZeroU32::MIN.saturating_add(65_535),
    }
}

/// The ingress peers `setting` lists, comma-separated IP addresses; none when it is absent or
/// empty, and [`StartRefusal::Setting`] naming [`INGRESS_PEERS`] for any entry that is not an
/// address.
pub fn ingress_peers(setting: Option<&str>) -> Result<Vec<IpAddr>, StartRefusal> {
    match setting.map(str::trim) {
        None | Some("") => Ok(Vec::new()),
        Some(listed) => listed
            .split(',')
            .map(|entry| entry.trim().parse::<IpAddr>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| StartRefusal::Setting {
                name: INGRESS_PEERS,
            }),
    }
}

/// The address a request counts against: `peer`, or, when `peer` is one of `ingress`, the first
/// `X-Forwarded-For` entry from the right that is not itself a listed peer, its lines read as one
/// list; `peer` again when that entry is missing or not an address. An IPv4-mapped address, the
/// peer's, a listed one or a forwarded one, is read as its IPv4 address throughout, so it counts
/// as that address; only a native IPv6 address counts as its /64, as `<prefix>::/64`.
pub fn address(peer: IpAddr, headers: &HeaderMap, ingress: &[IpAddr]) -> String {
    let listed = |at: IpAddr| ingress.iter().any(|peer| plain(*peer) == plain(at));
    let from = if listed(peer) {
        forwarded(headers, listed).unwrap_or(peer)
    } else {
        peer
    };
    match plain(from) {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let [a, b, c, d, ..] = v6.segments();
            format!("{}/64", Ipv6Addr::new(a, b, c, d, 0, 0, 0, 0))
        }
    }
}

/// `at`, with an IPv4-mapped IPv6 address read as its IPv4 address.
fn plain(at: IpAddr) -> IpAddr {
    match at {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(at, IpAddr::V4),
        IpAddr::V4(_) => at,
    }
}

/// The first `X-Forwarded-For` entry from the right that `listed` does not name, when it is an
/// address; the header's lines are one list in order.
fn forwarded(headers: &HeaderMap, listed: impl Fn(IpAddr) -> bool) -> Option<IpAddr> {
    let entries: Vec<&str> = headers
        .get_all(FORWARDED_FOR)
        .iter()
        .map(|line| line.to_str().ok())
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .flat_map(|line| line.split(','))
        .collect();
    entries
        .into_iter()
        .rev()
        .map(|entry| entry.trim().parse::<IpAddr>().ok())
        .find(|entry| entry.is_none_or(|at| !listed(at)))?
}

/// The device key of a request carrying exactly one well-formed device cookie (32 bytes as
/// unpadded base64url): the lowercase hex SHA-256 of its value, never the value itself.
pub fn device(headers: &HeaderMap) -> Option<String> {
    let mut found = headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .filter(|(name, _)| *name == DEVICE_COOKIE)
        .map(|(_, value)| value);
    let value = match (found.next(), found.next()) {
        (Some(value), None) => value,
        _ => return None,
    };
    let bytes: [u8; 32] = URL_SAFE_NO_PAD.decode(value).ok()?.try_into().ok()?;
    (URL_SAFE_NO_PAD.encode(bytes) == value).then(|| hex_digest(value))
}

/// The lowercase hex SHA-256 of `value`.
fn hex_digest(value: &str) -> String {
    digest(&SHA256, value.as_bytes())
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The WebAuthn RP ID of `app_origin`: its host, when it is an `https` origin whose host has a dot.
pub fn rp_id(app_origin: &str) -> Option<String> {
    let rest = app_origin.strip_prefix("https://")?;
    let host = rest.split_once(':').map_or(rest, |(host, _)| host);
    (host.contains('.') && !host.contains('/')).then(|| host.to_owned())
}

/// A challenge's id: the ULID whose time is `issued_at`'s millisecond and whose randomness is the
/// challenge's first ten bytes.
pub fn challenge_id(issued_at: UtcNanos, bytes: &[u8; 32]) -> String {
    let millis = u64::try_from(issued_at.secs())
        .unwrap_or_default()
        .saturating_mul(1_000)
        .saturating_add(u64::from(issued_at.nanos() / 1_000_000))
        .to_be_bytes();
    let raw: Vec<u8> = millis
        .iter()
        .skip(2)
        .chain(bytes.iter().take(10))
        .copied()
        .collect();
    encode_ulid(u128::from_be_bytes(raw.try_into().unwrap_or_default()))
}
