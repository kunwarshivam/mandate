//! Journal spec v0.79 §9.34 (DEC-2352, E8-17): the agent stream's `RetirementHeld` at schema version
//! 1, rule 265 on the draft, and stream rule 266 over the agent's records in commit order. Through
//! [`Draft::parse`] against the vectors' `retirements` section: the base and every valid draft parse
//! as the record and every invalid draft is refused with its reason at its path. Each of the
//! section's streams is judged by [`retirement_failure`] and by an independent oracle written here,
//! which reads the retirement's ends itself and walks each segment's consecutive records; both
//! agree with the vectors, and both refuse a record broken out of every valid stream.

use std::path::Path;

use mandate_canon::{Int, Key, Object, Value, parse, to_canonical};
use mandate_journal::{Draft, RetirementTerms, retirement_failure, retirement_held_silent};
use mandate_num::Ratio;
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

/// One version's terms as the `silence` vectors give them.
fn terms_of(case: &Value, version: &str) -> Option<RetirementTerms> {
    let mandate = list(case, "mandates")
        .iter()
        .find(|m| text(m, "version") == version)?;
    let t = mandate.get("terms")?;
    Some(RetirementTerms {
        profit_level: Ratio::parse(canonical(text(t, "profit_level"))).ok()?,
        end_date: text(t, "end_date").to_owned(),
        on_complete: text(t, "on_complete").to_owned(),
    })
}

/// A mandate document's decimal in the journal's canonical form (§4.6): no trailing fractional
/// zeros, and no point without a fraction.
fn canonical(decimal: &str) -> &str {
    if decimal.contains('.') {
        decimal.trim_end_matches('0').trim_end_matches('.')
    } else {
        decimal
    }
}

/// A decimal as an integer of millionths, the oracle's own fixed point: every figure the vectors
/// give has at most six fractional digits.
fn micros(decimal: &str) -> i128 {
    let (whole, fraction) = decimal.split_once('.').unwrap_or((decimal, ""));
    let negative = whole.starts_with('-');
    let whole: i128 = whole.trim_start_matches('-').parse().unwrap();
    assert!(
        fraction.len() <= 6,
        "the oracle's fixed point holds {decimal} exactly"
    );
    let digits = format!("{fraction:0<6}");
    let fraction: i128 = digits[..6].parse().unwrap();
    let value = whole * 1_000_000 + fraction;
    if negative { -value } else { value }
}

/// §9.34's `retirement_held_silent` read another way: the sequence's last `RetirementHeld`, then
/// every record after it, each end and each cancelling version clearing it and the version it was
/// held under; a profit level read as millionths and compared with the trigger's return by
/// cross-multiplying in `i128`, with no division to truncate.
fn silent_oracle(case: &Value) -> bool {
    let cause = case.get("cause").unwrap();
    let terms = |version: &str| {
        list(case, "mandates")
            .iter()
            .find(|m| text(m, "version") == version)
            .and_then(|m| m.get("terms"))
    };
    let mut last: Option<i64> = None;
    let mut held_under: Option<&Value> = None;
    for r in list(case, "records") {
        match text(r, "event_type") {
            "RetirementHeld" => {
                last = Some(seconds(text(payload(r), "risk_clock")));
                if held_under.is_none() {
                    held_under = terms(version(r));
                }
            }
            "AgentStopped" if text(payload(r), "agent_id") == AGENT => {
                last = None;
                held_under = None;
            }
            "OwnerCommandIssued"
                if text(payload(r), "scope") == "agent"
                    && text(payload(r), "subject") == AGENT
                    && matches!(text(payload(r), "command"), "resume" | "stop") =>
            {
                last = None;
                held_under = None;
            }
            "MandateVersionApplied"
                if last.is_some()
                    && text(payload(r), "agent_id") == AGENT
                    && text(payload(r), "result") == "applied" =>
            {
                let Some(new) = terms(text(payload(r), "new_version")) else {
                    continue;
                };
                let c = payload(cause);
                let cancels = match text(cause, "event_type") {
                    "RiskLimitTriggered" => {
                        text(c, "limit") == "profit_stop" && {
                            let base = micros(text(c, "capital_base"));
                            let gain = micros(text(c, "agent_equity")) - base;
                            base > 0 && micros(text(new, "profit_level")) * base > gain * 1_000_000
                        }
                    }
                    "GoalCompleted" if text(c, "reason") == "end_date" => {
                        held_under.is_some_and(|h| text(new, "end_date") > text(h, "end_date"))
                    }
                    "GoalCompleted" if text(c, "on_complete") == "release" => held_under
                        .is_some_and(|h| text(new, "on_complete") != text(h, "on_complete")),
                    _ => false,
                };
                if cancels {
                    last = None;
                    held_under = None;
                }
            }
            _ => {}
        }
    }
    last.is_some_and(|at| seconds(text(case, "now")) - at > 2 * DAY)
}

