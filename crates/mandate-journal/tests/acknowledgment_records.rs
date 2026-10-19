//! Journal spec v0.61 §9.28 (DEC-2509; E8-3's acknowledgment follow-up, M7 S2): the owner's
//! acknowledgment, `OwnerAcknowledged`, closed, and its request, `OwnerAcknowledgmentRequested`, on
//! the control stream at schema version 1 with their account-stream copies, rule 239, and rule 28's
//! scoping of `OwnerCommandRefused`'s three new reasons. Through [`Draft::parse`] against the
//! vectors' `acknowledgments` section: every base and valid draft parses as its record, and every
//! invalid draft is refused with its reason at its path. `append` sees one record and checks rule
//! 239 only, so stream rules 240 and 241 are the writers' and the executor's, and
//! [`check_stream`] judges neither. Before the registration the journal refuses both records as
//! not registered and the three reasons as unknown, the answers these tests fail on.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::{Draft, check_stream};

/// Each base draft of the section, by name, and the record it is.
const BASES: [(&str, &str); 8] = [
    ("requested", "OwnerAcknowledgmentRequested"),
    ("acknowledged", "OwnerAcknowledged"),
    ("acknowledged_plain", "OwnerAcknowledged"),
    ("requested_copy", "OwnerAcknowledgmentRequested"),
    ("acknowledged_copy", "OwnerAcknowledged"),
    ("refused_trigger_unknown", "OwnerCommandRefused"),
    ("refused_trigger_superseded", "OwnerCommandRefused"),
    ("refused_request_invalid", "OwnerCommandRefused"),
];

/// `OwnerCommandRefused`'s reasons §9.28 adds, the executor's refusals of an acknowledgment copy.
const NEW_REASONS: [&str; 3] = ["trigger_unknown", "trigger_superseded", "request_invalid"];

/// The records §9.28 closes, the only ones the executor cases' `refused_at_append` may name.
const SECTION_9_28: [&str; 2] = ["OwnerAcknowledgmentRequested", "OwnerAcknowledged"];

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("acknowledgments").cloned().unwrap()
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

