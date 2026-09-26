//! Hand-calculated fills (trading-domain spec §6.4; DEC-106). Every expected quantity and price is
//! worked out from the spec's rules in the doc comment above its test, digit by digit, and not
//! produced by running the code. The `rc_10`, `rc_12`, and `rc_19` tests reproduce every order of
//! the three backtest reference cases, order by order; the rest cover the rules those cases do not
//! reach.
//!
//! `test_default` throughout: no latency, half-spread 1 bps + fixed impact 2 bps, so s = 3 bps =
//! 0.0003, and a volume cap of 10% of the reference volume. A whole-share equity unless a test says
//! otherwise.
//!
//! Every expectation here was checked against a throwaway implementation of the stubs, which the
//! thirty planted bugs listed in `properties.rs` were then broken against, one at a time. Four of these
//! tests exist because no reference case decides the rule they pin: DEC-106 item 7's "no fill in the
//! trigger bar", item 8's never-marketable take-profit, item 10's market-order remainders, and
//! DEC-30's overnight session.

mod common;

use common::{
    AFTER_HOURS_OPEN, Median, NoMedian, approved, at, auction, bar, bar_at, bps, buy, canceled,
    continuous, crypto, decided, equity, fills, fills_with_median, first_order, for_the_day,
    in_extended_hours, in_session, limit, market, oco, pre_market, price, qty, reported,
    reported_legs, resting, resting_from, run, sell, stop, stop_limit, test_default,
};
use mandate_num::{Bps, Qty};
use mandate_sim::{
    Eligibility, Nanos, OrderEnd, OrderKind, OrderRef, Session, SimBar, SimConfig, SimOrder,
    Slippage,
};

/// RC-10's three bars: 09:40 to 09:42 on 2026-09-21, all regular session.
fn rc_10_bars() -> Vec<SimBar> {
    vec![
        bar("09:40", ["99.9", "100.2", "99.8", "100", "5000"]),
        bar("09:41", ["100", "100.5", "99.6", "100.4", "8000"]),
        bar("09:42", ["100.4", "100.6", "100.1", "100.3", "6000"]),
    ]
}

/// RC-19's four bars: 09:40 to 09:43 on 2026-09-21, all regular session.
fn rc_19_bars() -> Vec<SimBar> {
    vec![
        bar("09:40", ["99.9", "100.2", "99.8", "100", "5000"]),
        bar("09:41", ["100", "100.1", "99.6", "99.8", "8000"]),
        bar("09:42", ["100.4", "100.6", "100.1", "100.3", "6000"]),
        bar("09:43", ["100.2", "100.3", "99.9", "100", "7000"]),
    ]
}

/// RC-10 `market_sell_exit`. Decided at 09:41, zero latency, so bar 1 (09:41) is the first bar
/// starting at or after the decision and bar 0 never fills (rule 1). Bar 1's cap is 10% of bar 0's
/// 5000 = 500 ≥ 100 (rule 3). A market sell fills at the open less s: 100.00 × (1 − 0.0003) =
/// 100.00 − 0.03 = 99.97, taker (rule 4, DEC-106 item 6).
#[test]
fn rc_10_a_market_sell_fills_at_the_first_eligible_open_less_slippage() {
    assert_eq!(
        fills(&rc_10_bars(), &[sell(market(), "100", decided("09:41"))]),
        [(1, "100".to_owned(), "99.97".to_owned(), "taker")]
    );
}

/// RC-10 `limit_sell_touch_is_not_fill`. Bar 1 opens at 100.00, below the 100.50 limit, so the sell
/// is not marketable on arrival and rests (rule 5). Bar 1's high is exactly 100.50: a touch is not a
/// fill. Bar 2's high 100.60 passes it strictly, so the order fills at its limit, 100.50, as maker;
/// bar 2's cap is 10% of bar 1's 8000 = 800 ≥ 100.
#[test]
fn rc_10_a_touch_is_not_a_fill_and_the_next_bar_through_the_limit_is() {
    assert_eq!(
        fills(
            &rc_10_bars(),
            &[sell(limit("100.5"), "100", decided("09:41"))]
        ),
        [(2, "100".to_owned(), "100.5".to_owned(), "maker")]
    );
}

/// RC-10 `marketable_limit_buy`. Bar 1 opens at 100.00, at or below the 100.10 limit, so the buy is
/// marketable on arrival and fills at min(limit, open × (1 + s)) = min(100.10, 100.00 × 1.0003 =
/// 100.03) = 100.03, taker (rule 5).
#[test]
fn rc_10_a_marketable_limit_buy_fills_at_the_slipped_open_inside_its_limit() {
    assert_eq!(
        fills(
            &rc_10_bars(),
            &[buy(limit("100.1"), "100", decided("09:41"))]
        ),
        [(1, "100".to_owned(), "100.03".to_owned(), "taker")]
    );
}

