//! E6-3's acceptance: "simulation fuzzing across random market paths and mandates never produces an
//! order outside limits".
//!
//! Three things separate this from `properties.rs`. It draws the **mandate** rather than fixing it;
//! it walks a **sequence** against a shadow ledger it accumulates itself, so a split sequence cannot
//! creep past a limit; and it records **coverage inside the run**, in a counter the proptest cases
//! share, so a run that only ever denies fails rather than passing on vacuous assertions.
//!
//! The oracle is `common::oracle`: `i128` at 10^-9, its own accumulators, never
//! `Decision::checks` and never the crate's arithmetic. Where `ref.py`'s `order_decision` models a
//! rule it is the oracle for that arm; where it does not — §5.3 rule 9 and an `Unknown` order, for
//! which it has no input and for which `gate` allows every reduction — the oracle is rule 9 and
//! MI-1 directly (DEC-129 items 2 and 22).

mod common;

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU32, Ordering};

use common::oracle::{
    Breach, Limits, Proposal, ShadowLedger, breached, gross_limit, position_cap, scaled,
};
use common::{
    INSTRUMENT_2, INSTRUMENT_3, Scenario, asset, mandate_with, proposal, qty,
    two_stock_swing_limits, usd,
};
use mandate_risk::{AgentMode, GroupId, Origin, ReasonCode, Side, Verdict, evaluate};
use proptest::prelude::*;

/// What the proptest run actually reached, counted across cases. A fuzz whose assertions never see
/// an allow, or never see a limit bind, is not evidence that the limits hold.
static SAW_ALLOW: AtomicU32 = AtomicU32::new(0);
static SAW_CONCENTRATION: AtomicU32 = AtomicU32::new(0);
static SAW_ORDER_SIZE: AtomicU32 = AtomicU32::new(0);
static SAW_GROSS: AtomicU32 = AtomicU32::new(0);
static SAW_ORDERS_PER_DAY: AtomicU32 = AtomicU32::new(0);
static SAW_COOLDOWN: AtomicU32 = AtomicU32::new(0);
static SAW_UNIVERSE: AtomicU32 = AtomicU32::new(0);
static SAW_DEFER: AtomicU32 = AtomicU32::new(0);
static SAW_HOLD: AtomicU32 = AtomicU32::new(0);

fn note(d: &mandate_risk::Decision) {
    let counter = match (d.verdict, d.reason) {
        (Verdict::Allow, _) => &SAW_ALLOW,
        (Verdict::Defer, _) => &SAW_DEFER,
        (Verdict::Hold, _) => &SAW_HOLD,
        (Verdict::Deny, Some(ReasonCode::ConcentrationLimit)) => &SAW_CONCENTRATION,
        (Verdict::Deny, Some(ReasonCode::MaxOrderSize)) => &SAW_ORDER_SIZE,
        (Verdict::Deny, Some(ReasonCode::GrossExposureLimit)) => &SAW_GROSS,
        (Verdict::Deny, Some(ReasonCode::MaxOrdersPerDay)) => &SAW_ORDERS_PER_DAY,
        (Verdict::Deny, Some(ReasonCode::ReentryCooldown)) => &SAW_COOLDOWN,
        (Verdict::Deny, Some(ReasonCode::NotInWorkingUniverse)) => &SAW_UNIVERSE,
        (Verdict::Deny, _) => return,
    };
    counter.fetch_add(1, Ordering::Relaxed);
}

/// A drawn mandate, a drawn market path and a drawn sequence of proposals.
#[derive(Debug, Clone)]
struct Drawn {
    max_position_usd: u32,
    max_order_usd: u32,
    max_gross_exposure_usd: u32,
    max_orders_per_day: u32,
    reentry_cooldown_s: u32,
}

fn drawn_mandate() -> impl Strategy<Value = Drawn> {
    (
        300_u32..4_000,
        100_u32..1_500,
        400_u32..6_000,
        1_u32..6,
        prop::sample::select(vec![0_u32, 60, 1_800, 3_600]),
    )
        .prop_map(
            |(
                max_position_usd,
                max_order_usd,
                max_gross_exposure_usd,
                max_orders_per_day,
                reentry_cooldown_s,
            )| Drawn {
                max_position_usd,
                max_order_usd,
                max_gross_exposure_usd,
                max_orders_per_day,
                reentry_cooldown_s,
            },
        )
}

