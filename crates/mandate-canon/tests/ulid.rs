//! The canonical ULID text (#765). The oracles never call the codec to get an expected value: the
//! tables were computed outside the crate, and the properties spell and read digits by repeated
//! division and multiplication over this file's own copy of the alphabet.

use mandate_canon::{UlidTextError, decode_ulid, encode_ulid, is_ulid};
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// `(value, text)` pairs worked out by hand and checked in Python, not with this crate.
const TABLE: &[(u128, &str)] = &[
    (0, "00000000000000000000000000"),
    (1, "00000000000000000000000001"),
    (31, "0000000000000000000000000Z"),
    (32, "00000000000000000000000010"),
    (1 << 64, "0000000000000G000000000000"),
    ((1 << 125) - 1, "0ZZZZZZZZZZZZZZZZZZZZZZZZZ"),
    (1 << 125, "10000000000000000000000000"),
    (u128::MAX, "7ZZZZZZZZZZZZZZZZZZZZZZZZZ"),
    (
        0x0156_3e3a_b5d3_d676_4c61_efb9_9302_bd5b,
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
    ),
    (
        0x0110_c853_1d09_52d8_d73e_1194_e95b_5f19,
        "0123456789ABCDEFGHJKMNPQRS",
    ),
    (
        0xfadf_3bef_8022_190a_63a1_2a5b_1ae7_c232,
        "7TVWXYZ0123456789ABCDEFGHJ",
    ),
    (
        0x0192_3c05_0140_0000_0000_0000_0000_0321,
        "01J8Y0A0A000000000000000S1",
    ),
];

/// Texts that are not canonical ULIDs, with the reason the first failing check gives.
const REFUSED: &[(&str, UlidTextError)] = &[
    ("", UlidTextError::Length),
    ("0000000000000000000000000", UlidTextError::Length),
    ("000000000000000000000000000", UlidTextError::Length),
    ("80000000000000000000000000", UlidTextError::Overflow),
    ("ZZZZZZZZZZZZZZZZZZZZZZZZZZ", UlidTextError::Overflow),
    ("01arz3ndektsv4rrffq69g5fav", UlidTextError::NotCrockford),
    ("01ARZ3NDEKTSV4RRFFQ69G5FAv", UlidTextError::NotCrockford),
    ("0000000000000000000000000I", UlidTextError::NotCrockford),
    ("0000000000000000000000000L", UlidTextError::NotCrockford),
    ("0000000000000000000000000O", UlidTextError::NotCrockford),
    ("0000000000000000000000000U", UlidTextError::NotCrockford),
    ("0000000000000 000000000000", UlidTextError::NotCrockford),
    ("000000000000000000000000-0", UlidTextError::NotCrockford),
    ("00000000000000000000000000\n", UlidTextError::Length),
    ("0000000000000000000000000\n", UlidTextError::NotCrockford),
    ("\t000000000000000000000000+", UlidTextError::NotCrockford),
    (
        "0000000000000000000000000\u{e9}",
        UlidTextError::NotCrockford,
    ),
    ("000000000000000000000000\u{e9}", UlidTextError::Length),
    ("z0000000000000000000000000", UlidTextError::NotCrockford),
    ("8000000000000000000000000", UlidTextError::Length),
    ("8000000000000000000000000I", UlidTextError::NotCrockford),
];

/// The 26 digits of `value` by repeated division, least significant first, then reversed.
fn spelled(mut value: u128) -> String {
    let mut digits = [0_u8; 26];
    for slot in digits.iter_mut().rev() {
        *slot = ALPHABET[(value % 32) as usize];
        value /= 32;
    }
    assert_eq!(value, 0, "26 digits hold every u128");
    String::from_utf8(digits.to_vec()).unwrap()
}

/// The value of an all-alphabet text by multiplication, most significant first.
fn read(text: &str) -> u128 {
    text.bytes().fold(0_u128, |acc, b| {
        let digit = ALPHABET.iter().position(|&a| a == b).unwrap();
        acc * 32 + digit as u128
    })
}

fn run<S: Strategy>(strategy: S, body: impl Fn(S::Value) -> Result<(), TestCaseError>) {
    if let Err(failure) = TestRunner::deterministic().run(&strategy, body) {
        panic!("{failure}");
    }
}