/// RC-10 `volume_cap`. A 1000-share marketable buy limited at 100.60. Bar 1's cap is 10% of bar 0's
/// 5000 = 500, filled at min(100.60, 100.00 × 1.0003) = 100.03. Bar 2's cap is 10% of bar 1's 8000 =
/// 800, more than the 500 remaining; bar 2 still opens inside the limit, so the remainder is still
/// marketable and fills at min(100.60, 100.40 × 1.0003) = 100.40 + 0.03012 = 100.43012 (rules 3
/// and 5). Both fills are taker.
#[test]
fn rc_10_a_marketable_limit_fills_across_two_bars_under_the_volume_cap() {
    assert_eq!(
        fills(
            &rc_10_bars(),
            &[buy(limit("100.6"), "1000", decided("09:41"))]
        ),
        [
            (1, "500".to_owned(), "100.03".to_owned(), "taker"),
            (2, "500".to_owned(), "100.43012".to_owned(), "taker"),
        ]
    );
}

/// RC-12 `stop_triggers_intrabar`. A sell stop at 99.00 resting from bar 0. Bar 0's low is 99.40,
/// above the stop, so nothing triggers. Bar 1 opens at 99.50, still above, and its low 98.80 reaches
/// the stop, so the fill is at the stop less s: 99.00 × 0.9997 = 99.00 − 0.0297 = 98.9703, taker
/// (rule 6).
#[test]
fn rc_12_a_stop_reached_inside_the_bar_fills_at_the_stop_less_slippage() {
    let bars = [
        bar("10:00", ["99.6", "99.8", "99.4", "99.5", "9000"]),
        bar("10:01", ["99.5", "99.6", "98.8", "98.9", "9000"]),
    ];
    assert_eq!(
        fills(&bars, &[sell(stop("99"), "100", resting_from(0))]),
        [(1, "100".to_owned(), "98.9703".to_owned(), "taker")]
    );
}

/// RC-12 `stop_gap_through`. The same stop, but bar 1 opens at 98.50, already past the 99.00 stop,
/// so the fill is at the open less s: 98.50 × 0.9997 = 98.50 − 0.02955 = 98.47045 (rule 6, first
/// branch). A gap gives the order the gapped price, never the stop.
#[test]
fn rc_12_a_stop_gapped_through_fills_at_the_open_less_slippage() {
    let bars = [
        bar("10:00", ["99.6", "99.8", "99.4", "99.5", "9000"]),
        bar("10:01", ["98.5", "98.9", "98.2", "98.6", "9000"]),
    ];
    assert_eq!(
        fills(&bars, &[sell(stop("99"), "100", resting_from(0))]),
        [(1, "100".to_owned(), "98.47045".to_owned(), "taker")]
    );
}

/// RC-12 `stop_limit_gap_rests_as_limit`. A sell stop-limit, stop 99.00 and limit 98.80. Bar 1 opens
/// at 98.50, below the limit: rule 7 gives no fill in that bar, and the order rests as a limit at
/// 98.80 from bar 2 (DEC-106 item 7). Bar 2's high 98.90 passes the limit strictly, so the fill is at
/// the limit, 98.80, as maker (rule 5, DEC-106 item 6). Bar 1's high, 98.70, does not reach the limit,
/// so this case cannot tell rule 7's reading from one that fills in the trigger bar; the test below
/// can.

#[test]
fn rc_12_a_stop_limit_that_gaps_past_its_limit_rests_and_fills_at_the_limit() {
    let bars = [
        bar("10:00", ["99.6", "99.8", "99.4", "99.5", "9000"]),
        bar("10:01", ["98.5", "98.7", "98.2", "98.6", "9000"]),
        bar("10:02", ["98.6", "98.9", "98.4", "98.85", "9000"]),
    ];
    assert_eq!(
        fills(
            &bars,
            &[sell(stop_limit("99", "98.8"), "100", resting_from(0))]
        ),
        [(2, "100".to_owned(), "98.8".to_owned(), "maker")]
    );
}

/// RC-12 `oco_both_reachable_stop_first`. A protective pair on a long: take-profit limit 101.00 and
/// stop 99.00, resting from bar 0. Bar 1 opens at 100.00, reaching neither leg, and inside the bar
/// the high 101.50 passes the limit while the low 98.70 reaches the stop. With both reachable the
/// stop fills first, adverse first: 99.00 × 0.9997 = 98.9703, taker, and the limit leg is canceled
/// at that bar (rule 8).
#[test]
fn rc_12_an_oco_with_both_legs_reachable_fills_the_stop_first() {
    let bars = [
        bar("10:00", ["100", "100.2", "99.8", "100", "9000"]),
        bar("10:01", ["100", "101.5", "98.7", "100", "9000"]),
    ];
    let outcome = run(
        &test_default(),
        &equity(),
        &bars,
        &NoMedian,
        &[sell(oco("101", "99"), "100", resting_from(0))],
    )
    .unwrap();
    assert_eq!(
        reported_legs(&outcome),
        [(1, "stop", "100".to_owned(), "98.9703".to_owned(), "taker")]
    );
    assert_eq!(canceled(&outcome), [(1, "limit")]);
    assert_eq!(outcome.end_of(first_order()), Some(OrderEnd::Filled));
}

