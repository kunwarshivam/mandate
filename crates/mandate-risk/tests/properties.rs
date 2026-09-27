//! One property per invariant and per "never" or "always" in trading-domain spec §9.
//!
//! Every property computes its expectation its own way — through `common::oracle`, which
//! accumulates in `i128` at 10^-9 and never calls the crate's arithmetic — so a property cannot
//! pass because the gate and its oracle share a mistake. The generators are deliberately small and
//! total: whole-dollar prices, whole-share quantities and a single instrument, because what these
//! properties check is the decision, not the arithmetic's edge cases, which
//! `hand::` and `mandate-num`'s own suite pin.

mod common;

use common::oracle::{Breach, ShadowLedger, breached, gross_limit, position_cap, scaled};
use common::{INSTRUMENT_2, INSTRUMENT_3, Scenario, asset, proposal, qty, usd};
use mandate_risk::{AgentMode, CheckOutcome, Origin, Purpose, ReasonCode, Side, Verdict, evaluate};
use proptest::prelude::*;

/// Purposes that reduce risk. MI-1 is about exactly these four.
const REDUCING: [Origin; 7] = [
    Origin::OrderBuilder,
    Origin::GoalCompletion,
    Origin::RemovedInstrument,
    Origin::RiskEngine,
    Origin::TrimToTarget,
    Origin::StopWatchdog,
    Origin::OwnerClose,
];

fn reducing_origin() -> impl Strategy<Value = Origin> {
    prop::sample::select(REDUCING.to_vec())
}

fn whole_dollars() -> impl Strategy<Value = u32> {
    1_u32..3_000
}

