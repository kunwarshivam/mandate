//! Control-stream rows for the backtest and evaluation reads, built by hand as journal spec
//! §9.18 states each record: `mandate-journal` does not register either type yet, so `append`
//! would refuse them, and the rows are made directly. The reports are the schemas' own examples.

#![allow(dead_code, reason = "each test file uses part of the shared helpers")]

use mandate_canon::{Digest, Key, Value, parse, to_canonical};
use mandate_journal::StoredEvent;

/// The hosted workspace (the `common` module's).
pub const WORKSPACE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Y";

/// The examples' ids.
pub const BACKTEST_ID: &str = "bt_01JB3K6F0C9R2V7N4M8Q1T5W3Z";
pub const EVALUATION_ID: &str = "ev_01JB3K6F0C9R2V7N4M8Q1T5W3Z";

pub const BACKTEST_EXAMPLE: &str =
    include_str!("../../../../schemas/workspace-api/examples/read-models.backtest.json");
pub const BACKTEST_VALID: &str =
    include_str!("../../../../schemas/workspace-api/examples/read-models.backtest.valid.json");
pub const EVALUATION_EXAMPLE: &str =
    include_str!("../../../../schemas/workspace-api/examples/read-models.evaluation.json");
pub const BACKTEST_SCHEMA: &str =
    include_str!("../../../../schemas/workspace-api/read-models/backtest.schema.json");
pub const EVALUATION_SCHEMA: &str =
    include_str!("../../../../schemas/workspace-api/read-models/evaluation.schema.json");
pub const ENVELOPE_SCHEMA: &str =
    include_str!("../../../../schemas/workspace-api/envelope.schema.json");

pub fn ctl() -> String {
    format!("ctl:{WORKSPACE}")
}

/// `sha256:` and `n` as two hex digits, 32 times.
pub fn reference(n: u8) -> String {
    format!("sha256:{}", format!("{n:02x}").repeat(32))
}

/// A control-stream row at `seq` in `environment`, schema version 1.
pub fn row_in(environment: &str, seq: u64, event_type: &str, payload: &str) -> StoredEvent {
    let stream = ctl();
    let event_id = format!("01JBH3BTZ8K2M4N6P8R0S2T{seq:03}");
    let text = format!(
        r#"{{"environment":"{environment}","event_id":"{event_id}","event_type":"{event_type}","payload":{payload},"stream_id":"{stream}"}}"#
    );
    let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
    StoredEvent {
        stream_id: stream,
        seq,
        event_id,
        event_type: event_type.to_owned(),
        schema_version: 1,
        environment: environment.to_owned(),
        recorded_at: format!("2026-10-17T10:00:{seq:02}.000000000Z"),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    }
}

/// [`row_in`] on paper.
pub fn row(seq: u64, event_type: &str, payload: &str) -> StoredEvent {
    row_in("paper", seq, event_type, payload)
}

/// `value` as JSON text, or `null`.
fn json_text(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), |text| format!("\"{text}\""))
}

/// A `BacktestRunRecorded` payload with every §9.18 member: `report` and `refusal` as given.
pub fn backtest_payload(
    id: &str,
    version: &str,
    result: &str,
    report: Option<&str>,
    refusal: Option<&str>,
) -> String {
    format!(
        r#"{{"backtest_id":"{id}","mandate_version":"{version}","data_snapshot":"{}","model_outputs":"{}","theses":"{}","configuration":"{}","code_build":"{}","report":{},"report_version":2,"marker":"backtest","result":"{result}","refusal":{}}}"#,
        reference(1),
        reference(2),
        reference(3),
        reference(4),
        reference(5),
        json_text(report),
        json_text(refusal),
    )
}

/// An `EvaluationRunRecorded` payload with every §9.18 member, naming `report`.
pub fn evaluation_payload(id: &str, report: &str) -> String {
    format!(
        r#"{{"evaluation_id":"{id}","decision":"DEC-122","cut_off":"2026-12-01T00:00:00.000000000Z","theses":"{}","theses_from":["agent:{WORKSPACE}:agt_a:1"],"theses_to":["agent:{WORKSPACE}:agt_a:9"],"theses_head":["{}"],"closes":"{}","cost_model":"{}","code_build":"{}","report":"{report}","report_version":1,"passed":true}}"#,
        reference(5),
        reference(6),
        reference(7),
        reference(8),
        reference(9),
    )
}

/// The member at the slash-separated `path` of `value`.
pub fn at<'v>(value: &'v Value, path: &str) -> &'v Value {
    path.split('/')
        .filter(|key| !key.is_empty())
        .fold(value, |v, key| {
            v.get(key).unwrap_or_else(|| panic!("{path}"))
        })
}

/// `value` with the member at the slash-separated `path` set to `to` (its parent must exist).
pub fn set(value: &mut Value, path: &str, to: Value) {
    let keys: Vec<&str> = path.split('/').filter(|key| !key.is_empty()).collect();
    let (last, parents) = keys.split_last().expect("a path");
    let mut node = value;
    for key in parents {
        let Value::Object(members) = node else {
            panic!("{path}: {key} is not in an object");
        };
        node = members.get_mut(*key).unwrap_or_else(|| panic!("{path}"));
    }
    let Value::Object(members) = node else {
        panic!("{path}: not an object");
    };
    members.insert(Key::new(last).expect("a key"), to);
}

/// The example's report: its canonical bytes, and the reference they are stored under.
pub fn example_report(example: &str, member: &str) -> (Vec<u8>, String) {
    let value = parse(example.as_bytes()).expect("the example");
    let bytes = to_canonical(at(&value, &format!("{member}/report")));
    let reference = format!("sha256:{}", Digest::of(&bytes).to_hex());
    (bytes, reference)
}

/// The backtest example's `mandate_version`.
pub fn example_version() -> String {
    let value = parse(BACKTEST_EXAMPLE.as_bytes()).expect("the example");
    at(&value, "backtest/mandate_version")
        .as_str()
        .expect("a version")
        .to_owned()
}