/// RC-12 `oco_open_reaches_take_profit`. The same pair, but bar 1 opens at 101.20, already past the
/// 101.00 take-profit. The open reaches that leg, so it fills first under its own rule: a resting
/// sell limit gapped through in continuous trading fills at its limit, 101.00, as maker, and the
/// stop leg is canceled (rules 5 and 8).
#[test]
fn rc_12_an_oco_whose_open_reaches_the_take_profit_fills_that_leg() {
    let bars = [
        bar("10:00", ["100", "100.2", "99.8", "100", "9000"]),
        bar("10:01", ["101.2", "101.5", "98.7", "100", "9000"]),
    ];
    let outcome = run(
        &test_default(),
        &equity(),
        &bars,
        &NoMedian,
        &[sell(oco("101", "99"), "100", resting_from(0))],
    )
    .unwrap();
    assert_eq!(
        reported_legs(&outcome),
        [(1, "limit", "100".to_owned(), "101".to_owned(), "maker")]
    );
    assert_eq!(canceled(&outcome), [(1, "stop")]);
}

/// RC-12 `resting_limit_gap_through_continuous_fills_at_limit`. A resting buy limit at 99.00. Bar 1
/// opens at 98.50, through the limit, but this is continuous trading, not an auction: the market
/// cannot print through a resting limit, so the fill is at the limit, 99.00, as maker (rule 5).
#[test]
fn rc_12_a_resting_limit_gapped_through_in_continuous_trading_fills_at_the_limit() {
    let bars = [
        bar("10:00", ["99.4", "99.6", "99.1", "99.3", "9000"]),
        bar("10:01", ["98.5", "98.8", "98.4", "98.7", "9000"]),
    ];
    assert_eq!(
        fills(&bars, &[buy(limit("99"), "100", resting_from(0))]),
        [(1, "100".to_owned(), "99".to_owned(), "maker")]
    );
}

/// RC-12 `resting_limit_gap_through_auction_fills_at_open`. The same resting buy limit at 99.00, now
/// against a single auction bar: the regular session's first bar, whose 09:30 session start the data
/// covers, with a 20-session median of 9000 for a cap of 900 ≥ 100. The open, 98.50, gaps through
/// the limit at an auction, so the fill is at the open, with no slippage and neither maker nor taker
/// (rule 5's exception, DEC-106 item 5).
#[test]
fn rc_12_a_resting_limit_gapped_through_at_an_auction_fills_at_the_open() {
    let bars = [auction("09:30", ["98.5", "98.8", "98.4", "98.7", "30000"])];
    assert_eq!(
        fills_with_median("9000", &bars, &[buy(limit("99"), "100", resting())]),
        [(0, "100".to_owned(), "98.5".to_owned(), "auction")]
    );
}

/// RC-12 `stop_not_triggered_in_extended_hours`. A sell stop at 99.00 resting from bar 0, against a
/// pre-market bar whose low is 98.00 and a regular bar that stays above the stop. An equity stop
/// triggers only on a regular-session bar (rule 6), and without `extended_hours` the order may not
/// trade pre-market at all (rule 2, spec §4.3), so the 98.00 low is not a trigger. Bar 1, the
/// regular session's first covered bar, has a median of 9000 for a cap of 900, and its open 99.20
/// and low 99.10 both stay above the stop. Nothing fills.
#[test]
fn rc_12_an_equity_stop_does_not_trigger_in_extended_hours() {
    let bars = [
        pre_market("09:00", ["99.5", "99.6", "98", "99.3", "2000"]),
        auction("09:30", ["99.2", "99.5", "99.1", "99.4", "9000"]),
    ];
    assert!(
        fills_with_median("9000", &bars, &[sell(stop("99"), "100", resting_from(0))]).is_empty()
    );
}

/// RC-19 `resting_fills_on_arrival_bar`. A buy limit at 99.70 decided at 09:41, so bar 1 is its
/// first eligible bar. Bar 1 opens at 100.00, beyond the limit, so it is not marketable on arrival
/// and rests from that same bar; bar 1's low 99.60 passes 99.70 strictly, so it fills at the limit,
/// 99.70, as maker, on its arrival bar (rule 5). Bar 1's cap is 10% of bar 0's 5000 = 500 ≥ 100.
#[test]
fn rc_19_a_resting_limit_can_fill_on_its_arrival_bar() {
    assert_eq!(
        fills(
            &rc_19_bars(),
            &[buy(limit("99.7"), "100", decided("09:41"))]
        ),
        [(1, "100".to_owned(), "99.7".to_owned(), "maker")]
    );
}

/// RC-19 `marketable_remainder_becomes_resting`. A 1000-share buy limited at 100.05, decided at
/// 09:41. Bar 1 opens at 100.00, inside the limit, so 500 (10% of bar 0's 5000) fills at
/// min(100.05, 100.00 × 1.0003 = 100.03) = 100.03 as taker. Bar 2 opens at 100.40, beyond the limit,
/// so the remainder becomes resting; bar 2's low is 100.10, above the limit, so nothing fills there.
/// Bar 3's low 99.90 passes 100.05 strictly, and its cap, 10% of bar 2's 6000 = 600, covers the
/// remaining 500, which fills at the limit as maker (rule 5).
#[test]
fn rc_19_a_marketable_remainder_becomes_resting_when_a_bar_opens_beyond_the_limit() {
    assert_eq!(
        fills(
            &rc_19_bars(),
            &[buy(limit("100.05"), "1000", decided("09:41"))]
        ),
        [
            (1, "500".to_owned(), "100.03".to_owned(), "taker"),
            (3, "500".to_owned(), "100.05".to_owned(), "maker"),
        ]
    );
}

