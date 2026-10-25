//! The journal vectors' own checks fail on a fixture they should refuse (#2343, #2353): the
//! version check on a wrong version, hash algorithm or genesis hash, or an empty section; a
//! decimal accept case whose canonical form is not what normalizing gives; a reject case whose
//! input normalizes; and string escaping whose bytes or hash differ. Each passes as the fixture is
//! written, so the edit alone fails it.

use std::path::Path;
use std::sync::Arc;

use mandate_refcases::{Json, journal, read_fixture};
use serde_json::json;

fn fixture() -> Json {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
    Arc::unwrap_or_clone(read_fixture(&dir, "journal.json").expect("the fixture"))
}

/// Case `id` run over the fixture with `edit` applied.
fn run(id: &str, edit: impl FnOnce(&mut Json)) -> Result<(), String> {
    let mut fixture = fixture();
    edit(&mut fixture);
    let case = journal::cases(&Arc::new(fixture))
        .into_iter()
        .find(|case| case.id == id)
        .unwrap_or_else(|| panic!("no case {id}"));
    (case.run)()
}

fn refused(id: &str, edit: impl FnOnce(&mut Json)) -> String {
    match run(id, edit) {
        Ok(()) => panic!("{id} passed an edited fixture"),
        Err(message) => message,
    }
}

#[test]
fn the_version_check_refuses_a_wrong_version_algorithm_genesis_or_an_empty_section() {
    let id = "journal::version";
    assert_eq!(run(id, |_| {}), Ok(()));
    let fields = [
        ("version", json!(2), "fixture version"),
        ("hash_algorithm", json!("sha512"), "hash algorithm"),
        (
            "genesis_prev_hash",
            json!("ab".repeat(32)),
            "genesis prev_hash",
        ),
    ];
    for (key, value, named) in fields {
        let message = refused(id, |fx| fx[key] = value);
        assert!(message.contains(named), "{key}: {message}");
    }
    for path in [
        "/decimal_normalization/accept",
        "/decimal_normalization/reject",
        "/chain",
        "/tamper_cases",
        "/append_cases/cases",
        "/merkle/leaves",
    ] {
        let message = refused(id, |fx| {
            *fx.pointer_mut(path).expect("the section") = json!([]);
        });
        assert!(message.contains("is empty"), "{path}: {message}");
    }
}

#[test]
fn a_decimal_case_refuses_a_canonical_form_or_a_rejected_input_normalizing_disagrees_with() {
    let accept = r#"journal::decimal::accept::"150.00""#;
    assert_eq!(run(accept, |_| {}), Ok(()));
    let message = refused(accept, |fx| {
        fx["decimal_normalization"]["accept"][0]["canonical"] = json!("150.00");
    });
    assert!(message.contains("normalize"), "{message}");
    let reject = r#"journal::decimal::reject::"NaN""#;
    assert_eq!(run(reject, |_| {}), Ok(()));
    let message = refused(r#"journal::decimal::reject::"1""#, |fx| {
        fx["decimal_normalization"]["reject"][0] = json!("1");
    });
    assert!(message.contains("accepted"), "{message}");
}

#[test]
fn string_escaping_refuses_other_bytes_or_another_hash() {
    let id = "journal::string_escaping";
    assert_eq!(run(id, |_| {}), Ok(()));
    let message = refused(id, |fx| {
        fx["string_escaping"]["canonical_utf8_hex"] = json!("7b7d");
    });
    assert!(message.contains("canonical bytes"), "{message}");
    let message = refused(id, |fx| {
        fx["string_escaping"]["hash"] = json!("00".repeat(32))
    });
    assert!(message.contains("hash"), "{message}");
}
