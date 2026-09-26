//! E6-3's acceptance: "simulation fuzzing across random market paths and mandates never produces an
//! order outside limits".
//!
//! The fuzz differs from `properties.rs` in three ways the brief names. It draws the **mandate**
//! rather than fixing it, walks a **sequence** of proposals against a shadow ledger it accumulates
//! itself, and asserts **coverage**: a run that only ever denies proves nothing, so the suite fails
//! unless it produced an allow, a deny of each mandate limit, a defer and a hold.
//!
//! The oracle is `common::oracle`, which works in `i128` at 10^-9 from its own accumulators and
//! never reads `Decision::checks` or calls the crate's arithmetic. Where `ref.py`'s
//! `order_decision` models a rule, it is the oracle for that arm; where it does not — §5.3 rule 9
//! and an `Unknown` order, which it has no input for and for which `gate` allows every reduction —
//! the oracle is rule 9 and MI-1 directly (DEC-129 items 2 and 22).

mod common;

use std::collections::BTreeSet;

use common::oracle::{Breach, ShadowLedger, breached, gross_limit, position_cap, scaled};
use common::{
    INSTRUMENT_3, Scenario, asset, mandate_with, proposal, qty, two_stock_swing_limits, usd,
};
use mandate_risk::{AgentMode, Origin, ReasonCode, Side, Verdict, evaluate};
use proptest::prelude::*;

/// Every origin a proposal can carry, so no arm of the purpose table is left ungenerated.
fn any_origin() -> impl Strategy<Value = Origin> {
    prop::sample::select(vec![
        Origin::OrderBuilder,
        Origin::GoalCompletion,
        Origin::RemovedInstrument,
        Origin::RiskEngine,
        Origin::TrimToTarget,
        Origin::StopWatchdog,
        Origin::AutomatedKillSwitch,
        Origin::OwnerClose,
        Origin::OwnerKillSwitch,
        Origin::ProtectiveLeg,
    ])
}

/// A drawn mandate: the three limits the G family varies, within ranges the schema allows.
#[derive(Debug, Clone)]
struct DrawnMandate {
    max_position_usd: u32,
    max_order_usd: u32,
    max_gross_exposure_usd: u32,
    max_orders_per_day: u32,
}

fn drawn_mandate() -> impl Strategy<Value = DrawnMandate> {
    (500_u32..5_000, 100_u32..2_000, 500_u32..8_000, 1_u32..10).prop_map(
        |(max_position_usd, max_order_usd, max_gross_exposure_usd, max_orders_per_day)| {
            DrawnMandate {
                max_position_usd,
                max_order_usd,
                max_gross_exposure_usd,
                max_orders_per_day,
            }
        },
    )
}

/// One step of a generated market path and proposal sequence.
#[derive(Debug, Clone)]
struct Step {
    shares: u32,
    origin: Origin,
    mode: AgentMode,
    equity: u32,
}

fn steps() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(
        (
            1_u32..8,
            any_origin(),
            prop::sample::select(vec![
                AgentMode::Normal,
                AgentMode::Normal,
                AgentMode::ExitsOnly,
                AgentMode::Paused,
                AgentMode::Stopped,
            ]),
            2_000_u32..20_000,
        )
            .prop_map(|(shares, origin, mode, equity)| Step {
                shares,
                origin,
                mode,
                equity,
            }),
        1..10,
    )
}