/// RC-19 `touch_on_arrival_is_not_fill`. A buy limit at 99.60 decided at 09:41. Bar 1 opens beyond
/// it, so the order rests; bar 1's low is exactly 99.60, a touch, not a fill. No later bar's low
/// reaches 99.60 (100.10 and 99.90), so nothing fills (rule 5).
#[test]
fn rc_19_a_touch_on_the_arrival_bar_is_not_a_fill() {
    assert!(
        fills(
            &rc_19_bars(),
            &[buy(limit("99.6"), "100", decided("09:41"))]
        )
        .is_empty()
    );
}

/// Rule 5, the other side of RC-19's `marketable_remainder_becomes_resting`: once a bar that opens
/// beyond the limit has turned the remainder into a resting limit, a later bar that opens back inside
/// the limit does not make it marketable again. A 300-share buy limited at 100.05 against a 1000
/// median, so every bar caps at 100. Bar 0 opens at 100.00, inside the limit, and fills 100 at
/// min(100.05, 100.00 × 1.0003) = 100.03 as taker. Bar 1 opens at 100.40, beyond the limit, so the
/// remainder rests and fills 100 at the limit as maker, its low 99.90 having passed it. Bar 2 opens
/// at 100.00 again: a resting limit fills at its limit, 100.05, as maker, not at the slipped open.
#[test]
fn a_resting_remainder_does_not_become_marketable_again() {
    let bars = [
        auction("09:30", ["100", "100.2", "99.9", "100", "1000"]),
        bar("09:31", ["100.4", "100.6", "99.9", "100.2", "1000"]),
        bar("09:32", ["100", "100.3", "99.9", "100.1", "1000"]),
    ];
    assert_eq!(
        fills_with_median(
            "1000",
            &bars,
            &[buy(limit("100.05"), "300", decided("09:30"))]
        ),
        [
            (0, "100".to_owned(), "100.03".to_owned(), "taker"),
            (1, "100".to_owned(), "100.05".to_owned(), "maker"),
            (2, "100".to_owned(), "100.05".to_owned(), "maker"),
        ]
    );
}

/// Rule 1 with latency. A 60-second decision latency moves eligibility from the 09:40 bar to the
/// first bar starting at or after 09:41, which is bar 1; the approval latency applies only to an
/// order that needed approval, which moves that order on to bar 2. Bar 0 is the regular session's
/// first covered bar with a median of 5000, so a cap of 500 would have let it fill.
#[test]
fn latency_moves_eligibility_and_approval_latency_applies_only_when_approval_was_required() {
    let config = SimConfig {
        decision_latency: Nanos::from_millis(60_000).unwrap(),
        approval_latency: Nanos::from_millis(60_000).unwrap(),
        ..test_default()
    };
    let bars = [
        auction("09:30", ["100", "100.2", "99.8", "100", "5000"]),
        bar("09:31", ["100", "100.2", "99.8", "100", "5000"]),
        bar("09:32", ["100", "100.2", "99.8", "100", "5000"]),
    ];
    let decided_at_the_open = [sell(market(), "100", decided("09:30"))];
    let outcome = run(
        &config,
        &equity(),
        &bars,
        &Median(qty("5000")),
        &decided_at_the_open,
    )
    .unwrap();
    assert_eq!(
        reported(&outcome),
        [(1, "100".to_owned(), "99.97".to_owned(), "taker")]
    );

    let approved_at_the_open = [sell(market(), "100", approved("09:30"))];
    let outcome = run(
        &config,
        &equity(),
        &bars,
        &Median(qty("5000")),
        &approved_at_the_open,
    )
    .unwrap();
    assert_eq!(
        reported(&outcome),
        [(2, "100".to_owned(), "99.97".to_owned(), "taker")]
    );
}

/// Rule 3's sharing. Two buy limits at 100.10 arrive for bar 1, whose cap is 10% of bar 0's 5000 =
/// 500. In submission order the first order takes 400, all it wants, and the second takes the
/// remaining 100 of the cap. Bar 2 opens at 100.40, beyond the limit, so the second order's
/// remainder rests at 100.10 and bar 2's low, exactly 100.10, is a touch, not a fill: it ends open
/// with 300 unfilled.
#[test]
fn a_bars_volume_cap_is_shared_across_orders_in_submission_order() {
    let orders = [
        buy(limit("100.1"), "400", decided("09:41")),
        buy(limit("100.1"), "400", decided("09:41")),
    ];
    let outcome = run(
        &test_default(),
        &equity(),
        &rc_10_bars(),
        &NoMedian,
        &orders,
    )
    .unwrap();
    assert_eq!(
        reported(&outcome),
        [
            (1, "400".to_owned(), "100.03".to_owned(), "taker"),
            (1, "100".to_owned(), "100.03".to_owned(), "taker"),
        ]
    );
    assert_eq!(outcome.end_of(OrderRef::new(0)), Some(OrderEnd::Filled));
    assert_eq!(outcome.end_of(OrderRef::new(1)), Some(OrderEnd::Open));
}

