//! An owner's request for an order (journal spec §2 and §9.9, workspace API §4.6, DEC-2858;
//! backlog E10-30, slice B): the control stream's `OwnerRequestSubmitted`, copied at most once and
//! decided as a proposal the order builder clipped, never sized.
//!
//! The plan here is the test's: it returns a fixed proposal for a request, so each test's subject
//! is the runtime. Its `DecisionMade` names the request as `causation_id`, records `requested_by`
//! and `client_id`, and carries no conviction, score or model output (journal spec §9.1 rule 8,
//! DEC-2858 item 1); the mode, the dry run and the autonomy rules decide as for any opening, a
//! client's never AUTO (mandate spec §6.2 step 5a, MI-30). A request the workspace API would have
//! refused (another agent's, a sell, no quantity, an instrument outside the working universe) is
//! inert (DEC-2858 items 3, 4 and 6), and so is one the plan cannot price.

mod common;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use common::escalation::{mark, mode_applied, reconciliation, tail};
use common::{
    AGENT, AllowGate, DenyGate, Ran, Shell, TestIds, instrument, object, ports, price, qty, text,
    universe,
};
use mandate_accounting::{AssetClass, Side};
use mandate_canon::Value;
use mandate_runtime::{
    ActorKind, Autonomy, Classified, DecisionClip, Deferral, DryRunVerdict, EventDraft, EventId,
    FoldedEvent, Input, MandateView, OrderExecution, OrderPlan, OwnerRequest, Proposal, Purpose,
    Requester, SignalInputs, TimeInForce,
};
use proptest::prelude::{Just, Strategy, any, prop_oneof};
use proptest::test_runner::{Config as ProptestConfig, TestRunner};

/// When the fixture starts and when its requests are tailed.
const AT: i64 = 1_000;

/// A plan that answers every owner request with the same clipped buy and classifies as told, and
/// records each request it was handed, so a test sees exactly what the runtime asked of it.
struct OwnerPlan {
    clips: Vec<DecisionClip>,
    qty: &'static str,
    autonomy: Autonomy,
    prices: bool,
    /// What `held_for_review` answers: whether a review ceiling or the two-approver threshold
    /// holds the owner's request (DEC-2859).
    held: bool,
    asked: RefCell<Vec<OwnerRequest>>,
}

impl OwnerPlan {
    fn classified(autonomy: Autonomy) -> Self {
        Self {
            clips: Vec::new(),
            qty: "5",
            autonomy,
            prices: true,
            held: false,
            asked: RefCell::new(Vec::new()),
        }
    }

    fn held(autonomy: Autonomy) -> Self {
        Self {
            held: true,
            ..Self::classified(autonomy)
        }
    }

    fn unpriced() -> Self {
        Self {
            prices: false,
            ..Self::classified(Autonomy::Auto)
        }
    }
}

impl OrderPlan for OwnerPlan {
    fn plan(&self, _view: &MandateView, _inputs: &SignalInputs) -> Option<Proposal> {
        None
    }

    fn classify(&self, _view: &MandateView, _proposal: &Proposal) -> Classified {
        Classified {
            autonomy: self.autonomy,
            decided_by: Some(
                match self.autonomy {
                    Autonomy::Auto => "rule:owner-auto",
                    Autonomy::Ask => "rule:owner-ask",
                    Autonomy::Deny => "rule:owner-deny",
                }
                .to_owned(),
            ),
        }
    }

    fn held_for_review(&self, _view: &MandateView, _proposal: &Proposal) -> bool {
        self.held
    }

    fn requested(&self, _view: &MandateView, request: &OwnerRequest) -> Option<Proposal> {
        self.asked.borrow_mut().push(request.clone());
        self.prices.then(|| Proposal {
            instrument: request.instrument.clone(),
            asset_class: AssetClass::UsEquity,
            side: Side::Buy,
            qty: qty(self.qty),
            limit: price("155"),
            purpose: Purpose::Open,
            exit_origin: None,
            exit_conviction: None,
            buy_conviction: None,
            combined_score: Value::Null,
            outputs_used: BTreeSet::new(),
            model_weights: BTreeMap::new(),
            clips_applied: self.clips.clone(),
            execution: Some(OrderExecution {
                asset_class: AssetClass::UsEquity,
                tif: TimeInForce::Day,
                protection_required: false,
                protection: None,
            }),
        })
    }
}

