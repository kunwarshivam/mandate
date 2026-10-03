//! RC-15's restricted arm, which its own case cannot reach in this harness: the closing-only
//! restriction arrives in step 2 as a `broker_order_update` reject, and that event is E7-2's. The
//! test runs the founder's steps 3 and 4 unchanged, with the account in the state §7.3's second row
//! gives that reject (`closing_only`), and with AAPL marked at step 4's own limit price, since the
//! case holds AAPL with no mark and the harness refuses to value an unmarked position it is not
//! deciding on (DEC-199). The same steps under an active account show the state is what decides.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use mandate_num::{Price, Qty, SignedQty, Usd};
use mandate_risk::{
    AccountState, AssetClass, AssetId, EtpClass, Exchange, InstrumentSnapshot, MarketSnapshot,
    QuoteCurrency, RestingSide, SaneQuote,
};
use mandate_time::UtcNanos;
use serde_json::{Map, json};

use super::{Gate, held_overnight, listing, market};
use crate::trading_domain::{
    BrokerProfile, LATER_PROPOSAL_WAITS, config, initial, instruments, run_case, run_step,
};
use crate::{Json, ensure, expect_eq, list_at, read_fixture, str_at};

fn rc_15() -> Result<(Json, Json), String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
    let fixture = read_fixture(&dir, "trading-domain.json").map(Arc::unwrap_or_clone)?;
    let case = list_at(&fixture, "cases")?
        .iter()
        .find(|c| c.get("id").and_then(Json::as_str) == Some("RC-15"))
        .cloned()
        .ok_or("no case RC-15")?;
    Ok((fixture, case))
}

/// A step's stated `decision`, and the outcome of comparing the gate's decision with it.
type Compared = (Json, Result<(), String>);

/// Steps 3 and 4 of RC-15, each decided in `state`, as the decision the gate made on each. Each
/// step has a gate of its own, so that step 3 allowed and never filled does not hold step 4 back
/// (DEC-259 item 7): this test is about the account's state, not the order between them.
fn steps_3_and_4(state: AccountState) -> Result<Vec<Compared>, String> {
    let (fixture, case) = rc_15()?;
    let config = config(&fixture, &case)?;
    let instruments = instruments(&case)?;
    let mut account = initial(
        &case,
        BrokerProfile::parse(str_at(&case, "broker_profile")?)?,
        &instruments,
    )?;
    let mark = json!({
        "at": "2026-09-22T10:00:30-04:00",
        "event": "mark",
        "data": { "instrument": "AAPL", "price": "155.00", "source": "quote" }
    });
    run_step(
        &mut account,
        &mut Gate::read(&fixture, &case)?,
        &mark,
        1,
        &instruments,
        &config,
    )?;
    let steps = list_at(&case, "steps")?;
    let mut out = Vec::new();
    for n in [2, 3] {
        let mut gate = Gate::read(&fixture, &case)?;
        gate.restrict(state);
        let step = steps.get(n).ok_or("RC-15 has four steps")?;
        let expected = crate::at(step, "expect.decision")?.clone();
        let (_, decision) = run_step(&mut account, &mut gate, step, n, &instruments, &config)?;
        out.push((
            expected.clone(),
            gate.check_decision(decision.as_ref(), &expected),
        ));
    }
    Ok(out)
}

#[test]
fn rc_15_denies_an_opening_and_allows_a_risk_exit_in_a_closing_only_account() -> Result<(), String>
{
    let decided = steps_3_and_4(AccountState::ClosingOnly)?;
    let wanted = [
        json!({ "verdict": "deny", "reason_code": "account_restricted" }),
        json!({ "verdict": "allow" }),
    ];
    for ((expected, outcome), wanted) in decided.into_iter().zip(wanted) {
        ensure(expected == wanted, || {
            format!("RC-15 changed: its step states {expected}, the test was written for {wanted}")
        })?;
        outcome?;
    }
    Ok(())
}

#[test]
fn the_same_steps_in_an_active_account_allow_the_opening() -> Result<(), String> {
    let decided = steps_3_and_4(AccountState::Active)?;
    let outcomes: Vec<Result<(), String>> = decided.into_iter().map(|(_, o)| o).collect();
    ensure(
        outcomes
            == vec![
                Err("decision.verdict: expected deny, got allow".to_owned()),
                Ok(()),
            ],
        || format!("{outcomes:?}"),
    )
}