proptest! {
    /// MI-1, scoped to its own words: risk reduction is never denied by a mandate limit, a conduct
    /// control, a session rule, an instrument restriction, the eligibility floor, a day-trade
    /// budget, or buying power. The only denial a reducing purpose may carry is
    /// `account_trading_blocked`, the broker arm of MI-1's own list.
    #[test]
    #[ignore = "pending E6-3"]
    fn mi1_reduction_is_never_denied_by_a_limit(
        origin in reducing_origin(),
        position in 1_u32..50,
        limit in whole_dollars(),
        orders_today in 0_u32..40,
        mode in prop::sample::select(vec![
            AgentMode::Normal, AgentMode::ExitsOnly, AgentMode::Paused, AgentMode::Stopped,
        ]),
        blocked in any::<bool>(),
        unknown in any::<bool>(),
    ) {
        let mut s = Scenario::allowing();
        s.agent.orders_today = orders_today;
        s.agent.mode = mode;
        if blocked {
            s.account.state = mandate_risk::AccountState::Blocked;
        }
        if unknown {
            s.account.unknown_orders.insert(asset(INSTRUMENT_3));
        }
        s.agent.positions.insert(asset(INSTRUMENT_3), qty(&position.to_string()));
        s.agent.market_values.insert(asset(INSTRUMENT_3), usd(&(position * limit).to_string()));
        s.proposed = proposal(
            INSTRUMENT_3, Side::Sell, &position.to_string(), &limit.to_string(), origin,
        );

        let d = evaluate(&s.input()).expect("the gate decides");
        prop_assert!(
            d.verdict != Verdict::Deny || d.reason == Some(ReasonCode::AccountTradingBlocked),
            "a reducing purpose was denied {:?} by something that is not the broker",
            d.reason
        );
        if blocked {
            prop_assert_eq!(
                (d.verdict, d.reason),
                (Verdict::Deny, Some(ReasonCode::AccountTradingBlocked)),
                "the one permitted denial is asserted reachable, not merely tolerated"
            );
        }
    }

    /// A discretionary exit is never denied at all: §9.6 paces it, and a defer is never converted
    /// to a deny.
    #[test]
    #[ignore = "pending E6-8"]
    fn a_discretionary_exit_is_never_denied(
        position in 1_u32..50,
        limit in whole_dollars(),
        paused in any::<bool>(),
    ) {
        let mut s = Scenario::allowing();
        if paused {
            s.agent.mode = AgentMode::Paused;
        }
        s.agent.positions.insert(asset(INSTRUMENT_3), qty(&position.to_string()));
        s.proposed = proposal(
            INSTRUMENT_3, Side::Sell, &position.to_string(), &limit.to_string(),
            Origin::OrderBuilder,
        );

        let d = evaluate(&s.input()).expect("the gate decides");
        prop_assert!(
            matches!(d.verdict, Verdict::Allow | Verdict::Defer | Verdict::Hold),
            "a discretionary exit was denied: {:?}",
            d.reason
        );
    }

    /// A `Hold` carries only the three codes that can hold an order, and the two `agent_*` ones
    /// follow the mode rule exactly. The mode rule has no `Unknown`-order arm: that is check 4's.
    #[test]
    #[ignore = "pending E6-3"]
    fn a_hold_follows_the_mode_rule_exactly(
        origin in prop::sample::select(vec![
            Origin::RiskEngine, Origin::AutomatedKillSwitch, Origin::OwnerClose,
            Origin::OwnerKillSwitch, Origin::ProtectiveLeg, Origin::OrderBuilder,
        ]),
        mode in prop::sample::select(vec![
            AgentMode::Normal, AgentMode::ExitsOnly, AgentMode::Paused, AgentMode::Stopped,
        ]),
    ) {
        let mut s = Scenario::allowing();
        s.agent.mode = mode;
        s.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));
        s.proposed = proposal(INSTRUMENT_3, Side::Sell, "10", "100", origin);

        let d = evaluate(&s.input()).expect("the gate decides");

        let protective = origin == Origin::ProtectiveLeg;
        let kill_switch_written_out_rather_than_asking_the_crate = matches!(
            origin,
            Origin::AutomatedKillSwitch | Origin::OwnerKillSwitch
        );
        let kill_switch = kill_switch_written_out_rather_than_asking_the_crate;
        let expected_hold = mode == AgentMode::Stopped
            || (mode == AgentMode::Paused && !(protective || kill_switch));

        if expected_hold {
            prop_assert_eq!(d.verdict, Verdict::Hold, "the mode rule holds this order");
            prop_assert!(
                d.reason == Some(ReasonCode::AgentStopped)
                    || d.reason == Some(ReasonCode::AgentPaused),
                "a mode hold carries an agent_* code, not {:?}",
                d.reason
            );
        } else {
            prop_assert_ne!(
                d.verdict, Verdict::Hold,
                "the mode rule does not hold {:?} under {:?}", origin, mode
            );
        }
    }

    /// The per-instrument cap is the lower of the dollar and the fraction bound, and a value
    /// exactly at it passes. The oracle recomputes both bounds in `i128`.
    ///
    /// The notional varies through the **quantity** at a price the generated quote supports, never
    /// through the limit price: a price walked far from the quote would trip the collar at check 5
    /// and report `price_outside_collar` before the cap at check 2 was ever reached, so the
    /// property would fail on a correct gate once E6-8 lands.
    #[test]
    #[ignore = "pending E6-3"]
    fn position_cap_is_the_lower_of_both_bounds(
        equity in 1_000_u32..40_000,
        shares in 1_u32..30,
        held in 0_u32..2_000,
    ) {
        let order = shares * 100;
        let mut s = Scenario::allowing();
        s.instrument = common::equity_instrument(INSTRUMENT_2);
        s.risk = common::healthy_risk(&equity.to_string());
        s.account.equity = usd(&equity.to_string());
        if held > 0 {
            s.agent.market_values.insert(asset(INSTRUMENT_2), usd(&held.to_string()));
            s.agent.positions.insert(asset(INSTRUMENT_2), qty("1"));
        }
        s.proposed = proposal(
            INSTRUMENT_2, Side::Buy, &shares.to_string(), "100", Origin::OrderBuilder,
        );

        let cap = position_cap(scaled("1500"), scaled("0.2"), scaled(&equity.to_string()));
        let total = scaled(&held.to_string()) + scaled(&order.to_string());

        let d = evaluate(&s.input()).expect("the gate decides");
        if total > cap {
            prop_assert_eq!(
                d.reason, Some(ReasonCode::ConcentrationLimit),
                "{} is above the cap {}", total, cap
            );
        } else {
            prop_assert_ne!(
                d.reason, Some(ReasonCode::ConcentrationLimit),
                "{} is at or below the cap {}", total, cap
            );
        }
    }

    /// Gross exposure is bounded by equity as well as by the configured limit. As above, the
    /// notional varies through the quantity so the collar cannot fire first.
    #[test]
    #[ignore = "pending E6-3"]
    fn gross_exposure_is_bounded_by_equity(
        equity in 500_u32..5_000,
        held in 0_u32..4_000,
        shares in 1_u32..30,
    ) {
        let order = shares * 100;
        let mut s = Scenario::allowing();
        s.risk = common::healthy_risk(&equity.to_string());
        s.account.equity = usd(&equity.to_string());
        if held > 0 {
            s.agent.market_values.insert(asset(INSTRUMENT_2), usd(&held.to_string()));
            s.agent.positions.insert(asset(INSTRUMENT_2), qty("1"));
        }
        s.proposed = proposal(
            INSTRUMENT_3, Side::Buy, &shares.to_string(), "100", Origin::OrderBuilder,
        );

        let limit = gross_limit(scaled("2000"), scaled(&equity.to_string()));
        let gross = scaled(&held.to_string()) + scaled(&order.to_string());

        let d = evaluate(&s.input()).expect("the gate decides");
        if gross > limit && d.reason != Some(ReasonCode::ConcentrationLimit)
            && d.reason != Some(ReasonCode::MaxOrderSize) {
            prop_assert_eq!(
                d.reason, Some(ReasonCode::GrossExposureLimit),
                "{} is above min(2000, equity {})", gross, equity
            );
        }
    }

    /// Every limit comparison is strictly greater, so a value exactly at a limit passes (MC-G02).
    #[test]
    #[ignore = "pending E6-3"]
    fn a_value_exactly_at_a_limit_passes(order in 1_u32..=1_000) {
        let mut s = Scenario::allowing();
        s.proposed = proposal(
            INSTRUMENT_3, Side::Buy, "1", &order.to_string(), Origin::OrderBuilder,
        );
        let d = evaluate(&s.input()).expect("the gate decides");
        prop_assert_ne!(
            d.reason, Some(ReasonCode::MaxOrderSize),
            "{} is at or below max_order_usd 1000", order
        );
    }

    /// A stricter mode is never more permissive: raising the mode never turns a deny into an allow.
    #[test]
    #[ignore = "pending E6-3"]
    fn a_stricter_mode_is_never_more_permissive(order in whole_dollars()) {
        let mut s = Scenario::allowing();
        s.proposed = proposal(
            INSTRUMENT_3, Side::Buy, "1", &order.to_string(), Origin::OrderBuilder,
        );

        s.agent.mode = AgentMode::Normal;
        let normal = evaluate(&s.input()).expect("the gate decides");
        s.agent.mode = AgentMode::ExitsOnly;
        let stricter = evaluate(&s.input()).expect("the gate decides");

        prop_assert!(
            normal.verdict == Verdict::Allow || stricter.verdict != Verdict::Allow,
            "exits_only allowed an opening that normal denied"
        );
    }

    /// The re-entry cooldown covers every instrument of the group, not the proposed one alone.
    #[test]
    #[ignore = "pending E6-3"]
    fn cooldown_covers_the_whole_group(elapsed_s in 0_i64..7_200) {
        let mut s = Scenario::allowing();
        let exit_at = common::at("2026-09-21T14:00:00Z");
        s.now = mandate_time::UtcNanos::from_parts(exit_at.secs() + elapsed_s, exit_at.nanos())
            .expect("the test instant is in range");
        s.agent.last_exit_fill_at.insert(asset(INSTRUMENT_2), exit_at);
        s.agent.instrument_groups.insert(asset(INSTRUMENT_2), mandate_risk::GroupId(7));
        s.agent.instrument_groups.insert(asset(INSTRUMENT_3), mandate_risk::GroupId(7));

        let d = evaluate(&s.input()).expect("the gate decides");
        if elapsed_s < 3_600 {
            prop_assert_eq!(
                d.reason, Some(ReasonCode::ReentryCooldown),
                "{} s is inside reentry_cooldown_s 3600", elapsed_s
            );
        } else {
            prop_assert_ne!(
                d.reason, Some(ReasonCode::ReentryCooldown),
                "{} s is at or past reentry_cooldown_s 3600", elapsed_s
            );
        }
    }

    /// Every decision lists all eight checks, in §9.1 order, with the ones after a failure marked
    /// `NotReached`.
    #[test]
    #[ignore = "pending E6-3"]
    fn every_decision_lists_the_checks_it_reached(order in whole_dollars()) {
        let mut s = Scenario::allowing();
        s.proposed = proposal(
            INSTRUMENT_3, Side::Buy, "1", &order.to_string(), Origin::OrderBuilder,
        );
        let d = evaluate(&s.input()).expect("the gate decides");

        prop_assert_eq!(d.checks.len(), 8, "every §9.1 check is accounted for");
        let mut seen_failure = false;
        for outcome in &d.checks {
            match outcome {
                CheckOutcome::Failed(_, _) => {
                    prop_assert!(!seen_failure, "the gate stops at the first failure");
                    seen_failure = true;
                }
                CheckOutcome::NotReached(_) => prop_assert!(
                    seen_failure, "a check before the failure is never NotReached"
                ),
                CheckOutcome::Passed(_) => prop_assert!(
                    !seen_failure, "a check after the failure never reads as Passed"
                ),
            }
        }
    }

    /// The eligibility floor fails closed for anything the ETP source has not classified.
    #[test]
    #[ignore = "pending E6-7"]
    fn etp_fails_closed(unclassified in any::<bool>()) {
        let mut s = Scenario::allowing();
        s.instrument.etp = if unclassified {
            mandate_risk::EtpClass::Unclassified
        } else {
            mandate_risk::EtpClass::Complex
        };

        let d = evaluate(&s.input()).expect("the gate decides");
        prop_assert_eq!(
            d.reason, Some(ReasonCode::LeveragedEtpNotEnabled),
            "an unclassified ETP is treated exactly as a complex one"
        );
    }

    /// The order-to-fill ratio is compared without dividing, and only after enough orders.
    #[test]
    #[ignore = "pending E6-8"]
    fn the_ratio_is_compared_without_dividing(orders in 0_u32..80, fills in 0_u32..20) {
        let mut s = Scenario::allowing();
        s.conduct.orders_today_per_instrument.insert(asset(INSTRUMENT_3), orders);
        s.conduct.filled_today.insert(asset(INSTRUMENT_3), fills);

        let d = evaluate(&s.input()).expect("the gate decides");
        let breaches = orders >= 20 && orders > 10 * fills.max(1);
        if breaches {
            prop_assert_eq!(
                d.reason, Some(ReasonCode::ConductLimitBreached),
                "{} orders to {} fills is above 10", orders, fills
            );
        } else {
            prop_assert_ne!(
                d.reason, Some(ReasonCode::ConductLimitBreached),
                "{} orders to {} fills is within 10, or below the 20-order floor", orders, fills
            );
        }
    }

    /// A trim never sells below its target, and one appears exactly when the position is above it.
    ///
    /// Both halves matter: asserting only "no trim oversells" passes on a gate that proposes no
    /// trim at all, which is what an `Ok(Vec::new())` stub does. So the property also asserts the
    /// trim **exists** whenever the position exceeds `size_factor × cap` by the rebalance band, and
    /// compares its quantity against a target the property computes itself.
    ///
    /// The draw is a **share count**, not a dollar figure, so that the position's market value is
    /// exactly `shares × 100` and the one price is the same whether the gate sizes the trim by
    /// market value or by quantity. Drawing dollars and deriving shares by integer division put a
    /// different price in each, and the oracle below, which values the sell at 100 a share, would
    /// then reject correct trims: at 851 dollars over 8 shares the right trim is one share, and
    /// the oracle wants at least 101 dollars of it.
    ///
    /// The band is `rebalance_band × cap`, **not** `rebalance_band × target`: mandate §5.5 says a
    /// position trims when `MV − factor × cap ≥ rebalance_band × cap`, so the band is 0.05 × 1500 =
    /// 75 whatever the factor. Scaling it with the target made the oracle demand a trim at 8 shares
    /// and factor 0.5, where 50 over the 750 target is inside the 75 band and no trim is correct.
    #[test]
    #[ignore = "pending E6-4"]
    fn a_trim_never_sells_below_the_target(
        held_shares in 1_u32..=30,
        factor in prop::sample::select(vec!["1", "0.5", "0.2"]),
    ) {
        let price_per_share = 100_u32;
        let held_dollars = held_shares * price_per_share;
        let mut s = Scenario::allowing();
        s.mandate = common::mandate_with(common::two_trimming_rungs());
        s.risk.active_rungs = [(0_u8, 120_u64), (1, 120)].into_iter().collect();
        s.risk.size_factor = common::ratio(factor);
        s.agent.positions.insert(asset(INSTRUMENT_2), qty(&held_shares.to_string()));
        s.agent.market_values.insert(asset(INSTRUMENT_2), usd(&held_dollars.to_string()));
        let mut instruments = std::collections::BTreeMap::new();
        instruments.insert(asset(INSTRUMENT_2), common::equity_instrument(INSTRUMENT_2));

        let trims = mandate_risk::trim_proposals(
            s.now, &s.config, &s.mandate, &s.risk, &s.agent, &instruments,
        ).expect("the trims compute");

        let cap_is_min_of_1500_and_point_two_times_10000 = 1_500_u32;
        let target_dollars = match factor {
            "1" => cap_is_min_of_1500_and_point_two_times_10000,
            "0.5" => cap_is_min_of_1500_and_point_two_times_10000 / 2,
            _ => cap_is_min_of_1500_and_point_two_times_10000 / 5,
        };
        let band_dollars = cap_is_min_of_1500_and_point_two_times_10000 / 20;
        let over = held_dollars.saturating_sub(target_dollars);

        if over >= band_dollars && over > 0 {
            let trim = trims.first().expect(
                "a position above its target by the rebalance band is trimmed, not left alone",
            );
            let sold_dollars = trim.qty.to_string().parse::<u32>().unwrap_or(0) * price_per_share;
            prop_assert!(
                sold_dollars >= over,
                "selling {} of a {} position leaves it above the target {}",
                sold_dollars, held_dollars, target_dollars
            );
            prop_assert!(
                sold_dollars <= held_dollars,
                "a trim never sells more than the position"
            );
        } else {
            prop_assert!(
                trims.is_empty(),
                "a position at or below its target is not trimmed"
            );
        }
    }

    /// MI-8: identical inputs give identical decisions, check list included.
    #[test]
    #[ignore = "pending E6-3"]
    fn mi8_identical_inputs_give_identical_decisions(order in whole_dollars()) {
        let mut s = Scenario::allowing();
        s.proposed = proposal(
            INSTRUMENT_3, Side::Buy, "1", &order.to_string(), Origin::OrderBuilder,
        );
        let a = evaluate(&s.input()).expect("the gate decides");
        let b = evaluate(&s.input()).expect("the gate decides");
        prop_assert_eq!(a, b, "the gate is a pure function of its inputs");
    }

    /// A shadow ledger the property accumulates itself: no sequence of allowed openings walks past
    /// a limit, however it is split.
    #[test]
    #[ignore = "pending E6-3"]
    fn no_allowed_sequence_ever_exceeds_a_limit(
        share_counts in prop::collection::vec(1_u32..6, 1..8),
    ) {
        let orders: Vec<u32> = share_counts.iter().map(|n| n * 100).collect();
        let mut s = Scenario::allowing();
        let mut ledger = ShadowLedger::default();
        let equity = scaled("10000");
        let cap = position_cap(scaled("1500"), scaled("0.2"), equity);
        let gross_cap = gross_limit(scaled("2000"), equity);

        for order in orders {
            let notional = scaled(&order.to_string());
            s.agent.market_values.clear();
            s.agent.positions.clear();
            for (id, mv) in &ledger.market_value {
                if *mv > 0 {
                    s.agent.market_values.insert(
                        asset(id), usd(&(mv / 1_000_000_000).to_string()),
                    );
                    s.agent.positions.insert(asset(id), qty("1"));
                }
            }
            s.agent.orders_today = ledger.opening_orders_today;
            s.proposed = proposal(
                INSTRUMENT_3, Side::Buy, &(order / 100).to_string(), "100", Origin::OrderBuilder,
            );

            let d = evaluate(&s.input()).expect("the gate decides");
            if d.verdict == Verdict::Allow {
                let total = ledger.instrument_total(INSTRUMENT_3, notional);
                let gross = ledger.gross(notional);
                prop_assert_eq!(
                    breached(
                        common::oracle::Proposal {
                            instrument_total: total,
                            order: notional,
                            gross,
                            orders_today: ledger.opening_orders_today,
                            seconds_since_group_exit: None,
                            in_universe: true,
                        },
                        common::oracle::Limits {
                            cap,
                            max_order: scaled("1000"),
                            gross_cap,
                            max_orders_per_day: 50,
                            reentry_cooldown_s: 3_600,
                        },
                    ),
                    None::<Breach>,
                    "an allowed order left the ledger past a limit: total {}, cap {}, gross {}, \
                     gross cap {}",
                    total, cap, gross, gross_cap
                );
                ledger.apply_opening(INSTRUMENT_3, notional);
            }
        }
    }
}

