//! Journal spec §9.1 rule 9: a `DecisionMade`'s `outputs_used` lists the output events it rests on
//! strictly ascending, by event id, whatever the names of the models that wrote them. A decision
//! over two models whose outputs were journaled in the opposite order to their names is written,
//! and cites both in event order: a writer that ordered them by model name would draft a record
//! its own fold refuses as `non_canonical`, and the evaluation would never decide.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{
    ACCOUNT_STREAM, AllowGate, FixedPlan, Shell, TestIds, clock, event, fresh_output, ports, text,
    universe, with_clock,
};
use mandate_canon::{Digest, Value};
use mandate_runtime::{Autonomy, Input};

/// The models in name order: `a_quant` sorts first, and its output is journaled last.
const FIRST_BY_NAME: &str = "a_quant";
const LAST_BY_NAME: &str = "z_research";

fn output(model: &str, at: i64) -> mandate_runtime::ModelOutput {
    let mut output = fresh_output(at);
    output.model_id = model.to_owned();
    output.content_hash = Digest::of(model.as_bytes());
    output
}

fn recorded_ids(shell: &Shell) -> Vec<(String, String)> {
    shell
        .agent_journal
        .iter()
        .filter(|row| row.event_type == "ModelOutputRecorded")
        .map(|row| {
            let model = row
                .payload
                .get("model_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            (model, row.event_id.0.clone())
        })
        .collect()
}

#[test]
fn a_decision_cites_its_outputs_in_event_order_not_model_name_order() {
    let mut plan = FixedPlan::opening(Autonomy::Ask);
    if let Some(proposal) = plan.proposal.as_mut() {
        proposal.outputs_used = BTreeSet::from([FIRST_BY_NAME.to_owned(), LAST_BY_NAME.to_owned()]);
        proposal.model_weights = BTreeMap::from([
            (FIRST_BY_NAME.to_owned(), text("0.5")),
            (LAST_BY_NAME.to_owned(), text("0.5")),
        ]);
    }
    let (ids, gate, view) = (TestIds, AllowGate, universe(&["AAPL"]));
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            1,
            "ReconciliationRun",
            with_clock(&[("result", text("clean"))], 0),
        ))
        .unwrap_or_else(|e| panic!("{e}"));
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(output(LAST_BY_NAME, 100)), &ports);
    shell.run(Input::ModelOutput(output(FIRST_BY_NAME, 100)), &ports);

    let journaled = recorded_ids(&shell);
    let order: Vec<&str> = journaled.iter().map(|(model, _)| model.as_str()).collect();
    assert_eq!(
        order,
        [LAST_BY_NAME, FIRST_BY_NAME],
        "the fixture journals the later name first"
    );
    assert!(
        journaled[0].1 < journaled[1].1,
        "the first journaled has the lower id"
    );

    let ran = shell
        .step(Input::Tick(clock(100)), &ports)
        .unwrap_or_else(|e| panic!("the evaluation decides: {e}"));
    let made = ran
        .drafts
        .iter()
        .find(|d| d.event_type == "DecisionMade")
        .unwrap_or_else(|| panic!("a decision is made: {:?}", ran.draft_types()));
    let cited: Vec<&str> = made
        .payload
        .get("outputs_used")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let mut expected: Vec<&str> = journaled.iter().map(|(_, id)| id.as_str()).collect();
    expected.sort_unstable();
    assert_eq!(
        cited, expected,
        "both outputs, ascending by event id (rule 9)"
    );
}
