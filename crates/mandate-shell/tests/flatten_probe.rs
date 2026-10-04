//! The flatten probe's flip, tests first (DEC-77 stage 1; E7-7 stream L, claim #171): what
//! `RiskExitPath` must do once it binds `mandate_risk::agent_flatten`.
//!
//! Every test here is pending on the flip: the adapter still refuses with its own
//! `Cause::Unimplemented` report, and each test fails by that refusal propagating (DEC-110,
//! DEC-137). The implementation PR deletes the markers and changes nothing else here. The
//! fail-closed suite keeps its own pin of today's refusal
//! (`the_production_exit_probes_answer_unimplemented`), with its construction updated to the
//! reshaped stub's `synthetic()` — the probe's meaning is unchanged.
//!
//! The oracle is the risk crate itself: `plan` must equal `agent_flatten`'s plan mapped into the
//! runtime's types by the mapping written once below, so the adapter invents nothing
//! (DEC-166 item 2).

use std::collections::BTreeMap;

use mandate_accounting::InstrumentId;
use mandate_num::{Fraction, Qty, Usd};
use mandate_risk::{
    AgentId, AgentPosition, AssetId, ClientOrderId, FlattenInitiator, FlattenInput, FlattenPlan,
    Side, WorkingOrder, agent_flatten,
};
use mandate_runtime::{FlattenLeg, FlattenPlan as RuntimePlan, FlattenRequest, Initiator, Purpose};
use mandate_shell::adapters::{FlattenState, RiskExitPath};
use mandate_shell::stages::ExitPath;
use mandate_time::Date;

/// Ten shares held, one resting protective sell of four, and a broker position of fifteen: the
/// shape §5.5 exists for (MC-F01's, at the shell). The plan must sell exactly the sub-ledger's
/// ten, never the broker's fifteen, and cancel only this agent's own orders.
fn state() -> FlattenState {
    FlattenState {
        agent: AgentId(1),
        open_orders: BTreeMap::from([(
            ClientOrderId(7),
            WorkingOrder {
                agent: AgentId(1),
                instrument: AssetId::new("a").unwrap(),
                side: Side::Sell,
                max_cost: Usd::parse("0").unwrap(),
                open_qty: Qty::parse("4").unwrap(),
                protective: true,
                opening: false,
                submitted_on: Date::parse("2026-09-25").unwrap(),
            },
        )]),
        agent_positions: vec![AgentPosition {
            agent: AgentId(1),
            instrument: AssetId::new("a").unwrap(),
            asset_class: mandate_risk::AssetClass::UsEquity,
            qty: Qty::parse("10").unwrap(),
        }],
        broker_positions: BTreeMap::from([(AssetId::new("a").unwrap(), Qty::parse("15").unwrap())]),
        session: mandate_risk::Session::Regular,
        max_exit_offset: Fraction::parse("0.03").unwrap(),
        owner_floor_price: None,
    }
}

/// A risk-limit kill switch: the whole agent, nothing confirmed, the fold's outstanding ids.
fn kill_switch(working_orders: Vec<String>) -> FlattenRequest {
    FlattenRequest {
        initiator: Initiator::RiskLimit,
        instrument: None,
        confirmation: None,
        working_orders,
    }
}

/// The input the contract defines: the state's orders that the request names (its ids as
/// strings), the state's positions, session and prices, the request's initiator mapped — `Owner`
/// to `Owner`, `RiskLimit` to `RiskLimit` — and the request's confirmation as the confirmed bid.
fn input_of<'a>(state: &'a FlattenState, request: &FlattenRequest) -> FlattenInput<'a> {
    let _ = request;
    FlattenInput {
        agent: state.agent,
        open_orders: &state.open_orders,
        agent_positions: &state.agent_positions,
        broker_positions: &state.broker_positions,
        session: state.session,
        initiator: FlattenInitiator::RiskLimit,
        owner_confirmed_bid: None,
        max_exit_offset: state.max_exit_offset,
        owner_floor_price: state.owner_floor_price,
    }
}

