//! The recorded-response transport: replies recorded for an exact request body come first, then
//! the scripted queue; with neither, an attempt fails to connect. It logs every request.

mod common;

use common::*;
use mandate_canon::Digest;
use mandate_gateway::fake::RecordedTransport;
use mandate_gateway::provider::{
    Clock, Delivery, HttpRequest, ManualClock, ProviderKey, Transport,
};

fn request(body: &[u8]) -> HttpRequest {
    HttpRequest {
        host: HOST.to_owned(),
        path: "/p".to_owned(),
        body: body.to_vec(),
    }
}

#[test]
fn a_reply_recorded_for_a_request_is_replayed_before_the_script() {
    let mut transport = RecordedTransport::new();
    let mut clock = ManualClock::at(at(START));
    transport.record(
        Digest::of(b"a"),
        vec![delivered(200, b"for a".to_vec(), 250)],
    );
    transport.script(vec![delivered(500, b"scripted".to_vec(), 5)]);
    let deadline = at("2026-10-16T14:01:00.000000000Z");
    let key = ProviderKey::new(TEST_KEY.to_owned());
    let first = transport.send(&request(b"a"), Some(&key), deadline, &mut clock);
    assert_eq!(
        first,
        Delivery::Response {
            status: 200,
            body: b"for a".to_vec(),
            retry_after_ms: None
        }
    );
    assert_eq!(
        clock.now(),
        at("2026-10-16T14:00:00.250000000Z"),
        "the recorded latency passes"
    );
    let second = transport.send(&request(b"a"), None, deadline, &mut clock);
    assert!(
        matches!(second, Delivery::Response { status: 500, .. }),
        "then the script"
    );
    assert_eq!(
        transport.send(&request(b"b"), None, deadline, &mut clock),
        Delivery::ConnectFailed
    );
    let sent = transport.sent();
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[0].body_digest, Digest::of(b"a"));
    assert_eq!(sent[0].at, at(START));
    assert_eq!(sent[0].deadline, deadline);
    assert!(sent[0].with_key && !sent[1].with_key);
    assert_eq!(sent[2].body_digest, Digest::of(b"b"));
    transport.script(vec![delivered(200, vec![], 0)]);
    transport.clear_script();
    assert_eq!(
        transport.send(&request(b"c"), None, deadline, &mut clock),
        Delivery::ConnectFailed
    );
    assert_eq!(
        format!("{key:?}"),
        "ProviderKey(redacted)",
        "a key never prints"
    );
    assert_eq!(key.expose_secret(), TEST_KEY);
}

#[test]
fn the_manual_clock_moves_only_forward_and_when_told() {
    let mut clock = ManualClock::at(at(START));
    clock.advance_ms(1_500);
    assert_eq!(clock.now(), at("2026-10-16T14:00:01.500000000Z"));
    clock.wait_until(at(START));
    assert_eq!(
        clock.now(),
        at("2026-10-16T14:00:01.500000000Z"),
        "never backwards"
    );
    let mut last =
        ManualClock::at(mandate_time::UtcNanos::parse("9999-12-31T23:59:59.999999999Z").unwrap());
    let before = last.now();
    last.advance_ms(1);
    assert_eq!(
        last.now(),
        before,
        "a step past the last instant stays there"
    );
    assert_eq!(
        mandate_gateway::provider::before_ms(at(START), 1_000),
        at("2026-10-16T13:59:59.000000000Z")
    );
    assert_eq!(
        mandate_gateway::provider::before_ms(at("1970-01-01T00:00:00.500000000Z"), 1_000),
        mandate_time::UtcNanos::EPOCH
    );
    assert_eq!(
        mandate_gateway::provider::after_ms(at(START), 0),
        Some(at(START))
    );
}

#[test]
fn sequential_ids_are_ulids_counting_up() {
    use mandate_gateway::provider::{IdSource, SequentialIds};
    let mut ids = SequentialIds::starting_at(41);
    let (a, b) = (ids.next_id(), ids.next_id());
    assert!(mandate_canon::is_ulid(&a) && mandate_canon::is_ulid(&b));
    assert_eq!(mandate_canon::decode_ulid(&a), Ok(41));
    assert_eq!(mandate_canon::decode_ulid(&b), Ok(42));
}

#[test]
fn no_keys_holds_no_key() {
    use mandate_gateway::provider::{NoKeys, SecretError, SecretSource};
    assert_eq!(NoKeys.provider_key(UPSTREAM), Err(SecretError::Missing));
}
