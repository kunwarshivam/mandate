//! Route 2's challenge store (workspace API spec §4.10 route 2 item 3, DEC-2117, DEC-2119,
//! DEC-653): each challenge issued through the gate, at most 4,096 outstanding in all, and past
//! 3,072 none more to an address that already holds one; an id never issued twice, and a failed
//! draw issuing nothing.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use mandate_api_server::reduction::{self, CHALLENGES_IN_ALL, Reduction};
use mandate_api_server::session::{Random, Unauthenticated};
use mandate_authn::ClientKey;
use mandate_time::UtcNanos;

const T0: i64 = 1_791_000_000;

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("an instant")
}

/// A generator that hands out 0, 1, 2, ... in every byte of each draw, so no two draws repeat.
#[derive(Default)]
struct Counting(AtomicU8);

impl Random for Counting {
    fn fill(&self, out: &mut [u8]) -> Result<(), Unauthenticated> {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        out.iter_mut().for_each(|b| *b = n);
        Ok(())
    }
}

/// A generator whose every draw fails.
struct Failing;

impl Random for Failing {
    fn fill(&self, _out: &mut [u8]) -> Result<(), Unauthenticated> {
        Err(Unauthenticated)
    }
}

/// A generator that draws distinct 32-byte values for as long as a test needs, distinct in their
/// first eight bytes too.
#[derive(Default)]
struct Wide(Mutex<u64>);

impl Random for Wide {
    fn fill(&self, out: &mut [u8]) -> Result<(), Unauthenticated> {
        let mut n = self.0.lock().expect("unpoisoned");
        *n += 1;
        out.iter_mut().for_each(|b| *b = 0xa5);
        out[..8].copy_from_slice(&n.to_be_bytes());
        Ok(())
    }
}

/// A generator whose draws differ only past their first ten bytes, so two drawn in one
/// millisecond share a challenge id.
#[derive(Default)]
struct SharedPrefix(Mutex<u64>);

impl Random for SharedPrefix {
    fn fill(&self, out: &mut [u8]) -> Result<(), Unauthenticated> {
        let mut n = self.0.lock().expect("unpoisoned");
        *n += 1;
        out.iter_mut().for_each(|b| *b = 0x5a);
        out[24..].copy_from_slice(&n.to_be_bytes());
        Ok(())
    }
}

fn client(address: &str, device: &str) -> ClientKey {
    ClientKey {
        address: address.to_owned(),
        device: device.to_owned(),
    }
}

#[test]
fn the_store_issues_within_the_gate_and_holds_at_most_4096_in_all() {
    assert_eq!(CHALLENGES_IN_ALL, 4_096);
    assert_eq!(reduction::RESERVE_FROM, 3_072);
    let store = Reduction::new(Vec::new(), Box::new(Counting::default()));
    let now = at(T0);
    let first = store.issue(&client("a1", "d1"), now).expect("issued");
    assert_eq!(first.bytes, [0; 32]);
    assert_eq!(first.issued_at, now);
    assert_eq!(first.expires_at, at(T0 + 300));
    assert_eq!(
        first.challenge_id,
        reduction::challenge_id(now, &first.bytes)
    );
    for _ in 1..4 {
        store.issue(&client("a1", "d1"), now).expect("issued");
    }
    assert!(
        store.issue(&client("a1", "d1"), now).is_err(),
        "a fifth on one device"
    );
    for d in 2..=29 {
        store
            .issue(&client("a1", &format!("d{d}")), now)
            .expect("issued");
    }
    assert!(
        store.issue(&client("a1", "d99"), now).is_err(),
        "a 33rd on one address"
    );
    let edge = at(T0 + 299);
    assert!(
        store.issue(&client("a1", "d1"), edge).is_err(),
        "a second before expiry the device limit still binds"
    );
    assert!(
        store.issue(&client("a1", "d100"), edge).is_err(),
        "and so does the address limit"
    );
    assert!(
        store.contains(&first.challenge_id),
        "and nothing was dropped"
    );
    assert!(
        store.issue(&client("a1", "d1"), at(T0 + 300)).is_ok(),
        "expired challenges free their slots"
    );

    let wide = Reduction::new(Vec::new(), Box::new(Wide::default()));
    let held = wide.issue(&client("a0", "d0"), now).expect("issued");
    for n in 1..3_071 {
        wide.issue(&client(&format!("a{n}"), &format!("d{n}")), now)
            .expect("issued");
    }
    assert!(
        wide.issue(&client("a1", "e1"), now).is_ok(),
        "with 3,071 held a repeat address is still issued"
    );
    assert!(
        wide.issue(&client("a0", "e0"), now).is_err(),
        "past 3,072, an address that holds one gets no more"
    );
    assert!(
        wide.contains(&held.challenge_id),
        "the reserve refusal drops nothing"
    );
    assert!(wide.issue(&client("n0", "m0"), now).is_ok(), "the reserve");
    assert!(wide.issue(&client("n0", "m1"), now).is_err());
    for n in 1..1_024 {
        wide.issue(&client(&format!("n{n}"), &format!("m{n}")), now)
            .expect("issued from the reserve");
    }
    assert!(
        wide.issue(&client("fresh", "fresh"), now).is_err(),
        "the 4,097th outstanding in all"
    );
    assert!(
        wide.contains(&held.challenge_id),
        "the cap refusal drops nothing"
    );
    assert!(
        wide.issue(&client("fresh", "fresh"), edge).is_err(),
        "a second before expiry the cap still binds"
    );
    assert!(
        wide.issue(&client("a0", "e2"), edge).is_err(),
        "and so does the reserve"
    );
    assert!(wide.contains(&held.challenge_id));
    assert!(wide.issue(&client("fresh", "fresh"), at(T0 + 300)).is_ok());
    assert!(
        !wide.contains(&held.challenge_id),
        "an expired challenge is gone"
    );

    let shared = Reduction::new(Vec::new(), Box::new(SharedPrefix::default()));
    let kept = shared.issue(&client("a1", "d1"), now).expect("issued");
    assert!(
        shared.issue(&client("a2", "d2"), now).is_err(),
        "an id already held is never issued twice"
    );
    assert!(
        shared.contains(&kept.challenge_id),
        "the collision refusal keeps the one held"
    );
    assert!(
        !shared.contains("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
        "an id never issued"
    );
    let later = UtcNanos::from_parts(T0, 1_000_000).expect("an instant");
    assert!(shared.issue(&client("a2", "d2"), later).is_ok());

    let broken = Reduction::new(Vec::new(), Box::new(Failing));
    assert!(broken.issue(&client("a1", "d1"), now).is_err());
    assert!(broken.new_device().is_err());
    let device = Reduction::new(Vec::new(), Box::new(Counting::default()))
        .new_device()
        .expect("drawn");
    assert_eq!(device, URL_SAFE_NO_PAD.encode([0_u8; 32]));
}