/// DEC-199 item 6's listing and market, every member typed again here. Both structs are taken
/// apart whole, so a member `mandate-risk` adds does not compile until it is pinned. The median, the
/// trailing volume and the minimum order are also shown deciding a case at their edge
/// (`trading_domain_gate_harness`); the ADV only as at least 1,000,000, since its daily cap of
/// 500,000 shares cannot bind before the order cap of 50,000 does.
#[test]
fn the_listing_and_market_are_dec_199_item_6s() -> Result<(), String> {
    let num = |parsed: Result<Qty, _>| parsed.map_err(|e| format!("{e}"));
    let at = UtcNanos::parse_rfc3339("2026-09-22T14:00:00Z").map_err(|e| e.to_string())?;
    let limit = Price::parse("400").map_err(|e| e.to_string())?;
    let msft = AssetId::new("MSFT").map_err(|e| e.to_string())?;
    let InstrumentSnapshot {
        instrument,
        asset_class,
        exchange,
        status_active,
        tradable,
        fractionable,
        ipo,
        ptp_no_exception,
        etp,
        etp_classified_at,
        quote_currency,
        prior_close,
        median_dollar_volume_20d,
        median_dollar_volume_30d,
        min_order_size,
        qty_increment,
        halted,
        status_feed_current,
    } = listing(msft.clone(), (Some(Exchange::Nyse), None), true, limit, at)?;
    expect_eq("instrument", instrument, msft)?;
    expect_eq("asset_class", asset_class, AssetClass::UsEquity)?;
    expect_eq("exchange", exchange, Some(Exchange::Nyse))?;
    expect_eq("status_active", status_active, true)?;
    expect_eq("tradable", tradable, true)?;
    expect_eq("fractionable", fractionable, true)?;
    expect_eq("ipo", ipo, false)?;
    expect_eq("ptp_no_exception", ptp_no_exception, false)?;
    expect_eq("etp", etp, EtpClass::Plain)?;
    expect_eq("etp_classified_at", etp_classified_at, Some(at))?;
    expect_eq("quote_currency", quote_currency, Some(QuoteCurrency::Usd))?;
    expect_eq("prior_close", prior_close, Some(limit))?;
    expect_eq(
        "median_dollar_volume_20d",
        median_dollar_volume_20d,
        Some(Usd::parse("90000000").map_err(|e| e.to_string())?),
    )?;
    expect_eq("median_dollar_volume_30d", median_dollar_volume_30d, None)?;
    expect_eq("min_order_size", min_order_size, num(Qty::parse("1"))?)?;
    expect_eq(
        "qty_increment, a fractionable equity's grid of fractional shares",
        qty_increment,
        num(Qty::parse("0.000000001"))?,
    )?;
    expect_eq("halted", halted, false)?;
    expect_eq("status_feed_current", status_feed_current, true)?;

    let bid = Price::parse("399").map_err(|e| e.to_string())?;
    let ask = Price::parse("401").map_err(|e| e.to_string())?;
    let quote = SaneQuote { bid, ask, at };
    let MarketSnapshot {
        quote: stated,
        last_trade,
        trailing_5m_volume,
        adv_20d,
    } = market(quote, limit)?;
    expect_eq("quote", stated, Some(quote))?;
    expect_eq("last_trade", last_trade, Some((limit, at)))?;
    expect_eq(
        "trailing_5m_volume",
        trailing_5m_volume,
        Some(num(Qty::parse("1000000"))?),
    )?;
    expect_eq("adv_20d", adv_20d, Some(num(Qty::parse("10000000"))?))
}

/// The listing's quantity grid is the venue's (DEC-427 item 6): a whole-share equity's is 1, and a
/// crypto pair's is its `min_trade_increment`, never a grid read from `fractionable`.
#[test]
fn the_listing_states_the_venue_s_quantity_grid() -> Result<(), String> {
    let num = |parsed: Result<Qty, _>| parsed.map_err(|e| format!("{e}"));
    let at = UtcNanos::parse_rfc3339("2026-09-22T14:00:00Z").map_err(|e| e.to_string())?;
    let limit = Price::parse("400").map_err(|e| e.to_string())?;
    let msft = AssetId::new("MSFT").map_err(|e| e.to_string())?;
    let btc = AssetId::new("BTC/USD").map_err(|e| e.to_string())?;
    let whole = listing(msft, (Some(Exchange::Nyse), None), false, limit, at)?;
    expect_eq(
        "a whole-share equity's grid",
        whole.qty_increment,
        num(Qty::parse("1"))?,
    )?;
    let pair = super::Pair {
        quote_currency: Some(QuoteCurrency::Usd),
        min_order_size: num(Qty::parse("0.0001"))?,
    };
    for fractionable in [true, false] {
        let crypto = listing(btc.clone(), (None, Some(pair)), fractionable, limit, at)?;
        expect_eq(
            "a crypto pair's grid, its min_trade_increment",
            crypto.qty_increment,
            num(Qty::parse("0.0001"))?,
        )?;
    }
    let unstated = super::pair("BTC/USD", &json!({ "symbol": "BTC/USD" }))?;
    let defaulted = listing(btc, (None, Some(unstated)), true, limit, at)?;
    expect_eq(
        "a crypto pair stating no min_trade_increment, on the smallest Qty",
        defaulted.qty_increment,
        num(Qty::parse("0.000000001"))?,
    )?;
    Ok(())
}

#[test]
fn a_blocked_account_denies_both_steps_account_trading_blocked() -> Result<(), String> {
    let decided = steps_3_and_4(AccountState::Blocked)?;
    let outcomes: Vec<Result<(), String>> = decided.into_iter().map(|(_, o)| o).collect();
    ensure(
        outcomes
            == vec![
                Err(
                    "decision.reason_code: expected account_restricted, got account_trading_blocked"
                        .to_owned(),
                ),
                Err("decision.verdict: expected allow, got deny".to_owned()),
            ],
        || format!("{outcomes:?}"),
    )
}

fn case_named(id: &str) -> Result<(Json, Json), String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
    let fixture = read_fixture(&dir, "trading-domain.json").map(Arc::unwrap_or_clone)?;
    let case = list_at(&fixture, "cases")?
        .iter()
        .find(|c| c.get("id").and_then(Json::as_str) == Some(id))
        .cloned()
        .ok_or_else(|| format!("no case {id}"))?;
    Ok((fixture, case))
}

/// What a test does to a case before it runs.
type Edit = Box<dyn FnOnce(&mut Json) -> Result<(), String>>;

/// Case `id`, without its variants, with `edit` applied, run as `tests/refcases.rs` runs it.
fn run_edited(id: &str, edit: Edit) -> Result<(), String> {
    let (fixture, mut case) = case_named(id)?;
    object_at(&mut case, &[])?.remove("variants");
    edit(&mut case)?;
    run_case(&fixture, &case)
}

/// The object at `path` inside `value`.
fn object_at<'a>(value: &'a mut Json, path: &[&str]) -> Result<&'a mut Map<String, Json>, String> {
    let mut here = value;
    for key in path {
        here = here
            .get_mut(*key)
            .ok_or_else(|| format!("no `{key}` on the way to {path:?}"))?;
    }
    here.as_object_mut()
        .ok_or_else(|| format!("{path:?} is not an object"))
}

/// Sets `key` of the object at `path` to `new`.
fn put(case: &mut Json, path: &[&str], key: &str, new: Json) -> Result<(), String> {
    object_at(case, path)?.insert(key.to_owned(), new);
    Ok(())
}

fn steps_of(case: &mut Json) -> Result<&mut Vec<Json>, String> {
    case.get_mut("steps")
        .and_then(Json::as_array_mut)
        .ok_or_else(|| "the case has no steps".to_owned())
}

