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
//!
//! And `VerificationRun` version 2 (journal spec v0.36, DEC-788, DEC-789) against the vectors'
//! `verification_runs` section, with rules 132 and 133 swept by an oracle of their own. Until
//! E12-3's implementation registers version 2, the journal refuses it as `unknown_schema`, which
//! is the answer those tests fail on; version 1 keeps parsing and keeps rule 112.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::Draft;

/// §9.13's vectors.
const RECORDS_ACCESS: &str = "records_access";
/// §9.14's vectors.
const COLD_RECORDS: &str = "cold_records";
/// §9.13's `VerificationRun` version 2 vectors.
const VERIFICATION_RUNS: &str = "verification_runs";

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
/// at `version`, and every invalid one is refused with its reason at its path. There are never
/// fewer than `valid` valid and `invalid` invalid cases on its base drafts, and each of `rules` has
/// an invalid case.
fn every_case_of(
    (name, event_type, version): (&str, &str, u64),
    valid: usize,
    invalid: usize,
    rules: &[u32],
) {
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
        if got != Ok((event_type.to_owned(), version)) {
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
        (RECORDS_ACCESS, "RecordsAccessed", 1),
        7,
        47,
        &[81, 82, 107, 108],
    );
}

/// `ExportCreated`: its closed members and types, rule 107's ranges, rule 109's writer and its
/// `view` exactly for a view, and rule 83's refusal of a client.
#[test]
fn every_export_created_draft_parses_or_is_refused_as_its_case_says() {
    every_case_of((RECORDS_ACCESS, "ExportCreated", 1), 2, 23, &[83, 107, 109]);
}

/// `VerificationRun`: its closed members and types, rule 107's ranges, rule 110's writer per
/// trigger, rule 111's `failure.seq` and `to_hash`, rule 112's result, and rule 83's refusal of a
/// client.
#[test]
fn every_verification_run_draft_parses_or_is_refused_as_its_case_says() {
    every_case_of(
        (RECORDS_ACCESS, "VerificationRun", 1),
        5,
        31,
        &[83, 107, 110, 111, 112],
    );
}

/// §11's per-event checks 1 to 6, `anchor_head_mismatch`, `anchor_self_mismatch`,
/// `intent_action_mismatch`, `mode_event_mismatch`, `held_mismatch`, the connection's two, and
/// `break_glass_cause_mismatch`, then its per-range checks, as rule 111 lists them.
const PER_EVENT: [&str; 15] = [
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
    "held_mismatch",
    "connection_lifecycle_mismatch",
    "connection_cause_mismatch",
    "break_glass_cause_mismatch",
];
const PER_RANGE: [&str; 4] = [
    "anchor_root_mismatch",
    "tsa_token_invalid",
    "segment_manifest_mismatch",
    "segment_gap",
];

/// Rule 111 for each of §9.13's nineteen check codes, which the vectors exercise only in part: a
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
    every_case_of((COLD_RECORDS, "AnchorComputed", 1), 3, 30, &[113, 114, 115]);
}

