//! E10-15 (DEC-670 to DEC-673; journal spec v0.23 §3, §9.9 to §9.11, §11): the workspace API
//! records, the `client` actor, and the hold, against the vectors' `workspace_api`,
//! `client_actor` and `hold` sections. Until E10-15's implementation the journal refuses these
//! records as not catalogued or not registered, and the range test stops at its stub.

use std::ops::RangeInclusive;
use std::path::Path;

use mandate_canon::{Digest, Key, Object, Value, parse, to_canonical};
use mandate_journal::{
    Draft, HeldAnchor, StoredEvent, TrustedStart, check_batch, verify_agent_stream_anchored,
};

fn section(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get(name).cloned().unwrap()
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

/// A body with a case's changes applied, in canonical bytes.
fn changed(base: Option<&Value>, case: &Value) -> Vec<u8> {
    let mut body = base.and_then(Value::as_object).cloned().unwrap();
    for change in list(case, "changes") {
        apply(&mut body, change);
    }
    to_canonical(&Value::Object(body))
}

/// A case's draft: its named base draft with its changes applied.
fn draft(section: &Value, case: &Value) -> Vec<u8> {
    let base = section
        .get("drafts")
        .and_then(|d| d.get(text(case, "base_draft")));
    changed(base, case)
}

/// The sections, each with its least valid and invalid drafts and the rules its invalid drafts
/// cite: §9.9 (DEC-670), §3's `client` actor with §9.10 (DEC-671), and §9.11 (DEC-672).
const SECTIONS: [(&str, usize, usize, RangeInclusive<u32>); 3] = [
    ("workspace_api", 13, 147, 69..=80),
    ("client_actor", 9, 71, 81..=89),
    ("hold", 12, 78, 90..=95),
];

/// Every base and valid draft parses as its own type and version, and every invalid draft is
/// refused with its reason at its path, with never fewer cases and each rule cited.
#[test]
#[ignore = "pending E10-15"]
fn every_e10_15_draft_parses_or_is_refused_as_its_case_says() {
    let mut failed = Vec::new();
    for (name, valid, invalid, rules) in SECTIONS {
        let section = section(name);
        let bases = section.get("drafts").and_then(Value::as_object).unwrap();
        let valid_cases = list(&section, "valid_drafts");
        let valid_drafts = valid_cases.iter().map(|case| draft(&section, case));
        for bytes in bases.values().map(to_canonical).chain(valid_drafts) {
            let written = parse(&bytes).unwrap();
            let version = written.get("schema_version").and_then(Value::as_int);
            let want = (text(&written, "event_type").to_owned(), version);
            let got =
                Draft::parse(&bytes).map(|d| (d.event_type().to_owned(), Some(d.schema_version())));
            if got.as_ref() != Ok(&want) {
                failed.push(format!(
                    "{name} {}: {got:?}",
                    String::from_utf8_lossy(&bytes)
                ));
            }
        }
        let invalid_cases = list(&section, "invalid_drafts");
        for case in invalid_cases {
            let expect = case.get("expect").unwrap();
            let want = (text(expect, "reason"), text(expect, "path"));
            let got = Draft::parse(&draft(&section, case)).err();
            let got = got.as_ref().map(|e| (e.reason.code(), e.path.as_str()));
            if got != Some(want) {
                failed.push(format!(
                    "{name} {}: want {want:?}, got {got:?}",
                    text(case, "name")
                ));
            }
        }
        let counts = (valid_cases.len(), invalid_cases.len());
        assert!(
            counts.0 >= valid && counts.1 >= invalid,
            "{name}: {counts:?} cases; never fewer"
        );
        for rule in rules {
            let named = format!("rule {rule}");
            let cited = |c: &Value| text(c, "clause").split(':').next() == Some(&named);
            assert!(
                invalid_cases.iter().any(cited),
                "{name}: {named} has no invalid draft"
            );
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Rule 84's batch clause: a compromised revocation names, earlier in its batch, the kill switch at
/// its connection's scope. Each valid batch commits, with or without an unrelated draft between,
/// and each invalid one fails at its draft.
#[test]
#[ignore = "pending E10-15"]
fn a_compromised_revocation_follows_its_connections_kill_switch_in_its_batch() {
    let section = section("client_actor");
    let base = |member: &Value| match member.get("kill_switch") {
        Some(Value::Bool(true)) => section.get("kill_switch"),
        _ => section
            .get("drafts")
            .and_then(|d| d.get(text(member, "base_draft"))),
    };
    let batch = |case: &Value| -> Vec<Draft> {
        let members = list(case, "drafts").iter();
        members
            .map(|m| Draft::parse(&changed(base(m), m)).unwrap())
            .collect()
    };
    let unrelated = parse(br#"{"base_draft":"client_connected"}"#).unwrap();
    let unrelated = Draft::parse(&draft(&section, &unrelated)).unwrap();
    let (valid, invalid) = (
        list(&section, "valid_batches"),
        list(&section, "invalid_batches"),
    );
    let check = |drafts: &[Draft]| {
        let refused = check_batch(drafts).err();
        refused.map(|(i, e)| (u64::try_from(i).ok(), e.reason.code(), e.path))
    };
    let index = |e: &Value| e.get("draft_index").and_then(Value::as_int);
    let cases = valid.iter().map(|c| (c, None));
    let mut failed = Vec::new();
    for (case, expect) in cases.chain(invalid.iter().map(|c| (c, c.get("expect")))) {
        let want = expect.map(|e| (index(e), text(e, "reason"), text(e, "path").to_owned()));
        let mut drafts = batch(case);
        let mut got = check(&drafts);
        if got.is_none() && want.is_none() {
            drafts.insert(1, unrelated.clone());
            got = check(&drafts);
        }
        if got != want {
            failed.push(format!(
                "{}: want {want:?}, got {got:?}",
                text(case, "name")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        !valid.is_empty() && invalid.len() >= 6,
        "{} and {} batches",
        valid.len(),
        invalid.len()
    );
}

/// One stored agent-stream row at `seq` of an `event_type` with `payload`.
fn stored(seq: u64, event_type: &str, version: u64, payload: &Value) -> StoredEvent {
    let body = format!(
        r#"{{"event_type":"{event_type}","schema_version":{version},"payload":{}}}"#,
        String::from_utf8(to_canonical(payload)).unwrap()
    );
    StoredEvent {
        stream_id: "agent:ws_01J8Z2:agent_a".to_owned(),
        seq,
        event_id: format!("01J8Z6R0A{seq:017}"),
        event_type: event_type.to_owned(),
        schema_version: version,
        environment: "paper".to_owned(),
        recorded_at: "2026-10-08T16:00:00.000000000Z".to_owned(),
        prev_hash: Digest::of(b""),
        hash: Digest::of(b""),
        body: body.into_bytes(),
    }
}

/// §11's `held_mismatch` (§9.11, DEC-673) over the `hold` section's ranges, each with the anchor
/// its caller derives (item 1); a full chain from seq 1 is anchored on nothing, so answers the same
/// told `Unknown`. Only the first failing record is reported, at its `seq`.
#[test]
#[ignore = "pending E10-15"]
fn the_owners_hold_is_carried_through_every_range() {
    let section = section("hold");
    let cases = list(&section, "range_verification");
    let mut failed = Vec::new();
    for case in cases {
        let from = case.get("from_seq").and_then(Value::as_int).unwrap();
        let rows: Vec<StoredEvent> = list(case, "events")
            .iter()
            .zip(from..)
            .map(|(event, seq)| {
                let version = event.get("schema_version").and_then(Value::as_int).unwrap();
                let payload = event.get("payload").unwrap();
                stored(seq, text(event, "event_type"), version, payload)
            })
            .collect();
        let anchor = case.get("anchor").filter(|a| a.as_object().is_some());
        let anchors = match anchor {
            None if from == 1 => vec![HeldAnchor::Unknown, HeldAnchor::NoVersionTwo],
            None => vec![HeldAnchor::Unknown],
            Some(a) if a.get("v2_before") == Some(&Value::Bool(true)) => {
                vec![HeldAnchor::Carried {
                    seq: from - 1,
                    held: a.get("held") == Some(&Value::Bool(true)),
                }]
            }
            Some(_) => vec![HeldAnchor::NoVersionTwo],
        };
        let expect = list(case, "expect");
        let want = expect
            .first()
            .and_then(Value::as_int)
            .map(|i| (from + i, "held_mismatch"));
        for anchor in anchors {
            let start = TrustedStart {
                from_seq: from,
                prev_hash: Digest::of(b""),
            };
            let got = verify_agent_stream_anchored(&rows, start, anchor)
                .err()
                .map(|f| (f.seq, f.check.code()));
            if got != want {
                failed.push(format!(
                    "{} with {anchor:?}: expected the first of {expect:?} from seq {from}, got {got:?}",
                    text(case, "name")
                ));
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        cases.len() >= 24,
        "{} range cases; never fewer",
        cases.len()
    );
    let nothing_before = parse(br#"{"v2_before":false}"#).unwrap();
    let kinds: [(Option<&Value>, bool, &[u64]); 4] = [
        (None, true, &[0]),
        (None, true, &[]),
        (Some(&nothing_before), true, &[]),
        (None, false, &[1, 2]),
    ];
    for (anchor, later, expect) in kinds {
        let found = cases.iter().any(|c| {
            c.get("anchor").filter(|a| a.as_object().is_some()) == anchor
                && (c.get("from_seq").and_then(Value::as_int) != Some(1)) == later
                && list(c, "expect")
                    .iter()
                    .filter_map(Value::as_int)
                    .eq(expect.iter().copied())
        });
        assert!(
            found,
            "a case anchored {anchor:?}, after seq 1 {later}, expecting {expect:?}"
        );
    }
}