/// One step: a proposal, the market path's equity at that moment, the agent's mode, whether the
/// instrument is in the universe, and how long since the group's last exit fill.
#[derive(Debug, Clone)]
struct Step {
    shares: u32,
    origin: Origin,
    mode: AgentMode,
    equity: u32,
    in_universe: bool,
    seconds_since_exit: Option<i64>,
    other_instrument: bool,
}

fn steps() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(
        (
            1_u32..8,
            prop::sample::select(vec![
                Origin::OrderBuilder,
                Origin::OrderBuilder,
                Origin::RiskEngine,
                Origin::AutomatedKillSwitch,
                Origin::OwnerClose,
                Origin::OwnerKillSwitch,
                Origin::ProtectiveLeg,
                Origin::TrimToTarget,
                Origin::GoalCompletion,
            ]),
            prop::sample::select(vec![
                AgentMode::Normal,
                AgentMode::Normal,
                AgentMode::Normal,
                AgentMode::ExitsOnly,
                AgentMode::Paused,
                AgentMode::Stopped,
            ]),
            1_000_u32..20_000,
            prop::sample::select(vec![true, true, true, false]),
            prop::option::of(0_i64..7_200),
            any::<bool>(),
        )
            .prop_map(
                |(
                    shares,
                    origin,
                    mode,
                    equity,
                    in_universe,
                    seconds_since_exit,
                    other_instrument,
                )| Step {
                    shares,
                    origin,
                    mode,
                    equity,
                    in_universe,
                    seconds_since_exit,
                    other_instrument,
                },
            ),
        1..10,
    )
}