/// The mapping the adapter owes, written once so the oracle can use it: the risk crate's plan in
/// the runtime's types. Cancels become the strings the fold keys orders by; each sell and each
/// deferred sell becomes a leg, a deferred one waiting for the regular session; the flatten
/// purpose carries over; the request's confirmation carries over; the account-wide endpoints and
/// the mode-first flag have no runtime counterpart, because the runtime type cannot carry them
/// (§5.5's unrepresentable half). A sell's pricing is re-derived by the executor, so it is not
/// carried.
fn runtime_plan_of(
    plan: &FlattenPlan,
    state: &FlattenState,
    request: &FlattenRequest,
) -> RuntimePlan {
    let class_of = |instrument: &AssetId| {
        state
            .agent_positions
            .iter()
            .find(|position| &position.instrument == instrument)
            .map(|position| position.asset_class)
            .unwrap()
    };
    let leg_of = |instrument: &AssetId, qty: Qty, deferred: bool| FlattenLeg {
        instrument: InstrumentId::new(instrument.as_str()).unwrap(),
        asset_class: class_of(instrument),
        qty,
        deferred_to_regular_session: deferred,
    };
    RuntimePlan {
        cancel_client_order_ids: plan
            .cancel_client_order_ids
            .iter()
            .map(|id| id.0.to_string())
            .collect(),
        sells: plan
            .sells
            .iter()
            .map(|sell| leg_of(&sell.instrument, sell.qty, false))
            .chain(
                plan.deferred_sells
                    .iter()
                    .map(|sell| leg_of(&sell.instrument, sell.qty, true)),
            )
            .collect(),
        purpose: Purpose::Flatten,
        confirmation: request.confirmation.clone(),
    }
}

/// TI-4: the probe answers whether an agent-scoped flatten can be planned at all, over the
/// state's own synthetic kill switch. It is asked once, before anything is armed, so a run that
/// cannot plan its exit never opens a position (task brief, step 17).
#[ignore = "pending E7-7"]
#[test]
fn the_probe_answers_ok_over_the_synthetic_kill_switch() {
    RiskExitPath::new(state()).probe().unwrap();
}

/// The plan is the risk crate's own plan for the same state and request, mapped into the
/// runtime's types by the one mapping above: the adapter invents nothing.
#[ignore = "pending E7-7"]
#[test]
fn the_plan_is_the_risk_crates_own_plan_for_the_same_state_and_request() {
    let exit = RiskExitPath::new(state());
    let request = kill_switch(vec!["7".to_owned()]);
    let plan = exit.plan(&request).unwrap();

    let state = state();
    let oracle = agent_flatten(&input_of(&state, &request)).unwrap();
    assert_eq!(plan, runtime_plan_of(&oracle, &state, &request));
}

/// §5.5's structural pins: the plan sells exactly the sub-ledger — ten, never the broker's
/// fifteen — as a flatten whose cancels name only this agent's own orders, and a regular session
/// defers nothing.
#[ignore = "pending E7-7"]
#[test]
fn the_plan_sells_the_sub_ledger_and_never_the_brokers_position() {
    let exit = RiskExitPath::new(state());
    let plan = exit.plan(&kill_switch(vec!["7".to_owned()])).unwrap();

    assert_eq!(plan.purpose, Purpose::Flatten);
    let sold: Vec<Qty> = plan.sells.iter().map(|leg| leg.qty).collect();
    assert_eq!(sold, vec![Qty::parse("10").unwrap()]);
    assert!(
        plan.sells
            .iter()
            .all(|leg| !leg.deferred_to_regular_session)
    );
    assert!(
        plan.cancel_client_order_ids.iter().all(|id| id == "7"),
        "cancels name only the agent's own working order"
    );
}

/// The probe and the plan read the same folded state, so two plans over the same request are
/// equal: a replay shares the plan the first run made (task brief, "Durability").
#[ignore = "pending E7-7"]
#[test]
fn two_plans_over_the_same_state_and_request_are_equal() {
    let exit = RiskExitPath::new(state());
    let request = kill_switch(vec!["7".to_owned()]);
    assert_eq!(exit.plan(&request).unwrap(), exit.plan(&request).unwrap());
}