fn step_of(case: &mut Json, n: usize) -> Result<&mut Json, String> {
    steps_of(case)?
        .get_mut(n)
        .ok_or_else(|| format!("the case has no step {}", n.saturating_add(1)))
}

fn account(key: &'static str, value: Json) -> Edit {
    Box::new(move |c| put(c, &["initial", "account"], key, value))
}

fn waits() -> Result<(), String> {
    Err(LATER_PROPOSAL_WAITS.to_owned())
}

/// RC-09B as the founder wrote it: an opening allowed with one day trade left, the fill that opens
/// AAPL the same day, an opening in MSFT denied because AAPL is open same-day, the risk exit, and
/// its fill, which makes the third day trade in the window (§9.2, DEC-259 item 7).
#[test]
fn rc_09b_passes_as_written() -> Result<(), String> {
    run_edited("RC-09B", Box::new(|_| Ok(())))
}

/// Each expectation RC-09B states is compared: a day-trade count one off, at the opening or after
/// the closing fill, or the MSFT opening allowed, fails the case at that step.
#[test]
fn rc_09b_fails_on_each_edited_expectation() -> Result<(), String> {
    let edits: [(usize, &str, Json, &str); 4] = [
        (
            0,
            "day_trade_count",
            json!(3),
            "step 1: day_trade_count: expected 3, got 2",
        ),
        (
            4,
            "day_trade_count",
            json!(2),
            "step 5: day_trade_count: expected 2, got 3",
        ),
        (
            2,
            "decision",
            json!({ "verdict": "allow" }),
            "step 3: decision.verdict: expected allow, got deny",
        ),
        (
            0,
            "decision",
            json!({ "verdict": "deny" }),
            "step 1: decision.verdict: expected deny, got allow",
        ),
    ];
    for (n, key, value, wanted) in edits {
        let outcome = run_edited(
            "RC-09B",
            Box::new(move |c| put(step_of(c, n)?, &["expect"], key, value)),
        );
        expect_eq(
            &format!("step {} {key}", n.saturating_add(1)),
            outcome,
            Err(wanted.to_owned()),
        )?;
    }
    Ok(())
}

/// `initial.account`'s §9.2 members reach the gate (DEC-284): prior-close equity at the threshold
/// lifts the budget, a missing prior day trade lowers the count, `intraday_margin` reads no
/// budget, a generic margin account must state its regime, and the multiplier must be 1.
#[test]
fn rc_09b_s_regime_and_figures_are_read() -> Result<(), String> {
    let step_3_allowed = || Err("step 3: decision.verdict: expected deny, got allow".to_owned());
    expect_eq(
        "last_equity at the threshold",
        run_edited("RC-09B", account("last_equity", json!("25000.00"))),
        step_3_allowed(),
    )?;
    expect_eq(
        "last_equity a cent below it",
        run_edited("RC-09B", account("last_equity", json!("24999.99"))),
        Ok(()),
    )?;
    expect_eq(
        "one prior day trade fewer",
        run_edited(
            "RC-09B",
            account(
                "prior_day_trades",
                json!([{ "date": "2026-09-18", "instrument": "CCC" }]),
            ),
        ),
        Err("step 1: day_trade_count: expected 2, got 1".to_owned()),
    )?;
    expect_eq(
        "intraday_margin",
        run_edited("RC-09B", account("regime", json!("intraday_margin"))),
        step_3_allowed(),
    )?;
    expect_eq(
        "no regime",
        run_edited(
            "RC-09B",
            Box::new(|c| {
                object_at(c, &["initial", "account"])?.remove("regime");
                Ok(())
            }),
        ),
        Err(
            "step 1: a generic margin account states no `regime`, which check 8 reads (§9.2)"
                .to_owned(),
        ),
    )?;
    expect_eq(
        "multiplier 1",
        run_edited("RC-09B", account("multiplier", json!(1))),
        Ok(()),
    )?;
    expect_eq(
        "multiplier 4",
        run_edited("RC-09B", account("multiplier", json!(4))),
        Err("`multiplier` 4 is not 1, which pauses the agents (§7.2's 1× requirement)".to_owned()),
    )
}

fn fill_of(at: &str, qty: &str, instrument: &str) -> Json {
    json!({
        "at": at,
        "event": "fill",
        "data": { "instrument": instrument, "side": "buy", "qty_gross": qty, "price": "150.00" }
    })
}

const TEN_OH_ONE: &str = "2026-09-21T10:01:00-04:00";
const TEN_OH_TWO: &str = "2026-09-21T10:02:00-04:00";

/// RC-09B with its step 2, the AAPL fill, replaced by `fills`.
fn rc_09b_filled_by(fills: Vec<Json>) -> Edit {
    Box::new(move |c| {
        let steps = steps_of(c)?;
        ensure(steps.len() > 1, || "RC-09B has a step 2".to_owned())?;
        steps.splice(1..2, fills);
        Ok(())
    })
}

/// A later proposal is decided only once every earlier allowed proposal was filled in full on its
/// side (DEC-259 item 7). With no fill, a partial fill, a fill in another instrument, a fill larger
/// than the order (even when one of its size follows), or the same buy proposed twice with no fill
/// between (#349's protection), the next proposal waits; two partial fills that together fill the
/// order let it be decided.
#[test]
fn a_later_proposal_waits_for_a_full_fill_on_its_side() -> Result<(), String> {
    let cases: [(&str, Edit, Result<(), String>); 7] = [
        ("no fill", rc_09b_filled_by(vec![]), waits()),
        (
            "a partial fill",
            rc_09b_filled_by(vec![fill_of(TEN_OH_ONE, "9", "AAPL")]),
            waits(),
        ),
        (
            "a fill in another instrument",
            rc_09b_filled_by(vec![fill_of(TEN_OH_ONE, "10", "MSFT")]),
            waits(),
        ),
        (
            "a fill larger than the order",
            rc_09b_filled_by(vec![fill_of(TEN_OH_ONE, "11", "AAPL")]),
            waits(),
        ),
        (
            "a fill larger than the order, then one of its size",
            rc_09b_filled_by(vec![
                fill_of(TEN_OH_ONE, "11", "AAPL"),
                fill_of(TEN_OH_TWO, "10", "AAPL"),
            ]),
            waits(),
        ),
        (
            "the same buy twice with no fill between",
            Box::new(|c| {
                let steps = steps_of(c)?;
                let first = steps.first().cloned().ok_or("RC-09B has a step 1")?;
                steps.insert(1, first);
                Ok(())
            }),
            waits(),
        ),
        (
            "two partial fills that fill it in full",
            rc_09b_filled_by(vec![
                fill_of(TEN_OH_ONE, "4", "AAPL"),
                fill_of(TEN_OH_TWO, "6", "AAPL"),
            ]),
            Ok(()),
        ),
    ];
    for (what, edit, wanted) in cases {
        expect_eq(what, run_edited("RC-09B", edit), wanted)?;
    }
    Ok(())
}

