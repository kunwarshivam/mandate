//! SNTP replies judged against the injected host clock (RFC 4330; journal spec §5.4), on synthetic
//! packets: no network.

use mandate_scheduler::sntp::{judge, request};
use mandate_time::UtcNanos;

const UNIX_FROM_NTP: u64 = 2_208_988_800;

fn at(secs: i64, nanos: u32) -> UtcNanos {
    UtcNanos::from_parts(secs, nanos).unwrap()
}

/// An NTP timestamp for whole seconds plus `millis`.
fn stamp(secs: i64, millis: u64) -> [u8; 8] {
    let ntp = u32::try_from(u64::try_from(secs).unwrap() + UNIX_FROM_NTP).unwrap();
    let fraction = u32::try_from((millis << 32) / 1_000).unwrap();
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(&ntp.to_be_bytes());
    out[4..].copy_from_slice(&fraction.to_be_bytes());
    out
}

/// A server reply to `request`: received at `t2` and sent at `t3`, each `(secs, millis)`.
fn reply(request: &[u8; 48], first: u8, stratum: u8, t2: (i64, u64), t3: (i64, u64)) -> [u8; 48] {
    let mut packet = [0u8; 48];
    packet[0] = first;
    packet[1] = stratum;
    packet[24..32].copy_from_slice(&request[40..48]);
    packet[32..40].copy_from_slice(&stamp(t2.0, t2.1));
    packet[40..48].copy_from_slice(&stamp(t3.0, t3.1));
    packet
}

const SERVER: u8 = 0b0010_0100;
const T: i64 = 1_792_155_600;

#[test]
fn the_offset_is_the_host_clock_minus_the_servers_and_the_delay_the_round_trip() {
    let sent = at(T, 0);
    let req = request(sent);
    assert_eq!(req[0], 0b0010_0011, "version 4, client mode");
    let answer = reply(&req, SERVER, 2, (T, 90), (T, 110));
    let sample = judge("time.example", &req, &answer, sent, at(T, 40_000_000)).unwrap();
    assert_eq!(sample.server, "time.example");
    assert_eq!(sample.stratum, 2);
    assert!(
        (sample.offset_ns + 80_000_000).abs() < 1_000,
        "host 80 ms behind: {}",
        sample.offset_ns
    );
    assert!(
        (sample.delay_ns - 20_000_000).abs() < 1_000,
        "{}",
        sample.delay_ns
    );
}

#[test]
fn a_host_clock_ahead_has_a_positive_offset() {
    let sent = at(T + 1, 0);
    let req = request(sent);
    let answer = reply(&req, SERVER, 1, (T, 0), (T, 0));
    let sample = judge("a", &req, &answer, sent, at(T + 1, 0)).unwrap();
    assert!((sample.offset_ns - 1_000_000_000).abs() < 1_000);
}

#[test]
fn a_reply_that_is_not_a_synchronized_servers_answer_to_this_request_is_no_sample() {
    let sent = at(T, 0);
    let req = request(sent);
    let good = reply(&req, SERVER, 2, (T, 0), (T, 0));
    assert!(judge("a", &req, &good, sent, sent).is_some());
    let client_mode = reply(&req, 0b0010_0011, 2, (T, 0), (T, 0));
    assert!(judge("a", &req, &client_mode, sent, sent).is_none());
    let alarm = reply(&req, 0b1110_0100, 2, (T, 0), (T, 0));
    assert!(
        judge("a", &req, &alarm, sent, sent).is_none(),
        "leap indicator 3: unsynchronized"
    );
    let other = request(at(T, 5));
    assert!(
        judge("a", &other, &good, sent, sent).is_none(),
        "an answer to another request"
    );
    let mut empty = good;
    empty[40..48].copy_from_slice(&[0; 8]);
    assert!(judge("a", &req, &empty, sent, sent).is_none());
}
