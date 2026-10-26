//! E7-4 slice 5, the acknowledgment copy (journal spec §9.28, DEC-2828): the executor copies each
//! owner's `OwnerAcknowledged` and `OwnerAcknowledgmentRequested` from the control stream into the
//! account stream, the original's payload and version unchanged and caused by it, and judges an
//! acknowledgment copy by §9.28's four checks. A copy that fails one is followed, as the very next
//! record of its batch, by `OwnerCommandRefused` with `command` `acknowledge`, the reason and the
//! processing time, so the risk book's row-by-row fold decides a copy with one row of lookahead
//! (lane RK's R2b). A request copy is never judged by itself.
//!
//! The cases are read from the committed fixture (`fixtures/refcases/journal.json`, the
//! `acknowledgments` section's `executor` vectors, generated from
//! `docs/specs/reference-cases/journal.yaml`), never transcribed. Each vector's account stream is
//! walked in order: a record whose `causation_id` names no control-stream record is folded as
//! already on the stream, and a copy is produced by handing the executor its control-stream
//! original at the copy's time, with what the shell computes from the control stream (DEC-2828
//! item 1). The vectors' account streams hold copies and never refusals, so no original is handed
//! twice. A record the vector lists in `refused_at_append` (rule 239) is handed all the same: the
//! executor must fail closed on it, as the reference's judgment does.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use common::{
    ACCOUNT_STREAM, CONTROL_STREAM, FixedInstruments, FixedMandate, Shell, TestIds, config, ports,
};
use mandate_canon::{Value, parse};
use mandate_executor::{
    AcknowledgmentHandoff, EventDraft, EventId, FoldedEvent, Input, Ports, RiskClock, Seq,
};
use mandate_time::UtcNanos;

/// §6.1's four record types whose step-up assertion a later one may not reuse.
const SPENDING: [&str; 4] = [
    "ApprovalResponseSubmitted",
    "OwnerCommandIssued",
    "OwnerAcknowledgmentRequested",
    "OwnerAcknowledged",
];

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let bytes = std::fs::read(path).expect("the committed journal fixture");
    let fixture = parse(&bytes).expect("the fixture is canonical JSON");
    fixture
        .get("acknowledgments")
        .cloned()
        .expect("the acknowledgments section")
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

fn name(vector: &Value) -> &str {
    text(vector, "name")
}

/// One envelope of a vector as the executor's fold reads it, on the harness's own stream.
fn folded(envelope: &Value, stream: &str, seq: u64) -> FoldedEvent {
    FoldedEvent {
        stream: stream.to_owned(),
        seq: Seq(seq),
        event_id: EventId(text(envelope, "event_id").to_owned()),
        event_type: text(envelope, "event_type").to_owned(),
        causation_id: envelope
            .get("causation_id")
            .and_then(Value::as_str)
            .map(|id| EventId(id.to_owned())),
        payload: envelope.get("payload").cloned().unwrap_or(Value::Null),
    }
}

/// The whole second an envelope's `event_time` names, the time the executor processes it at.
fn second_of(envelope: &Value) -> RiskClock {
    let at = UtcNanos::parse(text(envelope, "event_time")).expect("a canonical event_time");
    RiskClock::from_secs(at.secs())
}

/// The assertions §6.1's four record types spent on the control stream before `index`.
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

/// Mandate spec §6.1's methods: `cli_confirm` in paper, and in live only on the founder's own
/// connection (DEC-529 item 3).
fn methods(vector: &Value) -> BTreeSet<String> {
    let paper = text(vector, "environment") == "paper";
    let founder = vector.get("founder_connection") == Some(&Value::Bool(true));
    if paper || founder {
        BTreeSet::from(["cli_confirm".to_owned()])
    } else {
        BTreeSet::new()
    }
}

/// What the shell hands for control record `at`, copied as account record `copy` (DEC-2828 item 1).
fn handoff(vector: &Value, at: usize, copy: &Value) -> AcknowledgmentHandoff {
    let control = list(vector, "control");
    let original = &control[at];
    let request = original
        .get("payload")
        .and_then(|p| p.get("request"))
        .and_then(Value::as_str);
    let request_spent = request
        .and_then(|id| control.iter().position(|r| text(r, "event_id") == id))
        .map(|position| spent_before(control, position));
    AcknowledgmentHandoff {
        original: folded(original, CONTROL_STREAM, u64::try_from(at + 1).unwrap_or(0)),
        schema_version: original
            .get("schema_version")
            .and_then(Value::as_int)
            .expect("a schema_version"),
        processed_at: second_of(copy),
        spent: spent_before(control, at),
        request_spent,
        independent_overlay: vector.get("overlay_independent") == Some(&Value::Bool(true)),
        methods: methods(vector),
    }
}

