//! E7-9 (DEC-168, DEC-177, DEC-252): the journal vectors' `agent_stream` section, family by family.
//! Each test runs every reference case of one §9.1 event type, or the batches, or the range checks,
//! and each family holds at least one event the journal must accept and one it must refuse, so
//! neither a journal that accepts every draft nor one that refuses every draft passes any of them.
//! The cases themselves are `journal::agent_stream::*` in the `refcases` binary.

use std::path::Path;
use std::sync::Arc;

use mandate_refcases::{Json, journal, read_fixture};
use serde_json::json;

fn fixture() -> Json {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
    Arc::unwrap_or_clone(read_fixture(&dir, "journal.json").unwrap())
}

/// Runs every case whose ID starts with `journal::agent_stream::{family}::` and returns each
/// failure as `id: message`, after checking the family holds a case that must be accepted and one
/// that must be refused.
fn failures(fixture: Json, family: &str, accepting: &str, refusing: &str) -> Vec<String> {
    let prefix = format!("journal::agent_stream::{family}::");
    let cases: Vec<_> = journal::cases(&Arc::new(fixture))
        .into_iter()
        .filter(|c| c.id.starts_with(&prefix))
        .collect();
    for kind in [accepting, refusing] {
        assert!(
            cases.iter().any(|c| c.id.contains(kind)),
            "{family} has no `{kind}` case"
        );
    }
    cases
        .into_iter()
        .filter_map(|case| (case.run)().err().map(|e| format!("{}: {e}", case.id)))
        .collect()
}

fn assert_family(family: &str, accepting: &str, refusing: &str) {
    let failed = failures(fixture(), family, accepting, refusing);
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
#[ignore = "pending E7-9"]
fn the_agent_stream_opens_with_its_own_subject() {
    assert_family("StreamOpened", "::chain::", "::invalid::");
}

#[test]
#[ignore = "pending E7-9"]
fn an_observation_references_its_data_and_never_carries_it() {
    assert_family("ObservationRecorded", "::chain::", "::invalid::");
}

#[test]
#[ignore = "pending E7-9"]
fn a_model_output_is_recorded_as_given() {
    assert_family("ModelOutputRecorded", "::chain::", "::invalid::");
}

#[test]
#[ignore = "pending E7-9"]
fn a_decision_records_its_evaluation_its_gate_and_who_decided() {
    assert_family("DecisionMade", "::valid::", "::invalid::");
}

#[test]
#[ignore = "pending E7-9"]
fn an_intent_is_long_only_and_names_its_cause() {
    assert_family("IntentProposed", "::chain::", "::invalid::");
}

#[test]
#[ignore = "pending E7-9"]
fn a_mode_change_is_never_looser_than_the_owner_set() {
    assert_family("AgentModeChanged", "::valid::", "::invalid::");
}

#[test]
#[ignore = "pending E7-9"]
fn a_kill_switch_names_its_own_subject_and_an_owner_switch_its_command() {
    assert_family("KillSwitchActivated", "::valid::", "::invalid::");
}

#[test]
#[ignore = "pending E7-9"]
fn an_owner_exit_records_the_owner_and_step_up_whatever_the_bid() {
    assert_family("OwnerExitRequested", "::valid::", "::invalid::");
}

#[test]
#[ignore = "pending E7-9"]
fn a_batch_is_refused_when_an_intent_differs_from_its_decision() {
    assert_family(
        "batch",
        "decision_and_its_intent",
        "differs_from_its_decision",
    );
}

#[test]
#[ignore = "pending E7-9"]
fn the_chain_verifies_and_each_tampered_range_fails_its_own_check() {
    assert_family("chain", "::append", "::append");
    assert_family(
        "range",
        "intent_repeats_another_action",
        "kill_switch_names_no_event_on_the_full_chain",
    );
}

/// The case named `name` in `list` of the section, edited.
fn edited(list: &str, name: &str, edit: impl FnOnce(&mut Json)) -> Json {
    let mut fixture = fixture();
    let case = fixture["agent_stream"][list]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["name"] == name)
        .unwrap();
    edit(case);
    fixture
}

fn run(fixture: Json, id: &str) -> Result<(), String> {
    let case = journal::cases(&Arc::new(fixture))
        .into_iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("no case {id}"));
    (case.run)()
}

