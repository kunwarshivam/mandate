//! Journal spec v0.97 §9.37 (DEC-2278; E2-10): the news stream `news:{workspace_id}`, its
//! `StreamOpened`, and `NewsItemStored` at schema version 1, through the journal's own checks
//! against the vectors' `news_stream` section. Every base and valid draft parses; every invalid
//! draft is refused with its reason at its path (rules 283 and 286); every stream is judged by
//! [`check_stream`] as its vector says (rules 284 and 285).

use std::path::Path;
use std::sync::OnceLock;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::{
    Draft, StreamId, StreamType, check_stream, closes, reads_stream, stream_event_types,
    workspace_owns,
};

/// The section, read and parsed once for every test and property case in this binary.
fn section() -> Value {
    static SECTION: OnceLock<Value> = OnceLock::new();
    SECTION
        .get_or_init(|| {
            let path =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
            let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
            fixture.get("news_stream").cloned().unwrap()
        })
        .clone()
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

fn base(section: &Value, name: &str) -> Object {
    section
        .get("drafts")
        .and_then(|d| d.get(name))
        .and_then(Value::as_object)
        .cloned()
        .unwrap()
}

/// A case's draft: its named base draft with its changes applied, in canonical bytes.
fn draft(section: &Value, case: &Value) -> Vec<u8> {
    let mut body = base(section, text(case, "base_draft"));
    for change in list(case, "changes") {
        apply(&mut body, change);
    }
    to_canonical(&Value::Object(body))
}

fn news() -> StreamId {
    StreamId::parse("news:ws_01J8Z2").unwrap()
}

#[test]
#[ignore = "pending E2-10"]
fn every_news_base_and_valid_draft_parses() {
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
    assert_eq!(drafts.len(), 3, "the opening, an item, a licensed item");
    assert!(
        valid.len() >= 13,
        "{} valid drafts; never fewer",
        valid.len()
    );
}

/// The closed schema, rule 283's clauses in order, and rule 286's writer and subject: each draft
/// refused with its reason at the member named, the first the journal reports.
#[test]
#[ignore = "pending E2-10"]
fn every_invalid_news_draft_is_refused_with_its_reason_at_its_path() {
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
        invalid.len() >= 37,
        "{} invalid drafts; never fewer",
        invalid.len()
    );
}

/// Rules 284 and 285: each `streams` vector judged by [`check_stream`] from the stream's start, as
/// one batch and one record at a time, as the vector says.
#[test]
#[ignore = "pending E2-10"]
fn every_news_stream_is_judged_from_its_start_as_its_vector_says() {
    let section = section();
    let streams = list(&section, "streams");
    let mut failed = Vec::new();
    for case in streams {
        let records: Vec<Draft> = list(case, "records")
            .iter()
            .map(|record| Draft::parse(&to_canonical(record)).unwrap())
            .collect();
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
        let as_batch = check_stream(&[], &records)
            .err()
            .map(|(i, e)| (i, e.reason.code().to_owned(), e.path));
        let one_by_one = (0..records.len()).find_map(|i| {
            check_stream(&records[..i], &records[i..=i])
                .err()
                .map(|(at, e)| (i + at, e.reason.code().to_owned(), e.path))
        });
        if as_batch != want || one_by_one != want {
            failed.push(format!(
                "{}: expected {want:?}, as one batch {as_batch:?}, one by one {one_by_one:?}",
                text(case, "name")
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(streams.len() >= 8, "{} streams; never fewer", streams.len());
}

/// `append` reads the news stream's earlier records for a batch holding an item, so rules 284 and
/// 285 see every item already stored; the opening alone reads nothing.
#[test]
#[ignore = "pending E2-10"]
fn a_news_item_reads_its_stream() {
    let section = section();
    let item = Draft::parse(&to_canonical(&Value::Object(base(&section, "item")))).unwrap();
    assert!(reads_stream(std::slice::from_ref(&item)));
    let opened = Draft::parse(&to_canonical(&Value::Object(base(&section, "opened")))).unwrap();
    assert!(!reads_stream(std::slice::from_ref(&opened)));
}

/// §2: `news:{workspace_id}` is a stream type of its own, the workspace's, holding its opening
/// and `NewsItemStored` at version 1, and no other stream holds the record.
#[test]
#[ignore = "pending E2-10"]
fn the_news_stream_is_its_own_stream_type_and_holds_only_its_records() {
    let stream = news();
    for other in [
        "acct:ws_01J8Z2:a1",
        "agent:ws_01J8Z2:agent_a",
        "ctl:ws_01J8Z2",
        "clock:ws_01J8Z2",
        "ntf:ws_01J8Z2",
        "meter:ws_01J8Z2",
        "dmeter:dep_1",
    ] {
        assert_ne!(
            StreamId::parse(other).unwrap().stream_type(),
            stream.stream_type(),
            "{other}"
        );
    }
    assert!(StreamId::parse("news:ws_01J8Z2:extra").is_none());
    assert!(StreamId::parse("news:ws 1").is_none());
    assert!(workspace_owns("ws_01J8Z2", &stream));
    assert!(!workspace_owns("ws_01J8Z", &stream));
    assert!(closes("NewsItemStored", 1, stream.stream_type()));
    assert!(closes("StreamOpened", 1, stream.stream_type()));
    assert!(!closes("NewsItemStored", 2, stream.stream_type()));
    for other in [
        StreamType::Account,
        StreamType::Agent,
        StreamType::Control,
        StreamType::Scheduler,
        StreamType::Notice,
        StreamType::Meter,
        StreamType::DeploymentMeter,
    ] {
        assert!(!closes("NewsItemStored", 1, other), "{other:?}");
    }
    let held: Vec<&str> = stream_event_types(stream.stream_type()).collect();
    assert_eq!(held, ["StreamOpened", "NewsItemStored"]);
}