fn is_sell(origin: Origin) -> bool {
    matches!(
        origin,
        Origin::RiskEngine
            | Origin::TrimToTarget
            | Origin::StopWatchdog
            | Origin::AutomatedKillSwitch
            | Origin::OwnerClose
            | Origin::OwnerKillSwitch
            | Origin::ProtectiveLeg
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// No sequence of allowed openings, under any drawn mandate and any drawn market path, leaves
    /// the shadow ledger past a limit.
    #[test]
    fn no_allowed_sequence_ever_exceeds_a_drawn_mandates_limits(
        m in drawn_mandate(),
        path in steps(),
    ) {
        let mut limits = two_stock_swing_limits();
        limits.max_position_usd = usd(&m.max_position_usd.to_string());
        limits.max_order_usd = usd(&m.max_order_usd.to_string());
        limits.max_gross_exposure_usd = usd(&m.max_gross_exposure_usd.to_string());
        limits.max_orders_per_day = m.max_orders_per_day;
        limits.reentry_cooldown_s = m.reentry_cooldown_s;

        let mut s = Scenario::allowing();
        s.mandate = mandate_with(limits);
        s.agent.instrument_groups.insert(asset(INSTRUMENT_2), GroupId(7));
        s.agent.instrument_groups.insert(asset(INSTRUMENT_3), GroupId(7));
        let mut ledger = ShadowLedger::default();

        for step in path {
            let instrument = if step.other_instrument { INSTRUMENT_2 } else { INSTRUMENT_3 };
            let notional = scaled(&(step.shares * 100).to_string());
            let selling = is_sell(step.origin);

            s.agent.mode = step.mode;
            s.risk = common::healthy_risk(&step.equity.to_string());
            s.account.equity = usd(&step.equity.to_string());
            s.agent.orders_today = ledger.opening_orders_today;
            s.instrument = common::equity_instrument(instrument);
            s.agent.market_values.clear();
            s.agent.positions.clear();
            for (id, mv) in &ledger.market_value {
                if *mv > 0 {
                    s.agent.market_values.insert(asset(id), usd(&(mv / 1_000_000_000).to_string()));
                    s.agent.positions.insert(asset(id), qty("1"));
                }
            }
            if selling {
                s.agent.positions.insert(asset(instrument), qty(&step.shares.to_string()));
            }
            s.universe = if step.in_universe {
                common::working_universe(&[INSTRUMENT_2, INSTRUMENT_3])
            } else {
                common::working_universe(&[])
            };
            s.agent.last_exit_fill_at.clear();
            if let Some(secs) = step.seconds_since_exit {
                let exit_at = mandate_time::UtcNanos::from_parts(s.now.secs() - secs, 0)
                    .expect("the test instant is in range");
                s.agent.last_exit_fill_at.insert(asset(INSTRUMENT_2), exit_at);
            }
            s.proposed = proposal(
                instrument,
                if selling { Side::Sell } else { Side::Buy },
                &step.shares.to_string(),
                "100",
                step.origin,
            );

            let d = evaluate(&s.input()).expect("the gate decides");
            note(&d);

            if d.verdict == Verdict::Allow && !selling {
                let equity = scaled(&step.equity.to_string());
                let oracle_limits = Limits {
                    cap: position_cap(
                        scaled(&m.max_position_usd.to_string()), scaled("0.2"), equity,
                    ),
                    max_order: scaled(&m.max_order_usd.to_string()),
                    gross_cap: gross_limit(scaled(&m.max_gross_exposure_usd.to_string()), equity),
                    max_orders_per_day: m.max_orders_per_day,
                    reentry_cooldown_s: i64::from(m.reentry_cooldown_s),
                };
                let p = Proposal {
                    instrument_total: ledger.instrument_total(instrument, notional),
                    order: notional,
                    gross: ledger.gross(notional),
                    orders_today: ledger.opening_orders_today,
                    seconds_since_group_exit: step.seconds_since_exit,
                    in_universe: step.in_universe,
                };
                prop_assert_eq!(
                    breached(p, oracle_limits),
                    None::<Breach>,
                    "an allowed opening left the ledger past a limit: {:?} against {:?}",
                    p,
                    oracle_limits
                );
                ledger.apply_opening(instrument, notional);
            }

            if selling {
                prop_assert!(
                    d.verdict != Verdict::Deny
                        || d.reason == Some(ReasonCode::AccountTradingBlocked),
                    "MI-1: a reducing purpose was denied {:?}",
                    d.reason
                );
            }
        }
    }

    /// MI-8 over drawn mandates as well as drawn proposals.
    #[test]
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

/// The coverage gate, over what the proptest run above actually reached.
///
/// Named to sort last so the run has happened: nextest runs each test in its own process, so this
/// asserts over the counters the sequence property filled **in this process**. If the sequence
/// property never ran — a filter, a shrink to nothing, a rename — every counter is zero and this
/// fails, which is the point: a fuzz that produced no allow and no binding limit is not evidence,
/// and must not report success.
#[test]
fn zz_the_fuzz_run_reached_every_verdict_and_every_mandate_limit() {
    no_allowed_sequence_ever_exceeds_a_drawn_mandates_limits();

    let seen: Vec<(&str, u32)> = vec![
        ("an allow", SAW_ALLOW.load(Ordering::Relaxed)),
        (
            "a concentration denial",
            SAW_CONCENTRATION.load(Ordering::Relaxed),
        ),
        (
            "an order-size denial",
            SAW_ORDER_SIZE.load(Ordering::Relaxed),
        ),
        ("a gross-exposure denial", SAW_GROSS.load(Ordering::Relaxed)),
        (
            "an orders-per-day denial",
            SAW_ORDERS_PER_DAY.load(Ordering::Relaxed),
        ),
        (
            "a re-entry-cooldown denial",
            SAW_COOLDOWN.load(Ordering::Relaxed),
        ),
        (
            "a working-universe denial",
            SAW_UNIVERSE.load(Ordering::Relaxed),
        ),
        ("a hold", SAW_HOLD.load(Ordering::Relaxed)),
    ];
    let missing: BTreeSet<&str> = seen
        .iter()
        .filter_map(|(name, n)| (*n == 0).then_some(*name))
        .collect();
    assert!(
        missing.is_empty(),
        "the fuzz run never produced {missing:?}, so its other assertions are not evidence; \
         counts were {seen:?}"
    );
}
