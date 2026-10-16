//! DEC-923 in the runtime (DEC-529 item 3, DEC-927): on a `live` stream a `cli_confirm` grant is
//! admitted only within the view's `live_cli_confirm` scope, and its `ApprovalResponded`, naming the
//! approver and the step-up method, is journaled before the intent it releases. Every other live
//! deployment refuses the grant for its method and releases nothing; an exit is never asked, so it
//! is never held by any of this (`AGENTS.md` rule 13).
//!
//! Expectations come from the drafts' payloads and DEC-923's text, never from `mandate-approval`.

mod common;

use std::collections::BTreeSet;

use common::escalation::{
    ASKED_AT, Answer, Asked, asking_shell, next_control_seq, reconciliation, tail,
};
use common::{
    AllowGate, FixedPlan, OWNER, Ran, Shell, TestIds, clock, fresh_output, ports, price, qty,
    universe,
};
use mandate_canon::Value;
use mandate_journal::Environment;
use mandate_num::Usd;
use mandate_runtime::{
    ApprovalSettings, Autonomy, Effect, EventDraft, Input, LiveBroker, LiveScope, MandateView,
};
use proptest::prelude::*;

/// One share of `AAPL` at 90 USD, under DEC-529's 100 USD cap.
const LIMIT: &str = "90";

fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap_or_else(|e| panic!("usd {text}: {e}"))
}

/// Whole cents as the canonical decimal text `Usd` parses: no trailing fractional zeros.
fn cents(c: u32) -> String {
    let (whole, part) = (c / 100, c % 100);
    match (part, part % 10) {
        (0, _) => whole.to_string(),
        (_, 0) => format!("{whole}.{}", part / 10),
        _ => format!("{whole}.{part:02}"),
    }
}

fn one_share() -> FixedPlan {
    let mut plan = FixedPlan::opening(Autonomy::Ask);
    if let Some(proposal) = plan.proposal.as_mut() {
        proposal.qty = qty("1");
        proposal.limit = price(LIMIT);
    }
    plan
}

/// DEC-529's deployment: Robinhood, a 100 USD cap, `AAPL` the one pinned instrument, and the
/// fixture's approver as the workspace owner.
fn scope() -> LiveScope {
    LiveScope {
        broker: LiveBroker::Robinhood,
        max_order_usd: usd("100"),
        pinned_instruments: BTreeSet::from(["AAPL".to_owned()]),
        workspace_owner: mandate_approval::OpaqueUser(OWNER.to_owned()),
    }
}

fn live(scope: Option<LiveScope>) -> MandateView {
    let view = universe(&["AAPL"]);
    MandateView {
        approval: ApprovalSettings {
            environment: Environment::Live,
            live_cli_confirm: scope,
            ..view.approval
        },
        ..view
    }
}

fn member<'a>(draft: &'a EventDraft, key: &str) -> Option<&'a str> {
    draft.payload.get(key).and_then(Value::as_str)
}

fn only<'a>(ran: &'a Ran, event_type: &str) -> &'a EventDraft {
    let found: Vec<_> = ran
        .drafts
        .iter()
        .filter(|d| d.event_type == event_type)
        .collect();
    assert_eq!(found.len(), 1, "one {event_type}: {:?}", ran.draft_types());
    found[0]
}

/// The owner's fresh `cli_confirm` grant of the fixture's request, under `plan` and `view`.
fn granted(plan: &FixedPlan, view: &MandateView) -> (Asked, Ran) {
    let (ids, gate) = (TestIds, AllowGate);
    let ports = ports(&ids, &gate, plan, view);
    let (mut shell, asked) = asking_shell(&ports, Some(LIMIT));
    let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
    let ran = tail(&mut shell, &grant, &ports);
    (asked, ran)
}

fn refused_for_method(ran: &Ran) {
    let responded = only(ran, "ApprovalResponded");
    assert_eq!(member(responded, "result"), Some("refused"));
    assert_eq!(member(responded, "reason"), Some("step_up_method"));
    assert!(
        !ran.draft_types().contains(&"IntentProposed") && ran.handed.is_empty(),
        "a refused live grant releases nothing: {:?}",
        ran.draft_types()
    );
}

/// DEC-923 items 1 and 2: the one admission. The `ApprovalResponded` names the approver and
/// `cli_confirm`, and is journaled before the re-validation and the intent it releases, and the
/// intent before the handoff.
#[test]
fn the_owners_grant_on_dec_529s_robinhood_deployment_is_admitted_and_journaled_first() {
    let (asked, ran) = granted(&one_share(), &live(Some(scope())));
    assert_eq!(
        ran.draft_types(),
        vec!["ApprovalResponded", "ApprovalRevalidated", "IntentProposed"],
    );
    let responded = only(&ran, "ApprovalResponded");
    assert_eq!(
        member(responded, "approval"),
        Some(asked.approval.0.as_str())
    );
    assert_eq!(member(responded, "result"), Some("admitted"));
    assert_eq!(member(responded, "responder"), Some(OWNER));
    assert_eq!(
        responded
            .payload
            .get("step_up")
            .and_then(|s| s.get("method"))
            .and_then(Value::as_str),
        Some("cli_confirm"),
        "the grant records its step-up kind"
    );
    let revalidated = only(&ran, "ApprovalRevalidated");
    assert_eq!(member(revalidated, "result"), Some("act"));
    let intent = only(&ran, "IntentProposed");
    assert_eq!(intent.causation_id.as_ref(), Some(&revalidated.event_id));
    let position = |pred: &dyn Fn(&Effect) -> bool| ran.effects.iter().position(pred);
    let responded_at =
        position(&|e| matches!(e, Effect::Journal(d) if d.event_id == responded.event_id));
    let intent_at = position(&|e| matches!(e, Effect::Journal(d) if d.event_id == intent.event_id));
    let handed_at = position(&|e| matches!(e, Effect::Intent(_)));
    assert!(responded_at.is_some() && intent_at.is_some() && handed_at.is_some());
    assert!(
        responded_at < intent_at && intent_at < handed_at,
        "the admission is journaled before the intent, and the intent before acting (rule 5)"
    );
    assert_eq!(ran.handed.len(), 1);
}

