//! Journal spec v0.50 §9.23 (agent harness spec §6.9, DEC-2203): the agent stream's run records,
//! `RunStarted`, `ToolCallRecorded`, `ToolResultRecorded`, `RunSummaryRecorded`, and `RunEnded`, with
//! rules 171 to 183. §9.23 has no vectors, so the drafts are written here: one honest run, and each
//! case one edit to it with the reason and path the spec's text names, written out from §9.23, never
//! read from the journal's own schema. A stream is judged batch by batch from its start, by
//! `Draft::parse`, `check_batch`, and `check_stream` against the batches before it, and the first
//! draft that fails is reported by its index across the whole stream.

use std::path::Path;

use mandate_canon::{Key, Value, parse, to_canonical};
use mandate_journal::{Draft, StreamType, check_batch, check_stream, closes, reads_stream};

const RUN: &str = "01J9GT00A000000000000000R1";
const OTHER_RUN: &str = "01J9GT00A000000000000000R2";
const STARTED: &str = "01J9H000A000000000000000S1";
const TURN: &str = "01J9H001A000000000000000M1";
const CALL: &str = "01J9H001A000000000000000C1";
const READ: &str = "01J9H002A000000000000000T1";
const PROPOSE: &str = "01J9H002A000000000000000T2";
const READ_RESULT: &str = "01J9H003A000000000000000V1";
const PROPOSE_RESULT: &str = "01J9H003A000000000000000V2";
const SUMMARY: &str = "01J9H004A000000000000000Y1";
const ENDED: &str = "01J9H005A000000000000000E1";
const ITEM: &str = "01J9GV00A000000000000000B1";
const LATER_ITEM: &str = "01J9GV00B000000000000000B2";
const AAPL: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
const MSFT: &str = "7b4a1c2e-3333-4a2b-9c3d-000000000003";
const ENTRY: &str = "sha256:a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const T: &str = "2026-10-16T14:00:41.250000000Z";
const OBSERVED: &str = "01J9GZ00A000000000000000Q1";
const OTHER_OBSERVED: &str = "01J9GZ00A000000000000000Q2";

fn json(text: &str) -> Value {
    parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e:?}: {text}"))
}

fn digest(c: char) -> String {
    format!("sha256:{}", c.to_string().repeat(64))
}

/// Every `sha256:` reference in `value`, into `out`.
fn refs(value: &Value, out: &mut std::collections::BTreeSet<String>) {
    match value {
        Value::Str(s) if s.starts_with("sha256:") => {
            out.insert(s.clone());
        }
        Value::Array(items) => items.iter().for_each(|v| refs(v, out)),
        Value::Object(members) => members.values().for_each(|v| refs(v, out)),
        _ => {}
    }
}

/// An agent-stream draft of `event_type` at version 1, with `artifact_refs` the payload's own.
fn record(event_type: &str, event_id: &str, causation: Option<&str>, payload: Value) -> Value {
    let mut out = std::collections::BTreeSet::new();
    refs(&payload, &mut out);
    let causation = causation.map_or("null".to_owned(), |c| format!(r#""{c}""#));
    let mut draft = json(&format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}",
        "stream_id":"agent:ws_01J8Z2:agent_a","event_type":"{event_type}","schema_version":1,
        "event_time":"{T}","clock_source":"local","causation_id":{causation},
        "correlation_id":"{RUN}",
        "actor":{{"kind":"agent","id":"agent_a","version":"0.1.0","build":"{}"}},
        "config_refs":{{"model_version":"{ENTRY}"}},"payload":null,"artifact_refs":[],"pii_refs":[]}}"#,
        digest('c')
    ));
    set(&mut draft, "payload", Some(payload));
    set(
        &mut draft,
        "artifact_refs",
        Some(Value::Array(out.into_iter().map(Value::Str).collect())),
    );
    draft
}

/// The research turn's `ModelInvocationRecorded`: §9.21's vectors' call record, with the run's
/// correlation and one judged thesis verdict, the reply's mediated `propose_thesis` call.
fn turn() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    let mut call = fixture
        .get("thesis_invalidation")
        .and_then(|s| s.get("drafts"))
        .and_then(|d| d.get("call"))
        .cloned()
        .unwrap();
    set(&mut call, "event_id", Some(Value::Str(TURN.into())));
    set(&mut call, "correlation_id", Some(Value::Str(RUN.into())));
    set(&mut call, "payload.call_id", Some(Value::Str(CALL.into())));
    set(
        &mut call,
        "payload.verdicts",
        Some(json(
            r#"[{"candidate":0,"kind":"thesis","result":"judged"}]"#,
        )),
    );
    call
}

fn started() -> Value {
    record(
        "RunStarted",
        STARTED,
        None,
        json(&format!(
            r#"{{"kind":"ideate","trigger":{{"kind":"schedule","cause":null}},"output":"action_tools",
            "menu":["data.query","invalidate","propose_thesis","think.note"],"cut_off":"{T}",
            "allowlist_version":3,"data_universe_ref":"{}","cost_ceiling_usd":"0.5",
            "deadline":"2026-10-16T14:10:41.250000000Z","memory":[]}}"#,
            digest('f')
        )),
    )
}

fn tool_call(id: &str, index: u64, tool: &str, class: &str, arguments: char) -> Value {
    record(
        "ToolCallRecorded",
        id,
        Some(TURN),
        json(&format!(
            r#"{{"call_id":"{CALL}","index":{index},"tool":"{tool}","class":"{class}",
            "arguments_ref":"{}","provenance":[]}}"#,
            digest(arguments)
        )),
    )
}

fn tool_result(id: &str, call: &str, result: &str, code: Option<&str>, content: char) -> Value {
    let code = code.map_or("null".to_owned(), |c| format!(r#""{c}""#));
    record(
        "ToolResultRecorded",
        id,
        Some(call),
        json(&format!(
            r#"{{"tool_call":"{call}","result":"{result}","code":{code},"untrusted":false,
            "content_ref":"{}","bytes":20,"truncated":false,"observations":[]}}"#,
            digest(content)
        )),
    )
}

fn summary() -> Value {
    record(
        "RunSummaryRecorded",
        SUMMARY,
        None,
        json(&format!(
            r#"{{"instruments_read":["{AAPL}"],"watch":[{{"instrument_id":"{AAPL}","reason":"earnings"}}],
            "proposed":1,"admitted":1,"invalidated":0}}"#
        )),
    )
}

fn ended() -> Value {
    record(
        "RunEnded",
        ENDED,
        None,
        json(
            r#"{"reason":"done","bound":null,"turns":1,"tool_calls":2,"result_bytes":512,
            "discarded":[]}"#,
        ),
    )
}

/// One honest run, batch by batch: its start; the turn's record heading its tool calls (a read, and
/// an action mediated in that batch with its result); the read's result, dispatched; and the
/// summary and end.
fn run() -> Vec<Vec<Value>> {
    let mut read_result = tool_result(READ_RESULT, READ, "ok", None, '7');
    set(
        &mut read_result,
        "payload.untrusted",
        Some(Value::Bool(true)),
    );
    set(&mut read_result, "payload.bytes", Some(json("512")));
    set(
        &mut read_result,
        "payload.observations",
        Some(json(&format!(r#"["{ITEM}"]"#))),
    );
    vec![
        vec![started()],
        vec![
            turn(),
            tool_call(READ, 0, "data.query", "read", '7'),
            tool_call(PROPOSE, 1, "propose_thesis", "action", '8'),
            tool_result(PROPOSE_RESULT, PROPOSE, "ok", Some("admissible"), '8'),
        ],
        vec![read_result],
        vec![summary(), ended()],
    ]
}

/// Sets (`Some`) or deletes (`None`) the member at dotted `path`; `[n]` indexes an array.
fn set(value: &mut Value, path: &str, to: Option<Value>) {
    let mut names: Vec<&str> = path.split('.').collect();
    let last = names.pop().unwrap();
    let mut node = value;
    for name in names {
        node = step(node, name);
    }
    if let Some((member, index)) = indexed(last) {
        let Value::Array(items) = step(node, member) else {
            panic!("`{path}` is not an array");
        };
        match to {
            Some(v) => items[index] = v,
            None => {
                items.remove(index);
            }
        }
        return;
    }
    let Value::Object(members) = node else {
        panic!("`{path}` has no object");
    };
    match to {
        Some(v) => {
            members.insert(Key::new(last).unwrap(), v);
        }
        None => {
            assert!(members.remove(last).is_some(), "`{path}` deletes nothing");
        }
    }
}

fn indexed(name: &str) -> Option<(&str, usize)> {
    let (member, rest) = name.split_once('[')?;
    Some((member, rest.trim_end_matches(']').parse().ok()?))
}

fn step<'a>(node: &'a mut Value, name: &str) -> &'a mut Value {
    if let Some((member, index)) = indexed(name) {
        let Value::Array(items) = step(node, member) else {
            panic!("`{name}` is not an array");
        };
        return &mut items[index];
    }
    let Value::Object(members) = node else {
        panic!("`{name}` has no object");
    };
    members
        .get_mut(name)
        .unwrap_or_else(|| panic!("no `{name}`"))
}

/// The draft with `event_id` `id`, wherever it is in `stream`.
fn at<'a>(stream: &'a mut [Vec<Value>], id: &str) -> &'a mut Value {
    stream
        .iter_mut()
        .flatten()
        .find(|d| d.get("event_id").and_then(Value::as_str) == Some(id))
        .unwrap_or_else(|| panic!("no draft {id}"))
}

