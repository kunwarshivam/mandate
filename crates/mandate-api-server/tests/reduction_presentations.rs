//! A presentation to route 2's opening route (workspace API spec §4.10 route 2 items 1 and 3;
//! DEC-653, DEC-2119; E9-1 S2b-3b): the challenge is taken out on its first presentation whatever
//! follows, and the gate counts each presentation that opened nothing, raising the alert once when
//! a client first reaches its failure limit.

use std::sync::Mutex;

use mandate_api_server::reduction::Reduction;
use mandate_api_server::session::{Random, Unauthenticated};
use mandate_authn::ClientKey;
use mandate_time::UtcNanos;

const T0: i64 = 1_791_000_000;

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("an instant")
}

/// Distinct draws, none of them all zeros.
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

fn store() -> Reduction {
    Reduction::new(Vec::new(), Box::new(Wide::default()))
}

fn client(address: &str, device: &str) -> ClientKey {
    let (address, device) = (address.to_owned(), device.to_owned());
    ClientKey { address, device }
}

#[test]
#[ignore = "pending E9-1"]
fn a_challenge_is_taken_once_and_never_after_it_expires() {
    let store = store();
    let who = client("198.51.100.1", "d1");
    let issued = store.issue(&who, at(T0)).expect("issued");
    assert_eq!(
        store.take(&issued.challenge_id, at(T0 + 1)),
        Some(issued.clone())
    );
    assert!(!store.contains(&issued.challenge_id), "taken out");
    assert_eq!(store.take(&issued.challenge_id, at(T0 + 2)), None, "once");
    assert_eq!(
        store.take("01JB3K6F0C9R2V7N4M8Q1T5W3Y", at(T0 + 2)),
        None,
        "unknown"
    );
    let late = store.issue(&who, at(T0 + 10)).expect("issued");
    assert_eq!(
        store.take(&late.challenge_id, at(T0 + 10 + 299)),
        Some(late.clone()),
        "inside its 300 s"
    );
    let expired = store.issue(&who, at(T0 + 20)).expect("issued");
    assert_eq!(
        store.take(&expired.challenge_id, at(T0 + 20 + 300)),
        None,
        "at its expiry"
    );
    assert!(
        !store.contains(&expired.challenge_id),
        "and gone after the attempt"
    );
}

#[test]
#[ignore = "pending E9-1"]
fn a_settled_challenge_frees_its_slot_whatever_the_outcome() {
    let store = store();
    let who = client("198.51.100.2", "d2");
    let held: Vec<_> = (0..4)
        .map(|i| store.issue(&who, at(T0 + i)).expect("issued"))
        .collect();
    assert_eq!(
        store.issue(&who, at(T0 + 5)),
        Err(mandate_authn::Unauthenticated),
        "the device holds its 4"
    );
    let taken = store.take(&held[0].challenge_id, at(T0 + 6));
    assert!(taken.is_some());
    assert_eq!(
        store.issue(&who, at(T0 + 6)),
        Err(mandate_authn::Unauthenticated),
        "taken but not yet settled, the gate still counts it"
    );
    assert!(!store.settle(&who, taken.as_ref(), true, at(T0 + 7)));
    assert!(store.issue(&who, at(T0 + 8)).is_ok(), "an opening frees it");
    let refused = store.take(&held[1].challenge_id, at(T0 + 9));
    assert!(!store.settle(&who, refused.as_ref(), false, at(T0 + 9)));
    assert!(store.issue(&who, at(T0 + 10)).is_ok(), "so does a refusal");
}

#[test]
#[ignore = "pending E9-1"]
fn failures_count_only_when_nothing_opened_and_the_limit_raises_the_alert_once() {
    let store = store();
    let member = client("198.51.100.3", "laptop");
    for i in 0..20 {
        let issued = store.issue(&member, at(T0 + i)).expect("issued");
        let taken = store.take(&issued.challenge_id, at(T0 + i));
        assert!(!store.settle(&member, taken.as_ref(), true, at(T0 + i)));
    }
    let flood = client("198.51.100.3", "flood");
    let raised: Vec<bool> = (0..12)
        .map(|i| store.settle(&flood, None, false, at(T0 + 30 + i)))
        .collect();
    assert_eq!(
        raised,
        [vec![false; 9], vec![true], vec![false; 2]].concat(),
        "the tenth failure of a device reaches its limit, and only it raises the alert"
    );
    let member_issued = store.issue(&member, at(T0 + 50)).expect("issued");
    let taken = store.take(&member_issued.challenge_id, at(T0 + 50));
    assert!(
        !store.settle(&member, taken.as_ref(), false, at(T0 + 50)),
        "the member's own device is at one failure, its twenty openings counting none"
    );
    let raised_again = store.settle(&flood, None, false, at(T0 + 339));
    assert!(
        !raised_again,
        "all but its failures at 40 s and 41 s lapsed, so this third leaves it under its limit"
    );
    let after_lapse: Vec<bool> = (0..9)
        .map(|i| store.settle(&flood, None, false, at(T0 + 400 + i)))
        .collect();
    assert_eq!(
        after_lapse,
        [vec![false; 8], vec![true]].concat(),
        "once its failures lapsed below the limit, reaching it again raises the alert again"
    );
}

#[test]
#[ignore = "pending E9-1"]
fn refused_presentations_of_taken_challenges_count_and_the_tenth_raises_the_alert() {
    let store = store();
    let guesser = client("198.51.100.4", "guesser");
    let raised: Vec<bool> = (0..12)
        .map(|i| {
            let issued = store.issue(&guesser, at(T0 + i)).expect("issued");
            let taken = store.take(&issued.challenge_id, at(T0 + i));
            assert!(taken.is_some(), "a real challenge, taken");
            store.settle(&guesser, taken.as_ref(), false, at(T0 + i))
        })
        .collect();
    assert_eq!(
        raised,
        [vec![false; 9], vec![true], vec![false; 2]].concat(),
        "each refused presentation of a taken challenge is a failure"
    );
    let member = client("198.51.100.4", "member");
    for i in 0..12 {
        let issued = store.issue(&member, at(T0 + 20 + i)).expect("issued");
        let taken = store.take(&issued.challenge_id, at(T0 + 20 + i));
        assert!(!store.settle(&member, taken.as_ref(), true, at(T0 + 20 + i)));
    }
    let issued = store.issue(&member, at(T0 + 40)).expect("issued");
    let taken = store.take(&issued.challenge_id, at(T0 + 40));
    assert!(
        !store.settle(&member, taken.as_ref(), false, at(T0 + 40)),
        "the member's twelve openings counted none, so this failure is its first"
    );
}

#[test]
#[ignore = "pending E9-1"]
fn an_address_reaches_its_limit_at_its_hundredth_failure_across_devices() {
    let store = store();
    let raised: Vec<bool> = (0..104)
        .map(|i| {
            let device = format!("device-{}", i / 4);
            store.settle(&client("203.0.113.9", &device), None, false, at(T0 + i))
        })
        .collect();
    let expected: Vec<bool> = (0..104).map(|i| i == 99).collect();
    assert_eq!(
        raised, expected,
        "no device reaches its 10; the address reaches its 100 at the hundredth failure, once"
    );
}
