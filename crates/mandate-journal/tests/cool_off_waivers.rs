//! Journal spec v0.112 §9.38 (DEC-2146): `StepUpCoolOffWaived` on the control stream at schema
//! version 1, with rule 316 at append and batch rule 317 after rule 253, through the journal's own
//! checks against the vectors' `cool_off_waivers` section. Every base and valid draft parses; every
//! invalid draft is refused with its reason at its path; every batch is judged by `check_batch` as
//! it says; and stream rule 318 is the writer's, so [`check_stream`] refuses none of the section's
//! streams.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::{Draft, StreamType, check_batch, check_stream, closes};

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("cool_off_waivers").cloned().unwrap()
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

/// The vectors' section is §9.38's, with every part this file reads.
#[test]
fn the_cool_off_waiver_section_holds_every_part() {
    let section = section();
    assert!(text(&section, "spec").contains("§9.38"));
    for (part, at_least) in [
        ("invalid_drafts", 20),
        ("valid_drafts", 2),
        ("batches", 12),
        ("streams", 17),
    ] {
        let n = list(&section, part).len();
        assert!(n >= at_least, "{n} {part}; never fewer than {at_least}");
    }
}

#[test]
fn each_base_and_every_valid_step_up_cool_off_waived_draft_parse() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (name, body) in drafts {
        match Draft::parse(&to_canonical(body)) {
            Ok(d) if d.event_type() == "StepUpCoolOffWaived" => {}
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
fn every_invalid_step_up_cool_off_waived_draft_is_refused_with_its_reason_at_its_path() {
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

/// Rules 253 and 317 over each batch vector: each draft's own rules first, then each assertion
/// against the record before it, then each waiver against the assertion before it, reported by its
/// index in the batch.
#[test]
fn every_cool_off_waiver_batch_is_judged_as_its_vector_says() {
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
                expect
                    .get("index")
                    .and_then(Value::as_int)
                    .and_then(|i| usize::try_from(i).ok())
                    .unwrap_or_default(),
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

/// Stream rule 318 (the actor's own credential, its enrolment's `cool_off_ends` still ahead, one
/// waiver per challenge, and every assertion inside a cool-off waived) is the writer's: every
/// record of every stream case parses, the ones rule 318 refuses included, and [`check_stream`]
/// refuses none of them (§9.38).
#[test]
fn append_leaves_rule_318_to_the_writer() {
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

/// `StepUpCoolOffWaived` is closed on the control stream at version 1 and nowhere else (v0.112).
#[test]
fn the_catalogue_closes_step_up_cool_off_waived_on_the_control_stream_only() {
    assert!(closes("StepUpCoolOffWaived", 1, StreamType::Control));
    assert!(!closes("StepUpCoolOffWaived", 2, StreamType::Control));
    for stream in [
        StreamType::Agent,
        StreamType::Account,
        StreamType::Scheduler,
        StreamType::Notice,
        StreamType::Meter,
    ] {
        assert!(!closes("StepUpCoolOffWaived", 1, stream), "{stream:?}");
    }
}