/// RC-08 with its step 3, the denied proposal, listed again after it, expecting `decision`.
fn rc_08_twice(decision: Json) -> Edit {
    Box::new(move |c| {
        let mut copy = step_of(c, 2)?.clone();
        put(&mut copy, &["expect"], "decision", decision)?;
        steps_of(c)?.insert(3, copy);
        Ok(())
    })
}

/// A refused proposal leaves no trace (DEC-259 item 7): RC-08's denied proposal, listed twice, is
/// decided twice and denied twice.
#[test]
fn a_denied_proposal_never_holds_back_the_next() -> Result<(), String> {
    expect_eq(
        "RC-08 with its proposal twice",
        run_edited(
            "RC-08",
            rc_08_twice(json!({
                "verdict": "deny", "reason_code": "insufficient_settled_buying_power"
            })),
        ),
        Ok(()),
    )?;
    expect_eq(
        "RC-08's copy expecting an allow",
        run_edited("RC-08", rc_08_twice(json!({ "verdict": "allow" }))),
        Err("step 4: decision.verdict: expected allow, got deny".to_owned()),
    )
}
/// RC-09B's figures after each step, typed by hand from its steps: one order in AAPL submitted at
/// 10:00 and filled at 10:01 for 10 shares, an opening; then the risk exit submitted at 14:00 and
/// filled at 14:00:05 for 10 more, not an opening. The latest fill on each side is its fill step's
/// instant, never the proposal's (DEC-259 item 7).
#[test]
fn the_conduct_figures_are_the_fill_steps() -> Result<(), String> {
    let (fixture, case) = case_named("RC-09B")?;
    let config = config(&fixture, &case)?;
    let instruments = instruments(&case)?;
    let mut account = initial(
        &case,
        BrokerProfile::parse(str_at(&case, "broker_profile")?)?,
        &instruments,
    )?;
    let mut gate = Gate::read(&fixture, &case)?;
    let instant = |text: &str| UtcNanos::parse_rfc3339(text).map_err(|e| e.to_string());
    let aapl = AssetId::new("AAPL").map_err(|e| e.to_string())?;
    let qty = |text: &str| Qty::parse(text).map_err(|e| e.to_string());
    let steps = list_at(&case, "steps")?;
    let step = |n: usize| steps.get(n).ok_or("RC-09B has five steps");

    for n in 0..2 {
        run_step(&mut account, &mut gate, step(n)?, n, &instruments, &config)?;
    }
    let (conduct, orders_today) = gate.conduct(instant("2026-09-21T11:00:00-04:00")?)?;
    expect_eq("orders today", orders_today, 1)?;
    expect_eq(
        "orders per instrument",
        conduct.orders_today_per_instrument.clone(),
        BTreeMap::from([(aapl.clone(), 1)]),
    )?;
    expect_eq(
        "filled today",
        conduct.filled_today.clone(),
        BTreeMap::from([(aapl.clone(), 1)]),
    )?;
    expect_eq(
        "participation",
        conduct.participation_today.clone(),
        BTreeMap::from([(aapl.clone(), qty("10")?)]),
    )?;
    expect_eq(
        "latest fill per side",
        conduct.last_opposite_fill_at.clone(),
        BTreeMap::from([(
            (aapl.clone(), RestingSide::Buy),
            instant("2026-09-21T10:01:00-04:00")?,
        )]),
    )?;

    for n in 2..5 {
        run_step(&mut account, &mut gate, step(n)?, n, &instruments, &config)?;
    }
    let (conduct, orders_today) = gate.conduct(instant("2026-09-21T15:00:00-04:00")?)?;
    expect_eq("orders today after the exit", orders_today, 1)?;
    expect_eq(
        "orders per instrument after the exit",
        conduct.orders_today_per_instrument.clone(),
        BTreeMap::from([(aapl.clone(), 2)]),
    )?;
    expect_eq(
        "filled today after the exit",
        conduct.filled_today.clone(),
        BTreeMap::from([(aapl.clone(), 2)]),
    )?;
    expect_eq(
        "participation after the exit",
        conduct.participation_today.clone(),
        BTreeMap::from([(aapl.clone(), qty("20")?)]),
    )?;
    expect_eq(
        "latest fill per side after the exit",
        conduct.last_opposite_fill_at.clone(),
        BTreeMap::from([
            (
                (aapl.clone(), RestingSide::Buy),
                instant("2026-09-21T10:01:00-04:00")?,
            ),
            (
                (aapl.clone(), RestingSide::Sell),
                instant("2026-09-21T14:00:05-04:00")?,
            ),
        ]),
    )?;

    let (conduct, orders_today) = gate.conduct(instant("2026-09-22T10:00:00-04:00")?)?;
    expect_eq("orders today, the next day", orders_today, 0)?;
    expect_eq(
        "a day later only the latest fills remain",
        (
            conduct.orders_today_per_instrument.is_empty(),
            conduct.filled_today.is_empty(),
            conduct.participation_today.is_empty(),
            conduct.last_opposite_fill_at.len(),
        ),
        (true, true, true, 2),
    )
}

