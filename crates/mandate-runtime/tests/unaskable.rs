//! An ASK the runtime cannot build is refused on the record (DEC-2814; backlog
//! `E21-30-ask-silent-returns`): an opening classified `ask` whose approval request cannot be
//! bound, whose plan names another asset class, or whose content cannot be hashed is journaled as
//! the dry run's `deny` with its own reason code and notified once as any skipped decision, and
//! nothing is asked, proposed, or left pending. The same opening built well is asked, so a runtime
//! that refused every ask would pass none of these.

mod common;

use common::escalation::reconciliation;
use common::{
    AllowGate, FixedPlan, Ran, Shell, TestIds, clock, fresh_output, ports, price, qty, universe,
};
use mandate_accounting::AssetClass;
use mandate_canon::Value;
use mandate_runtime::{Autonomy, EventDraft, Input, OrderExecution, TimeInForce};

fn evaluated(plan: &FixedPlan) -> Ran {
    let (ids, gate, view) = (TestIds, AllowGate, universe(&["AAPL"]));
    let ports = ports(&ids, &gate, plan, &view);
    let mut shell = Shell::new(1);
    shell
        .fold_one(&reconciliation(1, 0))
        .unwrap_or_else(|e| panic!("reconciliation: {e}"));
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    shell.run(Input::Tick(clock(100)), &ports)
}

fn decision(ran: &Ran) -> &EventDraft {
    ran.drafts
        .iter()
        .find(|draft| draft.event_type == "DecisionMade")
        .unwrap_or_else(|| panic!("the evaluation decided: {:?}", ran.draft_types()))
}

fn member<'a>(draft: &'a EventDraft, key: &str) -> Option<&'a str> {
    draft.payload.get(key).and_then(Value::as_str)
}

/// The opening is refused `reason`, notified once, and nothing follows.
fn refused(ran: &Ran, reason: &str, why: &str) {
    let made = decision(ran);
    assert_eq!(member(made, "dry_run"), Some("deny"), "{why}");
    assert_eq!(member(made, "reason_code"), Some(reason), "{why}");
    assert_eq!(made.payload.get("autonomy"), Some(&Value::Null), "{why}");
    assert!(
        !ran.draft_types().contains(&"ApprovalRequested")
            && !ran.draft_types().contains(&"IntentProposed")
            && ran.handed.is_empty(),
        "{why}: {:?}",
        ran.draft_types()
    );
    assert_eq!(
        ran.notifications
            .iter()
            .filter(|key| **key == "decision_skipped")
            .count(),
        1,
        "{why}: {:?}",
        ran.notifications
    );
}

fn asking() -> FixedPlan {
    FixedPlan::opening(Autonomy::Ask)
}

#[test]
fn a_well_built_ask_is_asked() {
    let ran = evaluated(&asking());
    assert!(
        ran.draft_types().contains(&"ApprovalRequested"),
        "{:?}",
        ran.draft_types()
    );
    assert_eq!(member(decision(&ran), "dry_run"), Some("allow"));
}

#[test]
fn an_ask_without_a_label_is_refused_ask_unbound() {
    let plan = FixedPlan {
        decided_by: "",
        ..asking()
    };
    refused(&evaluated(&plan), "ask_unbound", "no label to bind");
}

#[test]
fn an_ask_whose_score_is_not_a_decimal_is_refused_ask_unbound() {
    let mut plan = asking();
    if let Some(proposal) = plan.proposal.as_mut() {
        proposal.combined_score = Value::Null;
    }
    refused(&evaluated(&plan), "ask_unbound", "no score to bind");
}

#[test]
fn an_ask_whose_plan_names_another_asset_class_is_refused_plan_mismatch() {
    let mut plan = asking();
    if let Some(proposal) = plan.proposal.as_mut() {
        proposal.execution = Some(OrderExecution {
            asset_class: AssetClass::Crypto,
            tif: TimeInForce::Gtc,
            protection_required: false,
            protection: None,
        });
    }
    refused(
        &evaluated(&plan),
        "plan_mismatch",
        "a crypto plan for an equity order",
    );
}

#[test]
fn an_ask_whose_order_value_cannot_be_written_is_refused_content_unhashable() {
    let mut plan = asking();
    if let Some(proposal) = plan.proposal.as_mut() {
        proposal.qty = qty("99999999999999999");
        proposal.limit = price("99999999999999999");
    }
    refused(
        &evaluated(&plan),
        "content_unhashable",
        "an order value past every decimal's range",
    );
}

/// An exit classified `ask` is proposed as AUTO is (DEC-278 item 1), whatever would stop an ask.
#[test]
fn an_exit_is_never_refused_for_an_ask_it_never_makes() {
    let plan = FixedPlan {
        decided_by: "",
        ..FixedPlan::exiting(Autonomy::Ask)
    };
    let ran = evaluated(&plan);
    assert!(
        ran.draft_types().contains(&"IntentProposed"),
        "{:?}",
        ran.draft_types()
    );
}

/// An `auto` opening the overlay made an `ask` because its dry run deferred a check, whose request
/// cannot then be bound, is a version 1 denial: nothing deferred is named, and its autonomy and
/// label are null, so it claims neither the overlay nor a pass (journal spec rules 5, 7 and 269).
#[test]
fn a_deferring_ask_that_cannot_be_bound_is_a_version_1_denial() {
    struct Deferring;
    impl mandate_runtime::GateDryRun for Deferring {
        fn check(&self, _: &mandate_runtime::Proposal) -> mandate_runtime::DryRunVerdict {
            mandate_runtime::DryRunVerdict::AllowDeferring {
                deferred: std::collections::BTreeSet::from([mandate_runtime::Deferral::Halt]),
                liquidity: None,
            }
        }
    }
    let mut plan = FixedPlan::opening(Autonomy::Auto);
    if let Some(proposal) = plan.proposal.as_mut() {
        proposal.combined_score = Value::Null;
    }
    let (ids, gate, view) = (TestIds, Deferring, universe(&["AAPL"]));
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell
        .fold_one(&reconciliation(1, 0))
        .unwrap_or_else(|e| panic!("reconciliation: {e}"));
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    let ran = shell.run(Input::Tick(clock(100)), &ports);
    refused(&ran, "ask_unbound", "the overlay's ask cannot be bound");
    let made = decision(&ran);
    assert_eq!(made.schema_version(), 1);
    assert_eq!(made.payload.get("deferred_to_binding"), None);
    assert_eq!(made.payload.get("decided_by"), Some(&Value::Null));
}
