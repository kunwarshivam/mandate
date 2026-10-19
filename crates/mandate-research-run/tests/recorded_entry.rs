//! DEC-2247 item 1: a recorded research entry reads as one `ResearchEntry`, the same content the
//! recorded bundle's loader pins and `model register --entry` registers. Each member is required
//! and nothing is defaulted. The oracle is the demo recording's entry, typed out here.

use mandate_canon::{Key, Value, parse};
use mandate_gateway::registry::{Identity, Snapshot};
use mandate_research_run::{EntryError, OutputBounds, ResearchEntry, RetrievalPlan};

const RECORDING: &str = include_str!("../../../deploy/cell/demo/research/research.json");
const MEMBERS: [&str; 8] = [
    "model_id",
    "model_version",
    "provider",
    "model_name",
    "snapshot",
    "deadline_ms",
    "max_output_tokens",
    "seed",
];

fn recorded_entry() -> Value {
    parse(RECORDING.as_bytes())
        .unwrap()
        .get("entry")
        .cloned()
        .unwrap()
}

/// The recorded entry with `member` set to `value`, or removed when `value` is `None`.
fn with(member: &str, value: Option<Value>) -> Value {
    let Value::Object(mut members) = recorded_entry() else {
        panic!("the entry is an object")
    };
    let key = Key::new(member).unwrap();
    match value {
        Some(value) => drop(members.insert(key, value)),
        None => drop(members.remove(&key)),
    }
    Value::Object(members)
}

#[test]
#[ignore = "pending E21-54"]
fn the_demo_recording_reads_as_its_entry_with_the_fixed_plan_and_bounds() {
    let expected = ResearchEntry {
        model_id: "llm.research_agent".to_owned(),
        model_version: "0.1.0".to_owned(),
        identity: Identity {
            provider: "recorded".to_owned(),
            model_name: "recorded/demo-research".to_owned(),
            snapshot: Snapshot::Dated("recorded-demo-2026-10-19".to_owned()),
            quantization: None,
        },
        deadline_ms: 120_000,
        max_output_tokens: 4_096,
        seed: Some(7),
        plan: RetrievalPlan::v0_1(),
        bounds: OutputBounds::v0_1(),
    };
    assert_eq!(ResearchEntry::recorded(&recorded_entry()), Ok(expected));
}

#[test]
#[ignore = "pending E21-54"]
fn each_member_missing_empty_or_out_of_range_is_refused_by_name() {
    for member in MEMBERS {
        assert_eq!(
            ResearchEntry::recorded(&with(member, None)),
            Err(EntryError::Member(member)),
            "{member} missing"
        );
    }
    for member in [
        "model_id",
        "model_version",
        "provider",
        "model_name",
        "snapshot",
    ] {
        assert_eq!(
            ResearchEntry::recorded(&with(member, Some(Value::Str(String::new())))),
            Err(EntryError::Member(member)),
            "{member} empty"
        );
    }
    for member in ["deadline_ms", "max_output_tokens", "seed"] {
        assert_eq!(
            ResearchEntry::recorded(&with(member, Some(Value::Str("7".to_owned())))),
            Err(EntryError::Member(member)),
            "{member} not an integer"
        );
    }
    let too_large = parse(b"4294967296").unwrap();
    assert_eq!(
        ResearchEntry::recorded(&with("max_output_tokens", Some(too_large))),
        Err(EntryError::Member("max_output_tokens"))
    );
}
