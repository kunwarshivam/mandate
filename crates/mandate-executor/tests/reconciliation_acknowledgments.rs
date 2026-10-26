//! E7-4b (journal spec v0.104 §9.28, DEC-2830): an owner's acknowledgment of a reconciliation hold.
//! The executor copies it as DEC-2828 copies every acknowledgment, judges checks 1 and 2 against the
//! hold the copy's `event` names (the executor's own `exits_only` or `paused` `AgentModeApplied`
//! under `reconciliation:<subject>`, standing while it is the latest record of its agent and
//! restriction), and checks 3 and 4 as for a limit. An accepted one lifts that hold alone: the very
//! next record after the copy is `AgentModeApplied` for the hold's agent to `normal` under the
//! hold's restriction, `originated`, caused by the copy. A refused one is followed by its refusal.
//!
//! The cases are the committed fixture's `reconciliation_acknowledgments` section, walked as
//! `acknowledgment_copies` walks its vectors. A copied mode in a vector (`originated` `false`) is
//! folded with a stand-in `causation_id`, the agent-stream origin every copied mode names, which the
//! vector leaves out.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use common::{
    ACCOUNT_STREAM, CONTROL_STREAM, FixedInstruments, FixedMandate, Shell, TestIds, config, ports,
};
use mandate_canon::{Value, parse};
use mandate_executor::{
    AcknowledgmentHandoff, EventDraft, EventId, FoldedEvent, Input, Mode, Ports, RiskClock, Seq,
};
use mandate_time::UtcNanos;

const SPENDING: [&str; 4] = [
    "ApprovalResponseSubmitted",
    "OwnerCommandIssued",
    "OwnerAcknowledgmentRequested",
    "OwnerAcknowledged",
];

fn vectors() -> Vec<Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).expect("the fixture")).expect("canonical JSON");
    let vectors = list(
        fixture
            .get("reconciliation_acknowledgments")
            .expect("the section"),
        "executor",
    )
    .to_vec();
    assert!(
        vectors.len() >= 18,
        "{} vectors; never fewer than 18",
        vectors.len()
    );
    vectors
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

fn folded(envelope: &Value, stream: &str, seq: u64) -> FoldedEvent {
    let payload = envelope.get("payload").cloned().unwrap_or(Value::Null);
    let copied_mode = text(envelope, "event_type") == "AgentModeApplied"
        && payload.get("originated") == Some(&Value::Bool(false));
    FoldedEvent {
        stream: stream.to_owned(),
        seq: Seq(seq),
        event_id: EventId(text(envelope, "event_id").to_owned()),
        event_type: text(envelope, "event_type").to_owned(),
        causation_id: envelope
            .get("causation_id")
            .and_then(Value::as_str)
            .map(|id| EventId(id.to_owned()))
            .or_else(|| copied_mode.then(|| EventId("agent-stream-origin".to_owned()))),
        payload,
    }
}

fn second_of(envelope: &Value) -> RiskClock {
    let at = UtcNanos::parse(text(envelope, "event_time")).expect("a canonical event_time");
    RiskClock::from_secs(at.secs())
}