/// A dry run that allows with a check left to the binding gate, as the cell's always does
/// (DEC-2388).
struct DeferGate;

impl mandate_runtime::GateDryRun for DeferGate {
    fn check(&self, _proposal: &Proposal) -> DryRunVerdict {
        DryRunVerdict::AllowDeferring {
            deferred: BTreeSet::from([Deferral::AgentEquity]),
            liquidity: None,
        }
    }
}

/// One `OwnerRequestSubmitted` as the workspace API commits it (journal spec §9.9).
struct Request {
    seq: u64,
    agent: &'static str,
    instrument: &'static str,
    side: &'static str,
    quantity: Option<&'static str>,
    client: Option<&'static str>,
}

impl Request {
    fn owner_buy(seq: u64) -> Self {
        Self {
            seq,
            agent: AGENT,
            instrument: "AAPL",
            side: "buy",
            quantity: Some("8"),
            client: None,
        }
    }

    fn client_buy(seq: u64) -> Self {
        Self {
            client: Some("client-desk"),
            ..Self::owner_buy(seq)
        }
    }

    fn id(&self) -> EventId {
        common::escalation::control_id(self.seq)
    }

    fn event(&self) -> FoldedEvent {
        let payload = object(&[
            ("agent_id", text(self.agent)),
            ("instrument_id", text(self.instrument)),
            ("side", text(self.side)),
            ("quantity", self.quantity.map_or(Value::Null, text)),
            (
                "requested_by",
                text(if self.client.is_some() {
                    "client"
                } else {
                    "owner"
                }),
            ),
            ("client_id", self.client.map_or(Value::Null, text)),
        ]);
        FoldedEvent {
            actor: ActorKind::User,
            ..common::escalation::from_user(self.seq, "OwnerRequestSubmitted", payload)
        }
    }
}

/// A started shell: reconciled clean at `AT`, `AAPL` marked at 155, so an ask can bind its
/// reference mark.
fn started(ports: &mandate_runtime::Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    for event in [reconciliation(1, AT), mark(2, "155", AT)] {
        shell
            .fold_one(&event)
            .unwrap_or_else(|e| panic!("{}: {e}", event.event_type));
    }
    shell.restart(ports).0
}

/// The one draft of `event_type` in `ran`.
fn only<'r>(ran: &'r Ran, event_type: &str) -> &'r EventDraft {
    let found: Vec<&EventDraft> = ran
        .drafts
        .iter()
        .filter(|draft| draft.event_type == event_type)
        .collect();
    match found.as_slice() {
        [one] => one,
        _ => panic!(
            "expected one {event_type}, got {}: {:?}",
            found.len(),
            ran.draft_types()
        ),
    }
}

fn member<'d>(draft: &'d EventDraft, name: &str) -> &'d Value {
    draft
        .payload
        .get(name)
        .unwrap_or_else(|| panic!("{} has no {name}", draft.event_type))
}

fn empty_list() -> Value {
    Value::Array(Vec::new())
}

/// The members DEC-2858 item 1 gives every owner request's decision.
fn assert_owner_decision(decision: &EventDraft, request: &EventId, requested_by: &str) {
    assert_eq!(
        decision.causation_id.as_ref(),
        Some(request),
        "caused by the request"
    );
    assert_eq!(member(decision, "requested_by"), &text(requested_by));
    for number in ["exit_conviction", "buy_conviction", "combined_score"] {
        assert_eq!(
            member(decision, number),
            &Value::Null,
            "{number}: no §8.3 evaluation produced it"
        );
    }
    for list in ["outputs_used", "model_weights"] {
        assert_eq!(member(decision, list), &empty_list(), "{list}");
    }
    assert_eq!(member(decision, "instrument_id"), &text("AAPL"));
    assert_eq!(member(decision, "side"), &text("buy"));
    assert_eq!(member(decision, "purpose"), &text("open"));
}

