//! Journal spec v0.54 §9.24 (DEC-1171, DEC-1173; E21-33): `AgentDeployed` version 2 names the
//! connection of the stored document its version names, and stream rule 201 binds it to that
//! connection's version-2 establishment on the same control stream. Judged against the vectors'
//! `agent_deployed` section, which `reference/journal/deployed.py` builds and judges with its own
//! fold:
//! - every base and valid draft parses through [`Draft::parse`], and every invalid draft is refused
//!   with its reason at its path; before DEC-1171's code change version 2 was `unknown_schema`;
//! - every stream is judged by §11's `connection_lifecycle_mismatch` as its vector says, rule 201
//!   at the vector's record; before the change rule 201 was not judged, so every invalid stream
//!   passed;
//! - every range is judged from its verified connection anchor, from genesis, or unanchored, as its
//!   vector says, and an unanchored range fails closed at its first version-2 deployment.
//!
//! Rule 200 is the mapping's (`mandate-spec`), and the manager's reading is
//! `mandate-deploy-manager`'s; neither is judged here.

use std::collections::BTreeMap;
use std::path::Path;

use mandate_canon::{Digest, Int, Key, Object, Value, parse, to_canonical};
use mandate_journal::{
    ConnectionAnchor, ConnectionAnchorError, ConnectionCheck, ConnectionCheckError,
    ConnectionFailure, ConnectionStart, ConnectionStreamRule, ConnectionVerifyError, Draft,
    LocatedConnectionFailure, PrefixError, StoredEvent, TrustedStart, VerifiedPrefix,
    verify_connection_lifecycle, verify_connection_lifecycle_from,
};

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("agent_deployed").cloned().unwrap()
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
    let index = value.get("index").and_then(Value::as_int).unwrap();
    usize::try_from(index).unwrap()
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

/// A stream case's records as stored rows, in commit order, each stream numbered from 1 and
/// sealed as its own chain, so a prefix of one stream binds through [`VerifiedPrefix::bind`].
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
                ("artifact_refs", Value::Array(vec![])),
                ("config_refs", Value::Object(BTreeMap::new())),
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

fn streams(section: &Value) -> BTreeMap<&str, &Value> {
    list(section, "streams")
        .iter()
        .map(|case| (text(case, "name"), case))
        .collect()
}

const RULE_201: ConnectionCheck =
    ConnectionCheck::LifecycleMismatch(ConnectionStreamRule::Deployed);

#[test]
fn every_agent_deployed_base_and_valid_draft_parses_at_its_version() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (name, body) in drafts {
        let want = text(body, "event_type");
        let parsed = Draft::parse(&to_canonical(body)).map(|d| d.event_type().to_owned());
        if parsed.as_deref().ok() != Some(want) {
            failed.push(format!("base {}: {parsed:?}", name.as_str()));
        }
    }
    let valid = list(&section, "valid_drafts");
    for case in valid {
        if let Err(e) = Draft::parse(&draft(&section, case)) {
            failed.push(format!("valid {}: {e:?}", text(case, "name")));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        drafts.contains_key("deployed_v2") && valid.len() >= 3,
        "the version-2 base and {} valid drafts; never fewer",
        valid.len()
    );
}

