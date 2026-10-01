//! RC-15's restricted arm, which its own case cannot reach in this harness: the closing-only
//! restriction arrives in step 2 as a `broker_order_update` reject, and that event is E7-2's. The
//! test runs the founder's steps 3 and 4 unchanged, with the account in the state §7.3's second row
//! gives that reject (`closing_only`), and with AAPL marked at step 4's own limit price, since the
//! case holds AAPL with no mark and the harness refuses to value an unmarked position it is not
//! deciding on (DEC-199). The same steps under an active account show the state is what decides.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use mandate_num::{Price, Qty, Usd};
use mandate_risk::{
    AccountState, AssetClass, AssetId, EtpClass, Exchange, InstrumentSnapshot, MarketSnapshot,
    QuoteCurrency, RestingSide, SaneQuote,
};
use mandate_time::UtcNanos;
use serde_json::{Map, json};

use super::{Gate, listing, market};
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
        halted,
        status_feed_current,
    } = listing(msft.clone(), Exchange::Nyse, true, limit, at)?;
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

/// Case `id` with `edit` applied, run as `tests/refcases.rs` runs it.
fn run_edited(id: &str, edit: Edit) -> Result<(), String> {
    let (fixture, mut case) = case_named(id)?;
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
    let waiting = "initial account `crypto_status` not interpreted until E6-10";
    expect_eq(
        "RC-09B",
        run_edited("RC-09B", account("crypto_status", json!("ACTIVE"))),
        Err(format!("{LATER_PROPOSAL_WAITS}; {waiting}")),
    )?;
    expect_eq(
        "RC-09B without the MSFT opening",
        run_edited(
            "RC-09B",
            Box::new(|c| {
                put(c, &["initial", "account"], "crypto_status", json!("ACTIVE"))?;
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
