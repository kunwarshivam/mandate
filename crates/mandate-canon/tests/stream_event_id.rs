//! The event id a deterministic writer derives for a draft on a stream (journal spec §3: an
//! `event_id` is a ULID, globally unique). The oracles never call the derivation to get an expected
//! value: the table was computed in Python with `hashlib`, and the property hashes the preimage
//! with `sha2` directly and spells the ULID with this file's own copy of the alphabet.

use mandate_canon::{is_ulid, stream_event_id};
use proptest::prelude::*;
use sha2::{Digest, Sha256};

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// `(stream, epoch, head, ordinal, id)` worked out in Python, not with this crate.
const TABLE: &[(&str, u64, u64, u32, &str)] = &[
    (
        "acct:01KDEM0000000000000000000D:01KDEA0000000001000000000A",
        1,
        0,
        1,
        "5J0PPXPFYG36SRBHS4DBCGMKBH",
    ),
    (
        "agent:01KDEM0000000000000000000D:demo-spy",
        1,
        0,
        0,
        "2CDSYN6WKXYQWTBZJJ559NG1FD",
    ),
    (
        "clock:01KDEM0000000000000000000D",
        3,
        41,
        2,
        "102DZ772CHZY9JJ9210PQRC8GZ",
    ),
];

/// The id spelled independently: the first 128 bits of the SHA-256 of the domain tag, the
/// stream's length and bytes, then the epoch, head and ordinal, big-endian, in 26 base-32 digits.
fn oracle(stream: &str, epoch: u64, head: u64, ordinal: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"mandate.stream_event_id.v1\0");
    hasher.update(u64::try_from(stream.len()).unwrap().to_be_bytes());
    hasher.update(stream.as_bytes());
    hasher.update(epoch.to_be_bytes());
    hasher.update(head.to_be_bytes());
    hasher.update(ordinal.to_be_bytes());
    let digest = hasher.finalize();
    let mut value = u128::from_be_bytes(digest[..16].try_into().unwrap());
    let mut digits = [0_u8; 26];
    for digit in digits.iter_mut().rev() {
        *digit = ALPHABET[(value % 32) as usize];
        value /= 32;
    }
    String::from_utf8(digits.to_vec()).unwrap()
}

#[test]
fn the_derivation_matches_the_worked_table() {
    for &(stream, epoch, head, ordinal, id) in TABLE {
        assert_eq!(
            stream_event_id(stream, epoch, head, ordinal),
            id,
            "{stream}"
        );
    }
}

/// The two streams the cell collided on (lane DR's X9): two accounts' first batches, and two
/// agents' first batches, now have different ids for every draft of the batch.
#[test]
fn the_first_batches_of_two_streams_never_share_an_id() {
    for (one, two) in [
        (
            "acct:ws:01KDEA0000000001000000000A",
            "acct:ws:01KDEA0000000002000000000A",
        ),
        (
            "agent:ws:demo-spy",
            "agent:ws:agt_01KDEM00000000000000000WEB",
        ),
        ("clock:ws1", "clock:ws2"),
    ] {
        for ordinal in 0..4 {
            assert_ne!(
                stream_event_id(one, 1, 0, ordinal),
                stream_event_id(two, 1, 0, ordinal)
            );
        }
    }
}

proptest! {
    /// Any stream, epoch, head and ordinal: the id is the oracle's, a canonical ULID, and the
    /// same every time it is derived, so a retry after a crash offers the journal the same id.
    #[test]
    fn the_id_is_the_oracles_canonical_and_deterministic(
        stream in "[a-z]{1,8}:[0-9A-Za-z_:]{0,60}",
        epoch in any::<u64>(),
        head in any::<u64>(),
        ordinal in any::<u32>(),
    ) {
        let id = stream_event_id(&stream, epoch, head, ordinal);
        prop_assert_eq!(&id, &oracle(&stream, epoch, head, ordinal));
        prop_assert!(is_ulid(&id), "{}", id);
        prop_assert_eq!(id, stream_event_id(&stream, epoch, head, ordinal));
    }

    /// Two different streams never share an id over many heads and ordinals, and on one stream two
    /// different positions never do either: the stream's length is hashed before its bytes, so no
    /// stream and position can spell another's.
    #[test]
    fn no_two_positions_share_an_id(
        one in "[a-z]{1,8}:[0-9A-Za-z_:]{0,40}",
        two in "[a-z]{1,8}:[0-9A-Za-z_:]{0,40}",
        epoch in 0_u64..4,
        heads in proptest::collection::vec(0_u64..10_000, 1..16),
        ordinals in 0_u32..8,
    ) {
        let mut seen = std::collections::BTreeMap::new();
        for stream in [one.as_str(), two.as_str()] {
            for &head in &heads {
                for ordinal in 0..ordinals {
                    let at = (stream.to_owned(), epoch, head, ordinal);
                    let id = stream_event_id(stream, epoch, head, ordinal);
                    if let Some(earlier) = seen.insert(id.clone(), at.clone()) {
                        prop_assert_eq!(earlier, at, "{} derived twice", id);
                    }
                }
            }
        }
    }
}
