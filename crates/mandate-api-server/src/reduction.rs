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

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex, PoisonError};

use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::header::{CACHE_CONTROL, COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use mandate_api::problem::Violation;
use mandate_authn::{ClientKey, ReductionGate, ReductionLimits, Unauthenticated};
use mandate_canon::{Value, encode_ulid, parse};
use mandate_time::UtcNanos;
use ring::digest::{SHA256, digest};

use crate::config::StartRefusal;
use crate::http::{App, browser, forbidden, invalid, unauthenticated};
use crate::routes::{enveloped, string};
use crate::session::{OsRandom, Random};

/// The device cookie's name (§4.10 route 2 item 3).
pub const DEVICE_COOKIE: &str = "__Host-mandate_device";

/// How long a browser keeps the device cookie: 30 days, in seconds.
pub const DEVICE_MAX_AGE_S: i64 = 2_592_000;

/// The most challenges outstanding in all (DEC-2117 item 3).
pub const CHALLENGES_IN_ALL: usize = 4_096;

/// Past this many outstanding, an address that already holds one gets no more, so the last 1,024
/// go one per address (DEC-2119 item 11).
pub const RESERVE_FROM: usize = 3_072;

/// The setting naming the ingress's own addresses (§3.11 item 2).
pub const INGRESS_PEERS: &str = "MANDATE_API_INGRESS_PEERS";

/// The forwarded-address header the ingress appends to.
const FORWARDED_FOR: &str = "x-forwarded-for";

/// DEC-2117 item 3's numbers: 4 challenges per device and 32 per address outstanding, and 10
/// failures per device and 100 per address kept.
pub fn limits() -> ReductionLimits {
    ReductionLimits {
        challenges_per_address: NonZeroU32::MIN.saturating_add(31),
        challenges_per_device: NonZeroU32::MIN.saturating_add(3),
        failures_per_address: NonZeroU32::MIN.saturating_add(99),
        failures_per_device: NonZeroU32::MIN.saturating_add(9),
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

/// A challenge the store issued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issued {
    pub challenge_id: String,
    /// The address that asked for it, which the reserve counts.
    pub address: String,
    pub bytes: [u8; 32],
    pub issued_at: UtcNanos,
    pub expires_at: UtcNanos,
}

/// The gate, the challenges outstanding by id, the ingress peers, and the generator the challenge
/// bytes and device cookies come from. A lock a panic poisoned is taken as it stands, so a
/// failure elsewhere never closes route 2 (rule 13); the store's own guard is always the
/// outstanding map before the gate.
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
            gate: Mutex::new(ReductionGate::new(limits())),
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
    /// when the gate refuses, when [`CHALLENGES_IN_ALL`] are outstanding, past [`RESERVE_FROM`]
    /// when `client`'s address already holds one, when the id is already held, or when the
    /// generator fails.
    pub fn issue(&self, client: &ClientKey, now: UtcNanos) -> Result<Issued, Unauthenticated> {
        let mut outstanding = self
            .outstanding
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        outstanding.retain(|_, held| now < held.expires_at);
        let held = outstanding.len();
        let holds_one = || {
            outstanding
                .values()
                .any(|issued| issued.address == client.address)
        };
        if held >= CHALLENGES_IN_ALL || (held >= RESERVE_FROM && holds_one()) {
            return Err(Unauthenticated);
        }
        let mut bytes = [0_u8; 32];
        self.random.fill(&mut bytes).map_err(|_| Unauthenticated)?;
        let id = challenge_id(now, &bytes);
        if outstanding.contains_key(&id) {
            return Err(Unauthenticated);
        }
        let challenge = self
            .gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .issue(client, bytes, now)?;
        let issued = Issued {
            challenge_id: id.clone(),
            address: client.address.clone(),
            bytes,
            issued_at: now,
            expires_at: challenge.expires_at,
        };
        outstanding.insert(id, issued.clone());
        Ok(issued)
    }

    /// Whether the challenge `challenge_id` is held, issued and not yet taken or found expired.
    pub fn contains(&self, challenge_id: &str) -> bool {
        self.outstanding
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(challenge_id)
    }

    /// Fresh device cookie bytes, unpadded base64url.
    pub fn new_device(&self) -> Result<String, Unauthenticated> {
        let mut bytes = [0_u8; 32];
        self.random.fill(&mut bytes).map_err(|_| Unauthenticated)?;
        Ok(URL_SAFE_NO_PAD.encode(bytes))
    }
}

impl Default for Reduction {
    fn default() -> Self {
        Self::new(Vec::new(), Box::new(OsRandom::new()))
    }
}

/// Whether `body` is exactly an empty JSON object.
fn empty_object(body: &[u8]) -> bool {
    matches!(parse(body), Ok(Value::Object(members)) if members.is_empty())
}

/// `POST /v1/reduction-sessions/challenges` (§4.10 route 2): 403 `forbidden` without the browser
/// marks, `invalid` for any body but `{}`, the one 401 over a limit, else `201` with
/// `commands/reduction-session-challenge`, `Cache-Control: no-store`, and a device cookie when
/// the call carried none.
pub async fn challenge(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !browser(&headers, &app.settings.app_origin) {
        return forbidden();
    }
    if !empty_object(&body) {
        return invalid(vec![Violation::Schema {
            path: String::new(),
            code: "additionalProperties".to_owned(),
            message: "The request is an empty object.".to_owned(),
        }]);
    }
    let issued = (|| {
        let rp_id = rp_id(&app.settings.app_origin)?;
        let presented = device(&headers);
        let fresh = match presented {
            Some(_) => None,
            None => Some(app.reduction.new_device().ok()?),
        };
        let device = presented.or_else(|| fresh.as_deref().map(hex_digest))?;
        let client = ClientKey {
            address: address(peer.ip(), &headers, app.reduction.ingress()),
            device,
        };
        let now = app.clock.now()?;
        let issued = app.reduction.issue(&client, now).ok()?;
        Some((rp_id, fresh, issued, now))
    })();
    let Some((rp_id, fresh, issued, now)) = issued else {
        return unauthenticated();
    };
    let mut response = enveloped(
        StatusCode::CREATED,
        now,
        vec![
            ("challenge_id", string(&issued.challenge_id)),
            (
                "webauthn_challenge",
                string(&URL_SAFE_NO_PAD.encode(issued.bytes)),
            ),
            ("rp_id", string(&rp_id)),
            ("issued_at", string(&issued.issued_at.to_string())),
            ("expires_at", string(&issued.expires_at.to_string())),
            ("as_of", Value::Array(Vec::new())),
        ],
    );
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    let cookie = fresh.and_then(|value| {
        HeaderValue::from_str(&format!(
            "{DEVICE_COOKIE}={value}; Path=/; Max-Age={DEVICE_MAX_AGE_S}; Secure; HttpOnly; SameSite=Strict"
        ))
        .ok()
    });
    if let Some(cookie) = cookie {
        headers.insert(SET_COOKIE, cookie);
    }
    response
}