#[test]
fn an_owners_buy_is_decided_as_the_owners_request_and_proposed() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        OwnerPlan::classified(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started(&ports);
    let request = Request::owner_buy(1);
    let ran = tail(&mut shell, &request.event(), &ports);
    assert_eq!(ran.draft_types(), vec!["DecisionMade", "IntentProposed"]);
    let decision = only(&ran, "DecisionMade");
    assert_owner_decision(decision, &request.id(), "owner");
    assert_eq!(member(decision, "client_id"), &Value::Null);
    assert_eq!(
        member(decision, "qty"),
        &text("5"),
        "the plan's clipped quantity"
    );
    assert_eq!(member(decision, "dry_run"), &text("allow"));
    assert_eq!(member(decision, "autonomy"), &text("auto"));
    let intent = only(&ran, "IntentProposed");
    assert_eq!(intent.causation_id.as_ref(), Some(&decision.event_id));
    assert_eq!(ran.handed.len(), 1, "the intent is handed");
    let asked = plan.asked.borrow();
    assert_eq!(
        asked.as_slice(),
        [OwnerRequest {
            request: request.id(),
            instrument: instrument("AAPL"),
            qty: qty("8"),
            requested_by: Requester::Owner,
        }],
        "the plan is handed the owner's own quantity, unsized"
    );
}

/// DEC-2858 item 10, founder-ruled: an owner's own request is its own approval. Under a mandate
/// that would ask, the owner's Send is the confirmation, so the decision runs `auto`, labelled
/// `owner_request`, and the intent is handed; no approval is ever requested of the owner.
#[test]
fn an_owners_request_the_mandate_would_ask_about_runs_as_the_owners_own() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        OwnerPlan::classified(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started(&ports);
    let request = Request::owner_buy(1);
    let ran = tail(&mut shell, &request.event(), &ports);
    assert_eq!(ran.draft_types(), vec!["DecisionMade", "IntentProposed"]);
    let decision = only(&ran, "DecisionMade");
    assert_owner_decision(decision, &request.id(), "owner");
    assert_eq!(member(decision, "autonomy"), &text("auto"));
    assert_eq!(member(decision, "decided_by"), &text("owner_request"));
    assert_eq!(ran.handed.len(), 1, "the owner's order is handed at once");
}

/// DEC-2859, the founder's: an owner's request a reviewer holds (a review ceiling, the admission
/// ceiling or the two-approver threshold) stays the ASK the mandate made, with its own label, and is
/// asked for approval as any ASK is; nothing is handed until an approver says yes.
#[test]
fn an_owners_request_a_reviewer_holds_is_asked() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        OwnerPlan::held(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started(&ports);
    let request = Request::owner_buy(1);
    let ran = tail(&mut shell, &request.event(), &ports);
    assert_eq!(
        ran.draft_types(),
        vec!["DecisionMade", "ApprovalRequested", "ApprovalDelivered"]
    );
    let decision = only(&ran, "DecisionMade");
    assert_owner_decision(decision, &request.id(), "owner");
    assert_eq!(member(decision, "autonomy"), &text("ask"));
    assert_eq!(member(decision, "decided_by"), &text("rule:owner-ask"));
    let asked = only(&ran, "ApprovalRequested");
    assert_eq!(member(asked, "combined_score"), &Value::Null);
    assert!(
        ran.handed.is_empty(),
        "a held request waits for its approver"
    );
}

