//! The opening policy intersected with the broker's capability profile (E7-23 B3, LT-3, DEC-529
//! item 2, DEC-854).
//!
//! The profiles are built here, cell by cell. Every hand test's expected outcome is worked in its
//! doc comment. The property's own oracle reads the generator's cell masks, not the profile, and
//! sizes in integer cents. No expected value comes from the builder.
//!
//! Every test is pending until B3's implementation PR and fails on `BuilderError::Unimplemented`.

mod common;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};

use common::{
    NOW, account, at, crypto_market, flat_account, momentum, news, output, price, qty, quiet_risk,
    swing_market, two_stock_swing, usd,
};
use mandate_builder::{
    AccountSnapshot, Action, BuilderMandate, HoldReason, Market, OrderShape, Venue, deployable,
    opening_form, propose_on,
};
use mandate_domain::{
    AssetClass, CapabilityProfile, Cell, Idempotency, MarketSession, OrderType, QuantityForm,
    Retry, Row, TimeInForce,
};
use mandate_domain::{AssetClass::Crypto, AssetClass::UsEquity};
use mandate_domain::{OrderType::Limit, OrderType::Market as MarketOrder};
use mandate_domain::{QuantityForm::Fractional, QuantityForm::Notional, QuantityForm::Whole};
use mandate_domain::{TimeInForce::Day, TimeInForce::Gtc, TimeInForce::Ioc};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};

const REGULAR: MarketSession = MarketSession::Regular;

fn cell(order_type: OrderType, quantity_form: QuantityForm, tifs: &[TimeInForce]) -> Cell {
    Cell {
        order_type,
        quantity_form,
        times_in_force: tifs.iter().copied().collect(),
        protection_forms: [].into(),
    }
}

fn profile(rows: Vec<(AssetClass, MarketSession, Vec<Cell>)>) -> CapabilityProfile {
    let rows = rows
        .into_iter()
        .map(|(asset_class, session, cells)| Row {
            asset_class,
            session,
            cells,
        })
        .collect();
    let idempotency = Idempotency {
        client_order_id: true,
        retry: Retry::Unknown,
        query_by_client_order_id: false,
    };
    CapabilityProfile::new(1, rows, idempotency).unwrap_or_else(|e| panic!("a valid profile: {e}"))
}

/// Trading spec §5.2's Robinhood row: market orders in every form, every other type whole only.
fn whole_only() -> CapabilityProfile {
    let both = [Day, Gtc];
    let mut cells = vec![
        cell(Limit, Whole, &both),
        cell(OrderType::StopLimit, Whole, &both),
    ];
    cells.extend([Whole, Fractional, Notional].map(|f| cell(MarketOrder, f, &both)));
    profile(vec![(UsEquity, REGULAR, cells)])
}

/// Trading spec §5.2's Alpaca equity row for limits: whole `day` or `gtc`, fractional and
/// notional `day` only.
fn fractional_day() -> CapabilityProfile {
    let cells = vec![
        cell(Limit, Whole, &[Day, Gtc]),
        cell(Limit, Fractional, &[Day]),
        cell(Limit, Notional, &[Day]),
    ];
    profile(vec![(UsEquity, REGULAR, cells)])
}

fn capped(max_order_usd: &str) -> BuilderMandate {
    let mut mandate = two_stock_swing();
    mandate.limits.max_order_usd = usd(max_order_usd);
    mandate
}

/// A fractionable equity at this ask, one cent under it bid, on a 0.0001-share grid.
fn fractionable(bid: &str, ask: &str) -> Market {
    let mut market = swing_market();
    market.bid = price(bid);
    market.ask = price(ask);
    market.increment = qty("0.0001");
    market
}

/// Both models at this conviction with full confidence, so b = c = the conviction.
fn signals(instrument: &str, conviction: &str) -> Vec<mandate_builder::ModelOutput> {
    [momentum(), news()]
        .iter()
        .map(|m| output(m, instrument, conviction, "1"))
        .collect()
}