proptest! {
    /// §9.1: whichever checks would fail, the earliest one decides the reported code.
    #[test]
    #[ignore = "pending E6-3"]
    fn the_first_failing_check_decides(
        outside_universe in any::<bool>(),
        oversized in any::<bool>(),
    ) {
        let mut s = Scenario::allowing();
        if outside_universe {
            s.universe = common::working_universe(&[]);
        }
        if oversized {
            s.proposed = proposal(INSTRUMENT_3, Side::Buy, "11", "100", Origin::OrderBuilder);
        }
        let d = evaluate(&s.input()).expect("the gate decides");
        if outside_universe {
            prop_assert_eq!(
                d.reason, Some(ReasonCode::NotInWorkingUniverse),
                "the universe is the first item of check 2"
            );
        } else if oversized {
            prop_assert_eq!(d.reason, Some(ReasonCode::MaxOrderSize), "order size follows it");
        }
    }

    /// Mark freshness is a check on openings; it never blocks a reduction.
    #[test]
    #[ignore = "pending E6-8"]
    fn mark_freshness_never_blocks_a_reduction(has_quote in any::<bool>()) {
        let mut s = Scenario::allowing();
        if !has_quote {
            s.market.quote = None;
        }
        s.agent.positions.insert(asset(INSTRUMENT_3), qty("10"));
        s.proposed = proposal(INSTRUMENT_3, Side::Sell, "10", "100", Origin::RiskEngine);

        let d = evaluate(&s.input()).expect("the gate decides");
        prop_assert_ne!(
            d.reason, Some(ReasonCode::StaleMark),
            "a risk exit accepts any mark source"
        );
    }

    /// Nothing opens outside the working universe, whatever the other inputs say.
    #[test]
    #[ignore = "pending E6-3"]
    fn an_opening_needs_the_working_universe(inside in any::<bool>(), shares in 1_u32..5) {
        let mut s = Scenario::allowing();
        s.universe = if inside {
            common::working_universe(&[INSTRUMENT_2, INSTRUMENT_3])
        } else {
            common::working_universe(&[INSTRUMENT_2])
        };
        s.proposed = proposal(
            INSTRUMENT_3, Side::Buy, &shares.to_string(), "100", Origin::OrderBuilder,
        );
        let d = evaluate(&s.input()).expect("the gate decides");
        prop_assert_eq!(
            d.reason == Some(ReasonCode::NotInWorkingUniverse), !inside,
            "an opening is denied exactly when its instrument is outside the universe"
        );
    }

    /// §9.6: only an **opposite**-side fill starts the sixty-second interval.
    #[test]
    #[ignore = "pending E6-8"]
    fn only_an_opposite_side_fill_starts_the_interval(
        opposite in any::<bool>(),
        elapsed in 0_i64..120,
    ) {
        let mut s = Scenario::allowing();
        let fill_at = mandate_time::UtcNanos::from_parts(s.now.secs() - elapsed, 0)
            .expect("the test instant is in range");
        let side = if opposite {
            mandate_risk::RestingSide::Sell
        } else {
            mandate_risk::RestingSide::Buy
        };
        s.conduct.last_opposite_fill_at.insert((asset(INSTRUMENT_3), side), fill_at);

        let d = evaluate(&s.input()).expect("the gate decides");
        let blocked = d.reason == Some(ReasonCode::OppositeFillInterval);
        prop_assert_eq!(
            blocked, opposite && elapsed < 60,
            "a buy after a sell fill within 60 s is blocked; a buy after a buy fill is not"
        );
    }

    /// A cancel that precedes a risk-reducing order is never denied, whatever the resting time.
    #[test]
    #[ignore = "pending E6-8"]
    fn a_cancel_that_precedes_a_reduction_is_never_denied(elapsed in 0_i64..10) {
        let config = common::test_default_config();
        let order = common::open_order(mandate_risk::AgentId(1), INSTRUMENT_3, "100");
        let resting = common::at("2026-09-21T15:00:00Z");
        let now = mandate_time::UtcNanos::from_parts(resting.secs() + elapsed, 0)
            .expect("the test instant is in range");

        let d = mandate_risk::evaluate_cancel(&mandate_risk::CancelInput {
            now,
            config: &config,
            order: &order,
            resting_since: resting,
            precedes_risk_reducing_order: true,
            marketable: false,
        })
        .expect("the gate decides the cancel");
        prop_assert_ne!(
            d.verdict, Verdict::Deny,
            "the resting-time rule does not apply before a risk-reducing order"
        );
    }

    /// The surveillance report flags every threshold it crosses, and none it does not.
    #[test]
    #[ignore = "pending E6-8"]
    fn the_report_flags_every_threshold_it_crosses(orders in 0_u32..60, fills in 0_u32..10) {
        let mut input = mandate_risk::SurveillanceInput::default();
        input.orders.insert(
            (mandate_risk::AgentId(1), asset(INSTRUMENT_3)),
            mandate_risk::OrderCounts { submitted: orders, filled: fills, cancels_excluded: 0 },
        );
        let day = mandate_time::Date::parse("2026-09-21").expect("a date parses");
        let report = mandate_risk::surveillance(day, &common::test_default_config(), &input)
            .expect("the report computes");

        let breaches = orders >= 20 && orders > 10 * fills.max(1);
        prop_assert_eq!(
            report.breaches.contains(&mandate_risk::SurveillanceBreach::OrderToFill),
            breaches,
            "{} orders to {} fills", orders, fills
        );
    }
}

/// Purpose assignment is total over the origins and sides v1 can produce, and never turns a buy
/// into an exit.
#[test]
#[ignore = "pending E6-3"]
fn a_buy_is_never_an_exit() {
    for origin in [
        Origin::OrderBuilder,
        Origin::RiskEngine,
        Origin::OwnerClose,
        Origin::OwnerKillSwitch,
        Origin::AutomatedKillSwitch,
        Origin::TrimToTarget,
        Origin::StopWatchdog,
        Origin::GoalCompletion,
        Origin::RemovedInstrument,
    ] {
        let purpose = mandate_risk::assign_purpose(origin, Side::Buy, qty("1"), qty("0"))
            .expect("a buy with no position is an open");
        assert_eq!(
            purpose,
            Purpose::Open,
            "a buy is an open or an increase whatever component proposed it: {origin:?}"
        );
    }
}