/// Rule 3's first-bar branch and DEC-106 item 4. Against a single auction bar, a 1000-share market
/// buy fills 10% of the 20-session median, truncated to whole shares: 10% of 5000 = 500 at
/// 100.00 × 1.0003 = 100.03. With no median for that minute the reference volume is unavailable and
/// the bar's cap is 0, so nothing fills.
#[test]
fn a_sessions_first_bar_caps_on_the_twenty_session_median_and_on_zero_without_one() {
    let bars = [auction("09:30", ["100", "100.6", "99.8", "100.4", "30000"])];
    let orders = [buy(market(), "1000", decided("09:30"))];
    assert_eq!(
        fills_with_median("5000", &bars, &orders),
        [(0, "500".to_owned(), "100.03".to_owned(), "taker")]
    );
    assert!(fills(&bars, &orders).is_empty());
}

/// Rule 2's day-order cancellation. A day order eligible from bar 0 may trade only the regular
/// session, whose last bar that day is bar 1, so its remainder is canceled after bar 1: the
/// after-hours bar 2 and the next trading day's bar 3 cannot fill it, although bar 3's low reaches
/// its 99.00 limit. The same order as GTC fills on bar 3, at the limit as maker; bar 3 is the next
/// day's first covered regular bar, with a median of 9000 for a cap of 900.
#[test]
fn a_day_orders_remainder_is_canceled_after_its_last_eligible_session() {
    let bars = [
        auction("09:30", ["100", "100.2", "99.8", "100", "9000"]),
        bar("09:31", ["100", "100.2", "99.8", "100", "9000"]),
        in_session(
            Session::AfterHours,
            AFTER_HOURS_OPEN,
            bar("16:01", ["99", "99.1", "98.5", "98.6", "9000"]),
        ),
        in_session(
            Session::Regular,
            "2026-09-22T09:30:00-04:00",
            bar_at(
                "2026-09-22T09:30:00-04:00",
                ["99.5", "99.6", "98.5", "98.9", "9000"],
            ),
        ),
    ];
    let resting_buy = buy(limit("99"), "100", resting_from(0));
    let day = run(
        &test_default(),
        &equity(),
        &bars,
        &Median(qty("9000")),
        &[for_the_day(resting_buy)],
    )
    .unwrap();
    assert!(reported(&day).is_empty());
    assert_eq!(
        day.end_of(first_order()),
        Some(OrderEnd::Expired { at_bar: 1 })
    );

    let gtc = run(
        &test_default(),
        &equity(),
        &bars,
        &Median(qty("9000")),
        &[resting_buy],
    )
    .unwrap();
    assert_eq!(
        reported(&gtc),
        [(3, "100".to_owned(), "99".to_owned(), "maker")]
    );
}

/// Rule 2 with `extended_hours`. An after-hours bar fills an exit the gate marked for the extended
/// sessions, and does not fill the same order without that flag (spec §4.3, DEC-37). The order is a
/// resting sell limit at 99.05 whose 99.10 high the bar passes strictly, so it fills at the limit as
/// maker. The bar opens its own session in the data, so its cap is 10% of the 9000 median = 900.
#[test]
fn an_exit_marked_for_extended_hours_fills_after_hours() {
    let bars = [in_session(
        Session::AfterHours,
        "2026-09-21T16:01:00-04:00",
        bar("16:01", ["99", "99.1", "98.5", "98.6", "9000"]),
    )];
    let exit = sell(limit("99.05"), "100", resting());
    assert_eq!(
        fills_with_median("9000", &bars, &[in_extended_hours(exit)]),
        [(0, "100".to_owned(), "99.05".to_owned(), "maker")]
    );
    assert!(fills_with_median("9000", &bars, &[exit]).is_empty());
}

/// The `sqrt` impact model (rule 4's configuration, DEC-106 item 3). With a half-spread of 1 bps and
/// a coefficient of 10 bps, a 1000-share market buy against a reference volume of 10000 has a fill
/// ratio of 1000 ÷ 10000 = 0.1. √0.1 = 0.31622776601683793319…, which at 18 places rounded up is
/// 0.316227766016837934, so the impact is 10 × that = 3.16227766016837934 bps and s =
/// 4.16227766016837934 bps. The slippage on the 100.00 open is 100.00 × 0.000416227766016837934 =
/// 0.0416227766016837934, rounded up at 9 places to 0.041622777, so the buy fills at
/// 100.041622777.
#[test]
fn sqrt_impact_takes_the_root_of_the_filled_share_of_reference_volume() {
    let config = SimConfig {
        slippage: Slippage::Sqrt {
            half_spread_bps: bps("1"),
            coefficient_bps: bps("10"),
        },
        ..test_default()
    };
    let bars = [auction("09:30", ["100", "100.6", "99.8", "100.4", "30000"])];
    let outcome = run(
        &config,
        &equity(),
        &bars,
        &Median(qty("10000")),
        &[buy(market(), "1000", decided("09:30"))],
    )
    .unwrap();
    assert_eq!(
        reported(&outcome),
        [(0, "1000".to_owned(), "100.041622777".to_owned(), "taker")]
    );
}

