//! The customer real-money gate's decision ([DEC-2134] items 2, 5 and 7; journal spec v0.91
//! §9.36), over random control streams: the setting is the `enabled` of the latest
//! `RealMoneyGateChanged` by `seq`, off with none or with one it cannot read, and a request that
//! names any environment but `paper` is refused while it is off and admitted only to the checks
//! after the gate while it is on. The oracle walks the stream from its highest `seq` down, apart
//! from the code's fold, and reads the payload's members itself.
//!
//! [DEC-2134]: ../../../docs/project/decisions/DEC-2134.md

use mandate_api_server::real_money::{Decision, Setting, decide, setting};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::StoredEvent;
use proptest::prelude::*;

const WORKSPACE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const OPERATOR: &str = "01JAA3G0A000000000000000P1";

/// One row of a generated control stream.
#[derive(Debug, Clone)]
enum Entry {
    /// A well-formed setting.
    Gate(bool),
    /// A setting whose `enabled` is not a boolean: never read as on.
    GateNotBoolean(&'static str),
    /// A setting whose body is not JSON: never read as on.
    GateUnreadable,
    /// A refusal, which changes no setting.
    Refused,
    /// Another record of the control stream.
    Other(&'static str),
}

fn body(event_type: &str, payload: &str) -> Vec<u8> {
    let text = format!(
        r#"{{"environment":"paper","event_type":"{event_type}","payload":{payload},"stream_id":"ctl:{WORKSPACE}"}}"#
    );
    to_canonical(&parse(text.as_bytes()).expect("JSON"))
}

fn stored(seq: u64, entry: &Entry) -> StoredEvent {
    let (event_type, body) = match entry {
        Entry::Gate(enabled) => (
            "RealMoneyGateChanged",
            body(
                "RealMoneyGateChanged",
                &format!(
                    r#"{{"decision":{},"enabled":{enabled},"operator":"{OPERATOR}"}}"#,
                    if *enabled { r#""DEC-2135""# } else { "null" }
                ),
            ),
        ),
        Entry::GateNotBoolean(value) => (
            "RealMoneyGateChanged",
            body(
                "RealMoneyGateChanged",
                &format!(r#"{{"decision":"DEC-2135","enabled":{value},"operator":"{OPERATOR}"}}"#),
            ),
        ),
        Entry::GateUnreadable => ("RealMoneyGateChanged", b"{not json".to_vec()),
        Entry::Refused => (
            "RealMoneyRefused",
            body(
                "RealMoneyRefused",
                &format!(
                    r#"{{"action":"deploy","principal":"{OPERATOR}","reason":"gate_off","requested":"live"}}"#
                ),
            ),
        ),
        Entry::Other(event_type) => (*event_type, body(event_type, r#"{"enabled":true}"#)),
    };
    StoredEvent {
        stream_id: format!("ctl:{WORKSPACE}"),
        seq,
        event_id: format!("01JBH3BTZ8K2M4N6P8R0S2T{seq:03}"),
        event_type: event_type.to_owned(),
        schema_version: 1,
        environment: "paper".to_owned(),
        recorded_at: "2026-10-26T09:00:00.000000000Z".to_owned(),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    }
}

fn entry() -> impl Strategy<Value = Entry> {
    prop_oneof![
        4 => any::<bool>().prop_map(Entry::Gate),
        1 => prop::sample::select(vec!["\"true\"", "1", "null", "{}"]).prop_map(Entry::GateNotBoolean),
        1 => Just(Entry::GateUnreadable),
        2 => Just(Entry::Refused),
        2 => prop::sample::select(vec![
            "StreamOpened",
            "MandateConfirmed",
            "PlatformOperatorAction",
            "StepUpConfirmed",
        ])
        .prop_map(Entry::Other),
    ]
}

/// A stream of `entries` at `seq` 1 to n, handed to the fold in the order `order` gives: the
/// journal hands rows in `seq` order, and the fold must not rely on it.
fn rows(entries: &[Entry], reversed: bool) -> Vec<StoredEvent> {
    let mut rows: Vec<StoredEvent> = entries
        .iter()
        .zip(1_u64..)
        .map(|(entry, seq)| stored(seq, entry))
        .collect();
    if reversed {
        rows.reverse();
    }
    rows
}

/// The oracle's setting: from the highest `seq` down, the first `RealMoneyGateChanged` answers,
/// and only a JSON body whose `payload.enabled` is the boolean `true` is on.
fn oracle(rows: &[StoredEvent]) -> bool {
    let mut by_seq: Vec<&StoredEvent> = rows.iter().collect();
    by_seq.sort_by_key(|row| std::cmp::Reverse(row.seq));
    by_seq
        .into_iter()
        .find(|row| row.event_type == "RealMoneyGateChanged")
        .and_then(|row| parse(&row.body).ok())
        .and_then(|envelope| envelope.get("payload")?.get("enabled").cloned())
        == Some(Value::Bool(true))
}

fn requested() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => Just("paper".to_owned()),
        3 => Just("live".to_owned()),
        1 => prop::sample::select(vec!["simulated", "", "Paper", "paper ", "PAPER", "live\n"])
            .prop_map(str::to_owned),
        1 => "[ -~]{0,12}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn the_gate_admits_past_itself_exactly_what_the_latest_setting_allows(
        entries in prop::collection::vec(entry(), 0..14),
        reversed in any::<bool>(),
        requested in requested(),
    ) {
        let rows = rows(&entries, reversed);
        let on = oracle(&rows);
        let read = setting(&rows);
        prop_assert_eq!(read, if on { Setting::On } else { Setting::Off });
        let want = match (requested.as_str(), on) {
            ("paper", _) => Decision::NotGated,
            (_, true) => Decision::Admitted,
            (_, false) => Decision::Refused,
        };
        prop_assert_eq!(decide(read, &requested), want);
    }
}

#[test]
fn a_stream_with_no_setting_is_off() {
    assert_eq!(setting(&[]), Setting::Off);
    assert_eq!(
        setting(&rows(
            &[Entry::Other("StreamOpened"), Entry::Refused],
            false
        )),
        Setting::Off
    );
}

#[test]
fn the_latest_setting_answers_whichever_order_the_rows_come_in() {
    for reversed in [false, true] {
        let on_then_off = rows(&[Entry::Gate(true), Entry::Gate(false)], reversed);
        assert_eq!(setting(&on_then_off), Setting::Off);
        let off_then_on = rows(&[Entry::Gate(false), Entry::Gate(true)], reversed);
        assert_eq!(setting(&off_then_on), Setting::On);
    }
}

/// A setting the fold cannot read closes the gate, even over an earlier on (`AGENTS.md` rule 3).
#[test]
fn an_unreadable_latest_setting_is_off() {
    for latest in [
        Entry::GateUnreadable,
        Entry::GateNotBoolean("\"true\""),
        Entry::GateNotBoolean("1"),
    ] {
        assert_eq!(
            setting(&rows(&[Entry::Gate(true), latest], false)),
            Setting::Off
        );
    }
}

/// Only exactly `paper` passes ungated; every other value, however close, is judged.
#[test]
fn only_paper_itself_is_not_gated() {
    assert_eq!(decide(Setting::Off, "paper"), Decision::NotGated);
    assert_eq!(decide(Setting::On, "paper"), Decision::NotGated);
    for other in ["live", "simulated", "", "Paper", "paper ", " paper"] {
        assert_eq!(decide(Setting::Off, other), Decision::Refused, "{other:?}");
        assert_eq!(decide(Setting::On, other), Decision::Admitted, "{other:?}");
    }
}

/// No route writes the setting ([DEC-2134] item 3): the only source files of this crate that
/// name `RealMoneyGateChanged` are the gate, which reads it and never builds an event of it, and
/// the operator command, which no module but the binary's `main.rs` uses.
#[test]
fn no_source_of_the_api_but_the_operator_command_writes_the_gates_setting() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut naming = Vec::new();
    let mut using_operator = Vec::new();
    for entry in std::fs::read_dir(&src).expect("src") {
        let path = entry.expect("an entry").path();
        let name = path
            .file_name()
            .expect("a name")
            .to_string_lossy()
            .into_owned();
        let text = std::fs::read_to_string(&path).expect("UTF-8 source");
        if text.contains("RealMoneyGateChanged") {
            naming.push(name.clone());
        }
        if text.contains("operator::") || text.contains("operator::{") {
            using_operator.push(name);
        }
    }
    naming.sort();
    assert_eq!(naming, ["operator.rs", "real_money.rs"]);
    assert_eq!(
        using_operator,
        ["main.rs"],
        "only the binary runs the operator command"
    );
    let gate = std::fs::read_to_string(src.join("real_money.rs")).expect("the gate");
    assert!(
        !gate.contains("event_type: CHANGED"),
        "the gate reads its setting and never writes it"
    );
}

/// The journal spec's own setting streams (§9.36's `real_money` vectors), each folded by the gate
/// at `seq` 1 to n, give the setting the vectors expect, so the reference's fold and this one read
/// every shared stream alike.
#[test]
fn the_gate_reads_each_vector_stream_as_the_spec_expects() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).expect("the fixture")).expect("JSON");
    let streams = fixture
        .get("real_money")
        .and_then(|s| s.get("streams"))
        .and_then(Value::as_array)
        .expect("the setting streams");
    assert!(streams.len() >= 6, "{} streams", streams.len());
    for case in streams {
        let name = case.get("name").and_then(Value::as_str).expect("a name");
        let records = case
            .get("records")
            .and_then(Value::as_array)
            .expect("records");
        let rows: Vec<StoredEvent> = records
            .iter()
            .zip(1_u64..)
            .map(|(record, seq)| {
                let body = to_canonical(record);
                StoredEvent {
                    stream_id: format!("ctl:{WORKSPACE}"),
                    seq,
                    event_id: format!("01JBH3BTZ8K2M4N6P8R0S2T{seq:03}"),
                    event_type: record
                        .get("event_type")
                        .and_then(Value::as_str)
                        .expect("a type")
                        .to_owned(),
                    schema_version: 1,
                    environment: "paper".to_owned(),
                    recorded_at: "2026-10-26T09:00:00.000000000Z".to_owned(),
                    prev_hash: Digest::ZERO,
                    hash: Digest::of(&body),
                    body,
                }
            })
            .collect();
        let want = case.get("expect").and_then(|e| e.get("enabled")) == Some(&Value::Bool(true));
        let want = if want { Setting::On } else { Setting::Off };
        assert_eq!(setting(&rows), want, "{name}");
        let mut reversed = rows;
        reversed.reverse();
        assert_eq!(setting(&reversed), want, "{name}, rows handed in reverse");
    }
}