fn run(
    profile: &CapabilityProfile,
    time_in_force: TimeInForce,
    mandate: &BuilderMandate,
    account: &AccountSnapshot,
    market: &Market,
    conviction: &str,
) -> Result<Action, &'static str> {
    let venue = Venue {
        profile,
        time_in_force,
    };
    let outputs = signals(market.instrument.as_str(), conviction);
    let risk = quiet_risk();
    let proposal = propose_on(&venue, mandate, account, market, &risk, &outputs, at(NOW));
    proposal.map(|p| p.action).map_err(|e| e.code())
}

/// A buy as `quantity @ limit = value`, or what came instead.
fn bought(action: &Result<Action, &'static str>) -> String {
    match action {
        Ok(Action::Buy {
            qty,
            limit_price,
            order_usd,
            ..
        }) => format!("{qty} @ {limit_price} = {order_usd}"),
        other => format!("not a buy: {other:?}"),
    }
}

/// A flat agent at full conviction under a 100 USD order cap, the binding limit.
fn open(p: &CapabilityProfile, tif: TimeInForce, market: &Market) -> Result<Action, &'static str> {
    run(p, tif, &capped("100"), &flat_account(), market, "1")
}

const NO: &str = "no_opening_form";

fn hold(reason: HoldReason) -> Result<Action, &'static str> {
    Ok(Action::Hold { reason })
}

/// DEC-529 item 2: 100 USD over an 87.5 ask is 1.142857 shares. A profile with whole limits only
/// truncates to one share, which costs 87.5. It lists no `ioc` limit, so an `ioc` opening has no
/// form at all.
#[test]
fn one_whole_share_under_a_100_usd_cap_where_the_profile_offers_only_whole_limits() {
    let market = fractionable("87.49", "87.5");
    assert_eq!(
        bought(&open(&whole_only(), Day, &market)),
        "1 @ 87.5 = 87.5"
    );
    assert_eq!(open(&whole_only(), Ioc, &market).map(|_| ()), Err(NO));
}

/// One share at 100.01 is above the 100 cap. In whole shares the budget truncates to zero, below
/// the band of 0.05 × 1500 = 75, so nothing is sent. Fractional `day` limits buy
/// 100 ÷ 100.01 = 0.99990001, truncated to 0.9999, worth 99.999999.
#[test]
fn a_share_above_the_cap_sends_nothing_where_only_whole_limits_are_offered() {
    let market = fractionable("100", "100.01");
    let below = HoldReason::BelowBandAfterClipping;
    assert_eq!(open(&whole_only(), Day, &market), hold(below));
    let fractional = open(&fractional_day(), Day, &market);
    assert_eq!(bought(&fractional), "0.9999 @ 100.01 = 99.999999");
}

/// LT-14: where fractional `day` limits are offered, a `day` opening sizes on the market's own
/// grid as `propose` does: 1.1428 shares worth 99.995. The same profile lists no `gtc` fractional
/// limit, so a `gtc` opening is one whole share.
#[test]
fn fractional_day_limits_size_on_the_grid_and_a_gtc_opening_takes_whole_shares() {
    let market = fractionable("87.49", "87.5");
    let day = open(&fractional_day(), Day, &market);
    assert_eq!(bought(&day), "1.1428 @ 87.5 = 99.995");
    let gtc = open(&fractional_day(), Gtc, &market);
    assert_eq!(bought(&gtc), "1 @ 87.5 = 87.5");
}

/// LT-3 and rule 13: a profile with market orders in every form and no limit cell refuses the buy,
/// and never falls back to a market order. The same profile never stops the exit of 2.5 shares at
/// the 87.49 bid (2.5 × 87.49 = 218.725), and never turns a hold into a refusal.
#[test]
fn an_empty_intersection_refuses_the_buy_and_never_an_exit_or_a_hold() {
    let cells = [Whole, Fractional, Notional].map(|f| cell(MarketOrder, f, &[Day, Gtc]));
    let no_limit = profile(vec![(UsEquity, REGULAR, cells.into())]);
    let market = fractionable("87.49", "87.5");
    let mandate = capped("100");
    assert_eq!(open(&no_limit, Day, &market).map(|_| ()), Err(NO));
    let held = account("2.5", "87.49");
    let exit = run(&no_limit, Day, &mandate, &held, &market, "-1");
    let sell = Action::Sell {
        purpose: mandate_domain::Purpose::DiscretionaryExit,
        qty: qty("2.5"),
        limit_price: price("87.49"),
        order_usd: usd("218.725"),
        shape: OrderShape::Limit,
    };
    assert_eq!(exit, Ok(sell));
    let neutral = run(&no_limit, Day, &mandate, &flat_account(), &market, "0");
    assert_eq!(neutral, hold(HoldReason::BetweenThresholds));
}

