//! E7-4 slice 5, the acknowledgment copy (journal spec §9.28, DEC-2828): the executor copies each
//! owner's `OwnerAcknowledged` and `OwnerAcknowledgmentRequested` from the control stream into the
//! account stream, the original's payload unchanged and caused by it, and judges an acknowledgment
//! copy by §9.28's four checks. A copy that fails one is followed, as the very next record of its
//! batch, by `OwnerCommandRefused` with `command` `acknowledge` and the reason, so the risk book's
//! row-by-row fold decides a copy with one row of lookahead (lane RK's R2b). A request copy is
//! never judged by itself.
//!
//! The cases are read from the committed fixture (`fixtures/refcases/journal.json`, the
//! `acknowledgments` section's `executor` vectors, generated from
//! `docs/specs/reference-cases/journal.yaml`), never transcribed. Each vector's account stream is
//! walked in order: a record the executor did not copy is folded as already on the stream, and a
//! copy is produced by handing the executor its control-stream original at the copy's time, with
//! the assertions the control stream spent before it, the overlay's requirement and the methods the
//! environment allows, as the shell computes them.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use common::{
    ACCOUNT_STREAM, CONTROL_STREAM, FixedInstruments, FixedMandate, Shell, TestIds, config, ports,
};
use mandate_canon::{Value, parse};
use mandate_executor::{
    AcknowledgmentHandoff, EventDraft, EventId, FoldedEvent, Input, RiskClock, Seq,
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

/// The whole second an envelope's `event_time` names, the risk clock the executor processes it at.
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

/// What one vector's walk saw for each copy: its index in the vector's account stream, and the
/// drafts handing its original produced.
struct Walked {
    copies: Vec<(usize, Value, Vec<EventDraft>)>,
}

/// Walks one vector's account stream through a started executor, as the module comment says.
fn walk(vector: &Value) -> Walked {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    walked(vector, &ports).0
}

/// The shell after one vector's walk, and every handoff the walk made, in order.
fn walked_shell(
    vector: &Value,
    ports: &mandate_executor::Ports<'_>,
) -> (Shell, Vec<AcknowledgmentHandoff>) {
    let (_, shell, handed) = walked(vector, ports);
    (shell, handed)
}

fn walked(
    vector: &Value,
    ports: &mandate_executor::Ports<'_>,
) -> (Walked, Shell, Vec<AcknowledgmentHandoff>) {
    let mut shell = Shell::new(1);
    shell
        .fold_one(&common::stream_opened())
        .expect("the stream opens");
    let mut shell = shell.restart_ready(ports);
    let control = list(vector, "control");
    let originals: BTreeMap<&str, usize> = control
        .iter()
        .enumerate()
        .map(|(i, record)| (text(record, "event_id"), i))
        .collect();
    let overlay = vector.get("overlay_independent") == Some(&Value::Bool(true));
    let mut copies = Vec::new();
    let mut handed = Vec::new();
    for (index, record) in list(vector, "account").iter().enumerate() {
        let original = record
            .get("causation_id")
            .and_then(Value::as_str)
            .and_then(|id| originals.get(id).copied());
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
        let handoff = AcknowledgmentHandoff {
            original: folded(
                &control[at],
                CONTROL_STREAM,
                u64::try_from(at + 1).unwrap_or(0),
            ),
            spent: spent_before(control, at),
            independent_overlay: overlay,
            methods: methods(vector),
        };
        handed.push(handoff.clone());
        let ran = shell.run(Input::Acknowledgment(handoff), ports);
        copies.push((index, control[at].clone(), ran.drafts));
    }
    (Walked { copies }, shell, handed)
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

/// Every copy is the original's payload unchanged, of the original's type, caused by the original,
/// and the first record of its batch.
#[test]
#[ignore = "pending E7-4"]
fn every_copy_is_its_original_unchanged_and_caused_by_it() {
    let mut failed = Vec::new();
    for vector in vectors() {
        for (index, original, drafts) in walk(&vector).copies {
            let first = drafts.first();
            let same = first.is_some_and(|copy| {
                copy.event_type == text(&original, "event_type")
                    && Some(&copy.payload) == original.get("payload")
                    && copy.causation_id.as_ref().map(|id| id.0.as_str())
                        == Some(text(&original, "event_id"))
            });
            if !same {
                failed.push(format!("{} copy {index}: {first:?}", text(&vector, "name")));
            }
        }
    }
    assert!(
        failed.is_empty(),
        "copies that are not their originals: {failed:#?}"
    );
}

/// Each acknowledgment copy is judged as its vector says: a lift is the copy alone, with no
/// refusal in its batch; a refusal is `OwnerCommandRefused` with `command` `acknowledge`, the
/// vector's reason and the original as `causation_id`, as the very next record after the copy.
#[test]
#[ignore = "pending E7-4"]
fn each_acknowledgment_copy_is_judged_as_its_vector_says() {
    let mut failed = Vec::new();
    for vector in vectors() {
        let want = expected(&vector);
        for (index, original, drafts) in walk(&vector).copies {
            if text(&original, "event_type") != "OwnerAcknowledged" {
                continue;
            }
            let Some(outcome) = want.get(&index) else {
                failed.push(format!(
                    "{} copy {index}: no expectation",
                    text(&vector, "name")
                ));
                continue;
            };
            let refusals: Vec<&EventDraft> = drafts
                .iter()
                .filter(|d| d.event_type == "OwnerCommandRefused")
                .collect();
            let judged = if outcome == "lift" {
                refusals.is_empty()
            } else {
                drafts.get(1).is_some_and(|next| {
                    next.event_type == "OwnerCommandRefused"
                        && next.payload.get("command").and_then(Value::as_str)
                            == Some("acknowledge")
                        && next.payload.get("reason").and_then(Value::as_str)
                            == Some(outcome.as_str())
                        && next.causation_id.as_ref().map(|id| id.0.as_str())
                            == Some(text(&original, "event_id"))
                }) && refusals.len() == 1
            };
            if !judged {
                let got: Vec<_> = drafts
                    .iter()
                    .map(|d| (d.event_type.clone(), d.payload.get("reason").cloned()))
                    .collect();
                failed.push(format!(
                    "{} copy {index}: expected {outcome}, got {got:?}",
                    text(&vector, "name")
                ));
            }
        }
    }
    assert!(
        failed.is_empty(),
        "acknowledgments judged otherwise: {failed:#?}"
    );
}

/// A request copy is never judged by itself: its batch holds the copy alone.
#[test]
#[ignore = "pending E7-4"]
fn a_request_copy_is_never_followed_by_a_refusal() {
    let mut failed = Vec::new();
    for vector in vectors() {
        for (index, original, drafts) in walk(&vector).copies {
            if text(&original, "event_type") == "OwnerAcknowledgmentRequested"
                && drafts.iter().any(|d| d.event_type == "OwnerCommandRefused")
            {
                failed.push(format!("{} request copy {index}", text(&vector, "name")));
            }
        }
    }
    assert!(
        failed.is_empty(),
        "request copies followed by a refusal: {failed:#?}"
    );
}

/// An original already copied is never copied again: handing it a second time drafts nothing, so
/// a shell that hands it again after a restart cannot judge it twice.
#[test]
#[ignore = "pending E7-4"]
fn an_original_already_copied_is_not_copied_again() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let vector = vectors()
        .into_iter()
        .find(|v| text(v, "name") == "plain_lifts")
        .expect("the plain_lifts vector");
    let mut shell = Shell::new(1);
    shell
        .fold_one(&common::stream_opened())
        .expect("the stream opens");
    let mut shell = shell.restart_ready(&ports);
    let control = list(&vector, "control");
    let account = list(&vector, "account");
    shell
        .fold_one(&folded(
            &account[0],
            ACCOUNT_STREAM,
            shell.head().0.saturating_add(1),
        ))
        .expect("the trigger folds");
    shell.run(Input::Tick(second_of(&account[1])), &ports);
    let handoff = AcknowledgmentHandoff {
        original: folded(&control[0], CONTROL_STREAM, 1),
        spent: BTreeSet::new(),
        independent_overlay: false,
        methods: methods(&vector),
    };
    let first = shell.run(Input::Acknowledgment(handoff.clone()), &ports);
    assert_eq!(
        first.drafts.len(),
        1,
        "the first hand copies it: {:?}",
        first.drafts
    );
    let again = shell.run(Input::Acknowledgment(handoff), &ports);
    assert!(
        again.drafts.is_empty(),
        "a second hand copies nothing: {:?}",
        again.drafts
    );
}

/// After a restart, which folds the account stream alone, no original the walk copied is copied
/// again: the fold reads each copy as journaled, so a shell that hands the control stream again
/// from its start judges nothing twice.
#[test]
#[ignore = "pending E7-4"]
fn after_a_restart_no_original_is_copied_again() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut failed = Vec::new();
    for vector in vectors() {
        let (shell, handed) = walked_shell(&vector, &ports);
        let mut restarted = shell.restart_ready(&ports);
        for handoff in handed {
            let again = restarted.run(Input::Acknowledgment(handoff), &ports);
            if !again.drafts.is_empty() {
                failed.push(format!("{}: {:?}", text(&vector, "name"), again.drafts));
            }
        }
    }
    assert!(
        failed.is_empty(),
        "originals copied again after a restart: {failed:#?}"
    );
}
