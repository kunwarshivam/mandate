//! Journal spec v0.84 (DEC-2131; E9-4 S5): `MandateConfirmed` version 3 (§9.9) and `AgentDeployed`
//! version 3 (§9.24) carry the passkey step-up evidence §9.32's rule 253 reads, with rules 267 and
//! 268, judged against the vectors' `stepped_up_records` section, which
//! `reference/journal/stepped_up.py` builds and judges with its own oracles:
//! - every base and valid draft parses through [`Draft::parse`], and every invalid draft is refused
//!   with its reason at its path; before the change version 3 was `unknown_schema`;
//! - every batch is judged by [`check_batch`] as its vector says, so a version 3 followed by its
//!   `StepUpAsserted` commits and a version 2 never does;
//! - every control stream is judged by §11's `connection_lifecycle_mismatch` as its vector says:
//!   rule 201 judges a version-3 deployment as a version 2, which it skipped before the change.
//!
//! §9.2's mapping (rule 200) is `mandate-spec`'s and the manager's reading
//! `mandate-deploy-manager`'s; neither is judged here.

use std::collections::BTreeMap;
use std::path::Path;

use mandate_canon::{Digest, Int, Key, Object, Value, parse, to_canonical};
use mandate_journal::{
    ConnectionCheck, ConnectionFailure, ConnectionStreamRule, ConnectionVerifyError, Draft,
    StoredEvent, StreamType, check_batch, closes, verify_connection_lifecycle,
};

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("stepped_up_records").cloned().unwrap()
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

fn index(value: &Value) -> usize {
    value
        .get("index")
        .and_then(Value::as_int)
        .and_then(|i| usize::try_from(i).ok())
        .unwrap()
}

/// A `{path, value}` change, dotted from the envelope, or `{path, delete}` removing the member.
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
        node.remove(last);
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

/// A stream case's records as stored rows, in commit order, each stream numbered from 1 and
/// sealed as its own chain.
fn rows(case: &Value) -> Vec<StoredEvent> {
    let mut tails: BTreeMap<String, (u64, Digest)> = BTreeMap::new();
    list(case, "records")
        .iter()
        .map(|record| {
            let stream_id = text(record, "stream_id").to_owned();
            let (seq, prev_hash) = tails.get(&stream_id).copied().unwrap_or((0, Digest::ZERO));
            let seq = seq + 1;
            let mut body = record.as_object().cloned().unwrap();
            for (name, value) in [
                ("seq", Value::Int(Int::new(seq).unwrap())),
                ("prev_hash", Value::Str(prev_hash.to_hex())),
                (
                    "recorded_at",
                    Value::Str("2026-10-17T16:00:00.000000000Z".into()),
                ),
            ] {
                body.insert(Key::new(name).unwrap(), value);
            }
            let body = Value::Object(body);
            let canonical = to_canonical(&body);
            let hash = Digest::of(&canonical);
            tails.insert(stream_id.clone(), (seq, hash));
            StoredEvent {
                seq,
                event_id: text(&body, "event_id").to_owned(),
                event_type: text(&body, "event_type").to_owned(),
                schema_version: body.get("schema_version").and_then(Value::as_int).unwrap(),
                environment: text(&body, "environment").to_owned(),
                recorded_at: text(&body, "recorded_at").to_owned(),
                prev_hash,
                hash,
                body: canonical,
                stream_id,
            }
        })
        .collect()
}

/// The vectors' section is v0.84's, with every part this file reads.
#[test]
fn the_stepped_up_section_holds_every_part() {
    let section = section();
    assert!(text(&section, "spec").contains("v0.84"));
    for (part, at_least) in [
        ("invalid_drafts", 43),
        ("valid_drafts", 10),
        ("batches", 10),
        ("streams", 7),
    ] {
        let n = list(&section, part).len();
        assert!(n >= at_least, "{n} {part}; never fewer than {at_least}");
    }
}