/// DEC-854 items 1 to 3: notional is never an opening form, the `limit` cell must list the
/// opening's time in force, fractional comes before whole, and a missing row offers nothing. LT-3:
/// a profile may declare limits before and after the session, but the policy opens a US equity in
/// the regular session only, so the profile narrows the policy and never widens it.
#[test]
fn the_form_is_fractional_then_whole_in_the_policy_session_and_time_in_force() {
    let form = |p: &CapabilityProfile, class, session, tif| {
        opening_form(p, class, session, tif).map_err(|e| e.code())
    };
    let mut cells = vec![
        cell(Limit, Notional, &[Day]),
        cell(MarketOrder, Fractional, &[Day]),
    ];
    let notional_only = profile(vec![(UsEquity, REGULAR, cells.clone())]);
    assert_eq!(form(&notional_only, UsEquity, REGULAR, Day), Err(NO));
    cells.push(cell(Limit, Whole, &[Day]));
    let with_whole = profile(vec![(UsEquity, REGULAR, cells)]);
    assert_eq!(form(&with_whole, UsEquity, REGULAR, Day), Ok(Whole));
    let table = [
        (whole_only(), Day, Ok(Whole)),
        (whole_only(), Gtc, Ok(Whole)),
        (whole_only(), Ioc, Err(NO)),
        (fractional_day(), Day, Ok(Fractional)),
        (fractional_day(), Gtc, Ok(Whole)),
        (fractional_day(), Ioc, Err(NO)),
    ];
    for (p, tif, expected) in table {
        assert_eq!(form(&p, UsEquity, REGULAR, tif), expected, "{tif:?}");
    }
    let crypto = form(&fractional_day(), Crypto, MarketSession::Crypto, Gtc);
    assert_eq!(crypto, Err(NO), "no crypto row is declared");
    let sessions = [REGULAR, MarketSession::PreMarket, MarketSession::AfterHours];
    let limits = [cell(Limit, Whole, &[Day]), cell(Limit, Fractional, &[Day])];
    let extended = profile(sessions.map(|s| (UsEquity, s, limits.to_vec())).into());
    let outcomes = sessions.map(|s| form(&extended, UsEquity, s, Day));
    assert_eq!(outcomes, [Ok(Fractional), Err(NO), Err(NO)]);
    let mut early = fractionable("87.49", "87.5");
    early.session = MarketSession::PreMarket;
    assert_eq!(open(&extended, Day, &early).map(|_| ()), Err(NO));
}

/// The row's second clause: deployment refuses a policy the profile cannot meet, for every asset
/// class the mandate allows, in that class's opening session, and refuses an empty list.
#[test]
fn deployment_refuses_a_policy_the_profile_cannot_meet() {
    let check = |p: &CapabilityProfile, openings: &[(AssetClass, TimeInForce)]| {
        let openings: BTreeMap<_, _> = openings.iter().copied().collect();
        deployable(p, &openings).map_err(|e| e.code())
    };
    let equity = [(UsEquity, Day)];
    assert_eq!(check(&whole_only(), &equity), Ok(()));
    assert_eq!(check(&whole_only(), &[(UsEquity, Ioc)]), Err(NO));
    let both = [(UsEquity, Day), (Crypto, Gtc)];
    assert_eq!(check(&whole_only(), &both), Err(NO));
    assert_eq!(check(&whole_only(), &[]), Err(NO));
    let whole_day = || vec![cell(Limit, Whole, &[Day])];
    let crypto_limits = vec![cell(Limit, Fractional, &[Gtc, Ioc])];
    let crypto_row = (Crypto, MarketSession::Crypto, crypto_limits);
    let crypto_only = profile(vec![crypto_row.clone()]);
    assert_eq!(
        check(&crypto_only, &both),
        Err(NO),
        "every class is checked"
    );
    let with_crypto = profile(vec![(UsEquity, REGULAR, whole_day()), crypto_row]);
    assert_eq!(check(&with_crypto, &both), Ok(()));
    let late = profile(vec![(UsEquity, MarketSession::AfterHours, whole_day())]);
    assert_eq!(
        check(&late, &equity),
        Err(NO),
        "an equity opens in the regular session"
    );
}