/// The control record each account record copies, by its `causation_id`, if it is a copy.
fn original_of(vector: &Value, record: &Value) -> Option<usize> {
    let id = record.get("causation_id").and_then(Value::as_str)?;
    list(vector, "control")
        .iter()
        .position(|r| text(r, "event_id") == id)
}

fn new_shell(ports: &Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell
        .fold_one(&common::stream_opened())
        .expect("the stream opens");
    shell.restart_ready(ports)
}

/// One copy the walk made: its index in the vector's account stream, the control original, the
/// copy the vector expects, and the drafts handing the original produced.
struct Copied {
    index: usize,
    original: Value,
    expected_copy: Value,
    drafts: Vec<EventDraft>,
}

/// Walks one vector's account stream as the module comment says. With `restart_before_each`, the
/// executor restarts from its account stream before each handoff, as after a crash between two
/// copies.
fn walk(vector: &Value, ports: &Ports<'_>, restart_before_each: bool) -> (Vec<Copied>, Shell) {
    let mut shell = new_shell(ports);
    let mut copies = Vec::new();
    for (index, record) in list(vector, "account").iter().enumerate() {
        let Some(at) = original_of(vector, record) else {
            let seq = shell.head().0.saturating_add(1);
            shell
                .fold_one(&folded(record, ACCOUNT_STREAM, seq))
                .unwrap_or_else(|e| panic!("{}: folding record {index}: {e}", name(vector)));
            continue;
        };
        if restart_before_each {
            shell = shell.restart_ready(ports);
        }
        shell.run(Input::Tick(second_of(record)), ports);
        let ran = shell.run(Input::Acknowledgment(handoff(vector, at, record)), ports);
        copies.push(Copied {
            index,
            original: list(vector, "control")[at].clone(),
            expected_copy: record.clone(),
            drafts: ran.drafts,
        });
    }
    (copies, shell)
}

/// The outcome each vector expects at an account-stream index: `lift`, or a refusal's reason.
fn expected(vector: &Value) -> BTreeMap<usize, String> {
    list(vector, "expect")
        .iter()
        .map(|e| {
            let index = e
                .get("index")
                .and_then(Value::as_int)
                .and_then(|i| usize::try_from(i).ok())
                .expect("an expectation's index");
            (index, text(e, "outcome").to_owned())
        })
        .collect()
}

fn vectors() -> Vec<Value> {
    let section = section();
    let vectors = list(&section, "executor").to_vec();
    assert!(
        vectors.len() >= 51,
        "{} executor vectors; never fewer than 51",
        vectors.len()
    );
    vectors
}

/// Whether `draft` is the copy of `original`: its type, version and payload unchanged, caused by
/// it.
fn is_copy_of(draft: &EventDraft, original: &Value) -> bool {
    draft.event_type == text(original, "event_type")
        && Some(draft.schema_version) == original.get("schema_version").and_then(Value::as_int)
        && Some(&draft.payload) == original.get("payload")
        && draft.causation_id.as_ref().map(|id| id.0.as_str()) == Some(text(original, "event_id"))
}

/// Whether `draft` is §9.28's refusal of `original`, for `reason`, at the copy's processing time.
fn is_refusal_of(draft: &EventDraft, original: &Value, reason: &str, at: &str) -> bool {
    let member = |name: &str| draft.payload.get(name).and_then(Value::as_str);
    draft.event_type == "OwnerCommandRefused"
        && member("command") == Some("acknowledge")
        && member("reason") == Some(reason)
        && member("effective_at") == Some(at)
        && draft.causation_id.as_ref().map(|id| id.0.as_str()) == Some(text(original, "event_id"))
}

/// The ports every walk runs under.
macro_rules! with_ports {
    ($ports:ident) => {
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
        let instruments = FixedInstruments;
        let config = config();
        let $ports = ports(&ids, &mandates, &instruments, &config);
    };
}

/// Every copy is its original's type, version and payload unchanged, caused by the original, and
/// the first record of its batch.
#[test]
#[ignore = "pending E7-4"]
fn every_copy_is_its_original_unchanged_and_caused_by_it() {
    with_ports!(ports);
    let mut failed = Vec::new();
    for vector in vectors() {
        for copied in walk(&vector, &ports, false).0 {
            let first = copied.drafts.first();
            if !first.is_some_and(|draft| is_copy_of(draft, &copied.original)) {
                failed.push(format!(
                    "{} copy {}: {first:?}",
                    name(&vector),
                    copied.index
                ));
            }
        }
    }
    assert!(
        failed.is_empty(),
        "copies that are not their originals: {failed:#?}"
    );
}