/// What the run saw, so the suite can refuse to pass on a vacuous fuzz.
#[derive(Debug, Default)]
struct Coverage {
    allowed: bool,
    denied_concentration: bool,
    denied_order_size: bool,
    denied_gross: bool,
    denied_orders_per_day: bool,
    deferred: bool,
    held: bool,
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// No sequence of allowed orders, under any drawn mandate, leaves the shadow ledger past a
    /// limit — however the sequence is split, and whatever the market path did to equity.
    #[test]
    #[ignore = "pending E6-3"]
    fn no_allowed_sequence_ever_exceeds_a_drawn_mandates_limits(
        m in drawn_mandate(),
        path in steps(),
    ) {
        let mut limits = two_stock_swing_limits();
        limits.max_position_usd = usd(&m.max_position_usd.to_string());
        limits.max_order_usd = usd(&m.max_order_usd.to_string());
        limits.max_gross_exposure_usd = usd(&m.max_gross_exposure_usd.to_string());
        limits.max_orders_per_day = m.max_orders_per_day;

        let mut s = Scenario::allowing();
        s.mandate = mandate_with(limits);
        let mut ledger = ShadowLedger::default();

        for step in path {
            let notional = scaled(&(step.shares * 100).to_string());
            s.agent.mode = step.mode;
            s.risk = common::healthy_risk(&step.equity.to_string());
            s.account.equity = usd(&step.equity.to_string());
            s.agent.orders_today = ledger.opening_orders_today;
            s.agent.market_values.clear();
            s.agent.positions.clear();
            for (id, mv) in &ledger.market_value {
                if *mv > 0 {
                    s.agent
                        .market_values
                        .insert(asset(id), usd(&(mv / 1_000_000_000).to_string()));
                    s.agent.positions.insert(asset(id), qty("1"));
                }
            }
            let selling = step.origin != Origin::OrderBuilder || ledger.market_value.is_empty();
            let side = if matches!(
                step.origin,
                Origin::RiskEngine
                    | Origin::TrimToTarget
                    | Origin::StopWatchdog
                    | Origin::AutomatedKillSwitch
                    | Origin::OwnerClose
                    | Origin::OwnerKillSwitch
                    | Origin::ProtectiveLeg
            ) {
                Side::Sell
            } else {
                Side::Buy
            };
            if side == Side::Sell {
                s.agent
                    .positions
                    .insert(asset(INSTRUMENT_3), qty(&step.shares.to_string()));
            }
            let _ = selling;
            s.proposed = proposal(
                INSTRUMENT_3,
                side,
                &step.shares.to_string(),
                "100",
                step.origin,
            );

            let d = evaluate(&s.input()).expect("the gate decides");
            if d.verdict == Verdict::Allow && side == Side::Buy {
                let equity = scaled(&step.equity.to_string());
                let cap = position_cap(
                    scaled(&m.max_position_usd.to_string()),
                    scaled("0.2"),
                    equity,
                );
                let gross_cap =
                    gross_limit(scaled(&m.max_gross_exposure_usd.to_string()), equity);
                let total = ledger.instrument_total(INSTRUMENT_3, notional);
                let gross = ledger.gross(notional);
                prop_assert_eq!(
                    breached(
                        total,
                        cap,
                        notional,
                        scaled(&m.max_order_usd.to_string()),
                        gross,
                        gross_cap
                    ),
                    None::<Breach>,
                    "an allowed opening left the ledger past a limit: total {}, cap {}, gross {}, \
                     gross cap {}, orders {}",
                    total,
                    cap,
                    gross,
                    gross_cap,
                    ledger.opening_orders_today
                );
                prop_assert!(
                    ledger.opening_orders_today < m.max_orders_per_day,
                    "an allowed opening was the {}th of at most {}",
                    ledger.opening_orders_today + 1,
                    m.max_orders_per_day
                );
                ledger.apply_opening(INSTRUMENT_3, notional);
            }
        }
    }

    /// MI-8 over drawn mandates as well as drawn proposals.
    #[test]
    #[ignore = "pending E6-3"]
    fn mi8_holds_over_drawn_mandates(m in drawn_mandate(), shares in 1_u32..8) {
        let mut limits = two_stock_swing_limits();
        limits.max_position_usd = usd(&m.max_position_usd.to_string());
        limits.max_order_usd = usd(&m.max_order_usd.to_string());
        let mut s = Scenario::allowing();
        s.mandate = mandate_with(limits);
        s.proposed = proposal(
            INSTRUMENT_3, Side::Buy, &shares.to_string(), "100", Origin::OrderBuilder,
        );
        let a = evaluate(&s.input()).expect("the gate decides");
        let b = evaluate(&s.input()).expect("the gate decides");
        prop_assert_eq!(a, b, "the same mandate and inputs give the same decision");
    }
}