/// DEC-854 item 3: whole shares are one share on a grid finer than a share, and the market's own
/// increment where that is coarser. At a 20 ask under the 100 cap, 100 ÷ 20 = 5 shares, which a
/// 2-share grid truncates to 4, worth 80 and above the 75 band. On the 0.0001 grid at an 87.5 ask
/// it is one share.
#[test]
fn whole_shares_keep_a_market_grid_coarser_than_one_share() {
    let mut coarse = swing_market();
    coarse.bid = price("19.99");
    coarse.ask = price("20");
    coarse.increment = qty("2");
    assert_eq!(bought(&open(&whole_only(), Day, &coarse)), "4 @ 20 = 80");
    let fine = fractionable("87.49", "87.5");
    assert_eq!(bought(&open(&whole_only(), Day, &fine)), "1 @ 87.5 = 87.5");
}

/// Rule 13 and DEC-854 item 4: a profile with whole limits only never cuts an exit down to whole
/// shares. Holding 2.5 shares at the 87.49 bid, the exit sells all 2.5 (2.5 × 87.49 = 218.725),
/// while a flat agent on the same profile and market buys one whole share.
#[test]
fn a_fractional_position_exits_whole_under_a_whole_only_profile() {
    let market = fractionable("87.49", "87.5");
    let held = account("2.5", "87.49");
    let exit = run(&whole_only(), Day, &capped("100"), &held, &market, "-1");
    let sell = Action::Sell {
        purpose: mandate_domain::Purpose::DiscretionaryExit,
        qty: qty("2.5"),
        limit_price: price("87.49"),
        order_usd: usd("218.725"),
        shape: OrderShape::Limit,
    };
    assert_eq!(exit, Ok(sell));
    assert_eq!(
        bought(&open(&whole_only(), Day, &market)),
        "1 @ 87.5 = 87.5"
    );
}

const ROWS: [(AssetClass, MarketSession); 4] = [
    (UsEquity, REGULAR),
    (UsEquity, MarketSession::PreMarket),
    (UsEquity, MarketSession::AfterHours),
    (Crypto, MarketSession::Crypto),
];
const TYPES: [OrderType; 3] = [MarketOrder, Limit, OrderType::StopLimit];
const FORMS: [QuantityForm; 3] = [Whole, Fractional, Notional];
const TIFS: [TimeInForce; 3] = [Day, Gtc, Ioc];

/// A decimal `value` × 10^−`scale`, in canonical text (no trailing zeros).
fn decimal(value: i128, scale: u32) -> String {
    let unit = 10i128.pow(scale);
    let (whole, fraction) = (value / unit, value % unit);
    if fraction == 0 {
        return whole.to_string();
    }
    let digits = format!("{fraction:0width$}", width = scale as usize);
    format!("{whole}.{}", digits.trim_end_matches('0'))
}

static BUYS_WHOLE: AtomicU32 = AtomicU32::new(0);
static BUYS_FRACTIONAL: AtomicU32 = AtomicU32::new(0);
static HOLDS: AtomicU32 = AtomicU32::new(0);
static REFUSALS: AtomicU32 = AtomicU32::new(0);

