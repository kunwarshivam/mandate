//! Journal spec v0.79 §9.34 (DEC-2352, E8-17): the agent stream's `RetirementHeld` at schema version
//! 1, rule 265 on the draft, and stream rule 266 over the agent's records in commit order. Through
//! [`Draft::parse`] against the vectors' `retirements` section: the base and every valid draft parse
//! as the record and every invalid draft is refused with its reason at its path. Each of the
//! section's streams is judged by [`retirement_failure`] and by an independent oracle written here,
//! which reads the retirement's ends itself and walks each segment's consecutive records; both
//! agree with the vectors, and both refuse a record broken out of every valid stream.

use std::path::Path;

use mandate_canon::{Int, Key, Object, Value, parse, to_canonical};
use mandate_journal::{Draft, retirement_failure};
use mandate_time::UtcNanos;

const AGENT: &str = "agent_a";
const DAY: i64 = 86_400;

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("retirements").cloned().unwrap()
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

fn payload(record: &Value) -> &Value {
    record.get("payload").unwrap_or(&Value::Null)
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

#[test]
fn the_retirements_section_holds_every_part() {
    let section = section();
    assert_eq!(
        text(&section, "spec"),
        "docs/specs/journal.md v0.79 §9.34 (DEC-2352)"
    );
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let names: Vec<&str> = drafts.keys().map(|k| k.as_str()).collect();
    assert_eq!(names, ["held"]);
    for (part, at_least) in [("invalid_drafts", 24), ("valid_drafts", 5), ("streams", 28)] {
        let n = list(&section, part).len();
        assert!(n >= at_least, "{n} {part}; never fewer than {at_least}");
    }
}

#[test]
fn the_base_and_every_valid_retirement_held_draft_parse_as_the_record() {
    let section = section();
    let mut failed = Vec::new();
    let base = section.get("drafts").and_then(|d| d.get("held")).unwrap();
    match Draft::parse(&to_canonical(base)) {
        Ok(d) if d.event_type() == "RetirementHeld" => {}
        got => failed.push(format!("base held: {:?}", got.err())),
    }
    for case in list(&section, "valid_drafts") {
        match Draft::parse(&draft(&section, case)) {
            Ok(d) if d.event_type() == "RetirementHeld" => {}
            got => failed.push(format!("valid {}: {:?}", text(case, "name"), got.err())),
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn every_invalid_retirement_held_draft_is_refused_with_its_reason_at_its_path() {
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

/// The index the vectors say the stream first breaks rule 266 at, or `None` for a valid one.
fn expected(stream: &Value) -> Option<usize> {
    let expect = stream.get("expect").unwrap();
    if text(expect, "outcome") == "Valid" {
        return None;
    }
    assert_eq!(text(expect, "rule"), "rule 266");
    expect
        .get("index")
        .and_then(Value::as_int)
        .and_then(|i| usize::try_from(i).ok())
}

fn seconds(clock: &str) -> i64 {
    UtcNanos::parse(clock).unwrap().secs()
}

fn period(record: &Value) -> Option<u64> {
    payload(record).get("period").and_then(Value::as_int)
}

fn version(record: &Value) -> &str {
    record
        .get("config_refs")
        .map_or("", |refs| text(refs, "mandate_version"))
}

/// Rule 266 read another way: alerts paired with their records by position first; then the stream
/// cut at each of this agent's ends, read here without the fold, and within a segment each held
/// record either begins a cause (first in its segment, or straight after a version applied to this
/// agent) from period 1, or continues the last by one with its `held_since`, a version in effect,
/// and a day of risk clock between them.
fn oracle(records: &[Value]) -> Option<usize> {
    let mut bad = Vec::new();
    for (i, r) in records.iter().enumerate() {
        let next = records.get(i + 1);
        let before = i.checked_sub(1).and_then(|b| records.get(b));
        if text(r, "event_type") == "RetirementHeld"
            && !next.is_some_and(|n| {
                text(n, "event_type") == "OwnerAlertSent"
                    && text(payload(n), "kind") == "retirement_held"
                    && text(payload(n), "subject") == text(r, "event_id")
            })
        {
            bad.push(i);
        }
        if text(r, "event_type") == "OwnerAlertSent"
            && text(payload(r), "kind") == "retirement_held"
            && !before.is_some_and(|b| {
                text(b, "event_type") == "RetirementHeld"
                    && text(b, "event_id") == text(payload(r), "subject")
            })
        {
            bad.push(i);
        }
    }
    let an_end = |r: &Value| match text(r, "event_type") {
        "AgentStopped" => text(payload(r), "agent_id") == AGENT,
        "OwnerCommandIssued" => {
            let c = payload(r);
            text(c, "scope") == "agent"
                && text(c, "subject") == AGENT
                && matches!(text(c, "command"), "resume" | "stop")
        }
        _ => false,
    };
    let mut seen: Vec<&str> = Vec::new();
    let mut last: Option<&Value> = None;
    let mut moved: Option<&str> = None;
    for (i, r) in records.iter().enumerate() {
        if an_end(r) {
            if let Some(l) = last {
                seen.push(text(l, "causation_id"));
            }
            last = None;
            moved = None;
            continue;
        }
        if text(r, "event_type") == "MandateVersionApplied" {
            let p = payload(r);
            if text(p, "agent_id") == AGENT && text(p, "result") == "applied" && last.is_some() {
                moved = Some(text(p, "new_version"));
            }
            continue;
        }
        if text(r, "event_type") != "RetirementHeld" {
            continue;
        }
        let cause = text(r, "causation_id");
        let fresh = match last {
            None => true,
            Some(l) => moved.is_some() && cause != text(l, "causation_id"),
        };
        if fresh {
            if let Some(l) = last {
                seen.push(text(l, "causation_id"));
            }
            if seen.contains(&cause) || period(r) != Some(1) {
                bad.push(i);
                break;
            }
        } else if let Some(l) = last {
            let ok = cause == text(l, "causation_id")
                && period(r) == period(l).map(|p| p + 1)
                && text(payload(r), "held_since") == text(payload(l), "held_since")
                && (version(r) == version(l) || moved == Some(version(r)))
                && seconds(text(payload(r), "risk_clock"))
                    - seconds(text(payload(l), "risk_clock"))
                    >= DAY;
            if !ok {
                bad.push(i);
                break;
            }
        }
        last = Some(r);
        moved = None;
    }
    bad.into_iter().min()
}

#[test]
fn every_retirement_stream_is_judged_as_the_vectors_judge_it_by_the_fold_and_the_oracle() {
    let section = section();
    let mut failed = Vec::new();
    for stream in list(&section, "streams") {
        let name = text(stream, "name");
        let records = list(stream, "records");
        let want = expected(stream);
        let fold = retirement_failure(AGENT, records);
        let read = oracle(records);
        if fold != want || read != want {
            failed.push(format!(
                "{name}: want {want:?}, fold {fold:?}, oracle {read:?}"
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// `stream` with its record at `at` changed by `change`.
fn broken(records: &[Value], at: usize, change: impl Fn(&mut Object)) -> Vec<Value> {
    let mut out = records.to_vec();
    if let Some(Value::Object(record)) = out.get_mut(at) {
        change(record);
    }
    out
}

fn set_payload(record: &mut Object, member: &str, value: Value) {
    if let Some(Value::Object(p)) = record.get_mut("payload") {
        p.insert(Key::new(member).unwrap(), value);
    }
}

/// Each check is shown to fail on a bug seeded into a valid stream before it is trusted: a held
/// record without its alert, one that skips a period, one with another `held_since`, one a day too
/// soon, and one under a version not in effect. The fold and the oracle both refuse each at the
/// broken record.
#[test]
fn the_fold_and_the_oracle_both_refuse_a_record_broken_out_of_every_valid_stream() {
    let section = section();
    let mut failed = Vec::new();
    let mut seeded = 0;
    for stream in list(&section, "streams")
        .iter()
        .filter(|s| expected(s).is_none())
    {
        let name = text(stream, "name");
        let records = list(stream, "records");
        let held: Vec<usize> = records
            .iter()
            .enumerate()
            .filter(|(_, r)| text(r, "event_type") == "RetirementHeld")
            .map(|(i, _)| i)
            .collect();
        let Some(&first) = held.first() else {
            continue;
        };
        let mut bugs: Vec<(&str, usize, Vec<Value>)> = Vec::new();
        let mut unalerted = records.to_vec();
        unalerted.remove(first + 1);
        bugs.push(("no alert", first, unalerted));
        bugs.push((
            "skips a period",
            first,
            broken(records, first, |r| {
                set_payload(r, "period", Value::Int(Int::new(2).unwrap()));
            }),
        ));
        if let Some(&second) = held.get(1).filter(|&&s| {
            text(&records[s], "causation_id") == text(&records[first], "causation_id")
        }) {
            bugs.push((
                "another held_since",
                second,
                broken(records, second, |r| {
                    set_payload(
                        r,
                        "held_since",
                        Value::Str("2026-09-01T00:00:00.000000000Z".into()),
                    );
                }),
            ));
            let soon = seconds(text(payload(&records[first]), "risk_clock")) + DAY - 1;
            bugs.push((
                "a day too soon",
                second,
                broken(records, second, |r| {
                    set_payload(
                        r,
                        "risk_clock",
                        Value::Str(UtcNanos::from_parts(soon, 0).unwrap().to_string()),
                    );
                }),
            ));
            bugs.push((
                "a version not in effect",
                second,
                broken(records, second, |r| {
                    r.insert(
                        Key::new("config_refs").unwrap(),
                        parse(br#"{"mandate_version":"sha256:9999999999999999999999999999999999999999999999999999999999999999"}"#)
                            .unwrap(),
                    );
                }),
            ));
        }
        for (bug, at, mutated) in bugs {
            seeded += 1;
            let fold = retirement_failure(AGENT, &mutated);
            let read = oracle(&mutated);
            if fold != Some(at) || read != Some(at) {
                failed.push(format!(
                    "{name} with {bug} at {at}: fold {fold:?}, oracle {read:?}"
                ));
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(seeded >= 20, "{seeded} seeded bugs; never fewer");
}

/// The vectors' first valid stream: one period and its alert.
fn one_period(section: &Value) -> Vec<Value> {
    let stream = list(section, "streams")
        .iter()
        .find(|s| text(s, "name") == "one_period")
        .unwrap();
    list(stream, "records").to_vec()
}

fn record(json: &str) -> Value {
    parse(json.as_bytes()).unwrap()
}

/// Rule 266(a) at its edges, which no vector reaches: an owner alert of another kind is no
/// retirement's and is not read; a `retirement_held` alert naming the record before it is refused
/// unless that record is a `RetirementHeld`; and a held record is answered only by its own alert,
/// `retirement_held` and naming it, so an alert of another kind or naming another record leaves it
/// unanswered (§9.34: "its `OwnerAlertSent` (`retirement_held`, `subject` it)", DEC-2831). The fold
/// and the oracle agree on each.
#[test]
fn a_held_record_is_answered_only_by_its_own_retirement_held_alert() {
    let section = section();
    let base = one_period(&section);
    assert_eq!(retirement_failure(AGENT, &base), None);
    let held_id = text(&base[0], "event_id").to_owned();

    let mut other_kind = base.clone();
    other_kind.push(record(
        r#"{"event_id":"01J8Z5RHB00000000000000000","event_type":"OwnerAlertSent","causation_id":"01J8Z5RHC00000000000000000","config_refs":{},"payload":{"subject":"01J8Z5RHC00000000000000000","kind":"kill_switch","owner_command":null}}"#,
    ));
    assert_eq!(
        retirement_failure(AGENT, &other_kind),
        None,
        "another kind's alert is not read"
    );

    let mut names_another_record = base.clone();
    names_another_record.push(record(
        r#"{"event_id":"01J8Z5RHD00000000000000000","event_type":"ApprovalTimedOut","causation_id":null,"config_refs":{},"payload":{}}"#,
    ));
    names_another_record.push(record(
        r#"{"event_id":"01J8Z5RHG00000000000000000","event_type":"OwnerAlertSent","causation_id":"01J8Z5RHD00000000000000000","config_refs":{},"payload":{"subject":"01J8Z5RHD00000000000000000","kind":"retirement_held","owner_command":null}}"#,
    ));
    let at = Some(names_another_record.len() - 1);
    assert_eq!(retirement_failure(AGENT, &names_another_record), at);
    assert_eq!(oracle(&names_another_record), at);

    for (case, change) in [
        ("of another kind", ("kind", "kill_switch")),
        (
            "naming another record",
            ("subject", "01J8Z5RHF00000000000000000"),
        ),
    ] {
        let mut unanswered = base.clone();
        if let Some(Value::Object(alert)) = unanswered.get_mut(1) {
            set_payload(alert, change.0, Value::Str(change.1.into()));
        }
        assert_eq!(
            retirement_failure(AGENT, &unanswered),
            Some(0),
            "a held record followed by an alert {case} is unanswered: {held_id}"
        );
        assert_eq!(
            oracle(&unanswered),
            Some(0),
            "and the oracle agrees: {case}"
        );
    }
}