/// DEC-106 item 2. A half-spread of 1.23456789 bps on a 100.00 open is 0.0123456789, which needs ten
/// places; a price holds nine (spec §2.1). Rounding the slippage up, once, moves the price against
/// the order whichever way it goes: the buy pays 100.00 + 0.012345679 = 100.012345679, more than the
/// unrounded 100.0123456789, and the sell receives 100.00 − 0.012345679 = 99.987654321, less than the
/// unrounded 99.9876543211.
#[test]
fn a_fill_price_beyond_nine_places_rounds_against_the_order() {
    let config = SimConfig {
        slippage: Slippage::Fixed {
            half_spread_bps: bps("1.23456789"),
            impact_bps: Bps::ZERO,
        },
        ..test_default()
    };
    let bars = [auction("09:30", ["100", "100.6", "99.8", "100.4", "30000"])];
    let orders = [
        buy(market(), "100", decided("09:30")),
        sell(market(), "100", decided("09:30")),
    ];
    let outcome = run(&config, &equity(), &bars, &Median(qty("5000")), &orders).unwrap();
    assert_eq!(
        reported(&outcome),
        [
            (0, "100".to_owned(), "100.012345679".to_owned(), "taker"),
            (0, "100".to_owned(), "99.987654321".to_owned(), "taker"),
        ]
    );
}

/// Rule 6 on the buy side, and rule 9. A buy stop at 5.00 whose bar opens at 4.90 and reaches 5.10:
/// the open has not passed the stop, the high has, so the fill is at the stop plus s, 5.00 × 1.0003
/// = 5.0015. That price is not on the 0.01 tick the instrument trades in, and rule 9 leaves it
/// alone.
#[test]
fn a_buy_stop_reached_by_the_high_fills_at_the_stop_and_is_not_tick_rounded() {
    let bars = [auction("09:30", ["4.9", "5.1", "4.85", "5.05", "30000"])];
    assert_eq!(
        fills_with_median("5000", &bars, &[buy(stop("5"), "100", resting())]),
        [(0, "100".to_owned(), "5.0015".to_owned(), "taker")]
    );
}

/// Rule 7's triggered branch: a sell fills at max(L, min(open, S) × (1 − s)), the better of the two
/// for the seller, because the limit is a floor it will not sell below. Stop 99.00 and limit 98.90 on
/// a bar that opens at 99.50 and falls to 98.80: the stop is reached inside the bar, so the triggered
/// price is 99.00 × 0.9997 = 98.9703, and the limit does not bind because 98.9703 is above it —
/// max(98.90, 98.9703) = 98.9703, taker.
#[test]
fn a_triggered_stop_limit_fills_at_the_better_of_its_limit_and_the_triggered_price() {
    let bars = [auction("09:30", ["99.5", "99.6", "98.8", "98.9", "30000"])];
    assert_eq!(
        fills_with_median(
            "5000",
            &bars,
            &[sell(stop_limit("99", "98.9"), "100", resting())]
        ),
        [(0, "100".to_owned(), "98.9703".to_owned(), "taker")]
    );
}

/// Rule 6 and DEC-106 item 9 for crypto: crypto trades continuously, so its bars belong to no
/// equity session and a stop triggers on any of them. The stop at 99.00 is reached by the low, so
/// the fill is 99.00 × 0.9997 = 98.9703 on a fractional quantity, and the cap truncates to the
/// fractional increment: 10% of 5000 = 500 ≥ 0.25.
#[test]
fn a_crypto_stop_triggers_on_a_continuous_bar() {
    let bars = [
        continuous("01:00", ["99.6", "99.8", "99.4", "99.5", "5000"]),
        continuous("01:01", ["99.5", "99.6", "98.8", "98.9", "5000"]),
    ];
    let outcome = run(
        &test_default(),
        &crypto(),
        &bars,
        &NoMedian,
        &[sell(stop("99"), "0.25", resting_from(0))],
    )
    .unwrap();
    assert_eq!(
        reported(&outcome),
        [(1, "0.25".to_owned(), "98.9703".to_owned(), "taker")]
    );
}