/// `SegmentExported`: its closed members and types; rule 116's tenant, bounds and genesis; rule
/// 117's manifest hash over DEC-263's six fields; and rule 118's writer.
#[test]
fn every_segment_exported_draft_parses_or_is_refused_as_its_case_says() {
    every_case_of(
        (COLD_RECORDS, "SegmentExported", 1),
        2,
        29,
        &[116, 117, 118],
    );
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

/// `VerificationRun` version 2 against the vectors' `verification_runs` section: its closed
/// members and types, `start`, `checked` and `incomplete` with its `cause` included; version 1 left
/// unedited and no version 3; rules 107, 110 and 111 judging version 2; every clause of rule 132;
/// rule 133's precedence; and rule 83's refusal of a client.
#[test]
fn every_verification_run_version_2_draft_parses_or_is_refused_as_its_case_says() {
    every_case_of(
        (VERIFICATION_RUNS, "VerificationRun", 2),
        12,
        52,
        &[83, 110, 132, 133],
    );
}

/// One version 2 `checked_range` from `from` to `to`, entered from a `kind` start, that walked
/// `checked` events and ended `outcome`: a failed range reports `rehash_mismatch` at `from` and
/// names no head; an incomplete one names `cause`.
fn checked_range(
    (stream, from, to): (&str, u64, u64),
    kind: &str,
    checked: u64,
    (outcome, cause): (&str, &str),
) -> String {
    let prev_hash = if from == 1 { "0" } else { "2" }.repeat(64);
    let manifest = (kind == "manifest").then(|| format!(r#""{}""#, "a".repeat(64)));
    let anchor = (kind == "anchor").then_some(r#""01J8Z3A1A000000000000000A1""#);
    let to_hash = (outcome != "fail").then(|| format!(r#""{}""#, "3".repeat(64)));
    let failure =
        (outcome == "fail").then(|| format!(r#"{{"check":"rehash_mismatch","seq":{from}}}"#));
    let incomplete = (outcome == "incomplete")
        .then(|| format!(r#"{{"check":"tsa_token_invalid","cause":"{cause}"}}"#));
    let or_null = |member: Option<String>| member.unwrap_or_else(|| "null".to_owned());
    format!(
        r#"{{"stream_id":"{stream}","from_seq":{from},"to_seq":{to},"prev_hash":"{prev_hash}",
        "to_hash":{},"start":{{"kind":"{kind}","manifest_hash":{},"anchor_event_id":{}}},
        "checked":{checked},"failure":{},"incomplete":{}}}"#,
        or_null(to_hash),
        or_null(manifest),
        anchor.unwrap_or("null"),
        or_null(failure),
        or_null(incomplete),
    )
}

/// What `Draft::parse` answers a version 2 run of `ranges` recording `result`: its version, or its
/// refusal's reason and path.
fn version_2_run(
    section: &Value,
    ranges: &[String],
    result: &str,
) -> Result<u64, (String, String)> {
    let case = parse(
        format!(
            r#"{{"base_draft":"run_passed","changes":[
            {{"path":"payload.ranges","value":[{}]}},
            {{"path":"payload.result","value":"{result}"}}]}}"#,
            ranges.join(",")
        )
        .as_bytes(),
    )
    .unwrap();
    Draft::parse(&draft(section, &case))
        .map(|d| d.schema_version())
        .map_err(|e| (e.reason.code().to_owned(), e.path))
}

const OUTCOMES: [&str; 3] = ["pass", "incomplete", "fail"];
const CAUSES: [&str; 2] = ["token_unverifiable", "anchor_unstamped"];

/// Rule 133 for every mix of three ranges' outcomes, each result tried: `fail` when one failed,
/// else `incomplete` when one is incomplete, else `pass`, recomputed here by counting, and any
/// other result refused at `payload.result`. So an incomplete check alone never fails a run, a
/// failure always outranks one, and rule 112's pass-or-fail never judges version 2.
#[test]
fn a_version_2_result_is_fail_over_incomplete_over_pass_for_every_mix_of_ranges() {
    let section = section(VERIFICATION_RUNS);
    let streams = [
        "acct:ws_01J8Z2:01J8Z2ACCT00000000000000A1",
        "agent:ws_01J8Z2:agent_a",
        "ctl:ws_01J8Z2",
    ];
    let mut failed = Vec::new();
    for mix in 0..27_usize {
        let outcomes: Vec<&str> = (0..3).map(|i| OUTCOMES[mix / 3_usize.pow(i) % 3]).collect();
        let ranges: Vec<String> = streams
            .iter()
            .zip(&outcomes)
            .enumerate()
            .map(|(i, (stream, outcome))| {
                let kind = if *outcome == "incomplete" {
                    "anchor"
                } else {
                    "manifest"
                };
                let checked = if *outcome == "fail" { 4 } else { 19 };
                checked_range((stream, 12, 30), kind, checked, (outcome, CAUSES[i % 2]))
            })
            .collect();
        let count = |outcome: &str| outcomes.iter().filter(|o| **o == outcome).count();
        let expected = match (count("fail"), count("incomplete")) {
            (0, 0) => "pass",
            (0, _) => "incomplete",
            _ => "fail",
        };
        for result in OUTCOMES {
            let got = version_2_run(&section, &ranges, result);
            let want = if result == expected {
                Ok(2)
            } else {
                Err(("schema".to_owned(), "payload.result".to_owned()))
            };
            if got != want {
                failed.push(format!(
                    "{outcomes:?} as {result}: want {want:?}, got {got:?}"
                ));
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Rule 132 for one range of every start kind, entry `seq`, length, outcome and count: a genesis
/// start enters at seq 1 and an anchor at seq 2 or later (`start.kind`); `checked` never exceeds
/// the range and is the whole range unless it failed (`checked`); and an anchor start's range is
/// never `pass` (`incomplete`). The expected answer is recomputed here clause by clause, the run's
/// result always rule 133's.
#[test]
fn rule_132_places_each_start_and_bounds_checked_for_every_range() {
    let section = section(VERIFICATION_RUNS);
    let mut failed = Vec::new();
    for (kind, from, length) in ["genesis", "manifest", "anchor"]
        .into_iter()
        .flat_map(|k| [1, 2, 3].map(|f| (k, f)))
        .flat_map(|(k, f)| [1, 2, 5].map(|n| (k, f, n)))
    {
        let to = from + length - 1;
        for outcome in OUTCOMES {
            for checked in 0..=length + 1 {
                let range = checked_range(
                    ("agent:ws_01J8Z2:agent_a", from, to),
                    kind,
                    checked,
                    (outcome, "token_unverifiable"),
                );
                let refused_at =
                    if (kind == "genesis" && from != 1) || (kind == "anchor" && from < 2) {
                        Some("start.kind")
                    } else if checked > length || (outcome != "fail" && checked != length) {
                        Some("checked")
                    } else if kind == "anchor" && outcome == "pass" {
                        Some("incomplete")
                    } else {
                        None
                    };
                let want = refused_at.map_or(Ok(2), |member| {
                    Err(("schema".to_owned(), format!("payload.ranges[0].{member}")))
                });
                let got = version_2_run(&section, &[range], outcome);
                if got != want {
                    let at = (kind, from, to, outcome, checked);
                    failed.push(format!("{at:?}: want {want:?}, got {got:?}"));
                }
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Version 1 is not edited (§8) and stays registered: its base drafts still parse at version 1;
/// rule 112 still judges it, so a flipped `result` is refused at `payload.result`; `incomplete` is
/// still no result of it; and its range still holds none of version 2's members.
#[test]
fn version_1_still_parses_and_rule_112_still_judges_it() {
    let section = section(RECORDS_ACCESS);
    let mut failed = Vec::new();
    for (base_draft, flipped) in [("verification_pass", "fail"), ("verification_fail", "pass")] {
        let case = |changes: &str| {
            parse(format!(r#"{{"base_draft":"{base_draft}","changes":[{changes}]}}"#).as_bytes())
                .unwrap()
        };
        let answer = |changes: &str| {
            Draft::parse(&draft(&section, &case(changes)))
                .map(|d| d.schema_version())
                .map_err(|e| (e.reason.code(), e.path))
        };
        let checks = [
            ("", Ok(1)),
            (
                &format!(r#"{{"path":"payload.result","value":"{flipped}"}}"#),
                Err(("schema", "payload.result".to_owned())),
            ),
            (
                r#"{"path":"payload.result","value":"incomplete"}"#,
                Err(("non_canonical", "payload.result".to_owned())),
            ),
        ];
        for (changes, want) in checks {
            let got = answer(changes);
            if got != want {
                failed.push(format!(
                    "{base_draft} with [{changes}]: want {want:?}, got {got:?}"
                ));
            }
        }
        let range = &list(base(&section, base_draft).get("payload").unwrap(), "ranges")[0];
        for member in ["start", "checked", "incomplete"] {
            let mut extended = range.as_object().cloned().unwrap();
            extended.insert(Key::new(member).unwrap(), Value::Null);
            let extended = String::from_utf8(to_canonical(&Value::Object(extended))).unwrap();
            let changes = format!(r#"{{"path":"payload.ranges","value":[{extended}]}}"#);
            let want = Err(("schema", format!("payload.ranges[0].{member}")));
            let got = answer(&changes);
            if got != want {
                failed.push(format!(
                    "{base_draft} with {member}: want {want:?}, got {got:?}"
                ));
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