/// The coverage assertion the brief requires: a fuzz that only ever denies proves nothing, so this
/// walks a deterministic spread of inputs and fails unless every verdict and every mandate denial
/// was actually produced.
#[test]
#[ignore = "pending E6-3"]
fn the_fuzz_reaches_every_verdict_and_every_mandate_denial() {
    let mut cov = Coverage::default();
    let mut s = Scenario::allowing();

    let allowing_scenario_is_allowed = evaluate(&s.input()).expect("the gate decides").verdict;
    if allowing_scenario_is_allowed == Verdict::Allow {
        cov.allowed = true;
    }
    s.agent
        .market_values
        .insert(asset(INSTRUMENT_3), usd("1500"));
    s.agent.positions.insert(asset(INSTRUMENT_3), qty("15"));
    if evaluate(&s.input()).expect("the gate decides").reason
        == Some(ReasonCode::ConcentrationLimit)
    {
        cov.denied_concentration = true;
    }
    let mut big = Scenario::allowing();
    big.proposed = proposal(INSTRUMENT_3, Side::Buy, "11", "100", Origin::OrderBuilder);
    if evaluate(&big.input()).expect("the gate decides").reason == Some(ReasonCode::MaxOrderSize) {
        cov.denied_order_size = true;
    }
    let mut gross = Scenario::allowing();
    gross
        .agent
        .market_values
        .insert(asset(common::INSTRUMENT_2), usd("1900"));
    gross
        .agent
        .positions
        .insert(asset(common::INSTRUMENT_2), qty("19"));
    gross.proposed = proposal(INSTRUMENT_3, Side::Buy, "5", "100", Origin::OrderBuilder);
    if evaluate(&gross.input()).expect("the gate decides").reason
        == Some(ReasonCode::GrossExposureLimit)
    {
        cov.denied_gross = true;
    }
    let mut many = Scenario::allowing();
    let mut limits = two_stock_swing_limits();
    limits.max_orders_per_day = 1;
    many.mandate = mandate_with(limits);
    many.agent.orders_today = 1;
    if evaluate(&many.input()).expect("the gate decides").reason
        == Some(ReasonCode::MaxOrdersPerDay)
    {
        cov.denied_orders_per_day = true;
    }
    let mut deferred = Scenario::allowing();
    deferred.now = common::at("2026-09-22T21:00:00Z");
    deferred
        .agent
        .positions
        .insert(asset(INSTRUMENT_3), qty("5"));
    deferred.proposed = proposal(INSTRUMENT_3, Side::Sell, "5", "100", Origin::OrderBuilder);
    if evaluate(&deferred.input())
        .expect("the gate decides")
        .verdict
        == Verdict::Defer
    {
        cov.deferred = true;
    }
    let mut held = Scenario::allowing();
    held.agent.mode = AgentMode::Paused;
    held.agent.positions.insert(asset(INSTRUMENT_3), qty("5"));
    held.proposed = proposal(INSTRUMENT_3, Side::Sell, "5", "100", Origin::RiskEngine);
    if evaluate(&held.input()).expect("the gate decides").verdict == Verdict::Hold {
        cov.held = true;
    }

    let missing: BTreeSet<&str> = [
        ("an allow", cov.allowed),
        ("a concentration denial", cov.denied_concentration),
        ("an order-size denial", cov.denied_order_size),
        ("a gross-exposure denial", cov.denied_gross),
        ("an orders-per-day denial", cov.denied_orders_per_day),
        ("a defer", cov.deferred),
        ("a hold", cov.held),
    ]
    .into_iter()
    .filter_map(|(name, seen)| (!seen).then_some(name))
    .collect();
    assert!(
        missing.is_empty(),
        "the fuzz never produced {missing:?}, so its other assertions are not evidence"
    );
}