/// RC-15 with its steps replaced by a risk exit of its AAPL proposed at 10:00:00 and filled at
/// 10:00:50, and an opening buy at `at` expecting `decision`.
fn exit_then_buy_at(at: &'static str, decision: Json) -> Edit {
    Box::new(move |c| {
        let members = object_at(c, &[])?;
        members.remove("variants");
        members.insert("steps".to_owned(), json!([
            {
                "at": "2026-09-22T10:00:00-04:00",
                "event": "mark",
                "data": { "instrument": "AAPL", "price": "150.00", "source": "quote" }
            },
            {
                "at": "2026-09-22T10:00:00-04:00",
                "event": "propose_order",
                "data": {
                    "instrument": "AAPL", "side": "sell", "type": "limit",
                    "qty": "10", "limit_price": "150.00", "purpose": "risk_exit"
                },
                "expect": { "decision": { "verdict": "allow" } }
            },
            {
                "at": "2026-09-22T10:00:50-04:00",
                "event": "fill",
                "data": { "instrument": "AAPL", "side": "sell", "qty_gross": "10", "price": "150.00" }
            },
            {
                "at": at,
                "event": "propose_order",
                "data": {
                    "instrument": "AAPL", "side": "buy", "type": "limit",
                    "qty": "1", "limit_price": "150.00", "purpose": "open"
                },
                "expect": { "decision": decision }
            }
        ]));
        Ok(())
    })
}

/// The latest fill reaches check 6 at its fill step's instant (§9.6's opposite-fill interval, 60
/// seconds in `test_default`): RC-15's account sells its AAPL for risk at 10:00:00, the fill lands
/// at 10:00:50, and an opening buy 70 seconds after the proposal but 20 after the fill is denied;
/// one 61 seconds after the fill is allowed.
#[test]
fn the_opposite_fill_interval_runs_from_the_fill_step() -> Result<(), String> {
    expect_eq(
        "20 seconds after the fill",
        run_edited(
            "RC-15",
            exit_then_buy_at(
                "2026-09-22T10:01:10-04:00",
                json!({ "verdict": "deny", "reason_code": "opposite_fill_interval" }),
            ),
        ),
        Ok(()),
    )?;
    expect_eq(
        "61 seconds after the fill",
        run_edited(
            "RC-15",
            exit_then_buy_at("2026-09-22T10:01:51-04:00", json!({ "verdict": "allow" })),
        ),
        Ok(()),
    )
}

/// While another item keeps a case from running, a later proposal is listed as waiting only when
/// the `fill` steps before it do not fill the proposal before it in full on its side
/// (`a_proposal_follows_one_not_filled_in_full`). RC-09B's MSFT opening is followed by the risk
/// exit with no fill, so it is listed; without that opening every proposal is filled in full
/// before the next, so only the other item is.
#[test]
fn a_case_that_cannot_run_lists_a_later_proposal_only_after_an_unfilled_one() -> Result<(), String>
{
    let waiting = "initial `agents` not interpreted until E7-5";
    expect_eq(
        "RC-09B",
        run_edited(
            "RC-09B",
            Box::new(|c| put(c, &["initial"], "agents", json!({}))),
        ),
        Err(format!("{LATER_PROPOSAL_WAITS}; {waiting}")),
    )?;
    expect_eq(
        "RC-09B without the MSFT opening",
        run_edited(
            "RC-09B",
            Box::new(|c| {
                put(c, &["initial"], "agents", json!({}))?;
                let steps = steps_of(c)?;
                ensure(steps.len() > 2, || "RC-09B has a step 3".to_owned())?;
                steps.remove(2);
                Ok(())
            }),
        ),
        Err(waiting.to_owned()),
    )
}

/// RC-09B's account holding 10 AAPL overnight, with `steps`.
fn rc_09b_holding_aapl(steps: Json) -> Edit {
    Box::new(move |c| {
        put(
            c,
            &["initial"],
            "positions",
            json!([{ "instrument": "AAPL", "qty": "10", "cost_basis": "1500.00" }]),
        )?;
        put(
            c,
            &["initial", "account"],
            "prior_day_trades",
            json!([{ "date": "2026-09-18", "instrument": "CCC" }]),
        )?;
        object_at(c, &[])?.insert("steps".to_owned(), steps);
        Ok(())
    })
}

fn aapl_fill(at: &str, side: &str, qty: &str) -> Json {
    json!({
        "at": at,
        "event": "fill",
        "data": { "instrument": "AAPL", "side": side, "qty_gross": qty, "price": "150.00" }
    })
}

/// The shares held at the start of today are each position less today's fills in it (DEC-284
/// item 5), typed by hand: 10 AAPL held overnight, 10 bought today and 15 sold, leaving 5. The
/// sale takes the 10 overnight shares first and 5 of today's, so it closes a same-day open: one day
/// trade today and one on 2026-09-18 make 2, one remains, and the 5 bought today and still held
/// are open same-day. An MSFT opening then needs 2 and is denied. Reading the 5 still held as the
/// overnight shares would count 2 today, leave none open same-day, and allow it.
#[test]
fn the_overnight_shares_are_the_position_less_today_s_fills() -> Result<(), String> {
    let steps = |decision: Json| {
        json!([
            aapl_fill("2026-09-21T10:00:00-04:00", "buy", "10"),
            aapl_fill("2026-09-21T11:00:00-04:00", "sell", "15"),
            {
                "at": "2026-09-21T12:00:00-04:00",
                "event": "propose_order",
                "data": {
                    "instrument": "MSFT", "side": "buy", "type": "limit",
                    "qty": "5", "limit_price": "400.00", "purpose": "open"
                },
                "expect": { "day_trade_count": 2, "decision": decision }
            }
        ])
    };
    expect_eq(
        "denied",
        run_edited(
            "RC-09B",
            rc_09b_holding_aapl(steps(json!({
                "verdict": "deny", "reason_code": "legacy_pdt_day_trade_budget"
            }))),
        ),
        Ok(()),
    )?;
    expect_eq(
        "an allow is refused",
        run_edited(
            "RC-09B",
            rc_09b_holding_aapl(steps(json!({ "verdict": "allow" }))),
        ),
        Err("step 3: decision.verdict: expected allow, got deny".to_owned()),
    )
}

