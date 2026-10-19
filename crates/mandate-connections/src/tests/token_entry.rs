//! The vault's token entry, format 1 (DEC-2624): one byte form, written and read the same way, and
//! no other bytes accepted.

use mandate_canon::{parse, to_canonical};
use proptest::prelude::{any, prop};
use proptest::test_runner::{Config, TestRunner};
use secrecy::{ExposeSecret, SecretString};

use super::support::TOKEN;
use crate::ConnectError;
use crate::grant::check_scope;
use crate::token_entry::{MAX_TOKEN_LEN, decode, encode};
use crate::vault::AccessToken;

const ENTRY: &str = r#"{"access_token":"token-canary-77c1","format":1,"scope":["data","trading"],"token_type":"bearer"}"#;

fn token(text: &str) -> AccessToken {
    AccessToken(SecretString::from(text))
}

fn decoded(bytes: &[u8]) -> Result<(String, Vec<String>), ConnectError> {
    decode(bytes).map(|entry| {
        let scopes = entry.scopes.iter().map(str::to_owned).collect();
        (entry.token.0.expose_secret().to_owned(), scopes)
    })
}

fn granted() -> (String, Vec<String>) {
    (
        TOKEN.to_owned(),
        vec!["data".to_owned(), "trading".to_owned()],
    )
}

#[test]
fn the_entry_is_exactly_its_canonical_form() {
    let scopes = check_scope("trading data").unwrap();
    let entry = encode(&token(TOKEN), &scopes).unwrap();
    assert_eq!(entry.expose_secret(), ENTRY);
    let canonical = to_canonical(&parse(ENTRY.as_bytes()).unwrap());
    assert_eq!(
        canonical,
        ENTRY.as_bytes(),
        "the form is the canonical JSON of its members"
    );
}

#[test]
fn an_entry_decodes_to_its_token_and_scopes() {
    assert_eq!(decoded(ENTRY.as_bytes()), Ok(granted()));
}

#[test]
fn every_other_form_is_refused() {
    let refused: Vec<String> = vec![
        String::new(),
        "not json".to_owned(),
        ENTRY.replace(':', ": "),
        format!("{ENTRY}\n"),
        format!(" {ENTRY}"),
        r#"{"format":1,"access_token":"token-canary-77c1","scope":["data","trading"],"token_type":"bearer"}"#.to_owned(),
        ENTRY.replace(r#","format":1"#, ""),
        ENTRY.replace(r#","format":1"#, r#","format":2"#),
        ENTRY.replace(r#","format":1"#, r#","format":"1""#),
        ENTRY.replace("bearer", "Bearer"),
        ENTRY.replace("bearer", "mac"),
        ENTRY.replace(r#"["data","trading"]"#, r#"["trading","data"]"#),
        ENTRY.replace(r#"["data","trading"]"#, r#"["data","trading","account:write"]"#),
        ENTRY.replace(r#"["data","trading"]"#, r#"["trading"]"#),
        ENTRY.replace(r#"["data","trading"]"#, r#""data trading""#),
        ENTRY.replace(r#","token_type":"bearer"}"#, r#","token_type":"bearer","expires_in":3600}"#),
        ENTRY.replace("token-canary-77c1", ""),
        ENTRY.replace("token-canary-77c1", "token canary"),
        ENTRY.replace("token-canary-77c1", "token\\u002dcanary"),
        ENTRY.replace("token-canary-77c1", "token\"canary"),
        ENTRY.replace("token-canary-77c1", "tokén"),
        ENTRY.replace("token-canary-77c1", &"t".repeat(MAX_TOKEN_LEN + 1)),
    ];
    for bytes in &refused {
        assert_eq!(
            decoded(bytes.as_bytes()).map(|_| ()),
            Err(ConnectError::TokenEntryInvalid),
            "{bytes:.80?}"
        );
    }
    let longest = ENTRY.replace("token-canary-77c1", &"t".repeat(MAX_TOKEN_LEN));
    assert!(
        decode(longest.as_bytes()).is_ok(),
        "a token of the longest length is held"
    );
}

#[test]
fn a_token_outside_the_form_is_never_written() {
    let scopes = check_scope("trading data").unwrap();
    let long = "t".repeat(MAX_TOKEN_LEN + 1);
    for text in ["", "a b", "a\"b", "a\\b", "tokén", "a\nb", long.as_str()] {
        assert_eq!(
            encode(&token(text), &scopes).map(|_| ()),
            Err(ConnectError::TokenEntryInvalid),
            "{text:.40?}"
        );
    }
}

#[test]
fn a_refusal_and_a_decoded_entry_name_nothing_of_the_token() {
    let refusal = decode(ENTRY.replace("bearer", "mac").as_bytes()).unwrap_err();
    let entry = decode(ENTRY.as_bytes()).unwrap();
    for shown in [
        format!("{refusal:?}"),
        refusal.to_string(),
        format!("{entry:?}"),
    ] {
        assert!(!shown.contains(TOKEN), "{shown}");
    }
}

/// Every token of the form round-trips, and its entry is canonical JSON, checked by the journal's
/// own canonicalizer, an implementation the codec does not use.
#[test]
fn every_token_of_the_form_round_trips_as_canonical_json() {
    let scopes = check_scope("data trading").unwrap();
    let allowed: Vec<char> = ('!'..='~').filter(|c| !matches!(c, '"' | '\\')).collect();
    let strategy = prop::collection::vec(any::<prop::sample::Index>(), 1..64);
    TestRunner::new(Config::with_cases(256))
        .run(&strategy, |picks| {
            let text: String = picks.iter().map(|pick| *pick.get(&allowed)).collect();
            let entry = encode(&token(&text), &scopes).expect("a token of the form");
            let bytes = entry.expose_secret().as_bytes();
            proptest::prop_assert_eq!(to_canonical(&parse(bytes).expect("JSON")), bytes.to_vec());
            proptest::prop_assert_eq!(decoded(bytes).map(|(t, _)| t), Ok(text));
            Ok(())
        })
        .expect("every token of the form round-trips");
}
