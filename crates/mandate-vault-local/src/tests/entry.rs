//! An entry's `key_ref` is checked, its file name is injective, and its associated data binds
//! the kind, the `key_ref`, and what it belongs to (DEC-692 item 4).

use std::collections::BTreeSet;

use mandate_connections::record::Environment;
use proptest::prelude::*;

use super::fixtures::{client_secret, code, conn, key_ref, token};
use crate::VaultError;
use crate::entry::{Entry, KeyRef, Kind, MAX_KEY_REF_LEN};

#[test]
fn a_key_ref_takes_only_its_alphabet_and_length() {
    for good in [
        "a",
        "platform/alpaca-oauth-client-secret",
        "ws_01/conn-A.Z9",
        &"x".repeat(MAX_KEY_REF_LEN),
    ] {
        assert_eq!(KeyRef::new(good).unwrap().as_str(), good);
    }
    for bad in [
        "",
        " ",
        "a b",
        "a\0b",
        "a\nb",
        "é",
        "a%2F",
        "a:b",
        "a\\b",
        &"x".repeat(MAX_KEY_REF_LEN + 1),
    ] {
        assert_eq!(KeyRef::new(bad), Err(VaultError::InvalidKeyRef), "{bad:?}");
    }
}

/// The oracle: each field as a two-byte big-endian length and its bytes, after the version tag.
fn fields(parts: &[&str]) -> Vec<u8> {
    let mut out = b"mandate-vault-local/v1\0".to_vec();
    for part in parts {
        out.extend_from_slice(&u16::try_from(part.len()).unwrap().to_be_bytes());
        out.extend_from_slice(part.as_bytes());
    }
    out
}

#[test]
fn the_associated_data_is_the_pinned_encoding() {
    assert_eq!(
        code("ws/c1", "c1").associated_data(),
        fields(&["code", "ws/c1", "connection", "c1", "paper"])
    );
    assert_eq!(
        Entry::token(key_ref("ws/c2"), conn("c2"), Environment::Live).associated_data(),
        fields(&["token", "ws/c2", "connection", "c2", "live"])
    );
    assert_eq!(
        client_secret().associated_data(),
        fields(&[
            "client_secret",
            "platform/alpaca-oauth-client-secret",
            "platform"
        ])
    );
}

#[test]
fn the_file_name_is_the_key_ref_in_hex_and_the_kind() {
    assert_eq!(code("ab/", "c").file_name(), "61622f.code");
    assert_eq!(token("ab/", "c").file_name(), "61622f.token");
    assert_eq!(
        Entry::client_secret(key_ref("ab/")).file_name(),
        "61622f.client_secret"
    );
    assert_eq!(Kind::Code.dir(), "pending");
    assert_eq!(Kind::Token.dir(), "tokens");
    assert_eq!(Kind::ClientSecret.dir(), "tokens");
}

fn any_entry() -> impl Strategy<Value = Entry> {
    let key = "[A-Za-z0-9._/-]{1,12}";
    let id = "[A-Za-z0-9_-]{1,6}";
    (0_u8..4, key, id, any::<bool>()).prop_map(|(kind, key, id, live)| {
        let environment = if live {
            Environment::Live
        } else {
            Environment::Paper
        };
        match kind {
            0 => Entry::code(key_ref(&key), conn(&id), environment),
            1 => Entry::token(key_ref(&key), conn(&id), environment),
            _ => Entry::client_secret(key_ref(&key)),
        }
    })
}

proptest! {
    /// Two different entries never share associated data, and two entries with a different
    /// `key_ref` or kind never share a file name.
    #[test]
    fn distinct_entries_never_collide(entries in prop::collection::vec(any_entry(), 2..24)) {
        let mut seen_data = BTreeSet::new();
        let mut seen_names = BTreeSet::new();
        let mut distinct = Vec::new();
        for entry in entries {
            if !distinct.contains(&entry) {
                distinct.push(entry);
            }
        }
        let mut name_keys = BTreeSet::new();
        for entry in &distinct {
            prop_assert!(seen_data.insert(entry.associated_data()), "{entry:?}");
            let name = entry.file_name();
            prop_assert!(name.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase() || b == b'.' || b == b'_' || b.is_ascii_lowercase()));
            if name_keys.insert((entry.key_ref().as_str().to_owned(), entry.kind().as_str())) {
                prop_assert!(seen_names.insert(name), "{entry:?}");
            }
        }
    }
}