/// Inputs the model rejects rather than guessing about. Each error carries the stable code the
/// runner reports (ADR-0001 ES-09).
#[test]
fn the_model_rejects_bars_and_orders_it_cannot_simulate() {
    let good = bar("09:40", ["99.9", "100.2", "99.8", "100", "5000"]);
    let order = buy(limit("100"), "100", resting_from(0));
    let simulate_with = |bars: &[SimBar], orders: &[SimOrder]| {
        run(&test_default(), &equity(), bars, &NoMedian, orders)
            .map(|_| "ok")
            .map_err(|e| e.code())
    };

    let repeated = [good.clone(), good.clone()];
    assert_eq!(simulate_with(&repeated, &[order]), Err("bars_out_of_order"));

    let crossed = SimBar {
        low: price("100.3"),
        ..good.clone()
    };
    assert_eq!(simulate_with(&[crossed], &[order]), Err("inconsistent_bar"));

    let early = SimBar {
        session_start: at("2026-09-21T09:41:00-04:00"),
        ..good.clone()
    };
    assert_eq!(
        simulate_with(&[early], &[order]),
        Err("bar_before_its_session")
    );

    let empty = SimOrder {
        qty: Qty::ZERO,
        ..order
    };
    assert_eq!(
        simulate_with(std::slice::from_ref(&good), &[empty]),
        Err("zero_quantity")
    );

    let crossed_stop_limit = SimOrder {
        kind: stop_limit("99", "99.5"),
        ..sell(market(), "100", resting_from(0))
    };
    assert_eq!(
        simulate_with(std::slice::from_ref(&good), &[crossed_stop_limit]),
        Err("stop_limit_crossed")
    );

    let crossed_oco = SimOrder {
        kind: OrderKind::Oco {
            limit: price("98"),
            stop: price("99"),
        },
        ..sell(market(), "100", resting_from(0))
    };
    assert_eq!(
        simulate_with(std::slice::from_ref(&good), &[crossed_oco]),
        Err("oco_legs_crossed")
    );

    let far_future = SimOrder {
        eligible_from: Eligibility::Resting { from_bar: 7 },
        ..order
    };
    assert_eq!(
        simulate_with(std::slice::from_ref(&good), &[far_future]),
        Err("resting_bar_out_of_range")
    );

    let off_increment = SimOrder {
        qty: qty("100.5"),
        ..order
    };
    assert_eq!(
        simulate_with(std::slice::from_ref(&good), &[off_increment]),
        Err("quantity_off_increment")
    );

    let extended_market = in_extended_hours(sell(market(), "100", resting_from(0)));
    assert_eq!(
        simulate_with(std::slice::from_ref(&good), &[extended_market]),
        Err("extended_hours_needs_a_limit")
    );

    let fractional_oco = sell(oco("101", "99"), "1", resting_from(0));
    assert_eq!(
        run(
            &test_default(),
            &crypto(),
            std::slice::from_ref(&good),
            &NoMedian,
            &[fractional_oco]
        )
        .map(|_| "ok")
        .map_err(|e| e.code()),
        Err("oco_on_a_fractional_instrument")
    );

    let crypto_day_order = for_the_day(sell(limit("99"), "1", resting_from(0)));
    assert_eq!(
        run(
            &test_default(),
            &crypto(),
            std::slice::from_ref(&good),
            &NoMedian,
            &[crypto_day_order]
        )
        .map(|_| "ok")
        .map_err(|e| e.code()),
        Err("day_order_on_a_continuous_instrument")
    );
}

/// DEC-106 item 7, the reading no reference case pins down. Bar 1 triggers the 99.00 stop on an open
/// of 98.50, below the 98.80 limit, so rule 7's "no fill" holds for that whole bar even though its
/// high of 98.90 prints through the limit; the order rests as a limit at 98.80 from bar 2, whose high
/// of 98.90 passes the limit strictly and fills it at 98.80 as maker. A model that rested in the
/// trigger bar would fill at bar 1 instead.
#[test]
fn a_stop_limit_that_gaps_beyond_its_limit_does_not_fill_in_its_trigger_bar() {
    let bars = [
        bar("10:00", ["99.6", "99.8", "99.4", "99.5", "9000"]),
        bar("10:01", ["98.5", "98.9", "98.2", "98.6", "9000"]),
        bar("10:02", ["98.6", "98.9", "98.4", "98.85", "9000"]),
    ];
    assert_eq!(
        fills(
            &bars,
            &[sell(stop_limit("99", "98.8"), "100", resting_from(0))]
        ),
        [(2, "100".to_owned(), "98.8".to_owned(), "maker")]
    );
}

/// DEC-106 items 8 and 10 with a volume cap. A protective pair on 800 shares, resting from bar 0:
/// take-profit 101.00 and stop 99.00. Bar 0's cap is 0, its session having started before the data.
/// Bar 1 caps at 10% of bar 0's 5000 = 500; its open, 100.00, reaches neither leg, its high of 100.50
/// does not pass the take-profit, and its low of 98.70 reaches the stop, so 500 fill at
/// 99.00 × 0.9997 = 98.9703 on the stop leg and the take-profit is canceled at that bar. The stop's
/// remainder is a market order (item 10), so bar 2, capped at 10% of 9000 = 900, fills the last 300
/// at its open: 101.50 × 0.9997 = 101.50 − 0.03045 = 101.46955.
#[test]
fn a_partial_oco_fill_cancels_the_other_leg_and_leaves_a_market_remainder() {
    let bars = [
        bar("10:00", ["100", "100.2", "99.8", "100", "5000"]),
        bar("10:01", ["100", "100.5", "98.7", "99", "9000"]),
        bar("10:02", ["101.5", "101.6", "101.2", "101.4", "9000"]),
    ];
    let outcome = run(
        &test_default(),
        &equity(),
        &bars,
        &NoMedian,
        &[sell(oco("101", "99"), "800", resting_from(0))],
    )
    .unwrap();
    assert_eq!(
        reported_legs(&outcome),
        [
            (1, "stop", "500".to_owned(), "98.9703".to_owned(), "taker"),
            (2, "stop", "300".to_owned(), "101.46955".to_owned(), "taker"),
        ]
    );
    assert_eq!(canceled(&outcome), [(1, "limit")]);
    assert_eq!(outcome.end_of(first_order()), Some(OrderEnd::Filled));
}