/// The first draft of `stream` that fails, by its index across the stream, with its reason and
/// path; `None` when every batch is accepted.
fn judge(stream: &[Vec<Value>]) -> Option<(usize, String, String)> {
    let mut earlier: Vec<Draft> = Vec::new();
    for batch in stream {
        let offset = earlier.len();
        let mut drafts = Vec::new();
        for (i, body) in batch.iter().enumerate() {
            match Draft::parse(&to_canonical(body)) {
                Ok(d) => drafts.push(d),
                Err(e) => return Some((offset + i, e.reason.code().to_owned(), e.path)),
            }
        }
        if let Err((i, e)) = check_batch(&drafts).and_then(|()| check_stream(&earlier, &drafts)) {
            return Some((offset + i, e.reason.code().to_owned(), e.path));
        }
        earlier.extend(drafts);
    }
    None
}

/// The index across `stream` of the draft with `event_id` `id`.
fn index_of(stream: &[Vec<Value>], id: &str) -> usize {
    stream
        .iter()
        .flatten()
        .position(|d| d.get("event_id").and_then(Value::as_str) == Some(id))
        .unwrap_or_else(|| panic!("no draft {id}"))
}

fn refused(stream: &[Vec<Value>], id: &str, reason: &str, path: &str) -> Result<(), String> {
    let want = Some((index_of(stream, id), reason.to_owned(), path.to_owned()));
    let got = judge(stream);
    if got == want {
        Ok(())
    } else {
        Err(format!("expected {want:?}, got {got:?}"))
    }
}

fn accepted(stream: &[Vec<Value>]) -> Result<(), String> {
    match judge(stream) {
        None => Ok(()),
        got => Err(format!("expected acceptance, got {got:?}")),
    }
}

type Edit = fn(&mut Vec<Vec<Value>>);

