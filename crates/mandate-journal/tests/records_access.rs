//! E12-3 (DEC-780; journal spec v0.26 §9.13): the control stream's `RecordsAccessed`,
//! `ExportCreated` and `VerificationRun`, closed at schema version 1, through the journal's own
//! draft check against the vectors' `records_access` section. Every base and valid draft parses,
//! and every invalid draft is refused with its reason at its path: the closed members, the
//! `stream_id` and `digest` types, rules 107 to 112, and §3's client actor (rules 81 to 83). Until
//! E12-3's implementation registers them, the journal refuses all three as `unknown_schema`, which
//! is the answer these tests fail on.
//!
//! The same for §9.14's `AnchorComputed` and `SegmentExported` (DEC-783, journal spec v0.27)
//! against the vectors' `cold_records` section: rules 113 to 118.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::Draft;

/// §9.13's vectors.
const RECORDS_ACCESS: &str = "records_access";
/// §9.14's vectors.
const COLD_RECORDS: &str = "cold_records";

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

/// A base draft by name.
fn base<'a>(section: &'a Value, name: &str) -> &'a Value {
    section.get("drafts").and_then(|d| d.get(name)).unwrap()
}

/// A case's draft: its named base draft with its changes applied, in canonical bytes.
fn draft(section: &Value, case: &Value) -> Vec<u8> {
    let mut body = base(section, text(case, "base_draft"))
        .as_object()
        .cloned()
        .unwrap();
    for change in list(case, "changes") {
        apply(&mut body, change);
    }
    to_canonical(&Value::Object(body))
}

/// Whether a case's clause cites `rule`: the text before its first `:`, such as `rule 108` or
/// `§3 rule 83`.
fn cites(case: &Value, rule: u32) -> bool {
    let cited = text(case, "clause").split(':').next().unwrap_or_default();
    let named = format!("rule {rule}");
    cited == named || cited.ends_with(&format!(" {named}"))
}

/// Every base and valid draft of `event_type` in the vectors' section `name` parses as its own type
/// at version 1, and every invalid one is refused with its reason at its path. There are never
/// fewer than `valid` valid and `invalid` invalid cases on its base drafts, and each of `rules` has
/// an invalid case.
fn every_case_of(name: &str, event_type: &str, valid: usize, invalid: usize, rules: &[u32]) {
    let section = section(name);
    let ours =
        |case: &&Value| text(base(&section, text(case, "base_draft")), "event_type") == event_type;
    let bases = section.get("drafts").and_then(Value::as_object).unwrap();
    let valid_cases: Vec<&Value> = list(&section, "valid_drafts").iter().filter(ours).collect();
    let invalid_cases: Vec<&Value> = list(&section, "invalid_drafts")
        .iter()
        .filter(ours)
        .collect();
    let mut failed = Vec::new();
    let own_bases = bases
        .values()
        .filter(|b| text(b, "event_type") == event_type);
    let valid_drafts = valid_cases.iter().map(|case| draft(&section, case));
    for bytes in own_bases.map(to_canonical).chain(valid_drafts) {
        let got = Draft::parse(&bytes).map(|d| (d.event_type().to_owned(), d.schema_version()));
        if got != Ok((event_type.to_owned(), 1)) {
            let body = String::from_utf8_lossy(&bytes);
            failed.push(format!("{body}: {got:?}"));
        }
    }
    for case in &invalid_cases {
        let expect = case.get("expect").unwrap();
        let want = (text(expect, "reason"), text(expect, "path"));
        let got = Draft::parse(&draft(&section, case)).err();
        let got = got.as_ref().map(|e| (e.reason.code(), e.path.as_str()));
        if got != Some(want) {
            let name = text(case, "name");
            failed.push(format!("{name}: want {want:?}, got {got:?}"));
        }
    }
    let counts = (valid_cases.len(), invalid_cases.len());
    assert!(
        counts.0 >= valid && counts.1 >= invalid,
        "{event_type}: {counts:?} cases; never fewer than {:?}",
        (valid, invalid)
    );
    for rule in rules {
        assert!(
            invalid_cases.iter().any(|case| cites(case, *rule)),
            "{event_type}: rule {rule} has no invalid draft"
        );
    }
    assert!(failed.is_empty(), "{event_type}:\n{}", failed.join("\n"));
}

/// `RecordsAccessed`: its closed members and types; rule 107's ranges (tenant, bounds, genesis,
/// order); rule 108's accessor, actor kind, an operator's break-glass `causation_id`, and sorted
/// resources; and a client's read in §3's one shape (rules 81 and 82), as its own accessor.
#[test]
fn every_records_accessed_draft_parses_or_is_refused_as_its_case_says() {
    every_case_of(
        RECORDS_ACCESS,
        "RecordsAccessed",
        7,
        47,
        &[81, 82, 107, 108],
    );
}

/// `ExportCreated`: its closed members and types, rule 107's ranges, rule 109's writer and its
/// `view` exactly for a view, and rule 83's refusal of a client.
#[test]
fn every_export_created_draft_parses_or_is_refused_as_its_case_says() {
    every_case_of(RECORDS_ACCESS, "ExportCreated", 2, 23, &[83, 107, 109]);
}