#[test]
fn every_silence_case_is_judged_as_the_vectors_judge_it_by_the_finding_and_the_oracle() {
    let section = section();
    let cases = list(&section, "silence");
    assert!(
        cases.len() >= 11,
        "{} silence cases; never fewer",
        cases.len()
    );
    let mut failed = Vec::new();
    for case in cases {
        let want = case.get("silent") == Some(&Value::Bool(true));
        let now = seconds(text(case, "now"));
        let found = retirement_held_silent(
            AGENT,
            case.get("cause").unwrap(),
            list(case, "records"),
            |version| terms_of(case, version),
            now,
        );
        let read = silent_oracle(case);
        if found != want || read != want {
            failed.push(format!(
                "{}: want {want}, finding {found}, oracle {read}",
                text(case, "name")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// The finding's edges the vectors leave open: at exactly 48 hours it is no silence, one second past
/// it is; another agent's stop ends nothing; a version whose terms cannot be read cancels nothing,
/// so a silence is still reported (`AGENTS.md` rule 3); and no held record is no sequence.
#[test]
fn the_silence_finding_reads_its_edges_conservatively() {
    let section = section();
    let case = list(&section, "silence")
        .iter()
        .find(|c| text(c, "name") == "held_and_writing")
        .unwrap();
    let cause = case.get("cause").unwrap();
    let records = list(case, "records");
    let last = seconds(text(payload(&records[0]), "risk_clock"));
    let terms = |version: &str| terms_of(case, version);
    let judge =
        |records: &[Value], now: i64| retirement_held_silent(AGENT, cause, records, terms, now);
    assert!(
        !judge(records, last + 2 * DAY),
        "48 hours exactly is not silent"
    );
    assert!(judge(records, last + 2 * DAY + 1), "a second past it is");
    let mut others = records.to_vec();
    others.push(record(
        r#"{"event_id":"01J8Z5RHH00000000000000000","event_type":"AgentStopped","causation_id":null,"config_refs":{},"payload":{"agent_id":"agent_b"}}"#,
    ));
    assert!(
        judge(&others, last + 3 * DAY),
        "another agent's stop ends nothing"
    );
    let mut stopped = records.to_vec();
    stopped.push(record(
        r#"{"event_id":"01J8Z5RHJ00000000000000000","event_type":"AgentStopped","causation_id":null,"config_refs":{},"payload":{"agent_id":"agent_a"}}"#,
    ));
    assert!(!judge(&stopped, last + 3 * DAY), "its own stop ends it");
    let mut unknown = records.to_vec();
    unknown.push(record(
        r#"{"event_id":"01J8Z5RHK00000000000000000","event_type":"MandateVersionApplied","causation_id":null,"config_refs":{},"payload":{"agent_id":"agent_a","new_version":"sha256:9999999999999999999999999999999999999999999999999999999999999999","result":"applied"}}"#,
    ));
    assert!(
        judge(&unknown, last + 3 * DAY),
        "a version it cannot read cancels nothing"
    );
    let mut rejected = records.to_vec();
    rejected.push(record(
        r#"{"event_id":"01J8Z5RHM00000000000000000","event_type":"MandateVersionApplied","causation_id":null,"config_refs":{},"payload":{"agent_id":"agent_a","new_version":"sha256:2222222222222222222222222222222222222222222222222222222222222222","result":"rejected"}}"#,
    ));
    assert!(
        judge(&rejected, last + 3 * DAY),
        "a rejected version cancels nothing"
    );
    let cancelling = list(&section, "silence")
        .iter()
        .find(|c| text(c, "name") == "cancelled_by_a_raised_level")
        .unwrap();
    let raised = |agent: &str| {
        let mut records = list(cancelling, "records").to_vec();
        for r in &mut records {
            if text(r, "event_type") == "MandateVersionApplied"
                && let Value::Object(record) = r
            {
                set_payload(record, "agent_id", Value::Str(agent.into()));
            }
        }
        retirement_held_silent(
            AGENT,
            cancelling.get("cause").unwrap(),
            &records,
            |version| terms_of(cancelling, version),
            seconds(text(cancelling, "now")),
        )
    };
    assert!(!raised(AGENT), "its own raised level cancels it");
    assert!(
        raised("agent_b"),
        "another agent's raised level cancels nothing"
    );
    assert!(!judge(&[], last + 30 * DAY), "no held record, no sequence");
}

/// `case` with `change` applied to its object.
fn changed(case: &Value, change: impl Fn(&mut Object)) -> Value {
    let mut out = case.as_object().cloned().unwrap();
    change(&mut out);
    Value::Object(out)
}

/// Each reading is shown to fail on a seeded bug before it is trusted: a cancelling case whose cause
/// becomes a drawdown trigger, or whose cancelling version is dropped, and an ended case whose end is
/// dropped, each turn silent; a silent case read a day earlier is not. The finding and the oracle
/// both follow each change.
#[test]
fn the_finding_and_the_oracle_both_follow_a_seeded_change_to_every_case() {
    let section = section();
    let cases = list(&section, "silence");
    let named = |name: &str| cases.iter().find(|c| text(c, "name") == name).unwrap();
    let both = |case: &Value| {
        let found = retirement_held_silent(
            AGENT,
            case.get("cause").unwrap(),
            list(case, "records"),
            |version| terms_of(case, version),
            seconds(text(case, "now")),
        );
        (found, silent_oracle(case))
    };
    let drop_type = |event_type: &'static str| {
        move |case: &mut Object| {
            if let Some(Value::Array(records)) = case.get_mut("records") {
                records.retain(|r| text(r, "event_type") != event_type);
            }
        }
    };
    let drawdown = changed(named("cancelled_by_a_raised_level"), |case| {
        if let Some(Value::Object(cause)) = case.get_mut("cause") {
            set_payload(cause, "limit", Value::Str("drawdown_rung".into()));
        }
    });
    let unversioned = changed(
        named("cancelled_by_a_later_end_date"),
        drop_type("MandateVersionApplied"),
    );
    let unreleased = changed(
        named("cancelled_by_on_complete"),
        drop_type("MandateVersionApplied"),
    );
    let unended = changed(named("ended"), |case| {
        if let Some(Value::Array(records)) = case.get_mut("records") {
            records.retain(|r| {
                !matches!(text(r, "event_type"), "AgentStopped" | "OwnerCommandIssued")
            });
        }
    });
    let earlier = changed(named("silent"), |case| {
        let now = seconds(case.get("now").and_then(Value::as_str).unwrap()) - DAY;
        case.insert(
            Key::new("now").unwrap(),
            Value::Str(UtcNanos::from_parts(now, 0).unwrap().to_string()),
        );
    });
    for (bug, case, want) in [
        ("a drawdown cause", &drawdown, true),
        ("no cancelling end_date", &unversioned, true),
        ("no cancelling on_complete", &unreleased, true),
        ("no end", &unended, true),
        ("a day earlier", &earlier, false),
    ] {
        assert_eq!(both(case), (want, want), "{bug}");
    }
}

/// A `silence` case built here: `cause`, the versions' terms, records, and `now`, judged by the
/// finding and by the oracle.
fn judged(
    cause: Value,
    mandates: &[(&str, &str, &str, &str)],
    records: Vec<Value>,
    now: i64,
) -> (bool, bool) {
    let mandates: Vec<Value> = mandates
        .iter()
        .map(|(v, level, end, on)| {
            record(&format!(
                r#"{{"version":"{v}","terms":{{"profit_level":"{level}","end_date":"{end}","on_complete":"{on}"}}}}"#
            ))
        })
        .collect();
    let mut case = Object::new();
    case.insert(Key::new("cause").unwrap(), cause);
    case.insert(Key::new("mandates").unwrap(), Value::Array(mandates));
    case.insert(Key::new("records").unwrap(), Value::Array(records));
    case.insert(
        Key::new("now").unwrap(),
        Value::Str(UtcNanos::from_parts(now, 0).unwrap().to_string()),
    );
    let case = Value::Object(case);
    let found = retirement_held_silent(
        AGENT,
        case.get("cause").unwrap(),
        list(&case, "records"),
        |version| terms_of(&case, version),
        now,
    );
    (found, silent_oracle(&case))
}

const V1: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const V2: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const V3: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const T0: i64 = 1_790_000_000;

fn held_at(id: &str, version: &str, at: i64) -> Value {
    record(&format!(
        r#"{{"event_id":"{id}","event_type":"RetirementHeld","causation_id":"01J8Z5RH0B000000000000000M","config_refs":{{"mandate_version":"{version}"}},"payload":{{"held_since":"{}","period":1,"waiting_on":["position"],"risk_clock":"{}"}}}}"#,
        UtcNanos::from_parts(at - DAY, 0).unwrap(),
        UtcNanos::from_parts(at, 0).unwrap()
    ))
}

fn applied(id: &str, version: &str) -> Value {
    record(&format!(
        r#"{{"event_id":"{id}","event_type":"MandateVersionApplied","causation_id":null,"config_refs":{{}},"payload":{{"agent_id":"agent_a","new_version":"{version}","result":"applied"}}}}"#
    ))
}

fn trigger(equity: &str, base: &str, limit: &str) -> Value {
    record(&format!(
        r#"{{"event_type":"RiskLimitTriggered","event_id":"01J8Z5RH0A000000000000000S","payload":{{"agent_id":"agent_a","agent_equity":"{equity}","capital_base":"{base}","limit":"{limit}"}}}}"#
    ))
}

fn end_date_goal() -> Value {
    record(
        r#"{"event_type":"GoalCompleted","event_id":"01J8Z5RH0B000000000000000M","payload":{"agent_id":"agent_a","reason":"end_date","then":null,"on_complete":null}}"#,
    )
}

/// The review of #2742's edges, each judged alike by the finding and the oracle (DEC-2835):
/// a second sequence is read against its own version, never the first one's; an `end_date` equal
/// to the one held under cancels nothing; a base that is not positive, figures that do not read,
/// and another limit's trigger with a level above any return cancel nothing; a level exactly at the
/// trigger's return cancels nothing, one above it does.
#[test]
fn every_cancellation_edge_is_read_alike_by_the_finding_and_the_oracle() {
    let late = T0 + 3 * DAY;
    let resumed = record(
        r#"{"event_id":"01J8Z5RHN00000000000000000","event_type":"OwnerCommandIssued","causation_id":null,"config_refs":{},"payload":{"command":"resume","scope":"agent","subject":"agent_a"}}"#,
    );
    let second_sequence = vec![
        held_at("01J8Z5RHP00000000000000000", V1, T0 - 10 * DAY),
        resumed,
        held_at("01J8Z5RHQ00000000000000000", V2, T0),
        applied("01J8Z5RHR00000000000000000", V3),
    ];
    let terms = [
        (V1, "0.1", "2026-09-01", "release"),
        (V2, "0.1", "2026-11-01", "release"),
        (V3, "0.1", "2026-10-01", "release"),
    ];
    assert_eq!(
        judged(end_date_goal(), &terms, second_sequence, late),
        (true, true),
        "a later end_date than the first sequence's but not the second's cancels nothing"
    );
    let same_date = vec![
        held_at("01J8Z5RHP00000000000000000", V1, T0),
        applied("01J8Z5RHR00000000000000000", V2),
    ];
    let equal = [
        (V1, "0.1", "2026-10-01", "release"),
        (V2, "0.1", "2026-10-01", "release"),
    ];
    assert_eq!(
        judged(end_date_goal(), &equal, same_date.clone(), late),
        (true, true),
        "an equal end_date"
    );
    for (case, cause, want) in [
        ("a zero base", trigger("-5", "0", "profit_stop"), true),
        ("a negative base", trigger("-5", "-10", "profit_stop"), true),
        (
            "equity that does not read",
            trigger("x", "10000", "profit_stop"),
            true,
        ),
        (
            "another limit with a high level",
            trigger("11200", "10000", "drawdown_rung"),
            true,
        ),
        (
            "a level at the return",
            trigger("12000", "10000", "profit_stop"),
            true,
        ),
        (
            "a level above the return",
            trigger("11999", "10000", "profit_stop"),
            false,
        ),
    ] {
        let levels = [
            (V1, "0.1", "2026-10-01", "release"),
            (V2, "0.2", "2026-10-01", "release"),
        ];
        let records = vec![
            held_at("01J8Z5RHP00000000000000000", V1, T0),
            applied("01J8Z5RHR00000000000000000", V2),
        ];
        if case == "equity that does not read" {
            let found = retirement_held_silent(
                AGENT,
                &cause,
                &records,
                |v| {
                    levels
                        .iter()
                        .find(|(x, ..)| *x == v)
                        .map(|(_, l, e, o)| RetirementTerms {
                            profit_level: Ratio::parse(l).unwrap(),
                            end_date: (*e).to_owned(),
                            on_complete: (*o).to_owned(),
                        })
                },
                late,
            );
            assert_eq!(found, want, "{case}");
            continue;
        }
        assert_eq!(
            judged(cause, &levels, records, late),
            (want, want),
            "{case}"
        );
    }
}

/// A held record whose risk clock cannot be read is as old as can be: the finding is reported,
/// never dropped (DEC-2835 item 3).
#[test]
fn a_held_record_with_an_unreadable_clock_is_reported_silent() {
    let mut held = held_at("01J8Z5RHP00000000000000000", V1, T0);
    if let Value::Object(r) = &mut held {
        set_payload(r, "risk_clock", Value::Str("not a clock".into()));
    }
    let found = retirement_held_silent(AGENT, &end_date_goal(), &[held], |_| None, T0);
    assert!(found);
}
