//! The vault's code entry, format 1 (DEC-2338 item 2): one byte form, written by the API and taken
//! by the token exchange the same way, and no other bytes accepted.

use mandate_canon::{parse, to_canonical};
use proptest::prelude::{any, prop};
use proptest::test_runner::{Config, TestRunner};
use secrecy::{ExposeSecret, SecretString};

use crate::ConnectError;
use crate::code_entry::{MAX_CODE_LEN, MAX_VERIFIER_LEN, MIN_VERIFIER_LEN, decode, encode};
use crate::start::PkceVerifier;
use crate::vault::AuthorizationCode;

const CODE: &str = "code-canary-5e1a";
const VERIFIER: &str = "verifier-canary-0123456789abcdefghijklmnopq.~_";
const ENTRY: &str = r#"{"code":"code-canary-5e1a","format":1,"verifier":"verifier-canary-0123456789abcdefghijklmnopq.~_"}"#;

fn code(text: &str) -> AuthorizationCode {
    AuthorizationCode(SecretString::from(text))
}

fn verifier(text: &str) -> PkceVerifier {
    PkceVerifier(SecretString::from(text))
}

fn decoded(bytes: &[u8]) -> Result<(String, String), ConnectError> {
    decode(bytes).map(|entry| {
        (
            entry.code.0.expose_secret().to_owned(),
            entry.verifier.0.expose_secret().to_owned(),
        )
    })
}

#[test]
#[ignore = "pending E10-13"]
fn the_entry_is_exactly_its_canonical_form() {
    let entry = encode(&code(CODE), &verifier(VERIFIER)).unwrap();
    assert_eq!(entry.expose_secret(), ENTRY);
    assert_eq!(
        to_canonical(&parse(ENTRY.as_bytes()).unwrap()),
        ENTRY.as_bytes(),
        "the form is the canonical JSON of its members"
    );
}

#[test]
#[ignore = "pending E10-13"]
fn an_entry_decodes_to_its_code_and_verifier() {
    assert_eq!(
        decoded(ENTRY.as_bytes()),
        Ok((CODE.to_owned(), VERIFIER.to_owned()))
    );
}