/// A case: its name, its edit to the honest run, and the draft (by `event_id`), reason, and path
/// it is refused with, or `None` when it is accepted.
type Case<'a> = (&'a str, Edit, Option<(&'a str, &'a str, &'a str)>);

/// Runs every case and reports all that fail.
fn cases(cases: &[Case]) {
    let mut failed = Vec::new();
    for (name, edit, expect) in cases {
        let mut stream = run();
        edit(&mut stream);
        let outcome = match expect {
            None => accepted(&stream),
            Some((id, reason, path)) => refused(&stream, id, reason, path),
        };
        if let Err(e) = outcome {
            failed.push(format!("{name}: {e}"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

fn put(stream: &mut [Vec<Value>], id: &str, path: &str, value: &str) {
    set(at(stream, id), path, Some(json(value)));
}

#[test]
fn the_honest_run_is_accepted_batch_by_batch() {
    assert_eq!(judge(&run()), None);
}

#[test]
fn every_run_record_is_closed_member_by_member() {
    cases(&[
        (
            "started.kind.form",
            |s| put(s, STARTED, "payload.kind", r#""chat""#),
            Some((STARTED, "non_canonical", "payload.kind")),
        ),
        (
            "started.kind.kind",
            |s| put(s, STARTED, "payload.kind", "7"),
            Some((STARTED, "schema", "payload.kind")),
        ),
        (
            "started.trigger.kind.form",
            |s| put(s, STARTED, "payload.trigger.kind", r#""timer""#),
            Some((STARTED, "non_canonical", "payload.trigger.kind")),
        ),
        (
            "started.trigger.cause.kind",
            |s| put(s, STARTED, "payload.trigger.cause", "7"),
            Some((STARTED, "schema", "payload.trigger.cause")),
        ),
        (
            "started.trigger.extra",
            |s| put(s, STARTED, "payload.trigger.zz_extra", "true"),
            Some((STARTED, "schema", "payload.trigger.zz_extra")),
        ),
        (
            "started.output.form",
            |s| put(s, STARTED, "payload.output", r#""chat""#),
            Some((STARTED, "non_canonical", "payload.output")),
        ),
        (
            "started.menu.kind",
            |s| put(s, STARTED, "payload.menu", r#""data.query""#),
            Some((STARTED, "schema", "payload.menu")),
        ),
        (
            "started.menu.item",
            |s| put(s, STARTED, "payload.menu", "[7]"),
            Some((STARTED, "schema", "payload.menu[0]")),
        ),
        (
            "started.cut_off.kind",
            |s| put(s, STARTED, "payload.cut_off", "7"),
            Some((STARTED, "schema", "payload.cut_off")),
        ),
        (
            "started.allowlist_version.kind",
            |s| put(s, STARTED, "payload.allowlist_version", r#""3""#),
            Some((STARTED, "schema", "payload.allowlist_version")),
        ),
        (
            "started.data_universe_ref.kind",
            |s| put(s, STARTED, "payload.data_universe_ref", "7"),
            Some((STARTED, "schema", "payload.data_universe_ref")),
        ),
        (
            "started.cost_ceiling_usd.kind",
            |s| put(s, STARTED, "payload.cost_ceiling_usd", "7"),
            Some((STARTED, "schema", "payload.cost_ceiling_usd")),
        ),
        (
            "started.deadline.kind",
            |s| put(s, STARTED, "payload.deadline", "7"),
            Some((STARTED, "schema", "payload.deadline")),
        ),
        (
            "started.memory.item",
            |s| put(s, STARTED, "payload.memory", r#"["x y"]"#),
            Some((STARTED, "non_canonical", "payload.memory[0]")),
        ),
        (
            "started.missing",
            |s| set(at(s, STARTED), "payload.memory", None),
            Some((STARTED, "schema", "payload.memory")),
        ),
        (
            "started.extra",
            |s| put(s, STARTED, "payload.zz_extra", "true"),
            Some((STARTED, "schema", "payload.zz_extra")),
        ),
        (
            "started.null",
            |s| put(s, STARTED, "payload.cut_off", "null"),
            Some((STARTED, "schema", "payload.cut_off")),
        ),
        (
            "started.at_version_2",
            |s| put(s, STARTED, "schema_version", "2"),
            Some((STARTED, "unknown_schema", "payload")),
        ),
        (
            "started.on_the_control_stream",
            |s| put(s, STARTED, "stream_id", r#""ctl:ws_01J8Z2""#),
            Some((STARTED, "wrong_stream", "event_type")),
        ),
        (
            "started.without_entry",
            |s| put(s, STARTED, "config_refs", "{}"),
            Some((STARTED, "missing_config_ref", "config_refs.model_version")),
        ),
        (
            "call.call_id.kind",
            |s| put(s, READ, "payload.call_id", "7"),
            Some((READ, "schema", "payload.call_id")),
        ),
        (
            "call.index.kind",
            |s| put(s, READ, "payload.index", r#""0""#),
            Some((READ, "schema", "payload.index")),
        ),
        (
            "call.tool.null",
            |s| put(s, READ, "payload.tool", "null"),
            Some((READ, "schema", "payload.tool")),
        ),
        (
            "call.class.form",
            |s| put(s, READ, "payload.class", r#""write""#),
            Some((READ, "non_canonical", "payload.class")),
        ),
        (
            "call.arguments_ref.kind",
            |s| put(s, READ, "payload.arguments_ref", "7"),
            Some((READ, "schema", "payload.arguments_ref")),
        ),
        (
            "call.provenance.item",
            |s| put(s, PROPOSE, "payload.provenance", "[7]"),
            Some((PROPOSE, "schema", "payload.provenance[0]")),
        ),
        (
            "call.extra",
            |s| put(s, READ, "payload.zz_extra", "true"),
            Some((READ, "schema", "payload.zz_extra")),
        ),
        (
            "call.missing",
            |s| set(at(s, READ), "payload.provenance", None),
            Some((READ, "schema", "payload.provenance")),
        ),
        (
            "result.tool_call.kind",
            |s| put(s, READ_RESULT, "payload.tool_call", "7"),
            Some((READ_RESULT, "schema", "payload.tool_call")),
        ),
        (
            "result.result.form",
            |s| put(s, READ_RESULT, "payload.result", r#""partial""#),
            Some((READ_RESULT, "non_canonical", "payload.result")),
        ),
        (
            "result.code.form",
            |s| put(s, PROPOSE_RESULT, "payload.code", r#""maybe""#),
            Some((PROPOSE_RESULT, "non_canonical", "payload.code")),
        ),
        (
            "result.untrusted.kind",
            |s| put(s, READ_RESULT, "payload.untrusted", r#""yes""#),
            Some((READ_RESULT, "schema", "payload.untrusted")),
        ),
        (
            "result.content_ref.kind",
            |s| put(s, READ_RESULT, "payload.content_ref", "7"),
            Some((READ_RESULT, "schema", "payload.content_ref")),
        ),
        (
            "result.bytes.kind",
            |s| put(s, READ_RESULT, "payload.bytes", r#""512""#),
            Some((READ_RESULT, "schema", "payload.bytes")),
        ),
        (
            "result.truncated.kind",
            |s| put(s, READ_RESULT, "payload.truncated", "0"),
            Some((READ_RESULT, "schema", "payload.truncated")),
        ),
        (
            "result.observations.item",
            |s| put(s, READ_RESULT, "payload.observations", "[7]"),
            Some((READ_RESULT, "schema", "payload.observations[0]")),
        ),
        (
            "result.extra",
            |s| put(s, READ_RESULT, "payload.zz_extra", "true"),
            Some((READ_RESULT, "schema", "payload.zz_extra")),
        ),
        (
            "summary.instruments_read.item",
            |s| put(s, SUMMARY, "payload.instruments_read", r#"["AAPL"]"#),
            Some((SUMMARY, "non_canonical", "payload.instruments_read[0]")),
        ),
        (
            "summary.watch.instrument.ticker",
            |s| put(s, SUMMARY, "payload.watch[0].instrument_id", r#""AAPL""#),
            Some((SUMMARY, "non_canonical", "payload.watch[0].instrument_id")),
        ),
        (
            "summary.watch.instrument.form",
            |s| put(s, SUMMARY, "payload.watch[0].instrument_id", r#""AA PL""#),
            Some((SUMMARY, "non_canonical", "payload.watch[0].instrument_id")),
        ),
        (
            "summary.watch.reason.form",
            |s| put(s, SUMMARY, "payload.watch[0].reason", r#""hunch""#),
            Some((SUMMARY, "non_canonical", "payload.watch[0].reason")),
        ),
        (
            "summary.watch.instrument.kind",
            |s| put(s, SUMMARY, "payload.watch[0].instrument_id", "7"),
            Some((SUMMARY, "schema", "payload.watch[0].instrument_id")),
        ),
        (
            "summary.watch.extra",
            |s| put(s, SUMMARY, "payload.watch[0].zz_extra", "true"),
            Some((SUMMARY, "schema", "payload.watch[0].zz_extra")),
        ),
        (
            "summary.proposed.kind",
            |s| put(s, SUMMARY, "payload.proposed", r#""1""#),
            Some((SUMMARY, "schema", "payload.proposed")),
        ),
        (
            "summary.admitted.kind",
            |s| put(s, SUMMARY, "payload.admitted", r#""1""#),
            Some((SUMMARY, "schema", "payload.admitted")),
        ),
        (
            "summary.invalidated.missing",
            |s| set(at(s, SUMMARY), "payload.invalidated", None),
            Some((SUMMARY, "schema", "payload.invalidated")),
        ),
        (
            "ended.reason.form",
            |s| put(s, ENDED, "payload.reason", r#""tired""#),
            Some((ENDED, "non_canonical", "payload.reason")),
        ),
        (
            "ended.bound.form",
            |s| put(s, ENDED, "payload.bound", r#""max_cost""#),
            Some((ENDED, "non_canonical", "payload.bound")),
        ),
        (
            "ended.turns.kind",
            |s| put(s, ENDED, "payload.turns", r#""1""#),
            Some((ENDED, "schema", "payload.turns")),
        ),
        (
            "ended.tool_calls.kind",
            |s| put(s, ENDED, "payload.tool_calls", r#""2""#),
            Some((ENDED, "schema", "payload.tool_calls")),
        ),
        (
            "ended.result_bytes.kind",
            |s| put(s, ENDED, "payload.result_bytes", r#""512""#),
            Some((ENDED, "schema", "payload.result_bytes")),
        ),
        (
            "ended.discarded.item",
            |s| put(s, ENDED, "payload.discarded", "[7]"),
            Some((ENDED, "schema", "payload.discarded[0]")),
        ),
        (
            "ended.extra",
            |s| put(s, ENDED, "payload.zz_extra", "true"),
            Some((ENDED, "schema", "payload.zz_extra")),
        ),
    ]);
}

#[test]
fn rule_171_every_run_record_is_the_agent_s_and_names_its_run() {
    let system = r#"{"kind":"system","id":"control_services","version":"0.1.0","build":"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"}"#;
    let mut failed = Vec::new();
    for id in [STARTED, READ, READ_RESULT, SUMMARY, ENDED] {
        for (what, path, value) in [
            ("actor", "actor.kind", system),
            ("run", "correlation_id", "null"),
        ] {
            let mut stream = run();
            let member = if what == "actor" { "actor" } else { path };
            put(&mut stream, id, member, value);
            if let Err(e) = refused(&stream, id, "schema", path) {
                failed.push(format!("{id} {what}: {e}"));
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn rule_172_a_run_s_start_is_canonical_and_its_kind_fits_its_trigger() {
    cases(&[
        (
            "menu.swapped",
            |s| put(s, STARTED, "payload.menu", r#"["think.note","data.query"]"#),
            Some((STARTED, "non_canonical", "payload.menu")),
        ),
        (
            "menu.twice",
            |s| put(s, STARTED, "payload.menu", r#"["data.query","data.query"]"#),
            Some((STARTED, "non_canonical", "payload.menu")),
        ),
        (
            "memory.swapped",
            |s| {
                put(
                    s,
                    STARTED,
                    "payload.memory",
                    &format!(r#"["{LATER_ITEM}","{ITEM}"]"#),
                )
            },
            Some((STARTED, "non_canonical", "payload.memory")),
        ),
        (
            "memory.twice",
            |s| {
                put(
                    s,
                    STARTED,
                    "payload.memory",
                    &format!(r#"["{ITEM}","{ITEM}"]"#),
                )
            },
            Some((STARTED, "non_canonical", "payload.memory")),
        ),
        (
            "memory.ascending",
            |s| {
                put(
                    s,
                    STARTED,
                    "payload.memory",
                    &format!(r#"["{ITEM}","{LATER_ITEM}"]"#),
                )
            },
            None,
        ),
        (
            "schedule.with_cause",
            |s| put(s, STARTED, "payload.trigger.cause", &format!(r#""{ITEM}""#)),
            Some((STARTED, "schema", "payload.trigger.cause")),
        ),
        (
            "owner_question.without_cause",
            |s| {
                put(s, STARTED, "payload.kind", r#""owner""#);
                put(s, STARTED, "payload.trigger.kind", r#""owner_question""#);
            },
            Some((STARTED, "schema", "payload.trigger.cause")),
        ),
        (
            "approval.without_cause",
            |s| {
                put(s, STARTED, "payload.kind", r#""explain""#);
                put(s, STARTED, "payload.trigger.kind", r#""approval""#);
            },
            Some((STARTED, "schema", "payload.trigger.cause")),
        ),
        (
            "event.without_cause",
            |s| {
                put(s, STARTED, "payload.kind", r#""monitor""#);
                put(s, STARTED, "payload.trigger.kind", r#""event""#);
            },
            Some((STARTED, "schema", "payload.trigger.cause")),
        ),
        (
            "ideate.on_an_event",
            |s| {
                put(s, STARTED, "payload.trigger.kind", r#""event""#);
                put(s, STARTED, "payload.trigger.cause", &format!(r#""{ITEM}""#));
            },
            Some((STARTED, "schema", "payload.trigger.kind")),
        ),
        (
            "monitor.on_a_schedule",
            |s| put(s, STARTED, "payload.kind", r#""monitor""#),
            None,
        ),
        ("monitor.on_an_event", |s| event_first(s, 'a'), None),
        (
            "monitor.on_an_approval",
            |s| {
                put(s, STARTED, "payload.kind", r#""monitor""#);
                put(s, STARTED, "payload.trigger.kind", r#""approval""#);
                put(s, STARTED, "payload.trigger.cause", &format!(r#""{ITEM}""#));
            },
            Some((STARTED, "schema", "payload.trigger.kind")),
        ),
        (
            "owner.on_a_question",
            |s| {
                put(s, STARTED, "payload.kind", r#""owner""#);
                put(s, STARTED, "payload.trigger.kind", r#""owner_question""#);
                put(s, STARTED, "payload.trigger.cause", &format!(r#""{ITEM}""#));
            },
            None,
        ),
        (
            "owner.on_a_schedule",
            |s| put(s, STARTED, "payload.kind", r#""owner""#),
            Some((STARTED, "schema", "payload.trigger.kind")),
        ),
        (
            "owner.on_an_approval",
            |s| {
                put(s, STARTED, "payload.kind", r#""owner""#);
                put(s, STARTED, "payload.trigger.kind", r#""approval""#);
                put(s, STARTED, "payload.trigger.cause", &format!(r#""{ITEM}""#));
            },
            Some((STARTED, "schema", "payload.trigger.kind")),
        ),
        (
            "explain.on_an_approval",
            |s| {
                put(s, STARTED, "payload.kind", r#""explain""#);
                put(s, STARTED, "payload.trigger.kind", r#""approval""#);
                put(s, STARTED, "payload.trigger.cause", &format!(r#""{ITEM}""#));
            },
            None,
        ),
        (
            "explain.on_a_question",
            |s| {
                put(s, STARTED, "payload.kind", r#""explain""#);
                put(s, STARTED, "payload.trigger.kind", r#""owner_question""#);
                put(s, STARTED, "payload.trigger.cause", &format!(r#""{ITEM}""#));
            },
            Some((STARTED, "schema", "payload.trigger.kind")),
        ),
        (
            "explain.on_a_schedule",
            |s| put(s, STARTED, "payload.kind", r#""explain""#),
            Some((STARTED, "schema", "payload.trigger.kind")),
        ),
        (
            "first.menu_before_cause",
            |s| {
                put(s, STARTED, "payload.menu", r#"["think.note","data.query"]"#);
                put(s, STARTED, "payload.trigger.cause", &format!(r#""{ITEM}""#));
            },
            Some((STARTED, "non_canonical", "payload.menu")),
        ),
        (
            "first.memory_before_cause",
            |s| {
                put(
                    s,
                    STARTED,
                    "payload.memory",
                    &format!(r#"["{ITEM}","{ITEM}"]"#),
                );
                put(s, STARTED, "payload.trigger.cause", &format!(r#""{ITEM}""#));
            },
            Some((STARTED, "non_canonical", "payload.memory")),
        ),
        (
            "first.cause_before_kind",
            |s| {
                put(s, STARTED, "payload.kind", r#""owner""#);
                put(s, STARTED, "payload.trigger.cause", &format!(r#""{ITEM}""#));
            },
            Some((STARTED, "schema", "payload.trigger.cause")),
        ),
    ]);
}

/// A second run of the agent: its own start and end, after the first run's, with `kind`, trigger
/// `trigger` and cause `cause`, under the run id `run`.
fn second_run(
    stream: &mut Vec<Vec<Value>>,
    run: &str,
    kind: &str,
    trigger: &str,
    cause: Option<&str>,
) {
    let mut start = started();
    set(
        &mut start,
        "event_id",
        Some(Value::Str("01J9H006A000000000000000S2".into())),
    );
    set(&mut start, "correlation_id", Some(Value::Str(run.into())));
    set(&mut start, "payload.kind", Some(Value::Str(kind.into())));
    set(
        &mut start,
        "payload.trigger.kind",
        Some(Value::Str(trigger.into())),
    );
    set(
        &mut start,
        "payload.trigger.cause",
        Some(cause.map_or(Value::Null, |c| Value::Str(c.into()))),
    );
    stream.push(vec![start]);
}

/// The first run as an `explain` of the approval `cause`.
fn explain_first(stream: &mut [Vec<Value>], cause: &str) {
    put(stream, STARTED, "payload.kind", r#""explain""#);
    put(stream, STARTED, "payload.trigger.kind", r#""approval""#);
    put(
        stream,
        STARTED,
        "payload.trigger.cause",
        &format!(r#""{cause}""#),
    );
}

/// The triggering item's `ObservationRecorded` (journal spec rule 271, DEC-2392): `id`, of run
/// `run`, about `instrument`, holding the item `data`.
fn observed(id: &str, run: &str, instrument: Option<&str>, data: char) -> Value {
    let mut item = record(
        "ObservationRecorded",
        id,
        None,
        json(&format!(
            r#"{{"source":"alpaca.news","instrument_id":null,"as_of":"{T}","data_ref":"{}"}}"#,
            digest(data)
        )),
    );
    set(&mut item, "correlation_id", Some(Value::Str(run.into())));
    set(
        &mut item,
        "payload.instrument_id",
        Some(instrument.map_or(Value::Null, |i| Value::Str(i.into()))),
    );
    item
}

/// The first run as a `monitor` run an item triggered: the item's `ObservationRecorded` heads the
/// start batch and the start names it.
fn event_first(stream: &mut [Vec<Value>], data: char) {
    put(stream, STARTED, "payload.kind", r#""monitor""#);
    put(stream, STARTED, "payload.trigger.kind", r#""event""#);
    put(
        stream,
        STARTED,
        "payload.trigger.cause",
        &format!(r#""{OBSERVED}""#),
    );
    if let Some(first) = stream.first_mut() {
        first.insert(0, observed(OBSERVED, RUN, Some(AAPL), data));
    }
}

/// A second `monitor` run, of `OTHER_RUN`, that the item `data` triggered, in its own start batch.
fn second_event_run(stream: &mut Vec<Vec<Value>>, data: char) {
    second_run(stream, OTHER_RUN, "monitor", "event", Some(OTHER_OBSERVED));
    if let Some(last) = stream.last_mut() {
        last.insert(0, observed(OTHER_OBSERVED, OTHER_RUN, Some(MSFT), data));
    }
}

#[test]
fn rule_173_a_run_starts_once_and_an_approval_or_question_starts_one_run() {
    const SECOND: &str = "01J9H006A000000000000000S2";
    cases(&[
        (
            "same_run_twice",
            |s| second_run(s, RUN, "ideate", "schedule", None),
            Some((SECOND, "schema", "correlation_id")),
        ),
        (
            "another_scheduled_run",
            |s| second_run(s, OTHER_RUN, "ideate", "schedule", None),
            None,
        ),
        (
            "explain.twice",
            |s| {
                explain_first(s, ITEM);
                second_run(s, OTHER_RUN, "explain", "approval", Some(ITEM));
            },
            Some((SECOND, "schema", "payload.trigger.cause")),
        ),
        (
            "explain.per_approval",
            |s| {
                explain_first(s, ITEM);
                second_run(s, OTHER_RUN, "explain", "approval", Some(LATER_ITEM));
            },
            None,
        ),
        (
            "owner.twice",
            |s| {
                put(s, STARTED, "payload.kind", r#""owner""#);
                put(s, STARTED, "payload.trigger.kind", r#""owner_question""#);
                put(s, STARTED, "payload.trigger.cause", &format!(r#""{ITEM}""#));
                second_run(s, OTHER_RUN, "owner", "owner_question", Some(ITEM));
            },
            Some((SECOND, "schema", "payload.trigger.cause")),
        ),
        (
            "monitor.on_two_items",
            |s| {
                event_first(s, 'a');
                second_event_run(s, 'b');
            },
            None,
        ),
        (
            "after_a_single_call_under_the_same_id",
            |s| s.insert(0, vec![single_call()]),
            None,
        ),
        (
            "first.run_before_cause",
            |s| {
                explain_first(s, ITEM);
                second_run(s, RUN, "explain", "approval", Some(ITEM));
            },
            Some((SECOND, "schema", "correlation_id")),
        ),
    ]);
}

/// Each of a run's later records, moved to `run` (a run with no start) or after the run's end.
#[test]
fn rule_174_a_run_s_records_follow_its_start_precede_its_end_and_summarize_it_once() {
    cases(&[
        (
            "call.of_no_run",
            |s| put(s, READ, "correlation_id", &format!(r#""{OTHER_RUN}""#)),
            Some((READ, "schema", "correlation_id")),
        ),
        (
            "result.of_no_run",
            |s| {
                put(
                    s,
                    READ_RESULT,
                    "correlation_id",
                    &format!(r#""{OTHER_RUN}""#),
                )
            },
            Some((READ_RESULT, "schema", "correlation_id")),
        ),
        (
            "summary.of_no_run",
            |s| put(s, SUMMARY, "correlation_id", &format!(r#""{OTHER_RUN}""#)),
            Some((SUMMARY, "schema", "correlation_id")),
        ),
        (
            "ended.of_no_run",
            |s| {
                put(s, SUMMARY, "correlation_id", &format!(r#""{OTHER_RUN}""#));
            },
            Some((SUMMARY, "schema", "correlation_id")),
        ),
        (
            "result.after_the_end",
            |s| {
                let r = s.remove(2);
                s.push(r);
            },
            Some((READ_RESULT, "schema", "correlation_id")),
        ),
        (
            "summary.twice",
            |s| {
                let mut again = summary();
                set(
                    &mut again,
                    "event_id",
                    Some(Value::Str("01J9H004A000000000000000Y2".into())),
                );
                s[3].insert(1, again);
            },
            Some(("01J9H004A000000000000000Y2", "schema", "correlation_id")),
        ),
        (
            "ended.without_summary",
            |s| {
                s[3].remove(0);
            },
            Some((ENDED, "schema", "correlation_id")),
        ),
        (
            "ended.before_summary",
            |s| {
                s[3].swap(0, 1);
            },
            Some((ENDED, "schema", "correlation_id")),
        ),
        (
            "ended.twice",
            |s| {
                let mut again = ended();
                set(
                    &mut again,
                    "event_id",
                    Some(Value::Str("01J9H005A000000000000000E2".into())),
                );
                s.push(vec![again]);
            },
            Some(("01J9H005A000000000000000E2", "schema", "correlation_id")),
        ),
        (
            "call.after_the_end",
            |s| {
                let mut late =
                    tool_call("01J9H002A000000000000000T3", 2, "think.note", "think", '9');
                set(&mut late, "causation_id", Some(Value::Null));
                s.push(vec![late]);
            },
            Some(("01J9H002A000000000000000T3", "schema", "correlation_id")),
        ),
    ]);
}

#[test]
fn rule_175_a_tool_call_names_its_run_s_answered_research_turn_and_a_tool_on_its_menu() {
    cases(&[
        (
            "names_no_turn",
            |s| {
                put(
                    s,
                    READ,
                    "payload.call_id",
                    r#""01J9H001A000000000000000C9""#,
                )
            },
            Some((READ, "schema", "payload.call_id")),
        ),
        (
            "names_a_turn_by_its_event_id",
            |s| put(s, READ, "payload.call_id", &format!(r#""{TURN}""#)),
            Some((READ, "schema", "payload.call_id")),
        ),
        (
            "names_a_refused_turn",
            |s| {
                put(s, TURN, "payload.outcome", r#""content_refused""#);
                put(s, TURN, "payload.verdicts", "[]");
            },
            Some((READ, "schema", "payload.call_id")),
        ),
        (
            "names_another_run_s_turn",
            |s| {
                second_run(s, OTHER_RUN, "ideate", "schedule", None);
                let first = s.remove(0);
                s.insert(0, first);
                put(s, TURN, "correlation_id", &format!(r#""{OTHER_RUN}""#));
                let started_second = s.pop().unwrap();
                s.insert(1, started_second);
            },
            Some((READ, "schema", "correlation_id")),
        ),
        (
            "names_another_entry_s_turn",
            |s| {
                put(
                    s,
                    READ,
                    "config_refs.model_version",
                    &format!(r#""{}""#, digest('9')),
                )
            },
            Some((READ, "schema", "config_refs.model_version")),
        ),
        (
            "a_tool_off_the_menu",
            |s| put(s, READ, "payload.tool", r#""shell.exec""#),
            Some((READ, "schema", "payload.tool")),
        ),
        (
            "first.turn_before_tool",
            |s| {
                put(
                    s,
                    READ,
                    "payload.call_id",
                    r#""01J9H001A000000000000000C9""#,
                );
                put(s, READ, "payload.tool", r#""shell.exec""#);
            },
            Some((READ, "schema", "payload.call_id")),
        ),
        (
            "first.run_before_entry",
            |s| {
                second_run(s, OTHER_RUN, "ideate", "schedule", None);
                let started_second = s.pop().unwrap();
                s.insert(1, started_second);
                put(s, TURN, "correlation_id", &format!(r#""{OTHER_RUN}""#));
                put(
                    s,
                    READ,
                    "config_refs.model_version",
                    &format!(r#""{}""#, digest('9')),
                );
            },
            Some((READ, "schema", "correlation_id")),
        ),
        (
            "first.entry_before_tool",
            |s| {
                put(
                    s,
                    READ,
                    "config_refs.model_version",
                    &format!(r#""{}""#, digest('9')),
                );
                put(s, READ, "payload.tool", r#""shell.exec""#);
            },
            Some((READ, "schema", "config_refs.model_version")),
        ),
    ]);
}

#[test]
fn rule_175_reads_the_turn_s_purpose() {
    let mut stream = run();
    put(&mut stream, TURN, "payload.purpose", r#""llm_signal""#);
    put(&mut stream, TURN, "payload.verdicts", "[]");
    assert_eq!(
        judge(&stream),
        Some((
            index_of(&stream, READ),
            "schema".to_owned(),
            "payload.call_id".to_owned()
        ))
    );
}

#[test]
fn rule_176_a_reply_s_tool_calls_are_recorded_in_order_from_0_each_once() {
    cases(&[
        (
            "starts_at_1",
            |s| {
                put(s, READ, "payload.index", "1");
                put(s, PROPOSE, "payload.index", "2");
            },
            Some((READ, "schema", "payload.index")),
        ),
        (
            "twice",
            |s| put(s, PROPOSE, "payload.index", "0"),
            Some((PROPOSE, "schema", "payload.index")),
        ),
        (
            "skips",
            |s| put(s, PROPOSE, "payload.index", "2"),
            Some((PROPOSE, "schema", "payload.index")),
        ),
        (
            "after_a_refused_call_of_the_same_reply",
            |s| {
                let read_result = s.remove(2).remove(0);
                s[1].insert(2, read_result);
                put(s, READ_RESULT, "payload.result", r#""refused""#);
                put(s, READ_RESULT, "payload.code", r#""out_of_universe""#);
                put(s, READ_RESULT, "payload.observations", "[]");
            },
            None,
        ),
    ]);
}

#[test]
fn rule_177_provenance_is_canonical_and_only_an_action_s() {
    cases(&[
        (
            "action.ascending",
            |s| {
                put(
                    s,
                    PROPOSE,
                    "payload.provenance",
                    &format!(r#"["{ITEM}","{LATER_ITEM}"]"#),
                )
            },
            None,
        ),
        (
            "action.swapped",
            |s| {
                put(
                    s,
                    PROPOSE,
                    "payload.provenance",
                    &format!(r#"["{LATER_ITEM}","{ITEM}"]"#),
                )
            },
            Some((PROPOSE, "non_canonical", "payload.provenance")),
        ),
        (
            "action.twice",
            |s| {
                put(
                    s,
                    PROPOSE,
                    "payload.provenance",
                    &format!(r#"["{ITEM}","{ITEM}"]"#),
                )
            },
            Some((PROPOSE, "non_canonical", "payload.provenance")),
        ),
        (
            "read",
            |s| put(s, READ, "payload.provenance", &format!(r#"["{ITEM}"]"#)),
            Some((READ, "schema", "payload.provenance")),
        ),
        (
            "think",
            |s| {
                put(s, READ, "payload.class", r#""think""#);
                put(s, READ, "payload.provenance", &format!(r#"["{ITEM}"]"#));
            },
            Some((READ, "schema", "payload.provenance")),
        ),
        (
            "first.order_before_class",
            |s| {
                put(
                    s,
                    READ,
                    "payload.provenance",
                    &format!(r#"["{LATER_ITEM}","{ITEM}"]"#),
                )
            },
            Some((READ, "non_canonical", "payload.provenance")),
        ),
    ]);
}

#[test]
fn rule_178_a_result_names_its_run_s_tool_call_once() {
    cases(&[
        (
            "names_nothing",
            |s| {
                put(
                    s,
                    READ_RESULT,
                    "payload.tool_call",
                    r#""01J9H002A000000000000000T9""#,
                )
            },
            Some((READ_RESULT, "schema", "payload.tool_call")),
        ),
        (
            "names_the_turn",
            |s| put(s, READ_RESULT, "payload.tool_call", &format!(r#""{TURN}""#)),
            Some((READ_RESULT, "schema", "payload.tool_call")),
        ),
        (
            "names_a_later_call",
            |s| {
                let read_result = s.remove(2).remove(0);
                s[1].insert(1, read_result);
            },
            Some((READ_RESULT, "schema", "payload.tool_call")),
        ),
        (
            "names_another_run_s_call",
            |s| {
                second_run(s, OTHER_RUN, "ideate", "schedule", None);
                let started_second = s.pop().unwrap();
                s.insert(2, started_second);
                put(
                    s,
                    READ_RESULT,
                    "correlation_id",
                    &format!(r#""{OTHER_RUN}""#),
                );
            },
            Some((READ_RESULT, "schema", "payload.tool_call")),
        ),
        (
            "twice",
            |s| {
                let mut again = tool_result("01J9H003A000000000000000V9", READ, "ok", None, '6');
                set(&mut again, "causation_id", Some(Value::Null));
                s[2].push(again);
            },
            Some(("01J9H003A000000000000000V9", "schema", "payload.tool_call")),
        ),
    ]);
}

/// The read's result, moved into the turn's batch, refused or failed as `result` with `code`.
fn read_outcome(stream: &mut [Vec<Value>], result: &str, code: &str) {
    put(
        stream,
        READ_RESULT,
        "payload.result",
        &format!(r#""{result}""#),
    );
    put(stream, READ_RESULT, "payload.code", code);
    put(stream, READ_RESULT, "payload.observations", "[]");
}

#[test]
fn rule_179_a_result_s_code_fits_its_outcome_and_its_call_s_class() {
    cases(&[
        (
            "read.ok_with_a_code",
            |s| put(s, READ_RESULT, "payload.code", r#""admissible""#),
            Some((READ_RESULT, "schema", "payload.code")),
        ),
        (
            "read.ok_with_no_observations",
            |s| put(s, READ_RESULT, "payload.observations", "[]"),
            None,
        ),
        (
            "read.ok_truncated",
            |s| put(s, READ_RESULT, "payload.truncated", "true"),
            None,
        ),
        (
            "think.ok_with_no_code",
            |s| {
                put(s, READ, "payload.class", r#""think""#);
                put(s, READ, "payload.tool", r#""think.note""#);
                put(s, READ_RESULT, "payload.observations", "[]");
            },
            None,
        ),
        (
            "think.ok_with_a_code",
            |s| {
                put(s, READ, "payload.class", r#""think""#);
                put(s, READ_RESULT, "payload.observations", "[]");
                put(s, READ_RESULT, "payload.code", r#""removed""#);
            },
            Some((READ_RESULT, "schema", "payload.code")),
        ),
        (
            "action.ok_with_no_code",
            |s| put(s, PROPOSE_RESULT, "payload.code", "null"),
            Some((PROPOSE_RESULT, "schema", "payload.code")),
        ),
        (
            "refused_with_no_code",
            |s| read_outcome(s, "refused", "null"),
            Some((READ_RESULT, "schema", "payload.code")),
        ),
        (
            "failed_with_no_code",
            |s| read_outcome(s, "failed", "null"),
            Some((READ_RESULT, "schema", "payload.code")),
        ),
        (
            "read.failed_unavailable",
            |s| read_outcome(s, "failed", r#""unavailable""#),
            None,
        ),
        (
            "read.failed_otherwise",
            |s| read_outcome(s, "failed", r#""unknown_item""#),
            Some((READ_RESULT, "schema", "payload.result")),
        ),
        (
            "think.failed",
            |s| {
                put(s, READ, "payload.class", r#""think""#);
                read_outcome(s, "failed", r#""unavailable""#);
            },
            Some((READ_RESULT, "schema", "payload.result")),
        ),
        (
            "action.failed",
            |s| put(s, PROPOSE_RESULT, "payload.result", r#""failed""#),
            Some((PROPOSE_RESULT, "schema", "payload.result")),
        ),
        (
            "action.failed_unavailable",
            |s| {
                put(s, PROPOSE_RESULT, "payload.result", r#""failed""#);
                put(s, PROPOSE_RESULT, "payload.code", r#""unavailable""#);
            },
            Some((PROPOSE_RESULT, "schema", "payload.result")),
        ),
        (
            "refused.out_of_universe",
            |s| read_outcome(s, "refused", r#""out_of_universe""#),
            None,
        ),
        (
            "refused.unknown_item",
            |s| read_outcome(s, "refused", r#""unknown_item""#),
            None,
        ),
        (
            "refused.call_budget",
            |s| {
                read_outcome(s, "refused", r#""call_budget""#);
                put(s, PROPOSE_RESULT, "payload.result", r#""refused""#);
                put(s, PROPOSE_RESULT, "payload.code", r#""call_budget""#);
            },
            None,
        ),
        (
            "refused.max_theses",
            |s| {
                put(s, PROPOSE_RESULT, "payload.result", r#""refused""#);
                put(s, PROPOSE_RESULT, "payload.code", r#""max_theses""#);
            },
            None,
        ),
        (
            "refused.max_repeat_denials",
            |s| read_outcome(s, "refused", r#""max_repeat_denials""#),
            None,
        ),
        (
            "refused.run_ended",
            |s| read_outcome(s, "refused", r#""run_ended""#),
            None,
        ),
        (
            "refused.unavailable",
            |s| read_outcome(s, "refused", r#""unavailable""#),
            Some((READ_RESULT, "schema", "payload.result")),
        ),
        (
            "refused.a_verdict",
            |s| {
                put(s, PROPOSE_RESULT, "payload.result", r#""refused""#);
                put(s, PROPOSE_RESULT, "payload.code", r#""universe_full""#);
            },
            Some((PROPOSE_RESULT, "schema", "payload.result")),
        ),
        (
            "action.ok_out_of_universe",
            |s| put(s, PROPOSE_RESULT, "payload.code", r#""out_of_universe""#),
            None,
        ),
        (
            "action.ok_universe_full",
            |s| put(s, PROPOSE_RESULT, "payload.code", r#""universe_full""#),
            None,
        ),
        (
            "action.ok_unknown_item",
            |s| put(s, PROPOSE_RESULT, "payload.code", r#""unknown_item""#),
            Some((PROPOSE_RESULT, "schema", "payload.result")),
        ),
        (
            "action.ok_call_budget",
            |s| put(s, PROPOSE_RESULT, "payload.code", r#""call_budget""#),
            Some((PROPOSE_RESULT, "schema", "payload.result")),
        ),
        (
            "action.ok_max_theses",
            |s| put(s, PROPOSE_RESULT, "payload.code", r#""max_theses""#),
            Some((PROPOSE_RESULT, "schema", "payload.result")),
        ),
        (
            "action.ok_max_repeat_denials",
            |s| put(s, PROPOSE_RESULT, "payload.code", r#""max_repeat_denials""#),
            Some((PROPOSE_RESULT, "schema", "payload.result")),
        ),
        (
            "action.ok_run_ended",
            |s| put(s, PROPOSE_RESULT, "payload.code", r#""run_ended""#),
            Some((PROPOSE_RESULT, "schema", "payload.result")),
        ),
        (
            "action.ok_unavailable",
            |s| put(s, PROPOSE_RESULT, "payload.code", r#""unavailable""#),
            Some((PROPOSE_RESULT, "schema", "payload.result")),
        ),
        (
            "observations.swapped",
            |s| {
                put(
                    s,
                    READ_RESULT,
                    "payload.observations",
                    &format!(r#"["{LATER_ITEM}","{ITEM}"]"#),
                )
            },
            Some((READ_RESULT, "non_canonical", "payload.observations")),
        ),
        (
            "observations.twice",
            |s| {
                put(
                    s,
                    READ_RESULT,
                    "payload.observations",
                    &format!(r#"["{ITEM}","{ITEM}"]"#),
                )
            },
            Some((READ_RESULT, "non_canonical", "payload.observations")),
        ),
        (
            "observations.ascending",
            |s| {
                put(
                    s,
                    READ_RESULT,
                    "payload.observations",
                    &format!(r#"["{ITEM}","{LATER_ITEM}"]"#),
                )
            },
            None,
        ),
        (
            "observations.on_a_refusal",
            |s| {
                read_outcome(s, "refused", r#""unknown_item""#);
                put(
                    s,
                    READ_RESULT,
                    "payload.observations",
                    &format!(r#"["{ITEM}"]"#),
                );
            },
            Some((READ_RESULT, "schema", "payload.observations")),
        ),
        (
            "observations.on_an_action",
            |s| {
                put(
                    s,
                    PROPOSE_RESULT,
                    "payload.observations",
                    &format!(r#"["{ITEM}"]"#),
                )
            },
            Some((PROPOSE_RESULT, "schema", "payload.observations")),
        ),
        (
            "observations.on_a_think",
            |s| put(s, READ, "payload.class", r#""think""#),
            Some((READ_RESULT, "schema", "payload.observations")),
        ),
        (
            "truncated.on_a_refusal",
            |s| {
                read_outcome(s, "refused", r#""unknown_item""#);
                put(s, READ_RESULT, "payload.truncated", "true");
            },
            Some((READ_RESULT, "schema", "payload.truncated")),
        ),
        (
            "truncated.on_a_failure",
            |s| {
                read_outcome(s, "failed", r#""unavailable""#);
                put(s, READ_RESULT, "payload.truncated", "true");
            },
            Some((READ_RESULT, "schema", "payload.truncated")),
        ),
        (
            "truncated.on_an_action",
            |s| put(s, PROPOSE_RESULT, "payload.truncated", "true"),
            Some((PROPOSE_RESULT, "schema", "payload.truncated")),
        ),
        (
            "first.code_before_result",
            |s| {
                put(s, PROPOSE_RESULT, "payload.code", "null");
                put(
                    s,
                    PROPOSE_RESULT,
                    "payload.observations",
                    &format!(r#"["{ITEM}"]"#),
                );
            },
            Some((PROPOSE_RESULT, "schema", "payload.code")),
        ),
        (
            "first.result_before_order",
            |s| {
                read_outcome(s, "failed", r#""unknown_item""#);
                put(
                    s,
                    READ_RESULT,
                    "payload.observations",
                    &format!(r#"["{LATER_ITEM}","{ITEM}"]"#),
                );
            },
            Some((READ_RESULT, "schema", "payload.result")),
        ),
        (
            "first.order_before_observations",
            |s| {
                put(
                    s,
                    PROPOSE_RESULT,
                    "payload.observations",
                    &format!(r#"["{LATER_ITEM}","{ITEM}"]"#),
                );
            },
            Some((PROPOSE_RESULT, "non_canonical", "payload.observations")),
        ),
        (
            "first.observations_before_truncated",
            |s| {
                put(
                    s,
                    PROPOSE_RESULT,
                    "payload.observations",
                    &format!(r#"["{ITEM}"]"#),
                );
                put(s, PROPOSE_RESULT, "payload.truncated", "true");
            },
            Some((PROPOSE_RESULT, "schema", "payload.observations")),
        ),
    ]);
}

#[test]
fn rule_180_a_reply_over_its_budget_runs_none_of_its_calls() {
    cases(&[
        (
            "all_over_budget",
            |s| {
                read_outcome(s, "refused", r#""call_budget""#);
                put(s, PROPOSE_RESULT, "payload.result", r#""refused""#);
                put(s, PROPOSE_RESULT, "payload.code", r#""call_budget""#);
            },
            None,
        ),
        (
            "one_over_budget_after_one_ran",
            |s| read_outcome(s, "refused", r#""call_budget""#),
            Some((READ_RESULT, "schema", "payload.code")),
        ),
        (
            "one_ran_after_one_over_budget",
            |s| {
                put(s, PROPOSE_RESULT, "payload.result", r#""refused""#);
                put(s, PROPOSE_RESULT, "payload.code", r#""call_budget""#);
            },
            Some((READ_RESULT, "schema", "payload.code")),
        ),
        (
            "another_reply_s_budget",
            |s| {
                let mut next = turn();
                set(
                    &mut next,
                    "event_id",
                    Some(Value::Str("01J9H001A000000000000000M2".into())),
                );
                set(
                    &mut next,
                    "payload.call_id",
                    Some(Value::Str("01J9H001A000000000000000C2".into())),
                );
                set(&mut next, "payload.verdicts", Some(json("[]")));
                let mut late =
                    tool_call("01J9H002A000000000000000T3", 0, "data.query", "read", '9');
                set(
                    &mut late,
                    "payload.call_id",
                    Some(Value::Str("01J9H001A000000000000000C2".into())),
                );
                let refused = tool_result(
                    "01J9H003A000000000000000V3",
                    "01J9H002A000000000000000T3",
                    "refused",
                    Some("call_budget"),
                    '5',
                );
                s.insert(3, vec![next, late, refused]);
            },
            None,
        ),
    ]);
}

#[test]
fn rule_181_a_summary_is_canonical_and_watches_at_most_ten_instruments_once_each() {
    cases(&[
        (
            "instruments_read.swapped",
            |s| {
                put(
                    s,
                    SUMMARY,
                    "payload.instruments_read",
                    &format!(r#"["{MSFT}","{AAPL}"]"#),
                )
            },
            Some((SUMMARY, "non_canonical", "payload.instruments_read")),
        ),
        (
            "instruments_read.twice",
            |s| {
                put(
                    s,
                    SUMMARY,
                    "payload.instruments_read",
                    &format!(r#"["{AAPL}","{AAPL}"]"#),
                )
            },
            Some((SUMMARY, "non_canonical", "payload.instruments_read")),
        ),
        (
            "instruments_read.ascending",
            |s| {
                put(
                    s,
                    SUMMARY,
                    "payload.instruments_read",
                    &format!(r#"["{AAPL}","{MSFT}"]"#),
                )
            },
            None,
        ),
        (
            "watch.ten",
            |s| put(s, SUMMARY, "payload.watch", &watch(10)),
            None,
        ),
        (
            "watch.eleven",
            |s| put(s, SUMMARY, "payload.watch", &watch(11)),
            Some((SUMMARY, "schema", "payload.watch")),
        ),
        (
            "watch.twice",
            |s| {
                put(
                    s,
                    SUMMARY,
                    "payload.watch",
                    &format!(
                        r#"[{{"instrument_id":"{AAPL}","reason":"earnings"}},{{"instrument_id":"{AAPL}","reason":"news_flow"}}]"#
                    ),
                )
            },
            Some((SUMMARY, "schema", "payload.watch")),
        ),
        (
            "watch.unordered",
            |s| {
                put(
                    s,
                    SUMMARY,
                    "payload.watch",
                    &format!(
                        r#"[{{"instrument_id":"{MSFT}","reason":"earnings"}},{{"instrument_id":"{AAPL}","reason":"news_flow"}}]"#
                    ),
                )
            },
            None,
        ),
        (
            "watch.empty",
            |s| put(s, SUMMARY, "payload.watch", "[]"),
            None,
        ),
        (
            "first.read_before_watch",
            |s| {
                put(
                    s,
                    SUMMARY,
                    "payload.instruments_read",
                    &format!(r#"["{MSFT}","{AAPL}"]"#),
                );
                put(s, SUMMARY, "payload.watch", &watch(11));
            },
            Some((SUMMARY, "non_canonical", "payload.instruments_read")),
        ),
    ]);
}

/// `n` watch entries on distinct instruments.
fn watch(n: usize) -> String {
    let entries: Vec<String> = (0..n)
        .map(|i| format!(r#"{{"instrument_id":"7b4a1c2e-2222-4a2b-9c3d-0000000000{i:02}","reason":"price_band"}}"#))
        .collect();
    format!("[{}]", entries.join(","))
}

#[test]
fn rule_182_an_end_names_its_bound_and_discards_nothing_when_done() {
    cases(&[
        (
            "done.max_turns",
            |s| put(s, ENDED, "payload.bound", r#""max_turns""#),
            None,
        ),
        (
            "done.max_tool_calls",
            |s| put(s, ENDED, "payload.bound", r#""max_tool_calls""#),
            None,
        ),
        (
            "done.max_theses",
            |s| put(s, ENDED, "payload.bound", r#""max_theses""#),
            Some((ENDED, "schema", "payload.bound")),
        ),
        (
            "done.max_repeat_denials",
            |s| put(s, ENDED, "payload.bound", r#""max_repeat_denials""#),
            Some((ENDED, "schema", "payload.bound")),
        ),
        (
            "bound.with_no_bound",
            |s| put(s, ENDED, "payload.reason", r#""bound""#),
            Some((ENDED, "schema", "payload.bound")),
        ),
        (
            "bound.max_turns",
            |s| {
                put(s, ENDED, "payload.reason", r#""bound""#);
                put(s, ENDED, "payload.bound", r#""max_turns""#);
            },
            Some((ENDED, "schema", "payload.bound")),
        ),
        (
            "bound.max_tool_calls",
            |s| {
                put(s, ENDED, "payload.reason", r#""bound""#);
                put(s, ENDED, "payload.bound", r#""max_tool_calls""#);
            },
            None,
        ),
        (
            "bound.max_repeat_denials",
            |s| {
                put(s, ENDED, "payload.reason", r#""bound""#);
                put(s, ENDED, "payload.bound", r#""max_repeat_denials""#);
            },
            None,
        ),
        (
            "bound.max_theses",
            |s| {
                put(s, ENDED, "payload.reason", r#""bound""#);
                put(s, ENDED, "payload.bound", r#""max_theses""#);
            },
            None,
        ),
        (
            "deadline.with_a_bound",
            |s| {
                put(s, ENDED, "payload.reason", r#""deadline""#);
                put(s, ENDED, "payload.bound", r#""max_turns""#);
            },
            Some((ENDED, "schema", "payload.bound")),
        ),
        (
            "cost.with_a_bound",
            |s| {
                put(s, ENDED, "payload.reason", r#""cost""#);
                put(s, ENDED, "payload.bound", r#""max_tool_calls""#);
            },
            Some((ENDED, "schema", "payload.bound")),
        ),
        (
            "mode.with_a_bound",
            |s| {
                put(s, ENDED, "payload.reason", r#""mode""#);
                put(s, ENDED, "payload.bound", r#""max_theses""#);
            },
            Some((ENDED, "schema", "payload.bound")),
        ),
        (
            "error.with_a_bound",
            |s| {
                put(s, ENDED, "payload.reason", r#""error""#);
                put(s, ENDED, "payload.bound", r#""max_repeat_denials""#);
            },
            Some((ENDED, "schema", "payload.bound")),
        ),
        (
            "answered.with_a_bound",
            |s| {
                put(s, ENDED, "payload.reason", r#""answered""#);
                put(s, ENDED, "payload.bound", r#""max_turns""#);
            },
            Some((ENDED, "schema", "payload.bound")),
        ),
        (
            "deadline.with_none",
            |s| put(s, ENDED, "payload.reason", r#""deadline""#),
            None,
        ),
        (
            "done.discards",
            |s| put(s, ENDED, "payload.discarded", &format!(r#"["{PROPOSE}"]"#)),
            Some((ENDED, "schema", "payload.discarded")),
        ),
        (
            "mode.discards",
            |s| {
                put(s, ENDED, "payload.reason", r#""mode""#);
                put(s, ENDED, "payload.discarded", &format!(r#"["{PROPOSE}"]"#));
            },
            None,
        ),
        (
            "bound.discards_two",
            |s| {
                put(s, ENDED, "payload.reason", r#""bound""#);
                put(s, ENDED, "payload.bound", r#""max_theses""#);
                put(
                    s,
                    ENDED,
                    "payload.discarded",
                    &format!(r#"["{READ}","{PROPOSE}"]"#),
                );
            },
            None,
        ),
        (
            "discarded.swapped",
            |s| {
                put(s, ENDED, "payload.reason", r#""error""#);
                put(
                    s,
                    ENDED,
                    "payload.discarded",
                    &format!(r#"["{PROPOSE}","{READ}"]"#),
                );
            },
            Some((ENDED, "non_canonical", "payload.discarded")),
        ),
        (
            "discarded.twice",
            |s| {
                put(s, ENDED, "payload.reason", r#""error""#);
                put(
                    s,
                    ENDED,
                    "payload.discarded",
                    &format!(r#"["{PROPOSE}","{PROPOSE}"]"#),
                );
            },
            Some((ENDED, "non_canonical", "payload.discarded")),
        ),
        (
            "first.bound_before_discarded",
            |s| {
                put(s, ENDED, "payload.bound", r#""max_theses""#);
                put(
                    s,
                    ENDED,
                    "payload.discarded",
                    &format!(r#"["{PROPOSE}","{READ}"]"#),
                );
            },
            Some((ENDED, "schema", "payload.bound")),
        ),
        (
            "first.order_before_done",
            |s| {
                put(
                    s,
                    ENDED,
                    "payload.discarded",
                    &format!(r#"["{PROPOSE}","{READ}"]"#),
                )
            },
            Some((ENDED, "non_canonical", "payload.discarded")),
        ),
    ]);
}

/// A turn record of the run after its end: `reason` the end's, and `verdicts` the late record's.
fn late_turn(stream: &mut Vec<Vec<Value>>, reason: &str, verdicts: &str) {
    put(stream, ENDED, "payload.reason", &format!(r#""{reason}""#));
    let mut late = turn();
    set(
        &mut late,
        "event_id",
        Some(Value::Str("01J9H001A000000000000000M3".into())),
    );
    set(
        &mut late,
        "payload.call_id",
        Some(Value::Str("01J9H001A000000000000000C3".into())),
    );
    set(&mut late, "payload.verdicts", Some(json(verdicts)));
    stream.push(vec![late]);
}

#[test]
fn rule_183_no_turn_record_follows_its_run_s_end_unless_cancelled_and_empty() {
    const LATE: &str = "01J9H001A000000000000000M3";
    cases(&[
        (
            "after_deadline.empty",
            |s| late_turn(s, "deadline", "[]"),
            None,
        ),
        ("after_mode.empty", |s| late_turn(s, "mode", "[]"), None),
        (
            "after_deadline.with_verdicts",
            |s| {
                late_turn(
                    s,
                    "deadline",
                    r#"[{"candidate":0,"kind":"thesis","result":"judged"}]"#,
                )
            },
            Some((LATE, "schema", "correlation_id")),
        ),
        (
            "after_mode.with_verdicts",
            |s| {
                late_turn(
                    s,
                    "mode",
                    r#"[{"candidate":0,"kind":"thesis","result":"judged"}]"#,
                )
            },
            Some((LATE, "schema", "correlation_id")),
        ),
        (
            "after_done.empty",
            |s| late_turn(s, "done", "[]"),
            Some((LATE, "schema", "correlation_id")),
        ),
        (
            "after_cost.empty",
            |s| late_turn(s, "cost", "[]"),
            Some((LATE, "schema", "correlation_id")),
        ),
        (
            "after_error.empty",
            |s| late_turn(s, "error", "[]"),
            Some((LATE, "schema", "correlation_id")),
        ),
        (
            "a_single_call_s_record",
            |s| {
                late_turn(s, "done", "[]");
                put(s, LATE, "correlation_id", &format!(r#""{OTHER_RUN}""#));
            },
            None,
        ),
        (
            "another_purpose",
            |s| {
                late_turn(s, "done", "[]");
                put(s, LATE, "payload.purpose", r#""llm_signal""#);
            },
            None,
        ),
    ]);
}

/// A run's records judged as part of other streams' batches: each stream's records are its own.
#[test]
fn run_records_of_another_agent_stream_never_count() {
    let mut stream = run();
    let mut other = started();
    set(
        &mut other,
        "event_id",
        Some(Value::Str("01J9H006A000000000000000S2".into())),
    );
    set(
        &mut other,
        "stream_id",
        Some(Value::Str("agent:ws_01J8Z2:agent_b".into())),
    );
    set(&mut other, "actor.id", Some(Value::Str("agent_b".into())));
    stream[0].insert(0, other);
    assert_eq!(judge(&stream), None, "agent_b's start is not agent_a's");
    let mut stream = run();
    set(
        at(&mut stream, STARTED),
        "stream_id",
        Some(Value::Str("agent:ws_01J8Z2:agent_b".into())),
    );
    set(
        at(&mut stream, STARTED),
        "actor.id",
        Some(Value::Str("agent_b".into())),
    );
    assert_eq!(
        judge(&stream),
        Some((
            index_of(&stream, READ),
            "schema".to_owned(),
            "correlation_id".to_owned()
        )),
        "agent_a's tool call has no start on its own stream"
    );
}

#[test]
fn the_catalogue_closes_the_run_records_on_the_agent_stream_and_their_rules_read_the_stream() {
    let types = [
        "RunStarted",
        "ToolCallRecorded",
        "ToolResultRecorded",
        "RunSummaryRecorded",
        "RunEnded",
    ];
    for event_type in types {
        assert!(closes(event_type, 1, StreamType::Agent), "{event_type}");
        assert!(!closes(event_type, 2, StreamType::Agent), "{event_type} v2");
        for stream in [
            StreamType::Account,
            StreamType::Control,
            StreamType::Scheduler,
            StreamType::Notice,
            StreamType::Meter,
        ] {
            assert!(!closes(event_type, 1, stream), "{event_type} on {stream:?}");
        }
    }
    for body in run().iter().flatten() {
        let draft = Draft::parse(&to_canonical(body)).unwrap();
        let event_type = draft.event_type().to_owned();
        assert!(
            reads_stream(std::slice::from_ref(&draft)),
            "{event_type} is judged by a stream rule"
        );
        assert!(
            mandate_journal::STREAM_RULE_TYPES.contains(&event_type.as_str()),
            "{event_type} is read for the stream rules"
        );
    }
}

/// A `single_call` research record carrying the run's id as its correlation: no `RunStarted`
/// precedes it, so it starts no run (rule 173 counts only `RunStarted`s, rule 183 reaches only a run
/// that started).
fn single_call() -> Value {
    let mut call = turn();
    set(
        &mut call,
        "event_id",
        Some(Value::Str("01J9GZ01A000000000000000M9".into())),
    );
    set(
        &mut call,
        "payload.call_id",
        Some(Value::Str("01J9GZ01A000000000000000C9".into())),
    );
    call
}

/// Rule 183 reaches a research record only when an earlier `RunStarted` names its run: an earlier
/// `RunEnded` alone, which no stream that passed rule 174 holds, does not make the record a late
/// turn. Judged straight through `check_stream`, since `append` never stores such an end.
#[test]
fn rule_183_reaches_only_a_run_that_started() {
    let parse = |body: &Value| Draft::parse(&to_canonical(body)).unwrap();
    let earlier = vec![parse(&ended())];
    let late = parse(&single_call());
    assert_eq!(
        check_stream(&earlier, std::slice::from_ref(&late)).err(),
        None
    );
    let mut started = vec![parse(&started())];
    started.extend(earlier);
    let refused = check_stream(&started, std::slice::from_ref(&late))
        .err()
        .map(|(i, e)| (i, e.reason.code().to_owned(), e.path));
    assert_eq!(
        refused,
        Some((0, "schema".to_owned(), "correlation_id".to_owned())),
        "after its run's start and a `done` end, a turn record with verdicts is refused"
    );
}

/// Rule 271 (DEC-2392): an event run's cause is an earlier `ObservationRecorded` of its own run,
/// about an instrument, and no earlier event run's cause holds the same item, so one item starts at
/// most one run. Each refusal is paired with the same stream accepted.
#[test]
fn rule_271_an_event_run_names_its_own_item_and_one_item_starts_one_run() {
    cases(&[
        ("honest", |s| event_first(s, 'a'), None),
        (
            "cause.names_no_record",
            |s| {
                event_first(s, 'a');
                put(s, STARTED, "payload.trigger.cause", &format!(r#""{ITEM}""#));
            },
            Some((STARTED, "schema", "payload.trigger.cause")),
        ),
        (
            "cause.names_a_record_that_is_no_observation",
            |s| {
                event_first(s, 'a');
                s.insert(0, vec![single_call()]);
                let first = single_call()
                    .get("event_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                put(
                    s,
                    STARTED,
                    "payload.trigger.cause",
                    &format!(r#""{first}""#),
                );
            },
            Some((STARTED, "schema", "payload.trigger.cause")),
        ),
        (
            "cause.is_another_run_s",
            |s| {
                event_first(s, 'a');
                put(s, OBSERVED, "correlation_id", &format!(r#""{OTHER_RUN}""#));
            },
            Some((STARTED, "schema", "payload.trigger.cause")),
        ),
        (
            "cause.names_no_instrument",
            |s| {
                event_first(s, 'a');
                put(s, OBSERVED, "payload.instrument_id", "null");
            },
            Some((STARTED, "schema", "payload.trigger.cause")),
        ),
        (
            "cause.comes_after_the_start",
            |s| {
                event_first(s, 'a');
                let item = s[0].remove(0);
                s.push(vec![item]);
            },
            Some((STARTED, "schema", "payload.trigger.cause")),
        ),
        (
            "one_item.two_runs",
            |s| {
                event_first(s, 'a');
                second_event_run(s, 'a');
            },
            Some((
                "01J9H006A000000000000000S2",
                "schema",
                "payload.trigger.cause",
            )),
        ),
        (
            "two_items.two_runs",
            |s| {
                event_first(s, 'a');
                second_event_run(s, 'b');
            },
            None,
        ),
        (
            "a_scheduled_run_after_an_event_run",
            |s| {
                event_first(s, 'a');
                second_run(s, OTHER_RUN, "monitor", "schedule", None);
            },
            None,
        ),
    ]);
}
