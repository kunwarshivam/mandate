//! The ULID text codec spells every opaque ID as the journal does (journal spec §3): checked
//! against an encoder written here by repeated division, known vectors, and the journal's own
//! rule for what a ULID's text is.

use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};

use crate::{OrgId, PrincipalId, SessionRef, UlidTextError, WorkspaceId};

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The oracle: 26 base-32 digits, most significant first.
fn encode(mut value: u128) -> String {
    let mut digits = [b'0'; 26];
    for digit in digits.iter_mut().rev() {
        *digit = ALPHABET[(value % 32) as usize];
        value /= 32;
    }
    String::from_utf8(digits.to_vec()).unwrap()
}

/// One type's reading of a ULID's text, as its value.
type Parse = fn(&str) -> Result<u128, UlidTextError>;

/// Every type's text for `value`, and what each parses that text back to.
fn spellings(value: u128) -> Vec<(Result<String, UlidTextError>, Result<u128, UlidTextError>)> {
    let back = |text: &Result<String, UlidTextError>, parse: Parse| {
        text.as_ref()
            .map_or(Err(UlidTextError::Invalid), |t| parse(t))
    };
    let texts = [
        PrincipalId(value).to_ulid_text(),
        OrgId(value).to_ulid_text(),
        WorkspaceId(value).to_ulid_text(),
        SessionRef(value).to_ulid_text(),
    ];
    let parsers: [Parse; 4] = [
        |t| PrincipalId::from_ulid_text(t).map(|v| v.0),
        |t| OrgId::from_ulid_text(t).map(|v| v.0),
        |t| WorkspaceId::from_ulid_text(t).map(|v| v.0),
        |t| SessionRef::from_ulid_text(t).map(|v| v.0),
    ];
    texts
        .into_iter()
        .zip(parsers)
        .map(|(text, parse)| {
            let read = back(&text, parse);
            (text, read)
        })
        .collect()
}

#[test]
fn every_id_spells_its_ulid_as_the_oracle_and_reads_it_back() {
    for (value, text) in [
        (0, "00000000000000000000000000"),
        (1, "00000000000000000000000001"),
        (32, "00000000000000000000000010"),
        (u128::MAX, "7ZZZZZZZZZZZZZZZZZZZZZZZZZ"),
    ] {
        assert_eq!(encode(value), text, "the oracle itself");
        for spelled in spellings(value) {
            assert_eq!(spelled, (Ok(text.to_owned()), Ok(value)));
        }
    }
    let mut runner = TestRunner::new(Config {
        cases: 512,
        failure_persistence: None,
        ..Config::default()
    });
    let result = runner.run(&any::<u128>(), |value| {
        for (text, back) in spellings(value) {
            prop_assert_eq!(text, Ok(encode(value)));
            prop_assert_eq!(back, Ok(value));
        }
        Ok(())
    });
    if let Err(failure) = result {
        panic!("{failure}");
    }
}

#[test]
fn a_text_the_journal_would_refuse_is_no_id() {
    let valid = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    let expected: u128 = valid.bytes().fold(0, |acc, b| {
        acc * 32 + ALPHABET.iter().position(|&c| c == b).unwrap() as u128
    });
    assert_eq!(
        PrincipalId::from_ulid_text(valid),
        Ok(PrincipalId(expected))
    );
    for text in [
        "",
        "01ARZ3NDEKTSV4RRFFQ69G5FA",
        "01ARZ3NDEKTSV4RRFFQ69G5FAVX",
        "01arz3ndektsv4rrffq69g5fav",
        "01ARZ3NDEKTSV4RRFFQ69G5FAI",
        "01ARZ3NDEKTSV4RRFFQ69G5FAL",
        "01ARZ3NDEKTSV4RRFFQ69G5FAO",
        "01ARZ3NDEKTSV4RRFFQ69G5FAU",
        "81ARZ3NDEKTSV4RRFFQ69G5FAV",
        "01ARZ3NDEKTSV4RRFFQ69G5FA ",
    ] {
        assert_eq!(
            PrincipalId::from_ulid_text(text),
            Err(UlidTextError::Invalid),
            "{text:?}"
        );
        assert_eq!(
            OrgId::from_ulid_text(text),
            Err(UlidTextError::Invalid),
            "{text:?}"
        );
        assert_eq!(
            WorkspaceId::from_ulid_text(text),
            Err(UlidTextError::Invalid),
            "{text:?}"
        );
        assert_eq!(
            SessionRef::from_ulid_text(text),
            Err(UlidTextError::Invalid),
            "{text:?}"
        );
    }
}