fn index(value: &Value) -> Option<usize> {
    value
        .get("index")
        .and_then(Value::as_int)
        .and_then(|i| usize::try_from(i).ok())
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

/// What `Draft::parse` answers a record with: its event type, or its refusal's reason and path.
fn answer(record: &Value) -> Result<String, (String, String)> {
    Draft::parse(&to_canonical(record))
        .map(|d| d.event_type().to_owned())
        .map_err(|e| (e.reason.code().to_owned(), e.path))
}

/// The vectors' section is §9.28's, with every part this file and the other crates' stories read.
#[test]
fn the_acknowledgment_section_holds_every_part() {
    let section = section();
    assert_eq!(
        text(&section, "spec"),
        "docs/specs/journal.md v0.57 §9.28 (DEC-2509)"
    );
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let names: Vec<&str> = drafts.keys().map(|k| k.as_str()).collect();
    let mut want: Vec<&str> = BASES.iter().map(|(n, _)| *n).collect();
    want.sort_unstable();
    assert_eq!(names, want, "one base draft per shape");
    for (part, at_least) in [
        ("invalid_drafts", 91),
        ("valid_drafts", 13),
        ("streams", 16),
        ("executor", 51),
    ] {
        let n = list(&section, part).len();
        assert!(n >= at_least, "{n} {part}; never fewer than {at_least}");
    }
}

#[test]
fn every_acknowledgment_base_and_valid_draft_parses() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (name, body) in drafts {
        let record = BASES
            .iter()
            .find(|(n, _)| *n == name.as_str())
            .map(|(_, r)| (*r).to_owned());
        let got = answer(body);
        if got.as_ref().ok() != record.as_ref() {
            failed.push(format!("base {}: {got:?}", name.as_str()));
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
fn every_invalid_acknowledgment_draft_is_refused_with_its_reason_at_its_path() {
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

/// Rule 28: `trigger_unknown`, `trigger_superseded` and `request_invalid` are the executor's
/// refusals of an acknowledgment copy, so each is accepted on the account stream and refused
/// `stream_mismatch` at `payload.reason` on the agent stream, for a Stop and a lift alike.
#[test]
fn the_three_new_refusal_reasons_are_the_account_streams_alone() {
    let section = section();
    let drafts = section.get("drafts").unwrap();
    let mut failed = Vec::new();
    for reason in NEW_REASONS {
        let base = format!("refused_{reason}");
        let body = drafts.get(&base).unwrap();
        assert_eq!(text(body.get("payload").unwrap(), "reason"), reason);
        assert!(text(body, "stream_id").starts_with("acct:"), "{base}");
        let got = answer(body);
        if got.as_deref() != Ok("OwnerCommandRefused") {
            failed.push(format!("{base} on the account stream: {got:?}"));
        }
        let off: Vec<&Value> = list(&section, "invalid_drafts")
            .iter()
            .filter(|c| text(c, "base_draft") == base)
            .collect();
        assert!(
            off.len() >= 2,
            "{base}: a Stop and a lift on the agent stream"
        );
        for case in off {
            let got = Draft::parse(&draft(&section, case)).err();
            let got = got.as_ref().map(|e| (e.reason.code(), e.path.as_str()));
            if got != Some(("stream_mismatch", "payload.reason")) {
                failed.push(format!("{}: {got:?}", text(case, "name")));
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// `append` sees one record and checks rule 239 only. In each control stream of the vectors,
/// every record parses but the one the case's first failure names when that failure is rule 239's,
/// which is refused `schema` at its path; and the stream rules [`check_stream`] judges refuse none
/// of a stream whose records all parse, since rules 240 and 241 are the writers' and the
/// executor's (no §11 range check holds them yet).
#[test]
fn append_judges_rule_239_alone_on_the_control_streams() {
    let section = section();
    let mut failed = Vec::new();
    for case in list(&section, "streams") {
        let name = text(case, "name");
        let expect = case.get("expect").unwrap();
        let refused_at = (expect.get("rule").and_then(Value::as_int) == Some(239))
            .then(|| index(expect))
            .flatten();
        let mut parsed = Vec::new();
        for (i, record) in list(case, "records").iter().enumerate() {
            let got = Draft::parse(&to_canonical(record));
            match (refused_at == Some(i), got) {
                (false, Ok(d)) => parsed.push(d),
                (true, Err(e)) if e.reason.code() == "schema" && e.path == text(expect, "path") => {
                }
                (_, got) => failed.push(format!("{name} record {i}: {:?}", got.err())),
            }
        }
        if refused_at.is_none()
            && let Err((i, e)) = check_stream(&[], &parsed)
        {
            failed.push(format!("{name}: check_stream refused record {i}: {e:?}"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// The executor cases' §9.28 records, on the control stream and as the account stream's copies,
/// each pass `append` but those the case lists as `refused_at_append`, which rule 239 refuses at
/// their path. The judgment at processing is the executor's (E7-4) and not read here.
#[test]
fn the_executor_cases_acknowledgment_records_pass_append_but_where_rule_239_refuses_them() {
    let section = section();
    let mut failed = Vec::new();
    for case in list(&section, "executor") {
        let name = text(case, "name");
        for stream in ["control", "account"] {
            for (i, record) in list(case, stream).iter().enumerate() {
                if !SECTION_9_28.contains(&text(record, "event_type")) {
                    continue;
                }
                let listed = list(case, "refused_at_append")
                    .iter()
                    .find(|r| text(r, "stream") == stream && index(r) == Some(i));
                let want = listed.map_or_else(
                    || Ok(text(record, "event_type").to_owned()),
                    |r| Err(("schema".to_owned(), text(r, "path").to_owned())),
                );
                let got = answer(record);
                if got != want {
                    failed.push(format!(
                        "{name} {stream} {i}: expected {want:?}, got {got:?}"
                    ));
                }
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