/// A short equity position is refused where a `day_trade_count` expectation reaches the fold,
/// never folded as long (#412 review, nit 3, DEC-314): `decide` refuses a short snapshot, but the
/// expectation reaches [`Gate::fold`] without it, and folding the position's magnitude would hold
/// 10 AAPL long, find no day trade in today's fills, and answer the prior day's one. The refusal
/// is pending E6-10: until its story lands the stub reports itself, and this test pins the
/// refusal's text against it.
#[test]
fn a_short_position_is_refused_where_a_day_trade_count_is_expected() -> Result<(), String> {
    expect_eq(
        "a short position under a day_trade_count expectation",
        run_edited(
            "RC-09B",
            Box::new(|c| {
                put(
                    c,
                    &["initial"],
                    "positions",
                    json!([{ "instrument": "AAPL", "qty": "-10", "cost_basis": "-1500.00" }]),
                )?;
                put(
                    c,
                    &["initial", "account"],
                    "prior_day_trades",
                    json!([{ "date": "2026-09-18", "instrument": "CCC" }]),
                )?;
                object_at(c, &[])?.insert(
                    "steps".to_owned(),
                    json!([{
                        "at": "2026-09-21T12:00:00-04:00",
                        "event": "mark",
                        "data": { "instrument": "AAPL", "price": "150.00", "source": "quote" },
                        "expect": { "day_trade_count": 1 }
                    }]),
                );
                Ok(())
            }),
        ),
        Err(
            "step 1: `AAPL` is held short, which the day-trade fold refuses (its magnitude would fold as long and understate the day-trade count)"
                .to_owned(),
        ),
    )
}

/// DEC-314 item 1's boundary, where it turns: only a negative `SignedQty` is a short. A flat
/// position, `0`, is held overnight as no shares and a long one as its shares, so a refusal widened
/// to zero (`is_negative() || is_zero()`) fails here. The boundary is pinned at
/// [`held_overnight`] because no case can hand the fold a flat position: the accounting keeps
/// none (`Account::opening` drops a zero entry and a fill that closes a position removes it), which
/// `a_flat_initial_position_is_no_position_to_the_fold` pins at the case level. Every negative is
/// refused, from a ten-millionth of a share through half a share to whole shares, so a
/// refusal narrowed to a whole share (`qty.abs() >= 1`), which would fold a sub-share short as long,
/// fails here too (#438 review, round 2, blocker 2), and so does one narrowed to a hundred-millionth
/// (`qty.abs() >= 0.00000001`), which would fold the smallest short a case can state, a billionth
/// of a share, as long (#438 review, round 3, m6). The refusal is DEC-314's text, swapped in for
/// the stub's report by E6-10's implementation (DEC-77's 2026-09-27 amendment, DEC-395 item 3).
#[test]
fn only_a_negative_quantity_is_a_short_to_the_fold() -> Result<(), String> {
    for (position, held) in [("0", "0"), ("10", "10")] {
        let qty = SignedQty::parse(position).map_err(|e| e.to_string())?;
        let shares = Qty::parse(held).map_err(|e| e.to_string())?;
        expect_eq(position, held_overnight("AAPL", qty), Ok(shares))?;
    }
    for position in ["-0.000000001", "-0.0000001", "-0.5", "-1", "-10"] {
        let qty = SignedQty::parse(position).map_err(|e| e.to_string())?;
        expect_eq(
            position,
            held_overnight("AAPL", qty),
            Err(
                "`AAPL` is held short, which the day-trade fold refuses (its magnitude would fold as long and understate the day-trade count)"
                    .to_owned(),
            ),
        )?;
    }
    Ok(())
}

/// A flat equity position stated in `initial.positions`, `qty: "0"`, is no position at all to the
/// fold (the accounting drops it), so under the short test's `day_trade_count` expectation the case
/// still runs and the count is the prior day's one; the count is compared, since 2 is refused.
#[test]
fn a_flat_initial_position_is_no_position_to_the_fold() -> Result<(), String> {
    let flat = |count: u64| -> Edit {
        Box::new(move |c| {
            put(
                c,
                &["initial"],
                "positions",
                json!([{ "instrument": "AAPL", "qty": "0", "cost_basis": "0" }]),
            )?;
            put(
                c,
                &["initial", "account"],
                "prior_day_trades",
                json!([{ "date": "2026-09-18", "instrument": "CCC" }]),
            )?;
            object_at(c, &[])?.insert(
                "steps".to_owned(),
                json!([{
                    "at": "2026-09-21T12:00:00-04:00",
                    "event": "mark",
                    "data": { "instrument": "AAPL", "price": "150.00", "source": "quote" },
                    "expect": { "day_trade_count": count }
                }]),
            );
            Ok(())
        })
    };
    expect_eq(
        "a flat AAPL position",
        run_edited("RC-09B", flat(1)),
        Ok(()),
    )?;
    expect_eq(
        "a flat AAPL position, expecting 2",
        run_edited("RC-09B", flat(2)),
        Err("step 1: day_trade_count: expected 2, got 1".to_owned()),
    )
}

/// RC-09 with `crypto_status` stated in the initial account or in a `broker_account_update` at
/// 09:30, before its first step.
fn rc_09_with_crypto_status(in_update: bool, status: Json) -> Result<(), String> {
    if !in_update {
        return run_edited("RC-09", account("crypto_status", status));
    }
    run_edited(
        "RC-09",
        Box::new(move |c| {
            steps_of(c)?.insert(
                0,
                json!({
                    "at": "2026-09-21T09:30:00-04:00",
                    "event": "broker_account_update",
                    "data": { "crypto_status": status }
                }),
            );
            Ok(())
        }),
    )
}

