//! `buy_action`: the classification facts of a buy (mandate spec §6.2 step 4, §6.3), recomputed by
//! hand, and shown to be the very facts `propose` attaches to the buy it proposes.

mod common;

use common::{
    NOW, REFERENCE_DAY, SWING_INSTRUMENT, account, asset, day, flat_account, momentum, news,
    output, price, qty, quiet_risk, swing_market, two_stock_swing, unit, usd,
};
use mandate_builder::{Action, ActionContext, RequestedBy, buy_action, propose};
use mandate_domain::{AssetClass, MarketSession, Purpose};
use mandate_num::{Signed, Unit};
use mandate_time::UtcNanos;

/// A flat agent with 300 already gross and 250 bought today buys 3 at the 100 ask: the order is
/// 300, the position after it 0 × 99.9 + 0 working + 300 = 300, gross 300 + 300 = 600, and bought
/// today 250 + 300 = 550. With no position it is an opening, and with no prior fill a first trade.
#[test]
fn a_flat_agents_buy_is_an_opening_with_its_after_values() {
    let mut flat = flat_account();
    flat.gross_usd = usd("300");
    let mut risk = quiet_risk();
    risk.bought_today_usd = usd("250");
    let action = buy_action(&flat, &swing_market(), &risk, unit("0.75"), qty("3"))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        action,
        ActionContext {
            purpose: Purpose::Open,
            order_usd: usd("300"),
            combined_score: unit("0.75"),
            instrument: asset(SWING_INSTRUMENT),
            asset_class: AssetClass::UsEquity,
            session: MarketSession::Regular,
            first_trade_in_instrument: true,
            new_instrument: false,
            thesis_confidence: Unit::ZERO,
            drawdown: Unit::ZERO,
            daily_pnl_fraction: Signed::ZERO,
            position_usd_after: usd("300"),
            gross_usd_after: usd("600"),
            bought_today_usd: usd("550"),
            position_pnl_fraction: Signed::ZERO,
            requested_by: RequestedBy::Agent,
            risk_day: day(REFERENCE_DAY),
        }
    );
}

/// Holding 5 marked at 99.9 with 50 already working, a buy of 2 at 100 is an increase: the
/// position after it is 5 × 99.9 + 50 + 200 = 749.5. An agent that has filled before is not on a
/// first trade.
#[test]
fn a_held_agents_buy_is_an_increase_valued_at_the_mark() {
    let mut held = account("5", "99.9");
    held.working_opening_cost = usd("50");
    let mut risk = quiet_risk();
    risk.has_prior_fill = true;
    let action = buy_action(&held, &swing_market(), &risk, Unit::ONE, qty("2"))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(action.purpose, Purpose::Increase);
    assert_eq!(action.order_usd, usd("200"));
    assert_eq!(action.position_usd_after, usd("749.5"));
    assert_eq!(action.gross_usd_after, usd("200"));
    assert_eq!(action.bought_today_usd, usd("200"));
    assert!(!action.first_trade_in_instrument);
}

/// A crossed quote and the overnight session refuse a buy; a locked quote does not.
#[test]
fn a_crossed_quote_and_the_overnight_session_refuse_and_a_locked_quote_does_not() {
    let code = |market| {
        buy_action(&flat_account(), &market, &quiet_risk(), Unit::ONE, qty("1"))
            .map(|a| a.order_usd)
    };
    let mut crossed = swing_market();
    crossed.bid = price("100.01");
    assert_eq!(code(crossed).map_err(|e| e.code()), Err("crossed_quote"));
    let mut locked = swing_market();
    locked.bid = locked.ask;
    assert_eq!(code(locked), Ok(usd("100")));
    let mut overnight = swing_market();
    overnight.session = MarketSession::Overnight;
    assert_eq!(
        code(overnight).map_err(|e| e.code()),
        Err("untradable_session")
    );
}

/// The facts `propose` attaches to its buy are `buy_action`'s for the proposed quantity and score.
#[test]
fn propose_attaches_exactly_the_buy_actions_facts() {
    let mandate = two_stock_swing();
    let outputs = [
        output(&news(), SWING_INSTRUMENT, "1", "1"),
        output(&momentum(), SWING_INSTRUMENT, "1", "1"),
    ];
    let now = UtcNanos::parse(NOW).unwrap_or_else(|e| panic!("{e}"));
    let proposal = propose(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &outputs,
        now,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let Action::Buy {
        qty: bought,
        action,
        order_usd,
        ..
    } = proposal.action
    else {
        panic!(
            "full conviction on a flat agent proposes a buy: {:?}",
            proposal.action
        );
    };
    assert_eq!(order_usd, action.order_usd);
    assert_eq!(
        Ok(action),
        buy_action(
            &flat_account(),
            &swing_market(),
            &quiet_risk(),
            proposal.combined.score,
            bought
        )
    );
}