/// DEC-923 item 3: without the scope, and with each of its conditions broken alone, the same grant
/// is refused for its method and releases nothing.
#[test]
fn every_other_live_deployment_refuses_the_grant() {
    let plan = one_share();
    refused_for_method(&granted(&plan, &live(None)).1);
    let broken = [
        LiveScope {
            broker: LiveBroker::Other,
            ..scope()
        },
        LiveScope {
            max_order_usd: usd("100.01"),
            ..scope()
        },
        LiveScope {
            pinned_instruments: BTreeSet::from(["AAPL".to_owned(), "MSFT".to_owned()]),
            ..scope()
        },
        LiveScope {
            pinned_instruments: BTreeSet::from(["MSFT".to_owned()]),
            ..scope()
        },
        LiveScope {
            workspace_owner: mandate_approval::OpaqueUser("user-other".to_owned()),
            ..scope()
        },
    ];
    for s in broken {
        refused_for_method(&granted(&plan, &live(Some(s))).1);
    }
    refused_for_method(&granted(&FixedPlan::opening(Autonomy::Ask), &live(Some(scope()))).1);
}

/// DEC-923 item 4, rule 13: an exit on a live deployment with no scope is proposed and handed with
/// no approval at all.
#[test]
fn an_exit_on_live_is_never_held_by_the_scope() {
    let (ids, gate) = (TestIds, AllowGate);
    let plan = FixedPlan::exiting(Autonomy::Auto);
    let view = live(None);
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = Shell::new(1);
    shell
        .fold_one(&reconciliation(1, 100))
        .unwrap_or_else(|e| panic!("{e}"));
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    let ran = shell.run(Input::Tick(clock(100)), &ports);
    assert!(
        ran.draft_types().contains(&"IntentProposed")
            && !ran.draft_types().contains(&"ApprovalRequested"),
        "{:?}",
        ran.draft_types()
    );
    assert_eq!(ran.handed.len(), 1);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]

    /// Over every combination of the scope's conditions and the order's size, the grant acts
    /// exactly when DEC-923's conditions all hold, by an oracle on whole cents, and the
    /// `ApprovalResponded` always precedes any intent.
    #[test]
    fn a_live_grant_acts_exactly_within_dec_923(
        robinhood in prop::bool::weighted(0.8),
        cap_cents in prop_oneof![Just(10_000u32), Just(10_001u32), 1_000u32..20_000],
        pinned_shape in prop_oneof![3 => Just(1usize), 1 => 0usize..4],
        shares in prop_oneof![3 => Just(1u32), 1 => Just(2u32)],
        owner_is_approver in prop::bool::weighted(0.8),
        scoped in prop::bool::weighted(0.8),
    ) {
        let pinned: BTreeSet<String> = match pinned_shape {
            0 => BTreeSet::new(),
            1 => BTreeSet::from(["AAPL".to_owned()]),
            2 => BTreeSet::from(["MSFT".to_owned()]),
            _ => BTreeSet::from(["AAPL".to_owned(), "MSFT".to_owned()]),
        };
        let expected = scoped
            && robinhood
            && cap_cents <= 10_000
            && pinned_shape == 1
            && shares * 9_000 <= 10_000
            && owner_is_approver;
        let s = LiveScope {
            broker: if robinhood { LiveBroker::Robinhood } else { LiveBroker::Other },
            max_order_usd: usd(&cents(cap_cents)),
            pinned_instruments: pinned,
            workspace_owner: mandate_approval::OpaqueUser(
                if owner_is_approver { OWNER } else { "user-other" }.to_owned(),
            ),
        };
        let mut plan = one_share();
        if let Some(p) = plan.proposal.as_mut() {
            p.qty = qty(&shares.to_string());
        }
        let (_, ran) = granted(&plan, &live(scoped.then_some(s)));
        let responded = only(&ran, "ApprovalResponded");
        prop_assert_eq!(
            member(responded, "result"),
            Some(if expected { "admitted" } else { "refused" })
        );
        prop_assert_eq!(ran.draft_types().contains(&"IntentProposed"), expected);
        prop_assert_eq!(ran.handed.len(), usize::from(expected));
        if let Some(intent_at) = ran.draft_types().iter().position(|t| *t == "IntentProposed") {
            let responded_at = ran.draft_types().iter().position(|t| *t == "ApprovalResponded");
            prop_assert!(responded_at < Some(intent_at));
        }
    }
}
