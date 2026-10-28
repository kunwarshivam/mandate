//! Journal spec rule 264 (DEC-2351 item 2, E8-17): the manager's own reading of an unapplied crash
//! loop is the vectors'. Each `kill_switch_unapplied` control stream is folded into a manager in
//! `seq` order, and at each `ProcessCrashLooped` the manager says whether that loop left a switch
//! unapplied. The expectation is the reference implementation's, read from the vectors and never
//! from this crate: a loop an alert names is unapplied unless the vectors refuse that alert under
//! rule 264, and a loop of a valid stream that no alert names is not.

use std::path::Path;

use mandate_canon::{Digest, Value, parse};
use mandate_deploy_manager::control::fact;
use mandate_deploy_manager::{Manager, Process, Slot};
use mandate_journal::StoredEvent;

fn fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    parse(&std::fs::read(path).unwrap()).unwrap()
}

fn text<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name).and_then(Value::as_str)
}

/// One vector record as the control stream stores it at `seq`.
fn stored(record: &Value, seq: u64) -> StoredEvent {
    let event_type = text(record, "event_type").unwrap().to_owned();
    let event_id = text(record, "event_id").unwrap().to_owned();
    let schema_version = record
        .get("schema_version")
        .and_then(Value::as_int)
        .unwrap();
    let causation = record.get("causation_id").cloned().unwrap_or(Value::Null);
    let payload = record.get("payload").cloned().unwrap();
    let body = mandate_canon::to_canonical(&parse(
        format!(
            r#"{{"event_id":"{event_id}","event_type":"{event_type}","schema_version":{schema_version},"causation_id":{},"payload":{}}}"#,
            String::from_utf8(mandate_canon::to_canonical(&causation)).unwrap(),
            String::from_utf8(mandate_canon::to_canonical(&payload)).unwrap(),
        )
        .as_bytes(),
    )
    .unwrap());
    StoredEvent {
        stream_id: "ctl:ws1".into(),
        seq,
        event_id,
        event_type,
        schema_version,
        environment: "paper".into(),
        recorded_at: format!("2026-10-22T13:00:{:02}.000000000Z", seq % 60),
        prev_hash: Digest::of(b"prev"),
        hash: Digest::of(&body),
        body,
    }
}

/// What the vectors say of each loop of `stream` they decide: its event id and whether it is
/// unapplied.
fn expected(stream: &Value) -> Vec<(String, bool)> {
    let records = stream.get("records").and_then(Value::as_array).unwrap();
    let expect = stream.get("expect").unwrap();
    let valid = text(expect, "outcome") == Some("Valid");
    let refused_at = (!valid && text(expect, "rule") == Some("rule 264"))
        .then(|| expect.get("index").and_then(Value::as_int))
        .flatten();
    let mut decided = Vec::new();
    for record in records
        .iter()
        .filter(|r| text(r, "event_type") == Some("ProcessCrashLooped"))
    {
        let id = text(record, "event_id").unwrap();
        let naming: Vec<usize> = records
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                text(r, "event_type") == Some("OwnerAlertSent")
                    && r.get("payload").and_then(|p| text(p, "subject")) == Some(id)
                    && r.get("payload").and_then(|p| text(p, "kind"))
                        == Some("kill_switch_unapplied")
            })
            .map(|(index, _)| index)
            .collect();
        match naming.as_slice() {
            [] if valid => decided.push((id.to_owned(), false)),
            [] => {}
            [at] => {
                let refused =
                    refused_at.is_some_and(|index| u64::try_from(*at).ok() == Some(index));
                decided.push((id.to_owned(), !refused));
            }
            _ => panic!(
                "{}: two alerts name one loop",
                text(stream, "name").unwrap()
            ),
        }
    }
    decided
}

/// What the manager says of each loop of `stream`, folded in order.
fn read(stream: &Value) -> Vec<(String, bool)> {
    let records = stream.get("records").and_then(Value::as_array).unwrap();
    let mut manager = Manager::new();
    let mut read = Vec::new();
    for (index, record) in records.iter().enumerate() {
        let row = stored(record, u64::try_from(index).unwrap() + 1);
        if let Some(folded) = fact(&row) {
            manager.fold(&folded);
        }
        if row.event_type == "ProcessCrashLooped" {
            let payload = record.get("payload").unwrap();
            let slot = Slot {
                process: Process::parse(text(payload, "process").unwrap()).unwrap(),
                subject: text(payload, "subject").unwrap().to_owned(),
            };
            read.push((row.event_id.clone(), manager.unapplied(&slot)));
        }
    }
    read
}

#[test]
fn the_manager_reads_every_vector_loop_as_rule_264_does() {
    let fixture = fixture();
    let streams = fixture
        .get("kill_switch_unapplied")
        .and_then(|section| section.get("streams"))
        .and_then(Value::as_array)
        .unwrap();
    let mut failed = Vec::new();
    let (mut unapplied, mut ordinary) = (0, 0);
    for stream in streams {
        let name = text(stream, "name").unwrap();
        let read = read(stream);
        for (loop_id, want) in expected(stream) {
            let got = read.iter().find(|(id, _)| *id == loop_id).map(|(_, u)| *u);
            if got != Some(want) {
                failed.push(format!("{name}: loop {loop_id}: got {got:?}, want {want}"));
            }
            if want {
                unapplied += 1;
            } else {
                ordinary += 1;
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        unapplied >= 10 && ordinary >= 8,
        "{unapplied} unapplied and {ordinary} ordinary loops decided; never fewer"
    );
}
