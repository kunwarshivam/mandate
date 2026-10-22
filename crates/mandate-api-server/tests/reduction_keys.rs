//! Route 2's request keys (workspace API spec §4.10 route 2 item 3, DEC-2117, DEC-2119): the
//! numbers the gate is built with, the ingress peers, the address a request counts against (an
//! IPv4 address, IPv4-mapped included, as itself; a native IPv6 address as its /64; a forwarded
//! one only from a listed peer), the device key, the RP ID, and a challenge's id.

use std::net::IpAddr;

use axum::http::{HeaderMap, HeaderValue, header};
use mandate_api_server::config::StartRefusal;
use mandate_api_server::reduction::{self, DEVICE_COOKIE, INGRESS_PEERS};
use mandate_authn::ReductionLimits;
use mandate_canon::decode_ulid;
use ring::digest::{SHA256, digest};

const T0: i64 = 1_791_000_000;
const DEVICE: &str = "q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA";

fn ip(text: &str) -> IpAddr {
    text.parse().expect("an address")
}

fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.append(
            header::HeaderName::from_bytes(name.as_bytes()).expect("a name"),
            HeaderValue::from_str(value).expect("a value"),
        );
    }
    map
}

#[test]
fn the_limits_are_dec_2117s_numbers() {
    let n = |v: u32| core::num::NonZeroU32::new(v).expect("not zero");
    assert_eq!(
        reduction::limits(),
        ReductionLimits {
            challenges_per_address: n(32),
            challenges_per_device: n(4),
            failures_per_address: n(100),
            failures_per_device: n(10),
        }
    );
}

#[test]
fn ingress_peers_name_addresses_and_refuse_anything_else() {
    assert_eq!(reduction::ingress_peers(None), Ok(Vec::new()));
    assert_eq!(reduction::ingress_peers(Some("")), Ok(Vec::new()));
    assert_eq!(
        reduction::ingress_peers(Some("127.0.0.1")),
        Ok(vec![ip("127.0.0.1")])
    );
    assert_eq!(
        reduction::ingress_peers(Some("127.0.0.1, ::1")),
        Ok(vec![ip("127.0.0.1"), ip("::1")])
    );
    for refused in [
        "10.0.0.0/8",
        "localhost",
        "127.0.0.1,",
        ",::1",
        "127.0.0.1;::1",
    ] {
        assert_eq!(
            reduction::ingress_peers(Some(refused)),
            Err(StartRefusal::Setting {
                name: INGRESS_PEERS
            }),
            "{refused}"
        );
    }
}

#[test]
fn an_ipv4_address_counts_as_itself_and_an_ipv6_as_its_64() {
    let none = HeaderMap::new();
    let at = |peer: &str| reduction::address(ip(peer), &none, &[]);
    assert_eq!(at("203.0.113.7"), "203.0.113.7");
    assert_eq!(
        at("::ffff:203.0.113.7"),
        "203.0.113.7",
        "an IPv4-mapped peer"
    );
    assert_eq!(at("2001:db8:1:2:3:4:5:6"), "2001:db8:1:2::/64");
    assert_eq!(at("2001:db8:1:2:ffff::1"), at("2001:db8:1:2::"));
    assert_ne!(at("2001:db8:1:3::1"), at("2001:db8:1:2::1"));
    assert_ne!(at("203.0.113.8"), at("203.0.113.7"));
}