/// DEC-106 item 8: a protective pair's take-profit leg is a working order away from the market, so it
/// is never marketable on arrival even when its **first eligible** bar opens past it. Decided at
/// 10:01, so bar 1 is that bar, and it opens at 101.20, beyond the 101.00 take-profit: the leg fills
/// at its limit, 101.00, as maker, not at 101.20 × 0.9997 = 101.16964, the better price a marketable
/// limit would have taken. Bar 1's cap is 10% of bar 0's 5000 = 500.
#[test]
fn an_ocos_take_profit_is_never_marketable_on_arrival() {
    let bars = [
        bar("10:00", ["100", "100.2", "99.8", "100", "5000"]),
        bar("10:01", ["101.2", "101.5", "100.9", "101.4", "9000"]),
    ];
    let outcome = run(
        &test_default(),
        &equity(),
        &bars,
        &NoMedian,
        &[sell(oco("101", "99"), "100", decided("10:01"))],
    )
    .unwrap();
    assert_eq!(
        reported_legs(&outcome),
        [(1, "limit", "100".to_owned(), "101".to_owned(), "maker")]
    );
    assert_eq!(canceled(&outcome), [(1, "stop")]);
}

/// DEC-106 item 10: once a stop has triggered, its remainder is a market order, filled at later
/// opens even when they are back above the stop. 700 shares on a 99.00 stop resting from bar 0. Bar 1
/// caps at 10% of bar 0's 5000 = 500 and its low of 98.80 reaches the stop, filling 500 at
/// 99.00 × 0.9997 = 98.9703. Bar 2 opens at 99.50, above the stop, and the remaining 200 fill there
/// as a market order: 99.50 × 0.9997 = 99.50 − 0.02985 = 99.47015.
#[test]
fn a_triggered_stops_remainder_is_a_market_order() {
    let bars = [
        bar("10:00", ["99.6", "99.8", "99.4", "99.5", "5000"]),
        bar("10:01", ["99.5", "99.6", "98.8", "98.9", "9000"]),
        bar("10:02", ["99.5", "99.7", "99.4", "99.6", "9000"]),
    ];
    assert_eq!(
        fills(&bars, &[sell(stop("99"), "700", resting_from(0))]),
        [
            (1, "500".to_owned(), "98.9703".to_owned(), "taker"),
            (2, "200".to_owned(), "99.47015".to_owned(), "taker"),
        ]
    );
}

/// Spec §4.3 and §5.2: the extended sessions take limit orders alone, so a market order waits for the
/// regular session. The pre-market bar opens its own session in the data, so its cap is 10% of the
/// 9000 median = 900 and a model that let a market order trade pre-market would fill there, at
/// 99.50 × 0.9997 = 99.47015; the session is the only thing stopping it. The fill instead comes on the
/// regular session's first bar, capped the same way: 99.20 × 0.9997 = 99.20 − 0.02976 = 99.17024.
#[test]
fn a_market_order_waits_for_the_regular_session() {
    let bars = [
        in_session(
            Session::PreMarket,
            "2026-09-21T09:00:00-04:00",
            bar("09:00", ["99.5", "99.6", "98", "99.3", "2000"]),
        ),
        auction("09:30", ["99.2", "99.5", "99.1", "99.4", "9000"]),
    ];
    assert_eq!(
        fills_with_median("9000", &bars, &[sell(market(), "100", decided("09:00"))]),
        [(1, "100".to_owned(), "99.17024".to_owned(), "taker")]
    );
}

/// DEC-30 and DEC-106 item 9: nothing trades overnight, whatever the order. The bar opens its own
/// session, so its cap is 10% of the 9000 median = 900, and its low of 98.50 passes the resting buy
/// limit at 99.50 strictly — the session is the only reason there is no fill, and `extended_hours`
/// does not reach it.
#[test]
fn an_overnight_bar_never_fills() {
    let bars = [in_session(
        Session::Overnight,
        "2026-09-21T20:05:00-04:00",
        bar_at(
            "2026-09-21T20:05:00-04:00",
            ["99", "99.6", "98.5", "99.2", "9000"],
        ),
    )];
    let exit = buy(limit("99.5"), "100", resting());
    assert!(fills_with_median("9000", &bars, &[exit]).is_empty());
    assert!(fills_with_median("9000", &bars, &[in_extended_hours(exit)]).is_empty());
}