/// LT-3 and LT-2 over generated profiles: a flat, fully bullish agent buys only in a form both the
/// policy and the profile allow, sized at that form's grid, and holds when that grid leaves less
/// than the band; an empty intersection refuses a buy, never a hold. The same profile never changes the exit of a position
/// (rule 13). The oracle reads the generator's masks: bit i of a cell's mask is `TIFS[i]`, and 0 is
/// no cell. It sizes the 20 to 1400 USD order cap, the binding limit, in integer cents.
#[test]
fn every_buy_is_one_the_policy_and_the_generated_profile_both_allow() {
    let scenario = (
        proptest::collection::vec(0u8..8, 36),
        0usize..4,
        500i128..20_000,
        prop::sample::select(vec![0u32, 2, 4]),
        20i128..=1400,
        0usize..3,
        1i128..50_000,
    );
    let config = Config {
        cases: 256,
        failure_persistence: None,
        ..Config::default()
    };
    let result = TestRunner::new(config).run(&scenario, |(masks, r, ask, k, cap, t, held)| {
        prop_assume!(masks.iter().any(|m| *m != 0));
        let rows = ROWS
            .iter()
            .enumerate()
            .map(|(ri, (class, session))| {
                let cells = (0..9)
                    .filter_map(|i| {
                        let mask = masks[ri * 9 + i];
                        let tifs: Vec<TimeInForce> = (0..3)
                            .filter(|b| mask >> b & 1 == 1)
                            .map(|b| TIFS[b])
                            .collect();
                        (mask != 0).then(|| cell(TYPES[i / 3], FORMS[i % 3], &tifs))
                    })
                    .collect::<Vec<_>>();
                (*class, *session, cells)
            })
            .filter(|(_, _, cells)| !cells.is_empty())
            .collect();
        let generated = profile(rows);
        let (class, session) = ROWS[r];
        let offered = |f: usize| masks[r * 9 + 3 + f] >> t & 1 == 1;
        let policy_session = class == Crypto || session == REGULAR;
        let form = [1, 0].into_iter().find(|f| policy_session && offered(*f));
        let (bid, ask_text) = (decimal(ask - 1, 2), decimal(ask, 2));
        let increment = decimal(1, k);
        let mut market = match class {
            Crypto => crypto_market(&bid, &ask_text, &increment),
            UsEquity => fractionable(&bid, &ask_text),
        };
        market.increment = qty(&increment);
        market.session = session;
        let mandate = capped(&cap.to_string());
        let opening = run(&generated, TIFS[t], &mandate, &flat_account(), &market, "1");
        let scale = if form == Some(0) { 0 } else { k };
        let units = cap * 100 * 10i128.pow(scale) / ask;
        if units * ask < 7500 * 10i128.pow(scale) {
            HOLDS.fetch_add(1, Ordering::Relaxed);
            let below = HoldReason::BelowBandAfterClipping;
            prop_assert_eq!(opening, Ok(Action::Hold { reason: below }));
        } else if form.is_none() {
            REFUSALS.fetch_add(1, Ordering::Relaxed);
            prop_assert_eq!(opening.map(|_| ()), Err(NO));
        } else {
            let grid = [&BUYS_WHOLE, &BUYS_FRACTIONAL][usize::from(scale > 0)];
            grid.fetch_add(1, Ordering::Relaxed);
            let (n, value) = (decimal(units, scale), decimal(units * ask, scale + 2));
            prop_assert_eq!(bought(&opening), format!("{n} @ {ask_text} = {value}"));
        }
        let position = decimal(held, 2);
        let held_account = account(&position, &bid);
        let exit = run(&generated, TIFS[t], &mandate, &held_account, &market, "-1");
        let shape = OrderShape::Limit;
        let expected = Action::Sell {
            purpose: mandate_domain::Purpose::DiscretionaryExit,
            qty: qty(&position),
            limit_price: price(&bid),
            order_usd: usd(&decimal(held * (ask - 1), 4)),
            shape,
        };
        prop_assert_eq!(exit, Ok(expected));
        Ok(())
    });
    if let Err(failure) = result {
        panic!("{failure}");
    }
    for (name, count) in [
        ("whole buys", &BUYS_WHOLE),
        ("fractional buys", &BUYS_FRACTIONAL),
        ("holds", &HOLDS),
        ("refusals", &REFUSALS),
    ] {
        assert_ne!(
            count.load(Ordering::Relaxed),
            0,
            "the generator reached no {name}"
        );
    }
}