#[test]
#[ignore = "pending E10-13"]
fn every_other_form_is_refused() {
    let short = "v".repeat(MIN_VERIFIER_LEN - 1);
    let long = "v".repeat(MAX_VERIFIER_LEN + 1);
    let refused: Vec<String> = vec![
        String::new(),
        "not json".to_owned(),
        ENTRY.replace(':', ": "),
        format!("{ENTRY}\n"),
        format!(" {ENTRY}"),
        format!(r#"{{"format":1,"code":"{CODE}","verifier":"{VERIFIER}"}}"#),
        ENTRY.replace(r#","format":1"#, ""),
        ENTRY.replace(r#","format":1"#, r#","format":2"#),
        ENTRY.replace(r#","format":1"#, r#","format":"1""#),
        ENTRY.replace(r#""}"#, r#"","state":"s"}"#),
        ENTRY.replace(CODE, ""),
        ENTRY.replace(CODE, "code canary"),
        ENTRY.replace(CODE, "code\\u002dcanary"),
        ENTRY.replace(CODE, "code\"canary"),
        ENTRY.replace(CODE, "codé"),
        ENTRY.replace(CODE, &"c".repeat(MAX_CODE_LEN + 1)),
        ENTRY.replace(VERIFIER, ""),
        ENTRY.replace(VERIFIER, &short),
        ENTRY.replace(VERIFIER, &long),
        ENTRY.replace(VERIFIER, &format!("{}!", &VERIFIER[1..])),
        ENTRY.replace(VERIFIER, &format!("{}+", &VERIFIER[1..])),
    ];
    for bytes in &refused {
        assert_eq!(
            decoded(bytes.as_bytes()).map(|_| ()),
            Err(ConnectError::CodeEntryInvalid),
            "{bytes:.80?}"
        );
    }
    assert_eq!(
        decode(b"{\"code\":\"a\xff\",\"format\":1,\"verifier\":\"x\"}").map(|_| ()),
        Err(ConnectError::CodeEntryInvalid),
        "bytes that are not UTF-8"
    );
}

#[test]
#[ignore = "pending E10-13"]
fn the_bounds_of_each_value_are_held() {
    let codes = ["c".to_owned(), "c".repeat(MAX_CODE_LEN)];
    let verifiers = ["v".repeat(MIN_VERIFIER_LEN), "v".repeat(MAX_VERIFIER_LEN)];
    for c in &codes {
        for v in &verifiers {
            let entry = encode(&code(c), &verifier(v)).expect("a value at its bound");
            assert_eq!(
                decoded(entry.expose_secret().as_bytes()),
                Ok((c.clone(), v.clone())),
                "{}, {}",
                c.len(),
                v.len()
            );
        }
    }
}

#[test]
#[ignore = "pending E10-13"]
fn a_value_outside_the_form_is_never_written() {
    let long_code = "c".repeat(MAX_CODE_LEN + 1);
    for text in [
        "",
        "a b",
        "a\"b",
        "a\\b",
        "codé",
        "a\nb",
        long_code.as_str(),
    ] {
        assert_eq!(
            encode(&code(text), &verifier(VERIFIER)).map(|_| ()),
            Err(ConnectError::CodeEntryInvalid),
            "code {text:.40?}"
        );
    }
    let short = "v".repeat(MIN_VERIFIER_LEN - 1);
    let long = "v".repeat(MAX_VERIFIER_LEN + 1);
    let plus = format!("{}+", &VERIFIER[1..]);
    let quote = format!("{}\"", &VERIFIER[1..]);
    for text in [
        "",
        short.as_str(),
        long.as_str(),
        plus.as_str(),
        quote.as_str(),
    ] {
        assert_eq!(
            encode(&code(CODE), &verifier(text)).map(|_| ()),
            Err(ConnectError::CodeEntryInvalid),
            "verifier {text:.40?}"
        );
    }
}

#[test]
#[ignore = "pending E10-13"]
fn a_refusal_and_a_decoded_entry_name_nothing_of_the_code_or_verifier() {
    let refusal = decode(ENTRY.replace(r#""format":1"#, r#""format":2"#).as_bytes()).unwrap_err();
    let entry = decode(ENTRY.as_bytes()).unwrap();
    for shown in [
        format!("{refusal:?}"),
        refusal.to_string(),
        format!("{entry:?}"),
    ] {
        assert!(
            !shown.contains(CODE) && !shown.contains(VERIFIER),
            "{shown}"
        );
    }
}

/// Every code and verifier of the form round-trip, and their entry is canonical JSON, checked by
/// the journal's own canonicalizer, an implementation the codec does not use.
#[test]
#[ignore = "pending E10-13"]
fn every_value_of_the_form_round_trips_as_canonical_json() {
    let code_chars: Vec<char> = ('!'..='~').filter(|c| !matches!(c, '"' | '\\')).collect();
    let verifier_chars: Vec<char> = ('!'..='~')
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~'))
        .collect();
    let strategy = (
        prop::collection::vec(any::<prop::sample::Index>(), 1..64),
        prop::collection::vec(
            any::<prop::sample::Index>(),
            MIN_VERIFIER_LEN..=MAX_VERIFIER_LEN,
        ),
    );
    TestRunner::new(Config::with_cases(256))
        .run(&strategy, |(code_picks, verifier_picks)| {
            let c: String = code_picks.iter().map(|p| *p.get(&code_chars)).collect();
            let v: String = verifier_picks
                .iter()
                .map(|p| *p.get(&verifier_chars))
                .collect();
            let entry = encode(&code(&c), &verifier(&v)).expect("values of the form");
            let bytes = entry.expose_secret().as_bytes();
            proptest::prop_assert_eq!(to_canonical(&parse(bytes).expect("JSON")), bytes.to_vec());
            proptest::prop_assert_eq!(decoded(bytes), Ok((c, v)));
            Ok(())
        })
        .expect("every value of the form round-trips");
}

/// Any bytes the codec did not write are refused, or decode to values that write back to exactly
/// those bytes: the entry has one byte form.
#[test]
#[ignore = "pending E10-13"]
fn any_bytes_that_decode_are_their_own_encoding() {
    let strategy = prop::collection::vec(any::<u8>(), 0..160);
    TestRunner::new(Config::with_cases(512))
        .run(&strategy, |bytes| {
            let mut near = ENTRY.as_bytes().to_vec();
            near.truncate(bytes.len().min(near.len()));
            near.extend_from_slice(&bytes);
            for candidate in [bytes, near] {
                if let Ok(entry) = decode(&candidate) {
                    let again = encode(&entry.code, &entry.verifier).expect("decoded values");
                    proptest::prop_assert_eq!(again.expose_secret().as_bytes(), &candidate[..]);
                }
            }
            Ok(())
        })
        .expect("decoded bytes are canonical");
}