fn spent_before(control: &[Value], index: usize) -> BTreeSet<String> {
    control
        .iter()
        .take(index)
        .filter(|record| SPENDING.contains(&text(record, "event_type")))
        .filter_map(|record| {
            record
                .get("payload")
                .and_then(|p| p.get("step_up"))
                .and_then(|s| s.get("assertion_id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

fn handoff(vector: &Value, at: usize, copy: &Value) -> AcknowledgmentHandoff {
    let control = list(vector, "control");
    let original = &control[at];
    AcknowledgmentHandoff {
        original: folded(original, CONTROL_STREAM, u64::try_from(at + 1).unwrap_or(0)),
        schema_version: original
            .get("schema_version")
            .and_then(Value::as_int)
            .expect("a schema_version"),
        processed_at: second_of(copy),
        spent: spent_before(control, at),
        request_spent: None,
        independent_overlay: vector.get("overlay_independent") == Some(&Value::Bool(true)),
        methods: BTreeSet::from(["cli_confirm".to_owned()]),
    }
}

struct Copied {
    index: usize,
    original: Value,
    drafts: Vec<EventDraft>,
}

fn walk(vector: &Value, ports: &Ports<'_>) -> (Vec<Copied>, Shell) {
    let mut shell = Shell::new(1);
    shell
        .fold_one(&common::stream_opened())
        .expect("the stream opens");
    let mut shell = shell.restart_ready(ports);
    let control = list(vector, "control");
    let mut copies = Vec::new();
    for (index, record) in list(vector, "account").iter().enumerate() {
        let original = record
            .get("causation_id")
            .and_then(Value::as_str)
            .and_then(|id| control.iter().position(|r| text(r, "event_id") == id));
        let Some(at) = original else {
            let seq = shell.head().0.saturating_add(1);
            shell
                .fold_one(&folded(record, ACCOUNT_STREAM, seq))
                .unwrap_or_else(|e| {
                    panic!("{}: folding record {index}: {e}", text(vector, "name"))
                });
            continue;
        };
        shell.run(Input::Tick(second_of(record)), ports);
        let ran = shell.run(Input::Acknowledgment(handoff(vector, at, record)), ports);
        copies.push(Copied {
            index,
            original: control[at].clone(),
            drafts: ran.drafts,
        });
    }
    (copies, shell)
}

macro_rules! with_ports {
    ($ports:ident) => {
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
        let instruments = FixedInstruments;
        let config = config();
        let $ports = ports(&ids, &mandates, &instruments, &config);
    };
}

fn member<'a>(draft: &'a EventDraft, name: &str) -> Option<&'a Value> {
    draft.payload.get(name)
}

/// Each acknowledgment copy's batch is exactly as its vector says: a lift is the copy, then the
/// hold's `AgentModeApplied` to `normal` under its restriction, for its agent, `originated`, caused
/// by the copy; a refusal is the copy, then `OwnerCommandRefused` with the vector's reason.
#[test]
fn each_acknowledgment_of_a_hold_is_judged_and_lifted_as_its_vector_says() {
    with_ports!(ports);
    let mut failed = Vec::new();
    for vector in vectors() {
        let want: BTreeMap<usize, &Value> = list(&vector, "expect")
            .iter()
            .map(|e| {
                let index = e
                    .get("index")
                    .and_then(Value::as_int)
                    .and_then(|i| usize::try_from(i).ok())
                    .expect("an index");
                (index, e)
            })
            .collect();
        let copies = walk(&vector, &ports).0;
        let judged: BTreeSet<usize> = copies
            .iter()
            .filter(|c| text(&c.original, "event_type") == "OwnerAcknowledged")
            .map(|c| c.index)
            .collect();
        if judged != want.keys().copied().collect() {
            failed.push(format!(
                "{}: copies {judged:?} against {:?}",
                text(&vector, "name"),
                want.keys()
            ));
        }
        for copied in copies
            .iter()
            .filter(|c| text(&c.original, "event_type") == "OwnerAcknowledged")
        {
            let Some(expect) = want.get(&copied.index) else {
                continue;
            };
            let drafts = &copied.drafts;
            let copy = drafts.first();
            let copy_right = copy.is_some_and(|d| {
                d.event_type == "OwnerAcknowledged"
                    && Some(&d.payload) == copied.original.get("payload")
                    && d.causation_id.as_ref().map(|id| id.0.as_str())
                        == Some(text(&copied.original, "event_id"))
            });
            let outcome = text(expect, "outcome");
            let rest_right = drafts.len() == 2
                && if outcome == "lift" {
                    let lift = &drafts[1];
                    lift.event_type == "AgentModeApplied"
                        && member(lift, "agent").and_then(Value::as_str)
                            == Some(text(expect, "agent"))
                        && member(lift, "to").and_then(Value::as_str) == Some("normal")
                        && member(lift, "restriction").and_then(Value::as_str)
                            == Some(text(expect, "restriction"))
                        && member(lift, "originated") == Some(&Value::Bool(true))
                        && lift.causation_id == copy.map(|c| c.event_id.clone())
                } else {
                    let refusal = &drafts[1];
                    refusal.event_type == "OwnerCommandRefused"
                        && member(refusal, "reason").and_then(Value::as_str) == Some(outcome)
                };
            if !(copy_right && rest_right) {
                let got: Vec<_> = drafts
                    .iter()
                    .map(|d| (d.event_type.clone(), d.payload.clone()))
                    .collect();
                failed.push(format!(
                    "{} copy {}: expected {outcome}, got {got:?}",
                    text(&vector, "name"),
                    copied.index
                ));
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A lift hands each agent back to the strictest restriction it still holds: after the incomplete
/// hold is lifted every agent trades normally, and where a second hold stands it still holds.
#[test]
fn a_lift_leaves_every_other_hold_in_force() {
    with_ports!(ports);
    for (name, mode) in [
        ("the_incomplete_hold_lifts", Mode::Normal),
        ("one_hold_of_two_lifts", Mode::ExitsOnly),
    ] {
        let vector = vectors()
            .into_iter()
            .find(|v| text(v, "name") == name)
            .expect("the vector");
        let shell = walk(&vector, &ports).1;
        assert_eq!(
            shell.state.effective_mode(&common::agent(common::AGENT)),
            mode,
            "{name}"
        );
    }
}
