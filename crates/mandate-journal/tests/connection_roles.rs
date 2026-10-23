//! Journal spec v0.77 §9.33 (DEC-2350; E21-33 S3): `ConnectionRolesMade`, `ConnectionRolesRefused`
//! and `ConnectionRolesRemoved` on the control stream at schema version 1, with rule 261, and the
//! stream rules 262 and 263, against the vectors' `connection_roles` section. Through
//! [`Draft::parse`]: each base and every valid draft parse as its record, and every invalid draft
//! is refused with its reason at its path. `append` judges rule 261 alone, so [`check_stream`]
//! refuses none of the section's streams; [`verify_connection_roles`], the verifier's fold, judges
//! rules 262 and 263 over each stream and answers the case's outcome. Before the registration the
//! journal refuses the three records as unknown event types, the answer these tests fail on.

use std::collections::BTreeMap;
use std::path::Path;

use mandate_canon::{Digest, Key, Object, Value, parse, to_canonical};
use mandate_journal::{
    ConnectionRolesError, ConnectionRolesRule, Draft, StoredEvent, check_stream,
    verify_connection_roles,
};

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("connection_roles").cloned().unwrap()
}

fn list<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    value
        .get(name)
        .and_then(Value::as_array)
        .unwrap_or_default()
}

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// A `{path, value}` or `{path, delete: true}` change, dotted from the envelope.
fn apply(draft: &mut Object, change: &Value) {
    let path = text(change, "path");
    let mut names: Vec<&str> = path.split('.').collect();
    let last = names.pop().unwrap();
    let mut node = draft;
    for name in names {
        node = match node.get_mut(name) {
            Some(Value::Object(inner)) => inner,
            other => panic!("`{path}` has no object `{name}`: {other:?}"),
        };
    }
    if change.get("delete") == Some(&Value::Bool(true)) {
        assert!(node.remove(last).is_some(), "`{path}` deletes nothing");
    } else {
        node.insert(
            Key::new(last).unwrap(),
            change.get("value").cloned().unwrap(),
        );
    }
}

/// A case's draft: its named base draft with its changes applied, in canonical bytes.
fn draft(section: &Value, case: &Value) -> Vec<u8> {
    let base = section
        .get("drafts")
        .and_then(|d| d.get(text(case, "base_draft")));
    let mut body = base.and_then(Value::as_object).cloned().unwrap();
    for change in list(case, "changes") {
        apply(&mut body, change);
    }
    to_canonical(&Value::Object(body))
}

/// A stream case's records as stored rows, in commit order, each stream numbered from 1.
fn rows(case: &Value) -> Vec<StoredEvent> {
    let mut next_seq: BTreeMap<String, u64> = BTreeMap::new();
    list(case, "records")
        .iter()
        .map(|body| {
            let stream_id = text(body, "stream_id").to_owned();
            let seq = next_seq.entry(stream_id.clone()).or_insert(0);
            *seq += 1;
            StoredEvent {
                seq: *seq,
                event_id: text(body, "event_id").to_owned(),
                event_type: text(body, "event_type").to_owned(),
                schema_version: body.get("schema_version").and_then(Value::as_int).unwrap(),
                environment: text(body, "environment").to_owned(),
                recorded_at: "2026-10-22T16:00:00.000000000Z".to_owned(),
                prev_hash: Digest::of(b""),
                hash: Digest::of(b""),
                body: to_canonical(body),
                stream_id,
            }
        })
        .collect()
}

/// The answer a stream case's `expect` names.
fn expected(case: &Value) -> Result<(), ConnectionRolesError> {
    let expect = case.get("expect").unwrap();
    match text(expect, "outcome") {
        "Valid" => Ok(()),
        "Invalid" => {
            let rule = match text(expect, "rule") {
                "rule 262" => ConnectionRolesRule::Answered,
                "rule 263" => ConnectionRolesRule::Removed,
                other => panic!("no §9.33 stream rule {other}"),
            };
            let index = expect.get("index").and_then(Value::as_int).unwrap();
            Err(ConnectionRolesError::Broken {
                index: usize::try_from(index).unwrap(),
                rule,
            })
        }
        other => panic!("no outcome {other}"),
    }
}

/// The vectors' section is §9.33's, with every part this file reads.
#[test]
fn the_connection_roles_section_holds_every_part() {
    let section = section();
    assert_eq!(
        text(&section, "spec"),
        "docs/specs/journal.md v0.81 §9.33 (DEC-2350)"
    );
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let names: Vec<&str> = drafts.keys().map(|k| k.as_str()).collect();
    assert_eq!(
        names,
        [
            "connection_roles_made",
            "connection_roles_refused",
            "connection_roles_removed"
        ],
        "three base drafts, in key order"
    );
    for (part, at_least) in [("invalid_drafts", 37), ("valid_drafts", 3), ("streams", 21)] {
        let n = list(&section, part).len();
        assert!(n >= at_least, "{n} {part}; never fewer than {at_least}");
    }
    assert_eq!(ConnectionRolesRule::Answered.number(), 262);
    assert_eq!(ConnectionRolesRule::Removed.number(), 263);
}