/// A canonical text: a first digit `0`..=`7`, then 25 alphabet digits.
fn canonical_text() -> impl Strategy<Value = String> {
    (0_usize..8, prop::collection::vec(0_usize..32, 25)).prop_map(|(first, rest)| {
        std::iter::once(first)
            .chain(rest)
            .map(|d| char::from(ALPHABET[d]))
            .collect()
    })
}

#[test]
fn the_table_is_its_own_oracle() {
    for (value, text) in TABLE {
        assert_eq!(spelled(*value), *text, "spelled({value})");
        assert_eq!(read(text), *value, "read({text})");
    }
}

#[test]
fn encode_spells_every_table_row() {
    for (value, text) in TABLE {
        assert_eq!(encode_ulid(*value), *text, "encode_ulid({value})");
    }
}

#[test]
fn decode_reads_every_table_row() {
    for (value, text) in TABLE {
        assert_eq!(decode_ulid(text), Ok(*value), "decode_ulid({text})");
    }
}

#[test]
fn decode_refuses_each_non_canonical_text_with_the_first_reason() {
    for (text, reason) in REFUSED {
        assert_eq!(decode_ulid(text), Err(*reason), "decode_ulid({text:?})");
    }
    assert_eq!(
        decode_ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV").map(|_| ()),
        Ok(())
    );
}

#[test]
fn is_ulid_is_true_exactly_for_the_accepted_texts() {
    for (_, text) in TABLE {
        assert!(is_ulid(text), "is_ulid({text})");
    }
    for (text, _) in REFUSED {
        assert!(!is_ulid(text), "is_ulid({text:?})");
    }
}

#[test]
fn every_value_round_trips_through_its_spelled_text() {
    run(any::<u128>(), |value| {
        let text = spelled(value);
        prop_assert_eq!(encode_ulid(value), text.clone());
        prop_assert_eq!(decode_ulid(&text), Ok(value));
        prop_assert!(is_ulid(&text));
        Ok(())
    });
}

#[test]
fn every_canonical_text_reads_as_its_value_and_spells_back() {
    run(canonical_text(), |text| {
        let value = read(&text);
        prop_assert_eq!(decode_ulid(&text), Ok(value));
        prop_assert_eq!(encode_ulid(value), text);
        Ok(())
    });
}

#[test]
fn one_foreign_character_anywhere_is_refused() {
    let foreign = prop::sample::select(vec![
        'I', 'L', 'O', 'U', 'a', 'z', ' ', '-', '\n', '\t', '+', '\u{e9}',
    ]);
    run(
        (canonical_text(), 0_usize..26, foreign),
        |(text, at, ch)| {
            let mut chars: Vec<char> = text.chars().collect();
            chars[at] = ch;
            let broken: String = chars.into_iter().collect();
            prop_assert_eq!(decode_ulid(&text), Ok(read(&text)));
            prop_assert_eq!(decode_ulid(&broken), Err(UlidTextError::NotCrockford));
            prop_assert!(!is_ulid(&broken));
            Ok(())
        },
    );
}

#[test]
fn a_first_digit_above_seven_overflows() {
    run((canonical_text(), 8_usize..32), |(text, first)| {
        let over = format!("{}{}", char::from(ALPHABET[first]), &text[1..]);
        prop_assert_eq!(decode_ulid(&text), Ok(read(&text)));
        prop_assert_eq!(decode_ulid(&over), Err(UlidTextError::Overflow));
        prop_assert!(!is_ulid(&over));
        Ok(())
    });
}

#[test]
fn each_refusal_has_its_own_stable_code_and_message() {
    let all = [
        (UlidTextError::Length, "length", "not 26 characters long"),
        (
            UlidTextError::NotCrockford,
            "not_crockford",
            "a character outside uppercase Crockford base32",
        ),
        (
            UlidTextError::Overflow,
            "overflow",
            "first digit above 7, so the value does not fit 128 bits",
        ),
    ];
    for (reason, code, message) in all {
        assert_eq!(reason.code(), code);
        assert_eq!(reason.to_string(), message);
    }
}
