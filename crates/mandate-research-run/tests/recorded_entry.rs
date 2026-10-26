//! DEC-2247 item 1: a recorded research entry reads as one `ResearchEntry`, the same content the
//! recorded bundle's loader pins and `model register --entry` registers. Each member is required
//! and nothing is defaulted. The oracle is the demo recording's entry, typed out here. An optional
//! `agentic` member names the entry's pinned loop (DEC-2263); the demo's names `v0_2`, and the same
//! entry without it is the single-call one.

use mandate_canon::{Key, Value, parse};
use mandate_gateway::registry::{Identity, Mode, Snapshot};
use mandate_research_run::agentic::LoopBounds;
use mandate_research_run::{EntryError, OutputBounds, ResearchEntry, RetrievalPlan, agentic_loop};

/// The agentic demo entry's content hash, pinned (DEC-2263 item 3).
const AGENTIC_HASH: &str = "bff754a2e669add9a7928a4b8c71c764cfe61455e361621ae6f6c262af0824cd";
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
        agentic: Some(LoopBounds::v0_2()),
    };
    assert_eq!(ResearchEntry::recorded(&recorded_entry()), Ok(expected));
}

#[test]
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

/// The demo recording's entry naming the `v0_2` loop.
fn agentic() -> ResearchEntry {
    ResearchEntry::recorded(&with("agentic", Some(Value::Str("v0_2".to_owned()))))
        .unwrap_or_else(|e| panic!("{e:?}"))
}

/// The demo recording's entry with no `agentic` member: the single-call entry.
fn single_call() -> ResearchEntry {
    ResearchEntry::recorded(&with("agentic", None)).unwrap_or_else(|e| panic!("{e:?}"))
}

/// Words the agentic system text never holds: it says what the tools may do, never to trade, to
/// size, or to recommend (the coordinator's condition on DEC-2263).
const NEVER_SAID: [&str; 7] = [
    "buy",
    "sell",
    "recommend",
    "should trade",
    "position size",
    "allocate",
    "profit",
];

#[test]
fn a_recording_without_the_member_is_a_single_call_entry() {
    let entry = single_call();
    assert_eq!(entry.agentic, None);
    assert_eq!(entry.content().mode, Mode::SingleCall);
}

#[test]
fn a_recording_naming_the_v0_2_loop_reads_its_bounds() {
    assert_eq!(agentic().agentic, Some(LoopBounds::v0_2()));
}

#[test]
fn a_recording_naming_any_other_loop_is_refused_by_name() {
    for named in [
        Value::Str("v0_3".to_owned()),
        Value::Str(String::new()),
        Value::Str("V0_2".to_owned()),
        parse(b"2").unwrap(),
        parse(b"true").unwrap(),
    ] {
        assert_eq!(
            ResearchEntry::recorded(&with("agentic", Some(named.clone()))),
            Err(EntryError::Member("agentic")),
            "{named:?}"
        );
    }
}

#[test]
fn the_agentic_entry_pins_its_loop_and_its_own_system_text() {
    let entry = agentic();
    let content = entry.content();
    let single = single_call().content();
    let pinned = agentic_loop(&LoopBounds::v0_2(), &OutputBounds::v0_1())
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(content.mode, Mode::Agentic(pinned));
    assert_eq!(content.template.user, single.template.user);
    assert_eq!(content.template.inputs, single.template.inputs);
    assert_ne!(content.template.system, single.template.system);
    assert_eq!(
        content.content_hash().to_string(),
        AGENTIC_HASH,
        "the system text and the loop are pinned content: a change to either is a new pin"
    );
    assert_ne!(content.content_hash(), single.content_hash());
}

#[test]
fn the_agentic_system_text_says_what_the_tools_may_do_and_never_to_trade() {
    let system = agentic().content().template.system.to_lowercase();
    for said in ["tools", "read", "note", "never orders", "untrusted"] {
        assert!(system.contains(said), "{said}: {system}");
    }
    for word in NEVER_SAID {
        assert!(!system.contains(word), "{word}: {system}");
    }
}