/// The harness reads every key of every expectation: each case passes as generated, and fails
/// once its expected reason, path, draft index, outcome, range code, or range seq is changed.
#[test]
#[ignore = "pending E7-9"]
fn every_expectation_key_is_read() {
    let floor = "journal::agent_stream::OwnerExitRequested::invalid::owner_exit_floor_absent";
    let pause = "journal::agent_stream::AgentModeChanged::valid::mode_owner_pause";
    let qty = "journal::agent_stream::batch::intent_quantity_differs_from_its_decision";
    let range = "journal::agent_stream::range::kill_switch_names_an_owner_pause";
    for id in [floor, pause, qty, range] {
        assert_eq!(run(fixture(), id), Ok(()), "{id} as generated");
    }

    let wrong = |list: &str, name: &str, key: &str, value: Json| {
        edited(list, name, |c| c["expect"][key] = value)
    };
    let floor_name = "owner_exit_floor_absent";
    assert_eq!(
        run(
            wrong(
                "invalid_drafts",
                floor_name,
                "reason",
                json!("non_canonical")
            ),
            floor
        ),
        Err(
            "refusal (draft, reason, path): expected (0, \"non_canonical\", \"payload.floor\"), \
             got (0, \"schema\", \"payload.floor\")"
                .to_owned()
        )
    );
    assert_eq!(
        run(
            wrong("invalid_drafts", floor_name, "path", json!("payload.bid")),
            floor
        ),
        Err(
            "refusal (draft, reason, path): expected (0, \"schema\", \"payload.bid\"), \
             got (0, \"schema\", \"payload.floor\")"
                .to_owned()
        )
    );
    assert_eq!(
        run(
            wrong(
                "invalid_batches",
                "intent_quantity_differs_from_its_decision",
                "draft_index",
                json!(0)
            ),
            qty
        ),
        Err(
            "refusal (draft, reason, path): expected (0, \"schema\", \"payload.qty\"), \
             got (1, \"schema\", \"payload.qty\")"
                .to_owned()
        )
    );
    let no_cause = edited("valid_drafts", "mode_owner_pause", |c| {
        c["changes"]
            .as_array_mut()
            .unwrap()
            .retain(|ch| ch["path"] != "causation_id");
    });
    let refused = run(no_cause, pause).unwrap_err();
    assert!(
        refused.starts_with("expected Valid, got Invalid"),
        "{refused}"
    );
    assert!(refused.contains("\"causation_id\""), "{refused}");
    assert_eq!(
        run(
            wrong(
                "range_verification",
                "kill_switch_names_an_owner_pause",
                "code",
                json!("intent_action_mismatch")
            ),
            range
        ),
        Err(
            "range failure (seq, code): expected (13, \"intent_action_mismatch\"), \
             got (13, \"mode_event_mismatch\")"
                .to_owned()
        )
    );
    assert_eq!(
        run(
            wrong(
                "range_verification",
                "kill_switch_names_an_owner_pause",
                "seq",
                json!(11)
            ),
            range
        ),
        Err(
            "range failure (seq, code): expected (11, \"mode_event_mismatch\"), \
             got (13, \"mode_event_mismatch\")"
                .to_owned()
        )
    );
}

/// Rule 10 compares decimals by value (§4.6): an intent that writes its decision's quantity and
/// limit with trailing zeros repeats the decision, and one a hundredth off does not.
#[test]
#[ignore = "pending E7-9"]
fn an_intent_is_compared_with_its_decision_by_value() {
    let id = "journal::agent_stream::batch::decision_and_its_intent";
    let with_intent_changes = |changes: Json| {
        edited("valid_batches", "decision_and_its_intent", |c| {
            c["drafts"][1]["changes"] = changes;
        })
    };
    let same_value = with_intent_changes(json!([
        {"path": "payload.qty", "value": "10.00"},
        {"path": "payload.limit_price", "value": "150.0"}
    ]));
    assert_eq!(run(same_value, id), Ok(()));

    let mut other_value = with_intent_changes(json!([{"path": "payload.qty", "value": "10.01"}]));
    other_value["agent_stream"]["valid_batches"][0]["expect"] = json!({
        "outcome": "Invalid", "reason": "schema", "path": "payload.qty", "draft_index": 1
    });
    assert_eq!(run(other_value, id), Ok(()));
}

/// §11: a `mode_event` naming no earlier event fails a full-chain run, and on a range whose trusted
/// start is after the event it names, the reference is left to the full-chain run and the range
/// verifies.
#[test]
#[ignore = "pending E7-9"]
fn a_reference_before_the_range_is_left_to_the_full_chain() {
    let name = "kill_switch_names_no_event_on_the_full_chain";
    let id = format!("journal::agent_stream::range::{name}");
    assert_eq!(run(fixture(), &id), Ok(()));
    let from_the_switch = edited("range_verification", name, |c| c["from_seq"] = json!(12));
    assert_eq!(
        run(from_the_switch, &id),
        Err("expected (13, \"mode_event_mismatch\"), but the range verified".to_owned())
    );
}
