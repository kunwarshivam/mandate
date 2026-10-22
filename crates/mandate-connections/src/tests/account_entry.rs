//! The personal-data vault's account entry, format 1 (DEC-2339): one byte form, written and read
//! the same way, no other bytes accepted, and the account id never printed.

use mandate_canon::{parse, to_canonical};
use proptest::prelude::prop;
use proptest::test_runner::{Config, TestRunner};
use secrecy::ExposeSecret;

use crate::ConnectError;
use crate::account_entry::{BrokerAccountId, MAX_ACCOUNT_ID_LEN, decode, encode};
use crate::record::AccountPiiRef;

const ID: &str = "904837e3-3b76-47ec-b432-046db621571b";
const REF: &str = "pii_01JA0000000000000000000000";
const ENTRY: &str = r#"{"account_id":"904837e3-3b76-47ec-b432-046db621571b","format":1,"pii_ref":"pii_01JA0000000000000000000000"}"#;

fn decoded(bytes: &[u8]) -> Result<(String, String), ConnectError> {
    decode(bytes).map(|entry| {
        (
            entry.pii_ref.as_str().to_owned(),
            entry.account_id.0.expose_secret().to_owned(),
        )
    })
}

/// The entry is the canonical JSON of the id, the format and the reference, and reads back as
/// the same two.
#[test]
fn the_entry_is_exactly_its_canonical_form() {
    let id = BrokerAccountId::new(ID).unwrap();
    let pii_ref = AccountPiiRef::new(REF).unwrap();
    let entry = encode(&pii_ref, &id).unwrap();
    assert_eq!(entry.expose_secret(), ENTRY);
    let canonical = to_canonical(&parse(ENTRY.as_bytes()).unwrap());
    assert_eq!(canonical, ENTRY.as_bytes());
    assert_eq!(
        decoded(ENTRY.as_bytes()),
        Ok((REF.to_owned(), ID.to_owned()))
    );
}

/// An id is 1 to 128 bytes from `!` to `~`, neither `"` nor `\`; anything else is refused, and
/// names nothing of it.
#[test]
fn an_account_id_is_printable_ascii_needing_no_escape() {
    assert_eq!(MAX_ACCOUNT_ID_LEN, 128);
    for kept in ["a", "!~", ID, &"x".repeat(128)] {
        assert_eq!(BrokerAccountId::new(kept).map(drop), Ok(()), "{kept:?}");
    }
    for refused in [
        "",
        &"x".repeat(129),
        "a b",
        "a\"b",
        "a\\b",
        "a\nb",
        "caf\u{e9}",
        "\u{7f}",
    ] {
        let error = BrokerAccountId::new(refused).map(drop).unwrap_err();
        assert_eq!(error, ConnectError::AccountEntryInvalid, "{refused:?}");
        if !refused.is_empty() {
            assert!(
                !format!("{error:?} {error}").contains(refused),
                "{refused:?}"
            );
        }
    }
}

/// Only format 1's exact bytes decode: another format, a member missing or added, reordered or
/// spaced members, an escape, a reference that is not `pii_` and a ULID, or no JSON at all.
#[test]
fn only_an_entry_in_its_exact_form_decodes() {
    for bytes in [
        String::new(),
        "{}".to_owned(),
        ENTRY.replace("\"format\":1", "\"format\":2"),
        ENTRY.replace(",\"format\":1", ""),
        ENTRY.replace("}", ",\"extra\":1}"),
        format!(r#"{{"format":1,"account_id":"{ID}","pii_ref":"{REF}"}}"#),
        ENTRY.replace(":", ": "),
        ENTRY.replace("904837e3", "9048\\u0037e3"),
        ENTRY.replace(REF, "pii_short"),
        ENTRY.replace(REF, "ref_01JA0000000000000000000000"),
        ENTRY.replace(ID, ""),
        format!("{ENTRY}\n"),
    ] {
        assert_eq!(
            decoded(bytes.as_bytes()),
            Err(ConnectError::AccountEntryInvalid),
            "{bytes}"
        );
    }
}

/// No `Debug` of an id, an entry or an error ever shows the id.
#[test]
fn no_debug_shows_the_account_id() {
    let id = BrokerAccountId::new(ID).unwrap();
    assert_eq!(format!("{id:?}"), "BrokerAccountId(redacted)");
    let entry = decode(ENTRY.as_bytes()).unwrap();
    assert!(!format!("{entry:?}").contains(ID));
    let refused = decode(ENTRY.replace("format\":1", "format\":3").as_bytes());
    assert!(!format!("{refused:?}").contains(ID));
}

/// Every id in the form round-trips through the entry unchanged.
#[test]
fn every_id_in_the_form_round_trips() {
    let pii_ref = AccountPiiRef::new(REF).unwrap();
    let mut runner = TestRunner::new(Config::default());
    runner
        .run(
            &prop::string::string_regex("[!#-\\[\\]-~]{1,128}").unwrap(),
            |text| {
                let id = BrokerAccountId::new(&text).unwrap();
                let entry = encode(&pii_ref, &id).unwrap();
                let back = decoded(entry.expose_secret().as_bytes()).unwrap();
                assert_eq!(back, (REF.to_owned(), text));
                Ok(())
            },
        )
        .unwrap();
}

/// Two ids are equal exactly when their text is.
#[test]
fn two_ids_are_equal_only_when_their_text_is() {
    assert_eq!(BrokerAccountId::new(ID), BrokerAccountId::new(ID));
    assert_ne!(BrokerAccountId::new(ID), BrokerAccountId::new("another-id"));
}

/// An entry is written only under a `pii_` reference: an identifier of another kind is refused,
/// though the id is in the form.
#[test]
fn an_entry_is_written_only_under_a_pii_reference() {
    let id = BrokerAccountId::new(ID).unwrap();
    for other in ["conn_01JA0000000000000000000000", "pii_not-a-ulid"] {
        let pii_ref = AccountPiiRef::new(other).unwrap();
        assert_eq!(
            encode(&pii_ref, &id).map(drop),
            Err(ConnectError::AccountEntryInvalid),
            "{other}"
        );
    }
}