/// A `crypto_status` is read wherever a case states it, initial or in an update, and refuses
/// nothing but crypto openings (DEC-315 item 1, DEC-395 item 4): under `ACTIVE` and `INACTIVE`
/// alike RC-09 passes as written, its AAPL opening denied by the day-trade budget, never by the
/// crypto status, and its BTCUSD risk exit allowed, since check 1's `crypto_active` never refuses a
/// risk exit (`AGENTS.md` rule 13). These replace the two pins of the stub's report (DEC-315
/// item 3).
#[test]
fn a_crypto_status_never_refuses_an_equity_decision_or_a_crypto_risk_exit() -> Result<(), String> {
    for in_update in [false, true] {
        for status in ["ACTIVE", "INACTIVE"] {
            expect_eq(
                &format!("{status}, in an update: {in_update}"),
                rc_09_with_crypto_status(in_update, json!(status)),
                Ok(()),
            )?;
        }
    }
    Ok(())
}

/// A `crypto_status` that is not a string is refused, initial or in an update, as `status` is,
/// rather than read as active or inactive (DEC-395 item 2).
#[test]
fn a_crypto_status_that_is_not_a_string_is_refused() -> Result<(), String> {
    for value in [json!(true), json!(null), json!(1)] {
        expect_eq(
            &format!("initial {value}"),
            rc_09_with_crypto_status(false, value.clone()),
            Err("account `crypto_status` is not a string".to_owned()),
        )?;
        expect_eq(
            &format!("updated {value}"),
            rc_09_with_crypto_status(true, value),
            Err("step 1: account `crypto_status` is not a string".to_owned()),
        )?;
    }
    Ok(())
}

/// Only `ACTIVE`, spelled exactly, leaves crypto openings open (DEC-395 item 2): a status in
/// another case, padded, or empty denies RC-09's BTCUSD opening `crypto_account_inactive`, as
/// `status` reads only an exact `ACTIVE` as active. `last_equity` lifts the legacy-pdt budget so
/// check 1 is what decides.
#[test]
fn only_an_exact_active_crypto_status_leaves_crypto_openings_open() -> Result<(), String> {
    let buy_btc = |status: &'static str, decision: Json| -> Edit {
        Box::new(move |c| {
            put(c, &["initial", "account"], "crypto_status", json!(status))?;
            put(c, &["initial", "account"], "last_equity", json!("25000.00"))?;
            object_at(c, &[])?.insert(
                "steps".to_owned(),
                json!([{
                    "at": "2026-09-21T10:00:00-04:00",
                    "event": "propose_order",
                    "data": {
                        "instrument": "BTCUSD", "side": "buy", "type": "limit",
                        "qty": "0.01", "limit_price": "60000.00", "purpose": "open"
                    },
                    "expect": { "decision": decision }
                }]),
            );
            Ok(())
        })
    };
    expect_eq(
        "ACTIVE",
        run_edited("RC-09", buy_btc("ACTIVE", json!({ "verdict": "allow" }))),
        Ok(()),
    )?;
    for status in ["active", "Active", " ACTIVE", "ACTIVE ", ""] {
        expect_eq(
            status,
            run_edited(
                "RC-09",
                buy_btc(
                    status,
                    json!({ "verdict": "deny", "reason_code": "crypto_account_inactive" }),
                ),
            ),
            Ok(()),
        )?;
    }
    Ok(())
}

/// An equity fill on a trading day before the step's is refused rather than folded, since its day
/// trades would need that day's fold (DEC-284 item 5). One earlier the same day is folded: the sale
/// of an overnight AAPL share at 09:45 puts AAPL among the securities sold earlier today, so
/// RC-09B's AAPL opening needs 2 with 1 remaining and is denied.
#[test]
fn an_equity_fill_on_an_earlier_trading_day_is_refused() -> Result<(), String> {
    let with_fill_at = |at: &str| {
        let at = at.to_owned();
        let edit: Edit = Box::new(move |c| {
            steps_of(c)?.insert(0, aapl_fill(&at, "sell", "1"));
            Ok(())
        });
        rc_09b_holding_aapl_then_rc_09b(edit)
    };
    expect_eq(
        "on Friday",
        run_edited("RC-09B", with_fill_at("2026-09-18T15:00:00-04:00")),
        Err(
            "step 2: `AAPL` was filled on a trading day before today's, whose day trades would need that day's fold (DEC-284)"
                .to_owned(),
        ),
    )?;
    expect_eq(
        "on Monday morning",
        run_edited("RC-09B", with_fill_at("2026-09-21T09:45:00-04:00")),
        Err("step 2: decision.verdict: expected allow, got deny".to_owned()),
    )
}

/// RC-09B's own steps on its account holding 10 AAPL overnight, its prior day trades as written,
/// then `edit`.
fn rc_09b_holding_aapl_then_rc_09b(edit: Edit) -> Edit {
    Box::new(move |c| {
        put(
            c,
            &["initial"],
            "positions",
            json!([{ "instrument": "AAPL", "qty": "10", "cost_basis": "1500.00" }]),
        )?;
        edit(c)
    })
}

/// RC-09 as the founder wrote it: the AAPL opening denied with no day trade left, then the crypto
/// risk exit allowed, the day-trade count unchanged by crypto (§9.2), and the denied opening
/// holding nothing back (DEC-259 item 7).
#[test]
fn rc_09_passes_as_written() -> Result<(), String> {
    run_edited("RC-09", Box::new(|_| Ok(())))
}

/// RC-09 with BTCUSD's `symbol` as `symbol` (or none when `None`), and its step 3 replaced by a
/// crypto opening of `qty` expecting `decision`.
fn rc_09_crypto_opening(symbol: Option<&'static str>, qty: &'static str, decision: Json) -> Edit {
    Box::new(move |c| {
        let listed = object_at(c, &["instruments", "BTCUSD"])?;
        match symbol {
            Some(symbol) => listed.insert("symbol".to_owned(), json!(symbol)),
            None => listed.remove("symbol"),
        };
        *step_of(c, 2)? = json!({
            "at": "2026-09-21T14:05:00-04:00",
            "event": "propose_order",
            "data": {
                "instrument": "BTCUSD", "side": "buy", "type": "limit",
                "qty": qty, "limit_price": "60000.00", "purpose": "open"
            },
            "expect": { "decision": decision }
        });
        Ok(())
    })
}