#[test]
fn a_forwarded_address_counts_only_from_a_listed_ingress_peer() {
    let loopback = ip("127.0.0.1");
    let ingress = [loopback];
    let forwarded = headers(&[("x-forwarded-for", "198.51.100.1, 203.0.113.9")]);
    assert_eq!(
        reduction::address(loopback, &forwarded, &ingress),
        "203.0.113.9",
        "the entry the ingress appended"
    );
    assert_eq!(
        reduction::address(loopback, &forwarded, &[]),
        "127.0.0.1",
        "no ingress named: the peer"
    );
    assert_eq!(
        reduction::address(ip("10.0.0.5"), &forwarded, &ingress),
        "10.0.0.5",
        "a peer that is not the ingress forwards nothing"
    );
    assert_eq!(
        reduction::address(loopback, &HeaderMap::new(), &ingress),
        "127.0.0.1"
    );
    let junk = headers(&[("x-forwarded-for", "203.0.113.9, nonsense")]);
    assert_eq!(reduction::address(loopback, &junk, &ingress), "127.0.0.1");
    let lines = headers(&[
        ("x-forwarded-for", "198.51.100.1"),
        ("x-forwarded-for", "198.51.100.2, 2001:db8:9:9::1"),
    ]);
    assert_eq!(
        reduction::address(loopback, &lines, &ingress),
        "2001:db8:9:9::/64",
        "the lines are one list"
    );
    let through = headers(&[(
        "x-forwarded-for",
        "203.0.113.9, 127.0.0.1, ::ffff:127.0.0.1",
    )]);
    assert_eq!(
        reduction::address(loopback, &through, &ingress),
        "203.0.113.9",
        "walked from the right past every listed peer"
    );
    let mapped = headers(&[("x-forwarded-for", "::ffff:203.0.113.9")]);
    assert_eq!(
        reduction::address(loopback, &mapped, &ingress),
        "203.0.113.9",
        "a forwarded IPv4-mapped address counts as its IPv4 address, never as ::/64"
    );
    let across = headers(&[
        ("x-forwarded-for", "198.51.100.1, 203.0.113.9"),
        ("x-forwarded-for", "127.0.0.1, ::ffff:127.0.0.1"),
    ]);
    assert_eq!(
        reduction::address(loopback, &across, &ingress),
        "203.0.113.9",
        "a last line of listed peers only: the answer is on an earlier line"
    );
    let only = headers(&[("x-forwarded-for", "127.0.0.1")]);
    assert_eq!(reduction::address(loopback, &only, &ingress), "127.0.0.1");
    assert_eq!(
        reduction::address(ip("::ffff:127.0.0.1"), &forwarded, &ingress),
        "203.0.113.9",
        "an IPv4-mapped peer matches its IPv4 address"
    );
    assert_eq!(
        reduction::address(loopback, &forwarded, &[ip("::ffff:127.0.0.1")]),
        "203.0.113.9",
        "and a mapped listing matches the IPv4 peer"
    );
}

#[test]
fn the_device_key_is_a_digest_of_one_well_formed_cookie() {
    let cookie = |value: &str| headers(&[("cookie", value)]);
    let key = reduction::device(&cookie(&format!("{DEVICE_COOKIE}={DEVICE}"))).expect("a key");
    let hex: String = digest(&SHA256, DEVICE.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(key, hex, "the value's SHA-256, never the value");
    assert_eq!(
        reduction::device(&cookie(&format!(
            "__Host-mandate_session=abc; {DEVICE_COOKIE}={DEVICE}"
        ))),
        Some(hex)
    );
    let other = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
    assert_ne!(
        reduction::device(&cookie(&format!("{DEVICE_COOKIE}={other}"))),
        Some(key)
    );
    for refused in [
        String::new(),
        format!("mandate_device={DEVICE}"),
        format!("{DEVICE_COOKIE}={}", &DEVICE[..42]),
        format!("{DEVICE_COOKIE}={DEVICE}="),
        format!("{DEVICE_COOKIE}=q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJB"),
        format!("{DEVICE_COOKIE}={DEVICE}; {DEVICE_COOKIE}={other}"),
        format!("{DEVICE_COOKIE}={}", &DEVICE[..42]),
        format!("{DEVICE_COOKIE}=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHg"),
        format!("{DEVICE_COOKIE}=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8g"),
    ] {
        assert_eq!(reduction::device(&cookie(&refused)), None, "{refused}");
    }
    let split = headers(&[
        ("cookie", &format!("{DEVICE_COOKIE}={DEVICE}")),
        ("cookie", &format!("{DEVICE_COOKIE}={other}")),
    ]);
    assert_eq!(
        reduction::device(&split),
        None,
        "a second device cookie on another Cookie line"
    );
    assert_eq!(reduction::device(&HeaderMap::new()), None);
}

#[test]
fn the_rp_id_is_the_app_origins_host() {
    assert_eq!(
        reduction::rp_id("https://app.owlhead.ai").as_deref(),
        Some("app.owlhead.ai")
    );
    assert_eq!(
        reduction::rp_id("https://app-staging.owlhead.ai:8443").as_deref(),
        Some("app-staging.owlhead.ai")
    );
    for refused in [
        "http://app.owlhead.ai",
        "https://localhost",
        "app.owlhead.ai",
        "https://",
    ] {
        assert_eq!(reduction::rp_id(refused), None, "{refused}");
    }
}

#[test]
fn a_challenge_id_is_the_ulid_of_its_millisecond_and_first_ten_bytes() {
    let bytes: [u8; 32] = core::array::from_fn(|i| u8::try_from(i * 7 % 256).expect("a byte"));
    let issued = mandate_time::UtcNanos::from_parts(T0, 123_456_789).expect("an instant");
    let id = reduction::challenge_id(issued, &bytes);
    let value = decode_ulid(&id).expect("a ULID");
    let millis = u128::try_from(T0).expect("positive") * 1_000 + 123;
    let random = bytes[..10]
        .iter()
        .fold(0_u128, |acc, b| (acc << 8) | u128::from(*b));
    assert_eq!(value, (millis << 80) | random);
}