#[test]
fn each_base_and_every_valid_connection_roles_draft_parse_as_its_record() {
    let section = section();
    let mut failed = Vec::new();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    for (name, body) in drafts {
        let want = match name.as_str() {
            "connection_roles_made" => "ConnectionRolesMade",
            "connection_roles_refused" => "ConnectionRolesRefused",
            _ => "ConnectionRolesRemoved",
        };
        match Draft::parse(&to_canonical(body)) {
            Ok(d) if d.event_type() == want => {}
            got => failed.push(format!("base {}: {:?}", name.as_str(), got.err())),
        }
    }
    for case in list(&section, "valid_drafts") {
        if let Err(e) = Draft::parse(&draft(&section, case)) {
            failed.push(format!("valid {}: {e:?}", text(case, "name")));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn every_invalid_connection_roles_draft_is_refused_with_its_reason_at_its_path() {
    let section = section();
    let mut failed = Vec::new();
    for case in list(&section, "invalid_drafts") {
        let expect = case.get("expect").unwrap();
        let want = (text(expect, "reason"), text(expect, "path"));
        let got = Draft::parse(&draft(&section, case)).err();
        let got = got.as_ref().map(|e| (e.reason.code(), e.path.as_str()));
        if got != Some(want) {
            failed.push(format!(
                "{}: expected {want:?}, got {got:?}",
                text(case, "name")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// `append` checks rule 261 alone: every record of every stream case parses, the ones rules 262
/// and 263 refuse included, and [`check_stream`] refuses none of them, since those rules are the
/// provisioner's against its own fold and the verifier's (§9.33).
#[test]
fn append_judges_rule_261_alone_and_leaves_rules_262_and_263_to_the_folds() {
    let section = section();
    let mut failed = Vec::new();
    for case in list(&section, "streams") {
        let name = text(case, "name");
        let mut parsed = Vec::new();
        for (i, record) in list(case, "records").iter().enumerate() {
            match Draft::parse(&to_canonical(record)) {
                Ok(d) => parsed.push(d),
                Err(e) => failed.push(format!("{name} record {i}: {e:?}")),
            }
        }
        if let Err((i, e)) = check_stream(&[], &parsed) {
            failed.push(format!("{name}: check_stream refused record {i}: {e:?}"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// The verifier's fold judges rules 262 and 263 over each stream case, and answers its outcome:
/// valid, or the rule and the index of the first record that breaks it.
#[test]
#[ignore = "pending E21-33"]
fn the_verifiers_fold_answers_every_stream_case() {
    let section = section();
    let mut failed = Vec::new();
    for case in list(&section, "streams") {
        let got = verify_connection_roles(&rows(case));
        let want = expected(case);
        if got != want {
            failed.push(format!(
                "{} ({}): expected {want:?}, got {got:?}",
                text(case, "name"),
                text(case, "clause")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// A stream case's records with `member` of `record`'s payload set to `value`.
fn edited(case: &Value, record: usize, member: &str, value: &str) -> Value {
    let mut case = case.clone();
    let Some(Value::Object(top)) = Some(&mut case) else {
        unreachable!()
    };
    let Some(Value::Array(records)) = top.get_mut("records") else {
        panic!("no records")
    };
    let Some(Value::Object(body)) = records.get_mut(record) else {
        panic!("no record {record}")
    };
    let Some(Value::Object(payload)) = body.get_mut("payload") else {
        panic!("no payload")
    };
    payload.insert(Key::new(member).unwrap(), Value::Str(value.to_owned()));
    case
}

/// Rule 263 reads the closer's own kind and the removal's own occasion: a `ConnectionRefused`
/// of any occasion but `connect` closes no request, and a removal naming a request's close as a
/// revocation names no closer of its occasion, so each is refused at the removal.
#[test]
fn a_removal_names_a_connect_refusal_of_its_own_occasion() {
    let section = section();
    let closed = list(&section, "streams")
        .iter()
        .find(|case| text(case, "name") == "removed_on_close")
        .unwrap();
    assert_eq!(verify_connection_roles(&rows(closed)), Ok(()));
    let broken = Err(ConnectionRolesError::Broken {
        index: 3,
        rule: ConnectionRolesRule::Removed,
    });
    for occasion in ["reconnect", "reauthorize"] {
        let case = edited(closed, 2, "occasion", occasion);
        assert_eq!(verify_connection_roles(&rows(&case)), broken, "{occasion}");
    }
    let case = edited(closed, 3, "occasion", "revoked");
    assert_eq!(verify_connection_roles(&rows(&case)), broken, "revoked");
}