/// A crypto opening is judged by the quote currency its `symbol` names (§3.2 item 7, DEC-285): a
/// `BTC/USD` opening is allowed, with no `tif` read as `gtc`, and one in a pair quoted in a
/// stablecoin, or in a symbol that names no quote, is denied `crypto_pair_not_usd`. A crypto exit
/// is never judged by its pair (`AGENTS.md` rule 13): RC-09's own risk exit is allowed in either.
#[test]
fn a_crypto_proposal_is_judged_by_its_symbol_s_quote() -> Result<(), String> {
    let not_usd = || json!({ "verdict": "deny", "reason_code": "crypto_pair_not_usd" });
    expect_eq(
        "BTC/USD",
        run_edited(
            "RC-09",
            rc_09_crypto_opening(Some("BTC/USD"), "0.001", json!({ "verdict": "allow" })),
        ),
        Ok(()),
    )?;
    for symbol in [Some("BTC/USDT"), None] {
        expect_eq(
            &format!("{symbol:?}"),
            run_edited("RC-09", rc_09_crypto_opening(symbol, "0.001", not_usd())),
            Ok(()),
        )?;
        expect_eq(
            &format!("RC-09's exit in {symbol:?}"),
            run_edited(
                "RC-09",
                Box::new(move |c| {
                    let listed = object_at(c, &["instruments", "BTCUSD"])?;
                    match symbol {
                        Some(symbol) => listed.insert("symbol".to_owned(), json!(symbol)),
                        None => listed.remove("symbol"),
                    };
                    Ok(())
                }),
            ),
            Ok(()),
        )?;
    }
    Ok(())
}

/// A crypto pair's minimum order is its `min_trade_increment` (DEC-285): an opening below it stops
/// at §5.3 rule 2's unregistered code, and one at it is decided.
#[test]
fn a_crypto_pair_s_minimum_order_is_its_increment() -> Result<(), String> {
    let with_increment = |qty: &'static str, decision: Json| -> Edit {
        Box::new(move |c| {
            put(
                c,
                &["instruments", "BTCUSD"],
                "min_trade_increment",
                json!("0.01"),
            )?;
            rc_09_crypto_opening(Some("BTC/USD"), qty, decision)(c)
        })
    };
    expect_eq(
        "below the increment",
        run_edited("RC-09", with_increment("0.001", json!({ "verdict": "allow" }))),
        Err(
            "step 3: `mandate_risk::evaluate`: the reason code of §5.3 rule 2's minimum size is not implemented yet (pending DEC-129 item 27) (unimplemented)"
                .to_owned(),
        ),
    )?;
    expect_eq(
        "at the increment",
        run_edited(
            "RC-09",
            with_increment("0.01", json!({ "verdict": "allow" })),
        ),
        Ok(()),
    )
}

/// An instrument states only its own class's members (DEC-85, DEC-285): an equity's `symbol` or
/// `min_trade_increment`, or a pair's `exchange` or `fractionable`, fails the case naming it.
#[test]
fn an_instrument_states_only_its_class_s_members() -> Result<(), String> {
    let stating = |instrument: &'static str, key: &'static str, value: Json| -> Edit {
        Box::new(move |c| put(c, &["instruments", instrument], key, value))
    };
    let rows: [(&str, &str, &str, Json); 4] = [
        ("RC-09B", "AAPL", "symbol", json!("AAPL/USD")),
        ("RC-09B", "AAPL", "min_trade_increment", json!("0.01")),
        ("RC-09", "BTCUSD", "exchange", json!("NASDAQ")),
        ("RC-09", "BTCUSD", "fractionable", json!(true)),
    ];
    for (case, instrument, key, value) in rows {
        expect_eq(
            &format!("{case}: `{instrument}` stating `{key}`"),
            run_edited(case, stating(instrument, key, value)),
            Err(format!("instrument `{instrument}`: unknown key `{key}`")),
        )?;
    }
    Ok(())
}

/// RC-09's crypto fill reaches the accounting as crypto, as it reaches the gate's day-trade fold
/// (§6.4, typed by hand): 0.01 BTC bought at 60,000.00 as a taker pays 25 bps, 0.000025 BTC, from
/// the asset received, so 0.009975 BTC is held, $600.00 leaves settled cash at once, and no cash
/// fee accrues. The fill the fold reads is crypto too, so it never counts as a day trade.
#[test]
fn rc_09_s_crypto_fill_is_crypto_to_the_accounting_and_the_gate() -> Result<(), String> {
    let (fixture, mut case) = case_named("RC-09")?;
    object_at(&mut case, &[])?.remove("variants");
    let config = config(&fixture, &case)?;
    let instruments = instruments(&case)?;
    let mut account = initial(
        &case,
        BrokerProfile::parse(str_at(&case, "broker_profile")?)?,
        &instruments,
    )?;
    let mut gate = Gate::read(&fixture, &case)?;
    let steps = list_at(&case, "steps")?;
    for n in 0..2 {
        let step = steps.get(n).ok_or("RC-09 has three steps")?;
        run_step(&mut account, &mut gate, step, n, &instruments, &config)?;
    }
    let (_, btc, _, _) = instruments
        .iter()
        .find(|(name, _, _, _)| name == "BTCUSD")
        .ok_or("RC-09 lists BTCUSD")?;
    let num = |text: &str| Qty::parse(text).map_err(|e| e.to_string());
    expect_eq(
        "BTCUSD held, net of the fee in kind",
        account.position(btc).qty().abs(),
        num("0.009975")?,
    )?;
    expect_eq(
        "settled cash",
        account.settled(),
        Usd::parse("9400").map_err(|e| e.to_string())?,
    )?;
    expect_eq(
        "cash fees accrued",
        account.fees_accrued().map_err(|e| e.to_string())?,
        Usd::ZERO,
    )?;
    expect_eq(
        "the fold's view of the fill",
        gate.fills
            .iter()
            .map(|f| (f.instrument.as_str().to_owned(), f.asset_class, f.qty))
            .collect::<Vec<_>>(),
        vec![("BTCUSD".to_owned(), AssetClass::Crypto, num("0.01")?)],
    )
}