#[test]
fn every_invalid_agent_deployed_draft_is_refused_with_its_reason_at_its_path() {
    let section = section();
    let invalid = list(&section, "invalid_drafts");
    let mut failed = Vec::new();
    for case in invalid {
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
    assert!(
        invalid.len() >= 11,
        "{} invalid drafts; never fewer",
        invalid.len()
    );
}

/// Each stream through §11's full-chain `connection_lifecycle_mismatch`: valid, or rule 201 at the
/// vector's record. The vectors' streams break no other connection rule, so any other answer is a
/// defect in the fold.
#[test]
fn every_agent_deployed_stream_is_judged_by_rule_201_as_its_vector_says() {
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
                    check: RULE_201,
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
    assert!(
        cases.len() >= 17 && refused >= 6,
        "{} streams, {refused} refused; never fewer",
        cases.len()
    );
}

/// Where a range of `stream` from `start` runs: from genesis at `seq` 1; anchored on the fold of
/// the rows before it once [`VerifiedPrefix::bind`] binds them; and unanchored when the vector says
/// so, or when the prefix does not bind or anchors nothing.
fn range_start(prefix: &[StoredEvent], anchored: bool) -> ConnectionStart {
    if prefix.is_empty() {
        return ConnectionStart::Genesis;
    }
    if !anchored {
        return ConnectionStart::Unanchored;
    }
    let start = TrustedStart {
        from_seq: prefix.last().map_or(1, |row| row.seq + 1),
        prev_hash: prefix.last().map_or(Digest::ZERO, |row| row.hash),
    };
    let bound = VerifiedPrefix::bind(prefix, start, &BTreeMap::<Digest, Vec<u8>>::new());
    match bound.map(|bound| ConnectionAnchor::from_verified(&bound)) {
        Ok(Ok(anchor)) => ConnectionStart::Anchored(anchor),
        Ok(Err(ConnectionAnchorError::Broken))
        | Err(PrefixError::Unverified(_) | PrefixError::Unbound) => ConnectionStart::Unanchored,
        unexpected => panic!("neither an anchor nor a refusal: {unexpected:?}"),
    }
}

/// Each range through §11's `connection_lifecycle_mismatch` from its start: valid, rule 201 at the
/// vector's record, or failing closed there with no anchor.
#[test]
fn every_agent_deployed_range_is_judged_from_its_anchor_as_its_vector_says() {
    let section = section();
    let streams = streams(&section);
    let cases = list(&section, "ranges");
    let mut failed = Vec::new();
    for case in cases {
        let rows = rows(streams.get(text(case, "stream")).unwrap());
        let start = usize::try_from(case.get("start").and_then(Value::as_int).unwrap()).unwrap();
        let anchored = case.get("anchored") == Some(&Value::Bool(true));
        let (prefix, range) = rows.split_at(start);
        let expect = case.get("expect").unwrap();
        let want = match (text(expect, "outcome"), text(expect, "cause")) {
            ("Valid", _) => Ok(()),
            (_, cause) => {
                let row = &rows[index(expect)];
                let check = match cause {
                    "unanchored" => ConnectionCheck::Unanchored,
                    _ => RULE_201,
                };
                Err(ConnectionCheckError::Failed(LocatedConnectionFailure {
                    stream_id: row.stream_id.clone(),
                    seq: row.seq,
                    check,
                }))
            }
        };
        let got = verify_connection_lifecycle_from(range_start(prefix, anchored), range);
        if got != want {
            failed.push(format!(
                "{} ({}): expected {want:?}, got {got:?}",
                text(case, "name"),
                text(case, "clause")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(cases.len() >= 11, "{} ranges; never fewer", cases.len());
}

/// DEC-885 I1 for rule 201: on every split of every one-stream case that breaks no rule before the
/// split, the range from the verified anchor is judged as the full chain judges it, at the same
/// record. A case that spans an account stream anchors nothing (DEC-913), so it is not split.
#[test]
fn an_anchored_range_agrees_with_the_full_chain_on_every_split_of_every_stream() {
    let section = section();
    let mut split = 0;
    for (name, case) in streams(&section) {
        let rows = rows(case);
        if rows.iter().any(|row| row.stream_id != rows[0].stream_id) {
            continue;
        }
        split += 1;
        let full = verify_connection_lifecycle_from(ConnectionStart::Genesis, &rows);
        let first = full
            .as_ref()
            .err()
            .map(|ConnectionCheckError::Failed(f)| f.seq);
        for k in 1..=rows.len() {
            if first.is_some_and(|seq| seq <= rows[k - 1].seq) {
                break;
            }
            let (prefix, range) = rows.split_at(k);
            let got = verify_connection_lifecycle_from(range_start(prefix, true), range);
            assert_eq!(got, full, "{name} split at {k}");
        }
    }
    assert!(split >= 16, "{split} one-stream cases split; never fewer");
}
