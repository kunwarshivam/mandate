//! Journal spec v0.48 §9.21 (E19-5, DEC-2202): the agent stream's `ThesisInvalidated`, through the
//! journal's own checks against the vectors' `thesis_invalidation` section. Every base and valid
//! draft parses; every invalid draft is refused with its reason at its path (rules 165 and 166, the
//! closed members and their types); every batch is judged by `check_batch` as it says (rule 167);
//! and every stream is judged batch by batch, by `check_batch` and then `check_stream` against the
//! stream's earlier records, as it says (rule 168).

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::{Draft, StreamType, check_batch, check_stream, closes, reads_stream};

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("thesis_invalidation").cloned().unwrap()
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

fn expected(case: &Value) -> Option<(usize, String, String)> {
    let expect = case.get("expect").unwrap();
    match text(expect, "outcome") {
        "Valid" => None,
        "Invalid" => Some((
            expect
                .get("index")
                .and_then(Value::as_int)
                .and_then(|i| usize::try_from(i).ok())
                .unwrap_or(0),
            text(expect, "reason").to_owned(),
            text(expect, "path").to_owned(),
        )),
        other => panic!("{}: unknown outcome {other}", text(case, "name")),
    }
}

fn parsed(records: &[Value]) -> Vec<Draft> {
    records
        .iter()
        .map(|record| {
            Draft::parse(&to_canonical(record))
                .unwrap_or_else(|e| panic!("{} parses on its own: {e:?}", text(record, "event_id")))
        })
        .collect()
}

#[test]
fn every_thesis_invalidation_base_and_valid_draft_parses() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (name, body) in drafts {
        if let Err(e) = Draft::parse(&to_canonical(body)) {
            failed.push(format!("base {}: {e:?}", name.as_str()));
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
        valid.len() >= 5,
        "{} valid drafts; never fewer",
        valid.len()
    );
}

#[test]
fn every_invalid_thesis_invalidation_is_refused_with_its_reason_at_its_path() {
    let section = section();
    let invalid = list(&section, "invalid_drafts");
    let mut failed = Vec::new();
    for case in invalid {
        let (_, reason, path) = expected(case).unwrap();
        let got = Draft::parse(&draft(&section, case)).err();
        let got = got.as_ref().map(|e| (e.reason.code(), e.path.as_str()));
        if got != Some((reason.as_str(), path.as_str())) {
            failed.push(format!(
                "{}: expected {:?}, got {got:?}",
                text(case, "name"),
                (reason, path)
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        invalid.len() >= 31,
        "{} invalid drafts; never fewer",
        invalid.len()
    );
    for rule in ["165", "166"] {
        assert!(
            invalid.iter().any(|c| text(c, "name").contains(rule)),
            "rule {rule} has an invalid draft"
        );
    }
}

/// Rule 167 over each batch vector: the first draft that breaks it, by its index in the batch.
#[test]
fn every_thesis_invalidation_batch_is_judged_as_its_vector_says() {
    let section = section();
    let batches = list(&section, "batches");
    let mut failed = Vec::new();
    for case in batches {
        let drafts = parsed(list(case, "drafts"));
        let got = check_batch(&drafts)
            .err()
            .map(|(i, e)| (i, e.reason.code().to_owned(), e.path));
        let want = expected(case);
        if got != want {
            failed.push(format!(
                "{}: expected {want:?}, got {got:?}",
                text(case, "name")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        batches.len() >= 15,
        "{} batches; never fewer",
        batches.len()
    );
}

/// Rule 168 over each stream vector, appended batch by batch: each batch is judged by
/// `check_batch` and then by `check_stream` against the records of the batches before it, and the
/// first record that breaks either is reported by its index across the whole stream.
#[test]
fn every_thesis_invalidation_stream_is_judged_from_its_start_as_its_vector_says() {
    let section = section();
    let streams = list(&section, "streams");
    let mut failed = Vec::new();
    for case in streams {
        let mut earlier: Vec<Draft> = Vec::new();
        let mut got = None;
        for batch in list(case, "batches").iter().filter_map(Value::as_array) {
            let drafts = parsed(batch);
            let offset = earlier.len();
            let broke = check_batch(&drafts)
                .and_then(|()| check_stream(&earlier, &drafts))
                .err();
            if let Some((i, e)) = broke {
                got = Some((offset + i, e.reason.code().to_owned(), e.path));
                break;
            }
            earlier.extend(drafts);
        }
        let want = expected(case);
        if got != want {
            failed.push(format!(
                "{}: expected {want:?}, got {got:?}",
                text(case, "name")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        streams.len() >= 17,
        "{} streams; never fewer",
        streams.len()
    );
}

/// What `mandate-runtime`'s `JournalCatalog` reads for agent harness spec §6.2's invalidation
/// precondition, and which drafts make `append` read the stream's earlier records.
#[test]
fn the_catalogue_closes_thesis_invalidated_on_the_agent_stream_and_its_rule_reads_the_stream() {
    assert!(closes("ThesisInvalidated", 1, StreamType::Agent));
    assert!(!closes("ThesisInvalidated", 2, StreamType::Agent));
    for stream in [
        StreamType::Account,
        StreamType::Control,
        StreamType::Scheduler,
        StreamType::Notice,
        StreamType::Meter,
    ] {
        assert!(!closes("ThesisInvalidated", 1, stream), "{stream:?}");
    }
    let section = section();
    let invalidated = parsed(&[section
        .get("drafts")
        .and_then(|d| d.get("invalidated"))
        .cloned()
        .unwrap()]);
    assert!(reads_stream(&invalidated), "rule 168 reads the stream");
    let admitted = parsed(&[section
        .get("drafts")
        .and_then(|d| d.get("admitted"))
        .cloned()
        .unwrap()]);
    assert!(
        !reads_stream(&admitted),
        "a thesis record alone is judged by no stream rule"
    );
}