/// `VerificationRun`: its closed members and types, rule 107's ranges, rule 110's writer per
/// trigger, rule 111's `failure.seq` and `to_hash`, rule 112's result, and rule 83's refusal of a
/// client.
#[test]
fn every_verification_run_draft_parses_or_is_refused_as_its_case_says() {
    every_case_of(
        RECORDS_ACCESS,
        "VerificationRun",
        5,
        31,
        &[83, 107, 110, 111, 112],
    );
}

/// §11's per-event checks 1 to 6, `anchor_head_mismatch`, `anchor_self_mismatch`,
/// `intent_action_mismatch` and `mode_event_mismatch`, then its per-range checks, as rule 111 lists
/// them.
const PER_EVENT: [&str; 11] = [
    "non_canonical",
    "column_mismatch",
    "seq_gap",
    "rehash_mismatch",
    "prev_hash_mismatch",
    "artifact_missing",
    "artifact_mismatch",
    "anchor_head_mismatch",
    "anchor_self_mismatch",
    "intent_action_mismatch",
    "mode_event_mismatch",
];
const PER_RANGE: [&str; 4] = [
    "anchor_root_mismatch",
    "tsa_token_invalid",
    "segment_manifest_mismatch",
    "segment_gap",
];

/// Rule 111 for each of §9.13's fifteen check codes, which the vectors exercise only in part: a
/// failure names its `seq` exactly when its check is reported at an event, and is refused at
/// `failure.seq` otherwise.
#[test]
fn every_check_code_names_its_seq_exactly_when_it_is_reported_at_an_event() {
    let section = section(RECORDS_ACCESS);
    let mut failed = Vec::new();
    for (check, at_event) in PER_EVENT
        .iter()
        .map(|c| (c, true))
        .chain(PER_RANGE.iter().map(|c| (c, false)))
    {
        for seq in ["17", "null"] {
            let range = format!(
                r#"{{"stream_id":"ctl:ws_01J8Z2","from_seq":1,"to_seq":40,"prev_hash":"{}",
                "to_hash":null,"failure":{{"check":"{check}","seq":{seq}}}}}"#,
                "0".repeat(64)
            );
            let case = parse(
                format!(
                    r#"{{"base_draft":"verification_fail","changes":[
                    {{"path":"payload.ranges","value":[{range}]}}]}}"#
                )
                .as_bytes(),
            )
            .unwrap();
            let got = Draft::parse(&draft(&section, &case)).err();
            let got = got.map(|e| (e.reason.code(), e.path));
            let want = (at_event != (seq == "17"))
                .then(|| ("schema", "payload.ranges[0].failure.seq".to_owned()));
            if got != want {
                failed.push(format!("{check} at seq {seq}: want {want:?}, got {got:?}"));
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// `AnchorComputed`: its closed members and types, leaves' included; rule 113's leaves (non-empty,
/// tenant, `seq`, order, one per stream, its own stream's); rule 114's recomputed root, through one
/// leaf and five; a `null` token; and rule 115's writer.
#[test]
fn every_anchor_computed_draft_parses_or_is_refused_as_its_case_says() {
    every_case_of(COLD_RECORDS, "AnchorComputed", 3, 30, &[113, 114, 115]);
}

/// `SegmentExported`: its closed members and types; rule 116's tenant, bounds and genesis; rule
/// 117's manifest hash over DEC-263's six fields; and rule 118's writer.
#[test]
fn every_segment_exported_draft_parses_or_is_refused_as_its_case_says() {
    every_case_of(COLD_RECORDS, "SegmentExported", 2, 29, &[116, 117, 118]);
}

/// Rules 115 and 118 for every actor kind but `system`, where the vectors try only a `user`: an
/// anchor or a segment by anyone else is refused at `actor.kind`; a client, in §3's one shape, by
/// rule 83 at the same path.
#[test]
fn only_a_system_actor_writes_an_anchor_or_a_segment() {
    let section = section(COLD_RECORDS);
    let build = format!("sha256:{}", "c".repeat(64));
    let actors = [
        r#"{"kind":"user","id":"user_owner_01","version":"1","build":null}"#.to_owned(),
        format!(r#"{{"kind":"agent","id":"agent_a","version":"0.1.0","build":"{build}"}}"#),
        r#"{"kind":"broker","id":"alpaca","version":"v2","build":null}"#.to_owned(),
        r#"{"kind":"platform_operator","id":"operator_01","version":"1","build":null}"#.to_owned(),
        r#"{"kind":"client","id":"client_01","version":"1","build":null,
        "on_behalf_of":"user_owner_01"}"#
            .to_owned(),
    ];
    let mut failed = Vec::new();
    for base_draft in ["anchor", "segment"] {
        for actor in &actors {
            let case = parse(
                format!(
                    r#"{{"base_draft":"{base_draft}","changes":[{{"path":"actor","value":{actor}}}]}}"#
                )
                .as_bytes(),
            )
            .unwrap();
            let got = Draft::parse(&draft(&section, &case)).err();
            let got = got.map(|e| (e.reason.code(), e.path));
            let want = Some(("schema", "actor.kind".to_owned()));
            if got != want {
                failed.push(format!("{base_draft} by {actor}: got {got:?}"));
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