/// A held owner's request the dry run deferred is asked under the overlay's label, and one the
/// mandate lets run is not asked at all: step 5d only ever keeps an ASK, it never makes one.
#[test]
fn a_held_owners_request_is_asked_only_when_something_asked() {
    let (ids, gate, plan, view) = (
        TestIds,
        DeferGate,
        OwnerPlan::held(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let deferring = ports(&ids, &gate, &plan, &view);
    let mut shell = started(&deferring);
    let ran = tail(&mut shell, &Request::owner_buy(1).event(), &deferring);
    let decision = only(&ran, "DecisionMade");
    assert_eq!(member(decision, "autonomy"), &text("ask"));
    assert_eq!(member(decision, "decided_by"), &text("policy_overlay"));
    assert!(ran.handed.is_empty());
    let allowed = OwnerPlan::held(Autonomy::Auto);
    let allowing = ports(&ids, &AllowGate, &allowed, &view);
    let mut shell = started(&allowing);
    let ran = tail(&mut shell, &Request::owner_buy(1).event(), &allowing);
    let decision = only(&ran, "DecisionMade");
    assert_eq!(member(decision, "autonomy"), &text("auto"));
    assert_eq!(member(decision, "decided_by"), &text("rule:owner-auto"));
    assert_eq!(ran.handed.len(), 1);
}

/// A dry run that deferred checks to the binding gate asks about any other opening (mandate spec
/// §6.2 step 5c); the owner's own request still runs `auto`, the binding gate judging every
/// deferred check at submission. A client's is asked as ever.
#[test]
fn an_owners_request_the_dry_run_deferred_runs_and_a_clients_is_asked() {
    let (ids, gate, plan, view) = (
        TestIds,
        DeferGate,
        OwnerPlan::classified(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started(&ports);
    let owners = Request::owner_buy(1);
    let ran = tail(&mut shell, &owners.event(), &ports);
    assert_eq!(ran.draft_types(), vec!["DecisionMade", "IntentProposed"]);
    let decision = only(&ran, "DecisionMade");
    assert_eq!(member(decision, "dry_run"), &text("allow"));
    assert_eq!(member(decision, "autonomy"), &text("auto"));
    assert_eq!(member(decision, "decided_by"), &text("owner_request"));
    let mut shell = started(&ports);
    let clients = Request::client_buy(1);
    let ran = tail(&mut shell, &clients.event(), &ports);
    let decision = only(&ran, "DecisionMade");
    assert_eq!(member(decision, "autonomy"), &text("ask"));
    assert!(
        ran.handed.is_empty(),
        "a client's opening runs only once asked"
    );
}

/// A client's request the mandate asks about is asked with no score (DEC-2858 item 2): the
/// approver is shown who asked, never a model figure that decided nothing.
#[test]
fn a_clients_request_is_asked_with_no_score() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        OwnerPlan::classified(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started(&ports);
    let request = Request::client_buy(1);
    let ran = tail(&mut shell, &request.event(), &ports);
    assert_eq!(
        ran.draft_types(),
        vec!["DecisionMade", "ApprovalRequested", "ApprovalDelivered"]
    );
    let decision = only(&ran, "DecisionMade");
    assert_owner_decision(decision, &request.id(), "client");
    assert_eq!(member(decision, "autonomy"), &text("ask"));
    let asked = only(&ran, "ApprovalRequested");
    assert_eq!(asked.causation_id.as_ref(), Some(&decision.event_id));
    assert_eq!(
        member(asked, "combined_score"),
        &Value::Null,
        "DEC-2858 item 2"
    );
    let evidence = member(asked, "content")
        .get("evidence")
        .unwrap_or_else(|| panic!("the content has its evidence"));
    assert_eq!(evidence.get("combined_score"), Some(&Value::Null));
    assert_eq!(evidence.get("outputs"), Some(&empty_list()));
    assert!(ran.handed.is_empty(), "nothing is handed before a grant");
}

#[test]
fn a_clients_opening_is_never_auto() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        OwnerPlan::classified(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started(&ports);
    let request = Request::client_buy(1);
    let ran = tail(&mut shell, &request.event(), &ports);
    let decision = only(&ran, "DecisionMade");
    assert_owner_decision(decision, &request.id(), "client");
    assert_eq!(member(decision, "client_id"), &text("client-desk"));
    assert_eq!(member(decision, "autonomy"), &text("ask"), "MI-30");
    assert_eq!(member(decision, "decided_by"), &text("client_ceiling"));
    assert!(
        ran.handed.is_empty(),
        "a client's opening runs only once asked"
    );
    assert_eq!(
        plan.asked.borrow().first().map(|r| r.requested_by.clone()),
        Some(Requester::Client {
            client_id: "client-desk".to_owned()
        })
    );
}

#[test]
fn each_cap_that_cut_the_owners_quantity_is_recorded() {
    let (ids, gate, view) = (TestIds, AllowGate, universe(&["AAPL"]));
    let plan = OwnerPlan {
        clips: vec![DecisionClip::MaxOrderUsd, DecisionClip::PositionCap],
        ..OwnerPlan::classified(Autonomy::Auto)
    };
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started(&ports);
    let ran = tail(&mut shell, &Request::owner_buy(1).event(), &ports);
    let decision = only(&ran, "DecisionMade");
    assert_eq!(
        member(decision, "clips_applied"),
        &Value::Array(vec![text("max_order_usd"), text("position_cap")]),
        "a cut is recorded, never silent"
    );
}

#[test]
fn a_denied_owners_request_records_the_gates_reason_and_tells_the_owner() {
    let (ids, gate, plan, view) = (
        TestIds,
        DenyGate("max_order_size"),
        OwnerPlan::classified(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started(&ports);
    let request = Request::owner_buy(1);
    let ran = tail(&mut shell, &request.event(), &ports);
    assert_eq!(ran.draft_types(), vec!["DecisionMade"]);
    let decision = only(&ran, "DecisionMade");
    assert_owner_decision(decision, &request.id(), "owner");
    assert_eq!(member(decision, "dry_run"), &text("deny"));
    assert_eq!(member(decision, "reason_code"), &text("max_order_size"));
    assert_eq!(member(decision, "autonomy"), &Value::Null);
    assert!(ran.handed.is_empty());
    assert_eq!(ran.notifications, vec!["decision_skipped"]);
}

/// A request the mandate's autonomy rules deny is recorded as its decision and goes no further: no
/// intent, no ask, and no skipped decision's alert, because the dry run allowed it; the owner sees
/// the rule that denied it in the decision's `decided_by`.
#[test]
fn a_request_the_mandate_denies_is_recorded_and_raises_no_alert() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        OwnerPlan::classified(Autonomy::Deny),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started(&ports);
    let request = Request::owner_buy(1);
    let ran = tail(&mut shell, &request.event(), &ports);
    assert_eq!(ran.draft_types(), vec!["DecisionMade"]);
    let decision = only(&ran, "DecisionMade");
    assert_owner_decision(decision, &request.id(), "owner");
    assert_eq!(member(decision, "dry_run"), &text("allow"));
    assert_eq!(member(decision, "autonomy"), &text("deny"));
    assert_eq!(member(decision, "decided_by"), &text("rule:owner-deny"));
    assert!(ran.handed.is_empty());
    assert!(ran.notifications.is_empty(), "{:?}", ran.notifications);
}

#[test]
fn a_mode_that_admits_no_opening_denies_the_owners_request_by_name() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        OwnerPlan::classified(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    for (mode, code) in [
        ("exits_only", "agent_exits_only"),
        ("paused", "agent_paused"),
        ("stopped", "agent_stopped"),
    ] {
        let mut shell = started(&ports);
        let applied = mode_applied(3, mode, AT);
        tail(&mut shell, &applied, &ports);
        let request = Request::owner_buy(1);
        let ran = tail(&mut shell, &request.event(), &ports);
        let decision = only(&ran, "DecisionMade");
        assert_owner_decision(decision, &request.id(), "owner");
        assert_eq!(member(decision, "dry_run"), &text("deny"), "{mode}");
        assert_eq!(member(decision, "reason_code"), &text(code), "{mode}");
        assert!(ran.handed.is_empty(), "{mode}");
    }
}

#[test]
fn a_request_is_copied_once_however_often_it_is_tailed() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        OwnerPlan::classified(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started(&ports);
    let request = Request::owner_buy(1).event();
    let first = tail(&mut shell, &request, &ports);
    assert_eq!(first.draft_types(), vec!["DecisionMade", "IntentProposed"]);
    let again = shell.run(Input::Journal(request.clone()), &ports);
    assert!(
        again.drafts.is_empty(),
        "re-tailed: {:?}",
        again.draft_types()
    );
    let (mut restarted, _) = shell.restart(&ports);
    let after = restarted.run(Input::Journal(request), &ports);
    assert!(
        after.drafts.is_empty(),
        "re-tailed after a restart: {:?}",
        after.draft_types()
    );
}

#[test]
fn a_request_the_api_would_have_refused_is_inert() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        OwnerPlan::classified(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let refused = [
        Request {
            agent: "agent-b",
            ..Request::owner_buy(1)
        },
        Request {
            side: "sell",
            ..Request::owner_buy(1)
        },
        Request {
            quantity: None,
            ..Request::owner_buy(1)
        },
        Request {
            instrument: "MSFT",
            ..Request::owner_buy(1)
        },
    ];
    let mut shell = started(&ports);
    for (seq, request) in (1_u64..).zip(refused) {
        let request = Request { seq, ..request };
        let ran = tail(&mut shell, &request.event(), &ports);
        assert!(
            ran.is_empty(),
            "{} {} {:?} of {}: {:?}",
            request.agent,
            request.side,
            request.quantity,
            request.instrument,
            ran.draft_types()
        );
    }
    assert!(plan.asked.borrow().is_empty(), "none reaches the plan");
    let valid = tail(&mut shell, &Request::owner_buy(5).event(), &ports);
    assert_eq!(
        valid.draft_types(),
        vec!["DecisionMade", "IntentProposed"],
        "the same runtime still decides a request it can act on"
    );
}

#[test]
fn a_request_the_plan_cannot_price_journals_nothing() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        OwnerPlan::unpriced(),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started(&ports);
    let ran = tail(&mut shell, &Request::owner_buy(1).event(), &ports);
    assert!(ran.is_empty(), "{:?}", ran.draft_types());
    assert_eq!(plan.asked.borrow().len(), 1, "the plan was asked");
}

/// Live from the stub on: an owner's request is owner input routed as a request, never read as a
/// command or an answer, so one addressed to another deployment steps cleanly and does nothing
/// here, whether or not this runtime decides requests yet.
#[test]
fn another_agents_request_steps_cleanly_and_does_nothing() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        OwnerPlan::classified(Autonomy::Auto),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started(&ports);
    let request = Request {
        agent: "agent-b",
        ..Request::owner_buy(1)
    };
    let event = request.event();
    shell
        .fold_one(&event)
        .unwrap_or_else(|e| panic!("folding the request: {e}"));
    let ran = shell
        .step(Input::Journal(event), &ports)
        .unwrap_or_else(|e| panic!("stepping another agent's request refused: {e}"));
    assert!(ran.is_empty(), "{:?}", ran.draft_types());
    assert!(plan.asked.borrow().is_empty());
}

/// One step of the property run: a request (by the owner or a client, possibly a repeat of an
/// earlier one), or a restart.
#[derive(Debug, Clone)]
enum Step {
    Request { client: bool },
    Repeat(usize),
    Restart,
}

/// What the dry run answers every request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Allows,
    Defers,
    Denies,
}

/// A run: how the mandate classifies, whether a reviewer holds the owner's requests, what the dry
/// run answers, and the steps.
#[derive(Debug, Clone)]
struct Case {
    autonomy: Autonomy,
    held: bool,
    verdict: Verdict,
    steps: Vec<Step>,
}

fn cases() -> impl Strategy<Value = Case> {
    let autonomy = prop_oneof![
        Just(Autonomy::Auto),
        Just(Autonomy::Ask),
        Just(Autonomy::Deny)
    ];
    let step = prop_oneof![
        3 => any::<bool>().prop_map(|client| Step::Request { client }),
        2 => (0_usize..8).prop_map(Step::Repeat),
        1 => Just(Step::Restart),
    ];
    let verdict = prop_oneof![
        Just(Verdict::Allows),
        Just(Verdict::Defers),
        Just(Verdict::Denies)
    ];
    (
        autonomy,
        any::<bool>(),
        verdict,
        proptest::collection::vec(step, 1..12),
    )
        .prop_map(|(autonomy, held, verdict, steps)| Case {
            autonomy,
            held,
            verdict,
            steps,
        })
}

/// DEC-2858's invariants over every run, checked against the journal the shell kept rather than
/// the runtime's own state:
/// - each request is decided at most once, however often it is tailed or the process restarts,
///   and its decision names it as `causation_id` with its own `requested_by`;
/// - no owner request's decision is ever `requested_by` `agent`, nor carries a score;
/// - a client's opening is never `auto`, and an intent follows exactly the `auto` allowed decisions,
///   one each, caused by it, at the plan's quantity;
/// - an owner's own request is asked only when a reviewer holds it (DEC-2858 item 10, DEC-2859,
///   MI-35): a deny stays a deny, the dry run's denial records no autonomy, an ask a reviewer holds
///   stays that ask under its own label, and every other ask, the rules' or the deferral's, runs
///   `auto` as `owner_request`.
fn one_run(case: &Case) -> Result<(), String> {
    let (ids, view) = (TestIds, universe(&["AAPL"]));
    let plan = OwnerPlan {
        held: case.held,
        ..OwnerPlan::classified(case.autonomy)
    };
    let (allow, defer, deny) = (AllowGate, DeferGate, DenyGate("max_order_size"));
    let gate: &dyn mandate_runtime::GateDryRun = match case.verdict {
        Verdict::Allows => &allow,
        Verdict::Defers => &defer,
        Verdict::Denies => &deny,
    };
    let ports = ports(&ids, gate, &plan, &view);
    let mut shell = started(&ports);
    let mut sent: Vec<(FoldedEvent, bool)> = Vec::new();
    let mut next_seq = 1_u64;
    for step in &case.steps {
        match step {
            Step::Request { client } => {
                let request = if *client {
                    Request::client_buy(next_seq)
                } else {
                    Request::owner_buy(next_seq)
                };
                next_seq = next_seq.saturating_add(1);
                let event = request.event();
                shell.fold_one(&event).map_err(|e| e.to_string())?;
                shell
                    .step(Input::Journal(event.clone()), &ports)
                    .map_err(|e| e.to_string())?;
                sent.push((event, *client));
            }
            Step::Repeat(index) => {
                if let Some((event, _)) = sent.get(index % sent.len().max(1)) {
                    shell
                        .step(Input::Journal(event.clone()), &ports)
                        .map_err(|e| e.to_string())?;
                }
            }
            Step::Restart => shell = shell.restart(&ports).0,
        }
    }
    for (event, client) in &sent {
        let decisions: Vec<&FoldedEvent> = shell
            .agent_journal
            .iter()
            .filter(|e| {
                e.event_type == "DecisionMade" && e.causation_id.as_ref() == Some(&event.event_id)
            })
            .collect();
        let [decision] = decisions.as_slice() else {
            return Err(format!(
                "{} decided {} times",
                event.event_id.0,
                decisions.len()
            ));
        };
        let by = decision.payload.get("requested_by");
        let expected = text(if *client { "client" } else { "owner" });
        if by != Some(&expected) {
            return Err(format!("requested_by {by:?}"));
        }
        if decision.payload.get("combined_score") != Some(&Value::Null) {
            return Err("an owner request's decision carries a score".to_owned());
        }
        if *client && decision.payload.get("autonomy") == Some(&text("auto")) {
            return Err("a client's opening ran AUTO".to_owned());
        }
        let dry_run_denied = decision.payload.get("dry_run") != Some(&text("allow"));
        let owners_expected = match (case.verdict, case.autonomy, case.held) {
            _ if dry_run_denied => (Value::Null, Value::Null),
            (_, Autonomy::Deny, _) => (text("deny"), text("rule:owner-deny")),
            (Verdict::Allows, Autonomy::Auto, _) => (text("auto"), text("rule:owner-auto")),
            (Verdict::Allows | Verdict::Defers, Autonomy::Ask, true) => {
                (text("ask"), text("rule:owner-ask"))
            }
            (Verdict::Defers, Autonomy::Auto, true) => (text("ask"), text("policy_overlay")),
            (Verdict::Allows | Verdict::Defers, _, false) => (text("auto"), text("owner_request")),
            (Verdict::Denies, _, _) => {
                return Err("a request the dry run denied is recorded allowed".to_owned());
            }
        };
        let decided = (
            decision
                .payload
                .get("autonomy")
                .cloned()
                .unwrap_or(Value::Null),
            decision
                .payload
                .get("decided_by")
                .cloned()
                .unwrap_or(Value::Null),
        );
        if !*client && decided != owners_expected {
            return Err(format!(
                "the owner's request decided {decided:?}, not {owners_expected:?} (item 10)"
            ));
        }
        if decision.payload.get("qty") != Some(&text(plan.qty)) {
            return Err(format!(
                "qty {:?}, not the plan's",
                decision.payload.get("qty")
            ));
        }
        let auto = decision.payload.get("autonomy") == Some(&text("auto"));
        let allowed = decision.payload.get("dry_run") == Some(&text("allow"));
        let intents = shell
            .agent_journal
            .iter()
            .filter(|e| {
                e.event_type == "IntentProposed"
                    && e.causation_id.as_ref() == Some(&decision.event_id)
            })
            .count();
        let expected = usize::from(auto && allowed);
        if intents != expected {
            return Err(format!(
                "{intents} intents after an auto {auto}, allowed {allowed} decision"
            ));
        }
    }
    let decisions: BTreeMap<&EventId, &FoldedEvent> = shell
        .agent_journal
        .iter()
        .filter(|e| e.event_type == "DecisionMade")
        .map(|e| (&e.event_id, e))
        .collect();
    for intent in shell
        .agent_journal
        .iter()
        .filter(|e| e.event_type == "IntentProposed")
    {
        let decision = intent
            .causation_id
            .as_ref()
            .and_then(|cause| decisions.get(cause))
            .ok_or_else(|| "an intent no decision caused".to_owned())?;
        let auto = decision.payload.get("autonomy") == Some(&text("auto"));
        let allowed = decision.payload.get("dry_run") == Some(&text("allow"));
        if !(auto && allowed) {
            return Err(format!("an intent after {:?}", decision.payload));
        }
    }
    Ok(())
}

#[test]
fn owner_requests_hold_dec_2858s_invariants() -> Result<(), String> {
    let mut runner = TestRunner::new(ProptestConfig::with_cases(256));
    runner
        .run(&cases(), |case| {
            one_run(&case).map_err(proptest::test_runner::TestCaseError::fail)
        })
        .map_err(|e| e.to_string())
}

/// A plan that sizes and classifies as [`OwnerPlan`] does but keeps the port's default for step
/// 5d's reviewers, as a plan that cannot judge them would.
struct Unjudging(OwnerPlan);

impl OrderPlan for Unjudging {
    fn plan(&self, view: &MandateView, inputs: &SignalInputs) -> Option<Proposal> {
        self.0.plan(view, inputs)
    }

    fn classify(&self, view: &MandateView, proposal: &Proposal) -> Classified {
        self.0.classify(view, proposal)
    }

    fn requested(&self, view: &MandateView, request: &OwnerRequest) -> Option<Proposal> {
        self.0.requested(view, request)
    }
}

/// `AGENTS.md` rule 3 on step 5d (DEC-2859): a plan that cannot say whether a reviewer holds the
/// owner's request holds it, so the ASK stays an ASK and nothing is handed.
#[test]
fn a_plan_that_cannot_judge_the_reviewers_holds_the_owners_ask() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        Unjudging(OwnerPlan::classified(Autonomy::Ask)),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let mut shell = started(&ports);
    let ran = tail(&mut shell, &Request::owner_buy(1).event(), &ports);
    let decision = only(&ran, "DecisionMade");
    assert_eq!(member(decision, "autonomy"), &text("ask"));
    assert_eq!(member(decision, "decided_by"), &text("rule:owner-ask"));
    assert!(ran.handed.is_empty());
}
