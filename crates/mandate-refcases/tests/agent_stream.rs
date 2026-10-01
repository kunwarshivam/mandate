//! E7-9 (DEC-168, DEC-177, DEC-252): the journal vectors' `agent_stream` section, family by family.
//! Each test runs every reference case of one §9.1 event type, or the batches, or the range checks,
//! and each family holds at least one event the journal must accept and one it must refuse, so
//! neither a journal that accepts every draft nor one that refuses every draft passes any of them.
//! The cases themselves are `journal::agent_stream::*` in the `refcases` binary.

use std::path::Path;
use std::sync::Arc;

use Shape::{Boolean, Integer, List, Record, Text};
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

/// How a §9.1 member is written, and so which value is off its type: every string-shaped type
/// (`text`, `id`, `ulid`, `decimal`, `timestamp`, `ref`, a value list) refuses a number as
/// `schema`, `integer` and `boolean` refuse a string, and a list or a record refuses a scalar.
#[derive(Clone, Copy)]
enum Shape {
    Text,
    Integer,
    Boolean,
    List,
    Record,
}

/// An event type, the chain seq its sweep starts from, and its members in §9.1's order.
type Schema = (&'static str, u64, &'static [(&'static str, Shape)]);

/// Journal spec v0.6 §9.1's eight tables, member by member in the order given, and the chain event
/// each sweep starts from (one whose optional lists and records are filled in).
const SCHEMAS: [Schema; 8] = [
    (
        "StreamOpened",
        1,
        &[
            ("stream_type", Text),
            ("workspace_id", Text),
            ("agent_id", Text),
        ],
    ),
    (
        "ObservationRecorded",
        2,
        &[
            ("source", Text),
            ("instrument_id", Text),
            ("as_of", Text),
            ("data_ref", Text),
        ],
    ),
    (
        "ModelOutputRecorded",
        3,
        &[
            ("model_id", Text),
            ("model_version", Text),
            ("content_hash", Text),
            ("instrument_id", Text),
            ("as_of", Text),
            ("expires_at", Text),
            ("direction", Text),
            ("conviction", Text),
            ("confidence", Text),
            ("horizon_s", Integer),
            ("thesis_ref", Text),
            ("evidence", List),
            ("invalidation", Text),
            ("thesis_id", Text),
            ("lineage_id", Text),
            ("ignored", Text),
        ],
    ),
    (
        "DecisionMade",
        4,
        &[
            ("instrument_id", Text),
            ("side", Text),
            ("type", Text),
            ("tif", Text),
            ("qty", Text),
            ("limit_price", Text),
            ("purpose", Text),
            ("exit_origin", Text),
            ("exit_conviction", Text),
            ("buy_conviction", Text),
            ("combined_score", Text),
            ("outputs_used", List),
            ("model_weights", List),
            ("clips_applied", List),
            ("dry_run", Text),
            ("reason_code", Text),
            ("autonomy", Text),
            ("ask_suppressed", Text),
            ("decided_by", Text),
            ("delegation_id", Text),
            ("requested_by", Text),
            ("client_id", Text),
        ],
    ),
    (
        "IntentProposed",
        5,
        &[
            ("instrument_id", Text),
            ("side", Text),
            ("type", Text),
            ("tif", Text),
            ("qty", Text),
            ("limit_price", Text),
            ("purpose", Text),
        ],
    ),
    (
        "AgentModeChanged",
        11,
        &[
            ("from", Text),
            ("to", Text),
            ("reason", Text),
            ("lifecycle", Text),
        ],
    ),
    (
        "KillSwitchActivated",
        13,
        &[
            ("scope", Text),
            ("subject", Text),
            ("initiator", Text),
            ("mode_event", Text),
        ],
    ),
    (
        "OwnerExitRequested",
        9,
        &[
            ("scope", Text),
            ("subject", Text),
            ("confirmed", Boolean),
            ("bid", Text),
            ("bid_size", Text),
            ("floor", Text),
            ("user", Text),
            ("step_up_status", Text),
            ("step_up", Record),
        ],
    ),
];

/// A value off `shape`'s type.
fn off_type(shape: Shape) -> Json {
    match shape {
        Text | List | Record => json!(7),
        Integer | Boolean => json!("7"),
    }
}

/// One generated invalid draft: its event type, base seq, changes, and the member it must be
/// refused at as `schema`.
struct Sweep {
    event_type: &'static str,
    name: String,
    base: u64,
    changes: Json,
    path: String,
}

/// The seqs of the chain events of `event_type`.
fn chain_seqs(event_type: &str) -> Vec<u64> {
    fixture()["agent_stream"]["chain"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["event_type"] == event_type)
        .map(|e| e["seq"].as_u64().unwrap())
        .collect()
}

/// Every member of every §9.1 schema set off its type on its base event, and deleted on every chain
/// event of its type, so a member is also deleted where `null` would be valid and no rule can
/// refuse the draft in its place; plus the members of the lists'
/// items and of `step_up`, each change alone on its base chain event.
fn sweep() -> Vec<Sweep> {
    let mut out = Vec::new();
    let mut add = |event_type: &'static str, base: u64, name: String, change: Json, path: &str| {
        out.push(Sweep {
            event_type,
            name: format!("sweep_{}_{name}", event_type.to_lowercase()),
            base,
            changes: json!([change]),
            path: format!("payload.{path}"),
        });
    };
    for (event_type, base, members) in SCHEMAS {
        for (member, shape) in members.iter().copied() {
            let path = format!("payload.{member}");
            add(
                event_type,
                base,
                format!("{member}_off_type"),
                json!({"path": path, "value": off_type(shape)}),
                member,
            );
            for seq in chain_seqs(event_type) {
                add(
                    event_type,
                    seq,
                    format!("{member}_absent_at_{seq}"),
                    json!({"path": path, "delete": true}),
                    member,
                );
            }
        }
    }
    let items = [
        (
            "ModelOutputRecorded",
            3,
            "evidence",
            json!([7]),
            "evidence[0]",
        ),
        (
            "DecisionMade",
            4,
            "outputs_used",
            json!([7]),
            "outputs_used[0]",
        ),
        (
            "DecisionMade",
            4,
            "clips_applied",
            json!([7]),
            "clips_applied[0]",
        ),
        (
            "DecisionMade",
            4,
            "model_weights",
            json!([7]),
            "model_weights[0]",
        ),
        (
            "DecisionMade",
            4,
            "model_weights",
            json!([{"key": 7, "value": "0.9"}]),
            "model_weights[0].key",
        ),
        (
            "DecisionMade",
            4,
            "model_weights",
            json!([{"key": "quant.mean_reversion", "value": 7}]),
            "model_weights[0].value",
        ),
        (
            "DecisionMade",
            4,
            "model_weights",
            json!([{"value": "0.9"}]),
            "model_weights[0].key",
        ),
        (
            "DecisionMade",
            4,
            "model_weights",
            json!([{"key": "quant.mean_reversion"}]),
            "model_weights[0].value",
        ),
    ];
    for (i, (event_type, base, member, value, path)) in items.into_iter().enumerate() {
        add(
            event_type,
            base,
            format!("{member}_item_{i}"),
            json!({"path": format!("payload.{member}"), "value": value}),
            path,
        );
    }
    for member in ["assertion_id", "authenticated_at", "method"] {
        let path = format!("payload.step_up.{member}");
        let at = format!("step_up.{member}");
        add(
            "OwnerExitRequested",
            9,
            format!("step_up_{member}_absent"),
            json!({"path": path, "delete": true}),
            &at,
        );
        add(
            "OwnerExitRequested",
            9,
            format!("step_up_{member}_off_type"),
            json!({"path": path, "value": 7}),
            &at,
        );
    }
    out
}

/// §9.1's type and presence layer: each member of each schema, deleted or set off its type, is
/// refused as `schema` at that member, and never read as `null` (§4.2). The member table is §9.1's,
/// written out here, and must name exactly the members of the chain event each sweep starts from.
#[test]
#[ignore = "pending E7-9"]
fn every_member_is_required_and_typed() {
    let mut fixture = fixture();
    for (event_type, base, members) in SCHEMAS {
        let mut listed: Vec<&str> = members.iter().map(|(m, _)| *m).collect();
        let chain = fixture["agent_stream"]["chain"].as_array().unwrap();
        let entry = chain.iter().find(|e| e["seq"] == base).unwrap();
        assert_eq!(entry["event_type"], event_type);
        let mut written: Vec<&str> = entry["body"]["payload"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        listed.sort_unstable();
        written.sort_unstable();
        assert_eq!(listed, written, "{event_type}'s members");
    }
    let sweeps = sweep();
    let members: usize = SCHEMAS.iter().map(|(_, _, m)| m.len()).sum();
    assert_eq!(members, 69);
    let deletions: usize = SCHEMAS
        .iter()
        .map(|(event_type, _, m)| m.len() * chain_seqs(event_type).len())
        .sum();
    assert_eq!(sweeps.len(), 69 + deletions + 8 + 6);
    let drafts = fixture["agent_stream"]["invalid_drafts"]
        .as_array_mut()
        .unwrap();
    for s in &sweeps {
        drafts.push(json!({
            "name": s.name, "clause": "§9.1 types", "base_seq": s.base, "changes": s.changes,
            "expect": {"outcome": "Invalid", "reason": "schema", "path": s.path},
        }));
    }
    let wanted: Vec<String> = sweeps
        .iter()
        .map(|s| {
            format!(
                "journal::agent_stream::{}::invalid::{}",
                s.event_type, s.name
            )
        })
        .collect();
    let cases: Vec<_> = journal::cases(&Arc::new(fixture))
        .into_iter()
        .filter(|c| wanted.contains(&c.id))
        .collect();
    assert_eq!(cases.len(), sweeps.len());
    let failed: Vec<String> = cases
        .into_iter()
        .filter_map(|case| (case.run)().err().map(|e| format!("{}: {e}", case.id)))
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
