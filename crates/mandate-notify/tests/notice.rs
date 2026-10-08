//! NT-1 and NT-4: a notice id is minted from the random source alone, and can never be, or reveal,
//! a journal event id (notifications spec §2, §4.2).

mod common;

use common::{Broken, Recording, answer, hex};
use mandate_canon::Value;
use mandate_notify::{NoticeId, Notification, NotifyError, TextKey, payload};
use proptest::prelude::*;

/// The id as the payload carries it.
fn sent_id(notice: NoticeId) -> String {
    let value = answer(
        "payload",
        payload(&Notification {
            notice,
            text: TextKey::AttentionNeeded,
        }),
    );
    match value.get("notice") {
        Some(Value::Str(id)) => id.clone(),
        other => panic!("the payload's notice is {other:?}"),
    }
}

/// The journal's ULID shape (journal spec §3), checked here on its own.
fn parses_as_ulid(text: &str) -> bool {
    const CROCKFORD: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    text.len() == 26
        && text
            .to_ascii_uppercase()
            .bytes()
            .all(|b| CROCKFORD.contains(&b))
        && text.bytes().next().is_some_and(|b| b <= b'7')
}

/// A ULID's 128 bits, decoded here on its own.
fn ulid_bits(ulid: &str) -> [u8; 16] {
    const CROCKFORD: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let value = ulid.bytes().fold(0u128, |acc, c| {
        let digit = CROCKFORD.iter().position(|&d| d == c).unwrap();
        (acc << 5) | u128::try_from(digit).unwrap()
    });
    value.to_be_bytes()
}

#[test]
#[ignore = "pending E8-9"]
fn a_notice_id_is_the_random_sources_bits_in_lowercase_hex() {
    let mut random = Recording::seeded(7);
    let notice = answer("mint", NoticeId::mint(&mut random));
    assert_eq!(random.issued.len(), 1, "one draw per id");
    assert_eq!(sent_id(notice), hex(&random.issued[0]));
}

#[test]
#[ignore = "pending E8-9"]
fn a_failing_random_source_mints_no_notice() {
    assert_eq!(
        NoticeId::mint(&mut Broken),
        Err(NotifyError::EntropyUnavailable)
    );
}

/// DEC-702 item 2: an id is written as its source's bits in 32 lowercase hex digits, text reads
/// back only in that form, and the id it gives is the one that was minted.
#[test]
#[ignore = "pending E8-9"]
fn a_notice_id_round_trips_only_through_32_lowercase_hex() {
    let mut random = Recording::seeded(21);
    let minted = answer("mint", NoticeId::mint(&mut random));
    let id = hex(&random.issued[0]);
    assert_eq!(answer("hex", minted.hex()), id);
    assert_eq!(answer("parse", NoticeId::parse(&id)), minted);
    let upper = id.to_ascii_uppercase();
    for text in [
        "01J8ZNB0M000000000000000K1",
        "01j8znb0m000000000000000k1",
        "0123456789abcdef0123456789",
        "0123456789abcdef",
        &"0123456789abcdef".repeat(4),
        &upper,
        &id[..31],
        &format!("{id}0"),
        &format!("{}g", &id[..31]),
        &format!(" {}", &id[1..]),
        "",
    ] {
        assert_eq!(
            NoticeId::parse(text),
            Err(NotifyError::NotANoticeId),
            "{text:?}"
        );
    }
}

proptest! {
    /// NT-4: notices about one cause never share an id, and no id is, or parses as, a journaled
    /// event's ULID.
    #[test]
    #[ignore = "pending E8-9"]
    fn notices_about_one_cause_differ_and_none_is_an_event_id(
        seed in any::<u64>(),
        count in 2usize..8,
        journaled in prop::collection::vec("[0-7][0-9A-HJKMNP-TV-Z]{25}", 1..16),
    ) {
        let mut random = Recording::seeded(seed);
        let ids: Vec<String> = (0..count)
            .map(|_| sent_id(answer("mint", NoticeId::mint(&mut random))))
            .collect();
        prop_assert_eq!(random.issued.len(), count);
        for (id, bits) in ids.iter().zip(&random.issued) {
            prop_assert_eq!(id, &hex(bits));
            prop_assert!(id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
            prop_assert!(!parses_as_ulid(id));
            for event in &journaled {
                prop_assert!(!id.eq_ignore_ascii_case(event));
                prop_assert_ne!(bits, &ulid_bits(event));
            }
        }
        let mut distinct = ids.clone();
        distinct.sort();
        distinct.dedup();
        prop_assert_eq!(distinct.len(), count, "two notices for one cause share an id");
    }
}
