//! The paper credentials an executor reads from its vault lease (DEC-2306 item 2): the canonical
//! JSON object `{"key_id": …, "secret": …}` the connect flow writes at the connection's path. Every
//! refusal names its rule, and neither value ever reaches an error, `Debug`, or `Display`
//! (`AGENTS.md` rule 7).

use mandate_alpaca::{Credentials, KEY_ID_VAR, SECRET_VAR};
use mandate_canon::{Key, Object, Value, to_canonical};
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, TestRunner};

const KEY_ID: &str = "PKVAULTSENTINEL00001";
const SECRET: &str = "vaultSentinelSecretValue0123456789abcdefgh";

fn from_lookup(key_id: &str, secret: &str) -> Credentials {
    Credentials::from_lookup(|name| match name {
        KEY_ID_VAR => Some(key_id.to_owned()),
        SECRET_VAR => Some(secret.to_owned()),
        _ => None,
    })
    .unwrap_or_else(|e| panic!("both are set: {}", e.code()))
}

/// The canonical object of `members`, each a string.
fn secret_of(members: &[(&str, &str)]) -> Vec<u8> {
    let mut object = Object::new();
    for (name, value) in members {
        object.insert(
            Key::new(name).unwrap_or_else(|_| panic!("{name} is a key")),
            Value::Str((*value).to_owned()),
        );
    }
    to_canonical(&Value::Object(object))
}

/// Two credentials are equal exactly when both values are.
#[test]
fn credentials_are_equal_when_both_values_are() {
    assert_eq!(from_lookup(KEY_ID, SECRET), from_lookup(KEY_ID, SECRET));
    assert_ne!(from_lookup(KEY_ID, SECRET), from_lookup("PKOTHER", SECRET));
    assert_ne!(
        from_lookup(KEY_ID, SECRET),
        from_lookup(KEY_ID, "otherSecret")
    );
}

#[test]
fn the_vault_secret_reads_as_the_same_credentials() {
    let read =
        Credentials::from_vault_secret(&secret_of(&[("key_id", KEY_ID), ("secret", SECRET)]))
            .unwrap_or_else(|e| panic!("it reads: {}", e.code()));
    assert_eq!(read, from_lookup(KEY_ID, SECRET));
}

/// Each malformed secret is refused with its own code, and neither value appears in the error.
#[test]
fn a_malformed_secret_is_refused_by_its_rule_without_echoing_it() {
    let both = secret_of(&[("key_id", KEY_ID), ("secret", SECRET)]);
    let spaced = format!(r#"{{ "key_id": "{KEY_ID}", "secret": "{SECRET}" }}"#);
    let reordered = format!(r#"{{"secret":"{SECRET}","key_id":"{KEY_ID}"}}"#);
    let number = format!(r#"{{"key_id":"{KEY_ID}","secret":7}}"#);
    let extra = secret_of(&[("key_id", KEY_ID), ("secret", SECRET), ("x", SECRET)]);
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        ("not utf-8", vec![0xff, 0xfe], "not_json"),
        ("truncated", both[..both.len() - 1].to_vec(), "not_json"),
        ("whitespace", spaced.into_bytes(), "not_canonical"),
        ("member order", reordered.into_bytes(), "not_canonical"),
        (
            "an array",
            format!(r#"["{KEY_ID}","{SECRET}"]"#).into_bytes(),
            "not_an_object",
        ),
        ("a number", number.into_bytes(), "not_a_string"),
        ("no key id", secret_of(&[("secret", SECRET)]), "missing"),
        ("no secret", secret_of(&[("key_id", KEY_ID)]), "missing"),
        (
            "an empty key id",
            secret_of(&[("key_id", ""), ("secret", SECRET)]),
            "missing",
        ),
        (
            "an empty secret",
            secret_of(&[("key_id", KEY_ID), ("secret", "")]),
            "missing",
        ),
        ("another member", extra, "unknown_member"),
    ];
    for (name, secret, code) in cases {
        let error = Credentials::from_vault_secret(&secret)
            .err()
            .unwrap_or_else(|| panic!("{name}: refused"));
        assert_eq!(error.code(), code, "{name}");
        let shown = format!("{error} {error:?}");
        assert!(
            !shown.contains(KEY_ID) && !shown.contains(SECRET),
            "{name}: the error names neither value: {shown}"
        );
    }
}

/// Any non-empty pair of printable values reads back as the credentials holding them.
#[test]
fn any_canonical_pair_reads_as_its_credentials() {
    let mut runner = TestRunner::new(ProptestConfig {
        cases: 64,
        failure_persistence: None,
        ..ProptestConfig::default()
    });
    runner
        .run(&("[ -~]{1,40}", "[ -~]{1,80}"), |(key_id, secret)| {
            let read = Credentials::from_vault_secret(&secret_of(&[
                ("key_id", &key_id),
                ("secret", &secret),
            ]))
            .unwrap_or_else(|e| panic!("it reads: {e}"));
            prop_assert_eq!(read, from_lookup(&key_id, &secret));
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{e}"));
}
