//! Route 2's challenge route (workspace API spec §4.10 route 2, identity spec §6.4 route 2,
//! [DEC-2117](../../../docs/project/decisions/DEC-2117.md),
//! [DEC-2118](../../../docs/project/decisions/DEC-2118.md)): `POST
//! /v1/reduction-sessions/challenges`, unauthenticated and browser-only, which issues a WebAuthn
//! challenge through `mandate_authn::reduction::ReductionGate` (DEC-653) and names nothing
//! workspace-specific.
//!
//! A request counts against its address and its device. The address is the TCP peer, or, from a
//! peer `MANDATE_API_INGRESS_PEERS` lists, the last `X-Forwarded-For` entry; an IPv6 address
//! counts as its /64. The device is the `__Host-mandate_device` cookie, 32 random bytes the route
//! sets and keys only by their SHA-256. Over any limit, or with 4,096 challenges outstanding in
//! all, the route answers the one 401. Challenges live in the server's memory, 300 s, and are
//! never journaled.

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::response::Response;
use mandate_authn::{ClientKey, ReductionGate, ReductionLimits, Unauthenticated};
use mandate_time::UtcNanos;

use crate::config::StartRefusal;
use crate::http::App;
use crate::session::{OsRandom, Random};

/// The device cookie's name (§4.10 route 2 item 3).
pub const DEVICE_COOKIE: &str = "__Host-mandate_device";

/// How long a browser keeps the device cookie: 30 days, in seconds.
pub const DEVICE_MAX_AGE_S: i64 = 2_592_000;

/// The most challenges outstanding in all (DEC-2117 item 3).
pub const CHALLENGES_IN_ALL: usize = 4_096;

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

/// The address a request counts against: `peer`, or the last `X-Forwarded-For` entry when `peer`
/// is one of `ingress` and that entry is an address. An IPv4 address, IPv4-mapped included, is
/// itself; an IPv6 address is its /64, as `<prefix>::/64`.
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
    let _ = headers;
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

/// A challenge the route issued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issued {
    pub challenge_id: String,
    pub bytes: [u8; 32],
    pub issued_at: UtcNanos,
    pub expires_at: UtcNanos,
}

/// The gate, the challenges outstanding by id, the ingress peers, and the generator the challenge
/// bytes and device cookies come from.
pub struct Reduction {
    gate: Mutex<ReductionGate>,
    outstanding: Mutex<BTreeMap<String, Issued>>,
    ingress: Vec<IpAddr>,
    random: Box<dyn Random>,
}

impl Reduction {
    /// A store with nothing outstanding, behind `ingress`, drawing from `random`.
    pub fn new(ingress: Vec<IpAddr>, random: Box<dyn Random>) -> Self {
        Self {
            gate: Mutex::new(ReductionGate::new(limits_or_one())),
            outstanding: Mutex::new(BTreeMap::new()),
            ingress,
            random,
        }
    }

    /// The ingress peers the store reads forwarded addresses from.
    pub fn ingress(&self) -> &[IpAddr] {
        &self.ingress
    }

    /// Issues a challenge for `client` at `now`: fresh bytes, held until `now` plus 300 s. Refused
    /// when the gate refuses, when [`CHALLENGES_IN_ALL`] are outstanding, or when the generator
    /// fails.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn issue(&self, client: &ClientKey, now: UtcNanos) -> Result<Issued, Unauthenticated> {
        let _ = (client, now, &self.gate, &self.outstanding, &self.random);
        todo!()
    }

    /// Fresh device cookie bytes, unpadded base64url.
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn new_device(&self) -> Result<String, Unauthenticated> {
        todo!()
    }
}

impl Default for Reduction {
    fn default() -> Self {
        Self::new(Vec::new(), Box::new(OsRandom::new()))
    }
}

/// The gate's limits until [`limits`] is served: one of each, so a stub never widens a limit.
fn limits_or_one() -> ReductionLimits {
    ReductionLimits {
        challenges_per_address: core::num::NonZeroU32::MIN,
        challenges_per_device: core::num::NonZeroU32::MIN,
        failures_per_address: core::num::NonZeroU32::MIN,
        failures_per_device: core::num::NonZeroU32::MIN,
    }
}

/// `POST /v1/reduction-sessions/challenges` (§4.10 route 2): 403 `forbidden` without the browser
/// marks, `invalid` for any body but `{}`, the one 401 over a limit, else `201` with
/// `commands/reduction-session-challenge`, `Cache-Control: no-store`, and a device cookie when
/// the call carried none.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub async fn challenge(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let _ = (app, peer, headers, body);
    todo!()
}