#[test]
fn every_version_3_base_and_valid_draft_parses_at_its_version() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (name, body) in drafts {
        let want = (
            text(body, "event_type"),
            body.get("schema_version").and_then(Value::as_int),
        );
        let parsed = Draft::parse(&to_canonical(body))
            .map(|d| (d.event_type().to_owned(), d.schema_version()));
        let got = parsed.as_ref().ok().map(|(t, v)| (t.as_str(), Some(*v)));
        if got != Some(want) {
            failed.push(format!("base {}: {parsed:?}", name.as_str()));
        }
    }
    for case in list(&section, "valid_drafts") {
        if let Err(e) = Draft::parse(&draft(&section, case)) {
            failed.push(format!("valid {}: {e:?}", text(case, "name")));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    for name in ["confirmed_v3", "deployed_v3"] {
        assert!(drafts.contains_key(name), "the base {name}");
    }
}

/// Rules 267 and 268 and every member type of `step_up`, and rules 18, 22, 77, 78 and 83 at
/// version 3, each refused at its reason and path.
#[test]
fn every_invalid_version_3_draft_is_refused_with_its_reason_at_its_path() {
    let section = section();
    let mut failed = Vec::new();
    for case in list(&section, "invalid_drafts") {
        let expect = case.get("expect").unwrap();
        let want = (text(expect, "reason"), text(expect, "path"));
        let got = Draft::parse(&draft(&section, case)).err();
        let got = got.as_ref().map(|e| (e.reason.code(), e.path.as_str()));
        if got != Some(want) {
            failed.push(format!(
                "{} ({}): expected {want:?}, got {got:?}",
                text(case, "name"),
                text(case, "clause")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Rule 253 over each batch vector: a version 3 then its `StepUpAsserted` commits, and an
/// assertion after a version 2 is refused at `payload.step_up`.
#[test]
fn every_stepped_up_batch_is_judged_by_rule_253_as_its_vector_says() {
    let section = section();
    let mut failed = Vec::new();
    for case in list(&section, "batches") {
        let drafts: Result<Vec<Draft>, _> = list(case, "drafts")
            .iter()
            .enumerate()
            .map(|(i, record)| Draft::parse(&to_canonical(record)).map_err(|e| (i, e)))
            .collect();
        let got = drafts
            .and_then(|drafts| check_batch(&drafts))
            .err()
            .map(|(i, e)| (i, e.reason.code().to_owned(), e.path));
        let expect = case.get("expect").unwrap();
        let want = (text(expect, "outcome") == "Invalid").then(|| {
            (
                index(expect),
                text(expect, "reason").to_owned(),
                text(expect, "path").to_owned(),
            )
        });
        if got != want {
            failed.push(format!(
                "{}: expected {want:?}, got {got:?}",
                text(case, "name")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Each stream through §11's full-chain `connection_lifecycle_mismatch`: a version-3 deployment is
/// judged by rule 201 exactly as a version 2.
#[test]
fn every_stepped_up_stream_is_judged_by_rule_201_as_a_version_2() {
    let section = section();
    let cases = list(&section, "streams");
    let mut failed = Vec::new();
    let mut refused = 0;
    for case in cases {
        let expect = case.get("expect").unwrap();
        let want = match text(expect, "outcome") {
            "Valid" => Ok(()),
            _ => {
                assert_eq!(expect.get("rule").and_then(Value::as_int), Some(201));
                refused += 1;
                Err(ConnectionVerifyError::Mismatch(ConnectionFailure {
                    index: index(expect),
                    check: ConnectionCheck::LifecycleMismatch(ConnectionStreamRule::Deployed),
                }))
            }
        };
        let got = verify_connection_lifecycle(&rows(case));
        if got != want {
            failed.push(format!(
                "{} ({}): expected {want:?}, got {got:?}",
                text(case, "name"),
                text(case, "clause")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(refused >= 3, "{refused} refused streams; never fewer");
}

/// Both records close at versions 1 to 3 on the control stream, and nowhere else (v0.84).
#[test]
fn the_catalogue_closes_both_records_at_version_3_on_the_control_stream_only() {
    for event_type in ["MandateConfirmed", "AgentDeployed"] {
        for version in 1..=3 {
            assert!(
                closes(event_type, version, StreamType::Control),
                "{event_type} {version}"
            );
        }
        assert!(!closes(event_type, 4, StreamType::Control), "{event_type}");
        for stream in [
            StreamType::Agent,
            StreamType::Account,
            StreamType::Scheduler,
            StreamType::Notice,
            StreamType::Meter,
        ] {
            assert!(!closes(event_type, 3, stream), "{event_type} {stream:?}");
        }
    }
}