/// Each acknowledgment copy's batch is exactly as its vector says: a lift is the copy alone; a
/// refusal is the copy and then `OwnerCommandRefused` (`acknowledge`, the vector's reason, the copy's
/// time as `effective_at`, the original as `causation_id`), and nothing else. Every expectation
/// names an acknowledgment copy, and every acknowledgment copy has one.
#[test]
#[ignore = "pending E7-4"]
fn each_acknowledgment_copy_is_judged_as_its_vector_says() {
    with_ports!(ports);
    let mut failed = Vec::new();
    for vector in vectors() {
        let want = expected(&vector);
        let copies = walk(&vector, &ports, false).0;
        let judged: BTreeSet<usize> = copies
            .iter()
            .filter(|c| text(&c.original, "event_type") == "OwnerAcknowledged")
            .map(|c| c.index)
            .collect();
        if judged != want.keys().copied().collect() {
            failed.push(format!(
                "{}: copies {judged:?}, expectations {:?}",
                name(&vector),
                want.keys()
            ));
        }
        for copied in copies
            .iter()
            .filter(|c| text(&c.original, "event_type") == "OwnerAcknowledged")
        {
            let Some(outcome) = want.get(&copied.index) else {
                continue;
            };
            let drafts = &copied.drafts;
            let copy_first = drafts
                .first()
                .is_some_and(|d| is_copy_of(d, &copied.original));
            let rest_right = if outcome == "lift" {
                drafts.len() == 1
            } else {
                drafts.len() == 2
                    && is_refusal_of(
                        &drafts[1],
                        &copied.original,
                        outcome,
                        text(&copied.expected_copy, "event_time"),
                    )
            };
            if !(copy_first && rest_right) {
                let got: Vec<_> = drafts
                    .iter()
                    .map(|d| (d.event_type.clone(), d.payload.get("reason").cloned()))
                    .collect();
                failed.push(format!(
                    "{} copy {}: expected {outcome}, got {got:?}",
                    name(&vector),
                    copied.index
                ));
            }
        }
    }
    assert!(
        failed.is_empty(),
        "acknowledgments judged otherwise: {failed:#?}"
    );
}

/// A request copy is never judged by itself: its batch is the copy alone.
#[test]
#[ignore = "pending E7-4"]
fn a_request_copy_is_the_copy_alone() {
    with_ports!(ports);
    let mut failed = Vec::new();
    for vector in vectors() {
        for copied in walk(&vector, &ports, false).0 {
            if text(&copied.original, "event_type") == "OwnerAcknowledgmentRequested"
                && copied.drafts.len() != 1
            {
                failed.push(format!(
                    "{} request copy {}: {:?}",
                    name(&vector),
                    copied.index,
                    copied.drafts
                ));
            }
        }
    }
    assert!(
        failed.is_empty(),
        "request copies with more than the copy: {failed:#?}"
    );
}

/// A restart between any two copies changes no judgment: everything §9.28's checks read is either
/// on the account stream or in the handoff, so a walk that restarts before every handoff judges
/// every acknowledgment exactly as one that never restarts. A request's own reuse check is judged
/// from the handoff's `request_spent`, never from memory of the request's copy.
#[test]
#[ignore = "pending E7-4"]
fn a_restart_between_copies_changes_no_judgment() {
    with_ports!(ports);
    let mut failed = Vec::new();
    for vector in vectors() {
        let steady = walk(&vector, &ports, false).0;
        let restarted = walk(&vector, &ports, true).0;
        for (a, b) in steady.iter().zip(&restarted) {
            let shape = |c: &Copied| {
                c.drafts
                    .iter()
                    .map(|d| (d.event_type.clone(), d.payload.clone()))
                    .collect::<Vec<_>>()
            };
            if shape(a) != shape(b) {
                failed.push(format!("{} copy {}", name(&vector), a.index));
            }
        }
    }
    assert!(
        failed.is_empty(),
        "judgments a restart changed: {failed:#?}"
    );
}

/// An original already copied is never copied again, live or after a restart that folds the
/// account stream alone: handing it again drafts nothing, so a shell that hands the control stream
/// again from its start judges nothing twice.
#[test]
#[ignore = "pending E7-4"]
fn an_original_already_copied_is_not_copied_again() {
    with_ports!(ports);
    let mut failed = Vec::new();
    for vector in vectors() {
        let (_, mut shell) = walk(&vector, &ports, false);
        let account = list(&vector, "account");
        let handed: Vec<AcknowledgmentHandoff> = account
            .iter()
            .filter_map(|record| {
                original_of(&vector, record).map(|at| handoff(&vector, at, record))
            })
            .collect();
        for handoff in &handed {
            let again = shell.run(Input::Acknowledgment(handoff.clone()), &ports);
            if !again.drafts.is_empty() {
                failed.push(format!("{} live: {:?}", name(&vector), again.drafts));
            }
        }
        let mut restarted = shell.restart_ready(&ports);
        for handoff in handed {
            let again = restarted.run(Input::Acknowledgment(handoff), &ports);
            if !again.drafts.is_empty() {
                failed.push(format!("{} restarted: {:?}", name(&vector), again.drafts));
            }
        }
    }
    assert!(failed.is_empty(), "originals copied again: {failed:#?}");
}
