//! Journal spec v0.75 rules 258 to 260 (E7-4, DEC-2337), through the journal's own checks against
//! the vectors' `account_closes` section: §9.5's `AccountCloseRequested`, one request of one
//! instrument's close-position under an account kill switch. Every base and valid draft parses;
//! every invalid draft is refused with its reason at its path; and every stream vector is judged by
//! [`check_stream`] as it says (rule 260), from the stream's start, as one batch and one record at
//! a time.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::{Draft, StreamType, check_stream, closes, reads_stream};

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("account_closes").cloned().unwrap()
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

/// The base close and the closes rules 258 and 259 might be misread to refuse (a fraction of a
/// share, 12 places, exponent forms, a later attempt, a crypto pair) parse.
#[test]
fn every_account_close_base_and_valid_draft_parses() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (base, body) in drafts {
        if let Err(e) = Draft::parse(&to_canonical(body)) {
            failed.push(format!("base {}: {e:?}", base.as_str()));
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
        valid.len() >= 7,
        "{} valid drafts; never fewer",
        valid.len()
    );
}

/// Rules 258 and 259 and the closed schema: a quantity that is not positive, an attempt below 1,
/// a cause other than the switch, a null, absent or mistyped member, an extra one, another
/// version, or another stream, refused at the member named.
#[test]
fn every_invalid_account_close_is_refused_with_its_reason_at_its_path() {
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
        invalid.len() >= 24,
        "{} invalid drafts; never fewer",
        invalid.len()
    );
}

/// Rule 260 over each stream vector, from the stream's start, as one batch and one record after
/// the earlier ones: no two requests share a switch, an instrument, and an attempt.
#[test]
fn every_account_close_stream_is_judged_from_its_start_as_its_vector_says() {
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
    assert!(streams.len() >= 5, "{} streams; never fewer", streams.len());
}

/// `append` reads the account stream's earlier records for a batch holding a close request, so
/// rule 260 sees every earlier request.
#[test]
fn a_close_request_reads_its_stream() {
    let section = section();
    let close = section
        .get("drafts")
        .and_then(|d| d.get("close"))
        .map(|body| Draft::parse(&to_canonical(body)).unwrap())
        .unwrap();
    assert!(reads_stream(std::slice::from_ref(&close)));
}

/// The record is closed on the account stream at version 1 and nowhere else.
#[test]
fn the_catalogue_closes_account_close_requested_on_the_account_stream() {
    assert!(closes("AccountCloseRequested", 1, StreamType::Account));
    assert!(!closes("AccountCloseRequested", 2, StreamType::Account));
    for stream in [
        StreamType::Agent,
        StreamType::Control,
        StreamType::Scheduler,
        StreamType::Notice,
        StreamType::Meter,
    ] {
        assert!(!closes("AccountCloseRequested", 1, stream), "{stream:?}");
    }
}
