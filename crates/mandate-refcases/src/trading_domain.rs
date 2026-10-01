//! The `trading_domain` suite: `fixtures/refcases/trading-domain.json`, the JSON form of
//! [the trading-domain reference cases](../../../docs/specs/reference-cases/trading-domain.yaml)
//! (spec §14). One named test per case (`trading_domain::RC-01`) and per variant
//! (`trading_domain::RC-03::gate_rejects_zero_crossing_order`).
//!
//! E3-1 interprets `fill`, `mark`, `fees_charged`, and `advance_clock` (settlement) steps and the
//! accounting expectation keys; E3-2 adds `corporate_action_applied` (splits and cash dividends),
//! `broker_cash_posting` (cash in lieu), dividend payment on `advance_clock`, and the `receivables`
//! and `income` expectations (DEC-96); E3-3 adds the account type (`initial.account.type`) and the
//! `buying_power` expectation, the fold's model buying power with no reservations (DEC-105); E4-1
//! adds the backtest cases — `bars`, `orders`, `isolation`, and the `fills` and `canceled_legs`
//! expectations, run through `mandate-sim` (DEC-106 item 11); E6-9 adds `propose_order` steps and
//! the `decision` expectation, decided by `mandate_risk::evaluate`, and the account's §7.3 status
//! fields from `initial.account` and `broker_account_update` steps ([`gate`], DEC-199); E6-6 adds
//! the day-trade regime and its figures from `initial.account` and the `day_trade_count`
//! expectation (DEC-284), and decides a later proposal once every earlier allowed one was filled
//! in full (DEC-259 item 7); E6-10 decides crypto proposals, reading a pair's quote currency from
//! its `symbol` (DEC-285). A case that uses anything owned by a later story fails with "not
//! interpreted until <story>" for each such item; anything the vocabulary does not know fails as
//! unknown. Every key of every interpreted expectation is checked.

use core::fmt::Display;
use std::collections::BTreeSet;
use std::sync::Arc;

use mandate_accounting::{
    Account, AccountType, AccountingError, AssetClass, CashDividend, Config, CorporateAction,
    CryptoFees, EquityFees, Execution, FeeFamily, Input, InstrumentId, Liquidity, Position, Record,
    Reservations, Side, Split, TafCapBasis,
};
use mandate_canon::DecStr;
use mandate_num::{
    Bps, CostBasis, FeeCap, FeePerShare, FeeRate, Fraction, Price, Qty, Rounding, ShareIncrement,
    SignedQty, SplitRatio, Usd,
};
use mandate_risk::{AccountFill, AssetId, Decision};
use mandate_sim::{
    Eligibility, FirstBarVolumes, Instrument, Nanos, OcoLeg, OrderKind, OrderRef, Session, SimBar,
    SimConfig, SimError, SimFill, SimOrder, SimOutcome, Slippage, TimeInForce, simulate,
};
use mandate_time::{Date, TradingCalendar, UtcNanos, new_york_date_and_hour, new_york_midnight};
use serde_json::{Map, json};

use crate::{Case, Json, at, ensure, expect_eq, list_at, str_at, u64_at};

mod gate;

const SUITE: &str = "trading_domain";
/// The broker profile of a backtest case (spec §6.4): no broker, only bars.
const BACKTEST_PROFILE: &str = "backtest";
const SCHEMA_VERSION: u64 = 3;
const CALENDAR_RULE: &str = "weekdays not listed as holidays are trading days";
const MAX_EXTENDS: usize = 8;
/// Average cost is compared as round(B ÷ Q, 12, half_even).
const AVG_COST_SCALE: u32 = 12;

/// Step events owned by later stories.
const PENDING_EVENTS: &[(&str, &str)] = &[
    ("broker_order_update", "E7-2"),
    ("corporate_action_prepare", "E7-4"),
    ("reconciliation", "E7-3"),
    ("broker_position_update", "E7-3"),
    ("deploy_agent", "E7-5"),
    ("owner_ack", "E7-5"),
    ("kill_switch", "E6-5"),
    ("conduct_breach", "E6-8"),
];

/// Expectation keys owned by later stories. `agent_mode` is owned by whatever moves the mode after
/// the step's event ([`MODE_OWNERS`]).
const PENDING_EXPECT: &[(&str, &str)] = &[
    ("orders", "E7-2"),
    ("actions", "E7-4"),
    ("reconciliation", "E7-3"),
    ("protective_sell_qty", "E7-4"),
];

/// Who moves the agents' mode after each kind of step (DEC-199): the executor's restriction and
/// reconciliation handling for broker events (its RC-15 driver is pending E7-3), the account
/// ledger for external activity (E7-5), the kill switch (E6-5), and the conduct-breach transition
/// (E6-11). A broker update carrying a fill for an order the case never named is external.
const MODE_OWNERS: &[(&str, &str)] = &[
    ("broker_order_update", "E7-3"),
    ("broker_account_update", "E7-3"),
    ("broker_position_update", "E7-3"),
    ("reconciliation", "E7-3"),
    ("fill", "E7-5"),
    ("kill_switch", "E6-5"),
    ("conduct_breach", "E6-11"),
];

/// Who holds the agents' mode after an event [`MODE_OWNERS`] does not list, where nothing moves it:
/// the account ledger, which owns the agents in this harness (`initial.agents`, `deploy_agent`, and
/// a proposal's `agent`), so an `agent_mode` expectation there is its to compare.
const MODE_HOLDER: &str = "E7-5";

/// What a proposal's submission needs before a later proposal in the same case can be decided
/// against a working order: the order, its reservation, and its conduct figures, which `actions`
/// (E7-4) and the account ledger (E7-5) own. Until then a later proposal is decided only once every
/// earlier allowed proposal was filled in full on its side by the case's `fill` steps (DEC-199 item
/// 3 as narrowed by DEC-259 item 7), and is refused with this text otherwise, rather than run as if
/// the earlier order left no trace. It reads as a pending item whether a run or the case's shape
/// finds it.
pub(crate) const LATER_PROPOSAL_WAITS: &str =
    "a second `propose_order` step in one case not interpreted until E7-4 and E7-5";

/// `propose_order` members that later stories' arms read: another agent (the account ledger),
/// brackets and the exit ladder's pricing and status inputs (the exit sequences), and an owner's
/// confirmed bid (the owner-exit pacing, RC-25's arm).
const PENDING_PROPOSAL: &[(&str, &str)] = &[
    ("agent", "E7-5"),
    ("entry", "E7-4"),
    ("stop_loss", "E7-4"),
    ("take_profit", "E7-4"),
    ("pricing", "E7-4"),
    ("status_feed", "E7-4"),
    ("last_good_quote", "E7-4"),
    ("owner_confirmed_bid", "E6-8"),
];

const PENDING_INITIAL: &[(&str, &str)] = &[("open_orders", "E7-4"), ("agents", "E7-5")];

/// `initial.account` members a later story reads. The day-trade regime and its figures are read
/// from the initial account ([`gate`], DEC-284).
const PENDING_INITIAL_ACCOUNT: &[(&str, &str)] = &[("crypto_status", "E6-10")];

/// `broker_account_update` members a later story reads: a change to the regime or its figures in
/// the middle of a case is not read (DEC-284), and neither is the crypto status.
const PENDING_ACCOUNT_UPDATE: &[(&str, &str)] = &[
    ("regime", "E6-6"),
    ("prior_day_trades", "E6-6"),
    ("last_equity", "E6-6"),
    ("multiplier", "E6-6"),
    ("crypto_status", "E6-10"),
];

/// Instrument fields read only by the eligibility floor.
const PENDING_INSTRUMENT: &[(&str, &str)] = &[
    ("prior_close", "E6-7"),
    ("median_dollar_volume_20d", "E6-7"),
    ("leveraged_or_inverse", "E6-7"),
    ("ipo", "E6-7"),
];

pub fn cases(fixture: &Arc<Json>) -> Vec<Case> {
    let f = Arc::clone(fixture);
    let mut out = vec![Case::new(format!("{SUITE}::schema_version"), move || {
        expect_eq(
            "schema_version",
            u64_at(&f, "schema_version")?,
            SCHEMA_VERSION,
        )
    })];
    let listed = match list_at(fixture, "cases") {
        Ok(listed) => listed,
        Err(e) => {
            out.push(Case::new(format!("{SUITE}::cases"), move || Err(e)));
            return out;
        }
    };
    for (index, case) in listed.iter().enumerate() {
        let id = case
            .get("id")
            .and_then(Json::as_str)
            .map_or_else(|| format!("case_{index}"), str::to_owned);
        let f = Arc::clone(fixture);
        out.push(Case::new(format!("{SUITE}::{id}"), move || {
            run_listed(&f, index, None)
        }));
        let variants = case.get("variants").and_then(Json::as_array);
        for (v, variant) in variants.into_iter().flatten().enumerate() {
            let name = variant
                .get("name")
                .and_then(Json::as_str)
                .map_or_else(|| format!("variant_{v}"), str::to_owned);
            let f = Arc::clone(fixture);
            out.push(Case::new(format!("{SUITE}::{id}::{name}"), move || {
                run_listed(&f, index, Some(v))
            }));
        }
    }
    out
}

fn run_listed(fixture: &Json, index: usize, variant: Option<usize>) -> Result<(), String> {
    let case = list_at(fixture, "cases")?
        .get(index)
        .ok_or("case index out of range")?;
    let mut merged = case.clone();
    if let Some(members) = merged.as_object_mut() {
        members.remove("variants");
    }
    if let Some(v) = variant {
        let variant = list_at(case, "variants")?
            .get(v)
            .ok_or("variant index out of range")?;
        apply_variant(&mut merged, variant)?;
    }
    run_case(fixture, &merged)
}

/// Merges per the fixture rules: objects recursively; arrays of objects that all have `id` or
/// `name` by that key; anything else is replaced.
fn merge(base: &mut Json, over: &Json) {
    match (base, over) {
        (Json::Object(b), Json::Object(o)) => {
            for (key, value) in o {
                match b.get_mut(key) {
                    Some(slot) => merge(slot, value),
                    None => {
                        b.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (Json::Array(b), Json::Array(o)) if keyed(b) && keyed(o) => {
            for item in o {
                match b
                    .iter_mut()
                    .find(|existing| key_of(existing) == key_of(item))
                {
                    Some(existing) => merge(existing, item),
                    None => b.push(item.clone()),
                }
            }
        }
        (slot, value) => *slot = value.clone(),
    }
}

fn key_of(item: &Json) -> Option<&str> {
    item.get("id")
        .or_else(|| item.get("name"))
        .and_then(Json::as_str)
}

fn keyed(items: &[Json]) -> bool {
    !items.is_empty() && items.iter().all(|item| key_of(item).is_some())
}

fn apply_variant(case: &mut Json, variant: &Json) -> Result<(), String> {
    let members = fields(
        variant,
        "variant",
        &["name", "overrides", "expect_overrides"],
    )?;
    if let Some(overrides) = members.get("overrides") {
        let mut rest = overrides
            .as_object()
            .ok_or("variant `overrides` is not an object")?
            .clone();
        if let Some(steps) = rest.remove("steps")
            && let Some(slot) = case.as_object_mut()
        {
            slot.insert("steps".to_owned(), steps);
        }
        merge(case, &Json::Object(rest));
    }
    if let Some(expect) = members.get("expect_overrides") {
        let expect = expect
            .as_object()
            .ok_or("variant `expect_overrides` is not an object")?;
        for (key, value) in expect {
            let n: usize = key
                .strip_prefix("step_")
                .and_then(|n| n.parse().ok())
                .ok_or_else(|| format!("`expect_overrides.{key}` is not step_N"))?;
            let step = case
                .get_mut("steps")
                .and_then(Json::as_array_mut)
                .and_then(|steps| steps.get_mut(n.checked_sub(1)?))
                .and_then(Json::as_object_mut)
                .ok_or_else(|| format!("`expect_overrides.{key}` names no step"))?;
            let slot = step
                .entry("expect")
                .or_insert_with(|| Json::Object(Map::new()));
            merge(slot, value);
        }
    }
    Ok(())
}

/// The object at `value`, whose keys must all be in `allowed`.
fn fields<'a>(
    value: &'a Json,
    what: &str,
    allowed: &[&str],
) -> Result<&'a Map<String, Json>, String> {
    let members = value
        .as_object()
        .ok_or_else(|| format!("{what} is not an object"))?;
    match members.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(unknown) => Err(format!("{what}: unknown key `{unknown}`")),
        None => Ok(members),
    }
}

fn owner(table: &[(&str, &'static str)], key: &str) -> Option<&'static str> {
    table
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, story)| *story)
}

/// Everything in the case that a later story interprets, as "`what` … not interpreted until
/// <story>".
fn pending(case: &Json) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut note = |what: String, story: &str| {
        out.insert(format!("{what} not interpreted until {story}"));
    };
    let empty = Map::new();
    let instruments = case.get("instruments").and_then(Json::as_object);
    for instrument in instruments.into_iter().flat_map(Map::values) {
        for key in instrument.as_object().unwrap_or(&empty).keys() {
            if let Some(story) = owner(PENDING_INSTRUMENT, key) {
                note(format!("instrument field `{key}`"), story);
            }
        }
    }
    let initial = case
        .get("initial")
        .and_then(Json::as_object)
        .unwrap_or(&empty);
    for key in initial.keys() {
        if let Some(story) = owner(PENDING_INITIAL, key) {
            note(format!("initial `{key}`"), story);
        }
    }
    let account = initial
        .get("account")
        .and_then(Json::as_object)
        .unwrap_or(&empty);
    for key in account.keys() {
        if let Some(story) = owner(PENDING_INITIAL_ACCOUNT, key) {
            note(format!("initial account `{key}`"), story);
        }
    }
    let steps = case.get("steps").and_then(Json::as_array);
    let unfilled = steps.is_some_and(|steps| a_proposal_follows_one_not_filled_in_full(steps));
    for step in steps.into_iter().flatten() {
        let event = step.get("event").and_then(Json::as_str).unwrap_or("");
        if let Some(story) = owner(PENDING_EVENTS, event) {
            note(format!("`{event}` steps"), story);
        }
        let data = step.get("data");
        if event == "fill"
            && data.and_then(|d| d.get("source")).and_then(Json::as_str) == Some("external")
        {
            note("external fills (`source: external`)".to_owned(), "E7-5");
        }
        if event == "advance_clock" && data.and_then(|d| d.get("quote")).is_some() {
            note("quotes on `advance_clock`".to_owned(), "E7-4");
        }
        let expect = step
            .get("expect")
            .and_then(Json::as_object)
            .unwrap_or(&empty);
        for key in expect.keys() {
            if let Some(story) = owner(PENDING_EXPECT, key) {
                note(format!("expectation `{key}`"), story);
            }
        }
        let external = event == "broker_order_update"
            && data.is_some_and(|d| d.get("fill").is_some() && d.get("name").is_none());
        let mode_owner = if external {
            Some("E7-5")
        } else {
            owner(MODE_OWNERS, event)
        };
        if let Some(story) = mode_owner.filter(|_| expect.contains_key("agent_mode")) {
            note(format!("expectation `agent_mode` after `{event}`"), story);
        }
        if event == "propose_order" {
            pending_proposal(data, expect, &mut note);
        }
        if event == "broker_account_update" {
            let members = data.and_then(Json::as_object).unwrap_or(&empty);
            for key in members.keys() {
                if let Some(story) = owner(PENDING_ACCOUNT_UPDATE, key) {
                    note(format!("account update field `{key}`"), story);
                }
            }
        }
    }
    if unfilled && !out.is_empty() {
        out.insert(LATER_PROPOSAL_WAITS.to_owned());
    }
    out
}

/// Whether some proposal follows an earlier one that the `fill` steps between them, on its
/// instrument and side, do not fill in full. Whether that later proposal can be decided turns on
/// the earlier one's verdict (DEC-259 item 7: a refused proposal leaves no trace), which only a run
/// gives, so [`pending`] lists it only while another item keeps the case from running, and the run
/// refuses it otherwise ([`gate::Gate::decide`]).
fn a_proposal_follows_one_not_filled_in_full(steps: &[Json]) -> bool {
    let mut open: Option<(&Json, Option<Qty>)> = None;
    for step in steps {
        let data = step.get("data");
        let field = |key: &str| data.and_then(|d| d.get(key));
        let qty = |key: &str| {
            field(key)
                .and_then(Json::as_str)
                .and_then(|text| Qty::parse(text).ok())
        };
        match step.get("event").and_then(Json::as_str) {
            Some("propose_order") => {
                if open.is_some_and(|(_, left)| left != Some(Qty::ZERO)) {
                    return true;
                }
                open = Some((step, qty("qty")));
            }
            Some("fill") => {
                if let Some((proposal, left)) = open.as_mut() {
                    let on_its_side = ["instrument", "side"]
                        .iter()
                        .all(|key| proposal.get("data").and_then(|d| d.get(*key)) == field(key));
                    if on_its_side {
                        *left = left
                            .zip(qty("qty_gross"))
                            .and_then(|(left, filled)| left.checked_sub(filled).ok());
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// What a `propose_order` step needs beyond [`gate`]: a later story's member, and the
/// `buying_power` after the proposal, which counts the submitted order's reservation (the account
/// ledger, E7-5).
fn pending_proposal(
    data: Option<&Json>,
    expect: &Map<String, Json>,
    note: &mut impl FnMut(String, &str),
) {
    let empty = Map::new();
    let data = data.and_then(Json::as_object).unwrap_or(&empty);
    for key in data.keys() {
        if let Some(story) = owner(PENDING_PROPOSAL, key) {
            note(format!("proposal field `{key}`"), story);
        }
    }
    if expect.contains_key("buying_power") {
        note(
            "expectation `buying_power` after `propose_order`".to_owned(),
            "E7-5",
        );
    }
}

fn run_case(fixture: &Json, case: &Json) -> Result<(), String> {
    let pending = pending(case);
    if !pending.is_empty() {
        return Err(pending.into_iter().collect::<Vec<_>>().join("; "));
    }
    if str_at(case, "broker_profile")? == BACKTEST_PROFILE {
        return backtest::run(fixture, case);
    }
    fields(
        case,
        "case",
        &[
            "id",
            "title",
            "scope",
            "broker_profile",
            "config",
            "config_overrides",
            "calendar",
            "instruments",
            "initial",
            "steps",
        ],
    )?;
    let profile = BrokerProfile::parse(str_at(case, "broker_profile")?)?;
    let config = config(fixture, case)?;
    let instruments = instruments(case)?;
    let mut account = initial(case, profile, &instruments)?;
    let mut gate = gate::Gate::read(fixture, case)?;
    let mut baseline = Baseline::of(&account);
    for (n, step) in list_at(case, "steps")?.iter().enumerate() {
        let label = format!("step {}", n.saturating_add(1));
        let (record, decision) = run_step(&mut account, &mut gate, step, n, &instruments, &config)
            .map_err(|e| {
                if e == LATER_PROPOSAL_WAITS {
                    e
                } else {
                    format!("{label}: {e}")
                }
            })?;
        if let Some(expect) = step.get("expect") {
            let when = UtcNanos::parse_rfc3339(str_at(step, "at")?)
                .map_err(|e| format!("{label}: `at`: {e}"))?;
            check(
                &account,
                (record.as_ref(), decision.as_ref()),
                (&gate, &config, when),
                baseline.as_ref(),
                &instruments,
                expect,
            )
            .map_err(|e| format!("{label}: {e}"))?;
        }
        baseline = baseline.or_else(|| Baseline::of(&account));
    }
    Ok(())
}

fn dec(value: &Json, what: &str) -> Result<DecStr, String> {
    let text = value
        .as_str()
        .ok_or_else(|| format!("`{what}` is not a decimal string"))?;
    DecStr::parse(text).map_err(|e| format!("`{what}`: {e}"))
}

fn dec_at(value: &Json, key: &str) -> Result<DecStr, String> {
    dec(at(value, key)?, key)
}

fn date(value: &Json, what: &str) -> Result<Date, String> {
    let text = value
        .as_str()
        .ok_or_else(|| format!("`{what}` is not a date string"))?;
    Date::parse(text).map_err(|e| format!("`{what}`: {e}"))
}

fn dates(value: &Json, what: &str) -> Result<Vec<Date>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("`{what}` is not a list"))?
        .iter()
        .map(|d| date(d, what))
        .collect()
}

fn num<T>(parsed: Result<T, mandate_num::NumError>, what: &str) -> Result<T, String> {
    parsed.map_err(|e| format!("`{what}`: {e}"))
}

fn acct<T>(result: Result<T, AccountingError>) -> Result<T, String> {
    result.map_err(|e| format!("{e} ({})", e.code()))
}

/// The case's configuration: the named config with its `extends` chain, then `config_overrides`.
fn resolved_config(fixture: &Json, case: &Json) -> Result<Json, String> {
    let mut chain = Vec::new();
    let mut name = str_at(case, "config")?.to_owned();
    loop {
        ensure(chain.len() < MAX_EXTENDS, || {
            format!("config `{name}` extends too deeply")
        })?;
        let config = at(fixture, "configs")?
            .get(&name)
            .ok_or_else(|| format!("no config `{name}`"))?
            .clone();
        let parent = config
            .get("extends")
            .and_then(Json::as_str)
            .map(str::to_owned);
        chain.push(config);
        match parent {
            Some(parent) => name = parent,
            None => break,
        }
    }
    let mut resolved = Json::Object(Map::new());
    for config in chain.iter().rev() {
        merge(&mut resolved, config);
    }
    if let Some(members) = resolved.as_object_mut() {
        members.remove("extends");
    }
    if let Some(overrides) = case.get("config_overrides") {
        merge(&mut resolved, overrides);
    }
    Ok(resolved)
}

fn rounding(scale: u64, mode: &str) -> Json {
    json!({ "scale": scale, "mode": mode })
}

/// The fold implements the spec's fixed rules; a config that states another rule fails.
fn fixed(value: &Json, key: &str, expected: &Json) -> Result<(), String> {
    ensure(at(value, key)? == expected, || {
        format!(
            "config `{key}` is {}, but the spec fixes {expected}",
            at(value, key).unwrap_or(&Json::Null)
        )
    })
}

fn config(fixture: &Json, case: &Json) -> Result<Config, String> {
    let resolved = resolved_config(fixture, case)?;
    fields(
        &resolved,
        "config",
        &["fees", "backtest", "gate", "accounting"],
    )?;
    let accounting = at(&resolved, "accounting")?;
    fields(
        accounting,
        "config accounting",
        &["basis_reduction", "mark_adjustment"],
    )?;
    fixed(accounting, "basis_reduction", &rounding(12, "half_even"))?;
    fixed(accounting, "mark_adjustment", &rounding(12, "half_even"))?;
    let fees = at(&resolved, "fees")?;
    fields(
        fees,
        "config fees",
        &["effective_from", "equities", "crypto"],
    )?;
    date(at(fees, "effective_from")?, "effective_from")?;
    let eq = at(fees, "equities")?;
    fields(
        eq,
        "config fees.equities",
        &[
            "sec_rate",
            "taf_per_share",
            "taf_cap",
            "taf_cap_basis",
            "cat_per_share",
            "charge",
        ],
    )?;
    fixed(
        eq,
        "charge",
        &json!({ "time": "20:00 America/New_York", "rounding": rounding(2, "ceiling") }),
    )?;
    let taf_cap_basis = match str_at(eq, "taf_cap_basis")? {
        "per_execution" => TafCapBasis::PerExecution,
        "per_order" => TafCapBasis::PerOrder,
        other => return Err(format!("unknown taf_cap_basis `{other}`")),
    };
    let crypto = at(fees, "crypto")?;
    fields(
        crypto,
        "config fees.crypto",
        &[
            "maker_bps",
            "taker_bps",
            "asset_fee_rounding",
            "usd_fee_rounding",
            "charge",
        ],
    )?;
    fixed(crypto, "asset_fee_rounding", &rounding(9, "half_up"))?;
    fixed(crypto, "usd_fee_rounding", &rounding(2, "half_up"))?;
    fixed(crypto, "charge", &json!({ "time": "00:00 UTC" }))?;
    Ok(Config {
        equities: EquityFees {
            sec_rate: num(FeeRate::parse(dec_at(eq, "sec_rate")?.as_str()), "sec_rate")?,
            taf_per_share: num(
                FeePerShare::parse(dec_at(eq, "taf_per_share")?.as_str()),
                "taf_per_share",
            )?,
            taf_cap: num(FeeCap::parse(dec_at(eq, "taf_cap")?.as_str()), "taf_cap")?,
            taf_cap_basis,
            cat_per_share: num(
                FeePerShare::parse(dec_at(eq, "cat_per_share")?.as_str()),
                "cat_per_share",
            )?,
        },
        crypto: CryptoFees {
            maker: num(
                Bps::parse(dec_at(crypto, "maker_bps")?.as_str()),
                "maker_bps",
            )?,
            taker: num(
                Bps::parse(dec_at(crypto, "taker_bps")?.as_str()),
                "taker_bps",
            )?,
        },
        calendar: calendar(fixture, str_at(case, "calendar")?)?,
    })
}

fn calendar(fixture: &Json, name: &str) -> Result<TradingCalendar, String> {
    let c = at(fixture, "calendars")?
        .get(name)
        .ok_or_else(|| format!("no calendar `{name}`"))?;
    fields(
        c,
        "calendar",
        &[
            "valid_from",
            "valid_to",
            "rule",
            "trading_holidays",
            "early_closes",
            "settlement_holidays",
        ],
    )?;
    expect_eq("calendar rule", str_at(c, "rule")?, CALENDAR_RULE)?;
    dates(at(c, "early_closes")?, "early_closes")?;
    TradingCalendar::new(
        date(at(c, "valid_from")?, "valid_from")?,
        date(at(c, "valid_to")?, "valid_to")?,
        dates(at(c, "trading_holidays")?, "trading_holidays")?,
        dates(at(c, "settlement_holidays")?, "settlement_holidays")?,
    )
    .map_err(|e| format!("calendar `{name}`: {e}"))
}

/// Name, ID, asset class, and `fractionable` when the snapshot gives it.
type Instruments = Vec<(String, InstrumentId, AssetClass, Option<bool>)>;

fn instruments(case: &Json) -> Result<Instruments, String> {
    let listed = at(case, "instruments")?
        .as_object()
        .ok_or("`instruments` is not an object")?;
    listed
        .iter()
        .map(|(name, fields_of)| {
            let what = format!("instrument `{name}`");
            fields(
                fields_of,
                &what,
                &[
                    "asset_class",
                    "exchange",
                    "fractionable",
                    "symbol",
                    "min_trade_increment",
                ],
            )?;
            let class = match str_at(fields_of, "asset_class")? {
                "us_equity" => AssetClass::UsEquity,
                "crypto" => AssetClass::Crypto,
                other => return Err(format!("{what}: unknown asset_class `{other}`")),
            };
            let id = InstrumentId::new(name).map_err(|e| format!("{what}: {e}"))?;
            let fractionable = match fields_of.get("fractionable") {
                None => None,
                Some(f) => Some(
                    f.as_bool()
                        .ok_or_else(|| format!("{what}: `fractionable` is not a boolean"))?,
                ),
            };
            Ok((name.clone(), id, class, fractionable))
        })
        .collect()
}

fn instrument<'a>(
    instruments: &'a Instruments,
    name: &str,
) -> Result<&'a (String, InstrumentId, AssetClass, Option<bool>), String> {
    instruments
        .iter()
        .find(|(n, _, _, _)| n == name)
        .ok_or_else(|| format!("unknown instrument `{name}`"))
}

/// The broker profiles the harness knows (spec §7.2). Parsed once, before any account is read, so
/// that an unknown profile is reported as the profile it is and a stated `cash` account is never
/// reported as an unknown account type.
#[derive(Debug, Clone, Copy)]
enum BrokerProfile {
    Alpaca,
    Generic,
}

impl BrokerProfile {
    fn parse(profile: &str) -> Result<Self, String> {
        match profile {
            "alpaca" => Ok(Self::Alpaca),
            "generic" => Ok(Self::Generic),
            other => Err(format!(
                "unknown broker_profile `{other}`: the harness knows `alpaca` (margin accounts only) and `generic` (`cash` or `margin`)"
            )),
        }
    }
}

/// The account type (spec §7.2): the alpaca profile defaults to margin and has no cash accounts;
/// a generic broker's case must say which it is (DEC-105).
fn account_type(account: &Json, profile: BrokerProfile) -> Result<AccountType, String> {
    match (profile, account.get("type").map(Json::as_str)) {
        (BrokerProfile::Alpaca, None) | (_, Some(Some("margin"))) => Ok(AccountType::Margin),
        (BrokerProfile::Generic, Some(Some("cash"))) => Ok(AccountType::Cash),
        (BrokerProfile::Alpaca, Some(Some("cash"))) => {
            Err("an alpaca account is never a cash account".into())
        }
        (BrokerProfile::Generic, None) => {
            Err("a generic account needs `initial.account.type`".into())
        }
        (_, Some(Some(other))) => Err(format!("unknown account type `{other}`")),
        (_, Some(None)) => Err("`initial.account.type` is not a string".into()),
    }
}

fn initial(
    case: &Json,
    profile: BrokerProfile,
    instruments: &Instruments,
) -> Result<Account, String> {
    let initial = at(case, "initial")?;
    fields(initial, "initial", &["account", "positions"])?;
    let account = at(initial, "account")?;
    let mut account_keys = vec!["cash", "type"];
    account_keys.extend_from_slice(gate::STATUS_FIELDS);
    account_keys.extend_from_slice(gate::DAY_TRADE_FIELDS);
    fields(account, "initial account", &account_keys)?;
    let account_type = account_type(account, profile)?;
    let cash = at(account, "cash")?;
    fields(cash, "initial cash", &["settled"])?;
    let settled = num(Usd::parse(dec_at(cash, "settled")?.as_str()), "settled")?;
    let mut positions = Vec::new();
    for p in initial
        .get("positions")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        fields(p, "initial position", &["instrument", "qty", "cost_basis"])?;
        let (_, id, _, _) = instrument(instruments, str_at(p, "instrument")?)?;
        let qty = num(SignedQty::parse(dec_at(p, "qty")?.as_str()), "qty")?;
        let basis = num(
            CostBasis::parse(dec_at(p, "cost_basis")?.as_str()),
            "cost_basis",
        )?;
        positions.push((id.clone(), acct(Position::new(qty, basis))?));
    }
    Ok(Account::opening(account_type, settled, positions))
}

/// What one step did: the accounting record of its last input, and the gate's decision on a
/// `propose_order` step.
type StepOutcome = (Option<Record>, Option<Decision>);

fn run_step(
    account: &mut Account,
    gate: &mut gate::Gate,
    step: &Json,
    n: usize,
    instruments: &Instruments,
    config: &Config,
) -> Result<StepOutcome, String> {
    fields(step, "step", &["at", "event", "data", "expect"])?;
    let when = UtcNanos::parse_rfc3339(str_at(step, "at")?).map_err(|e| format!("`at`: {e}"))?;
    let empty = Json::Object(Map::new());
    let data = step.get("data").unwrap_or(&empty);
    let inputs = match str_at(step, "event")? {
        "propose_order" => {
            let decision = gate.decide(account, config, instruments, data, n, when)?;
            return Ok((None, Some(decision)));
        }
        "broker_account_update" => {
            gate.account_update(data)?;
            Vec::new()
        }
        "fill" => {
            let d = fields(
                data,
                "fill data",
                &[
                    "instrument",
                    "side",
                    "qty_gross",
                    "price",
                    "liquidity",
                    "client_order_id",
                    "source",
                ],
            )?;
            if let Some(source) = d.get("source") {
                return Err(format!("unknown fill source {source}"));
            }
            let (_, id, class, _) = instrument(instruments, str_at(data, "instrument")?)?;
            let side = match str_at(data, "side")? {
                "buy" => Side::Buy,
                "sell" => Side::Sell,
                other => return Err(format!("unknown side `{other}`")),
            };
            let liquidity = match d.get("liquidity").map(|l| l.as_str()) {
                None => None,
                Some(Some("maker")) => Some(Liquidity::Maker),
                Some(Some("taker")) => Some(Liquidity::Taker),
                Some(other) => return Err(format!("unknown liquidity {other:?}")),
            };
            let client_order_id = match d.get("client_order_id") {
                None => None,
                Some(c) => Some(
                    c.as_str()
                        .ok_or("`client_order_id` is not a string")?
                        .to_owned(),
                ),
            };
            let qty_gross = num(Qty::parse(dec_at(data, "qty_gross")?.as_str()), "qty_gross")?;
            gate.fill(AccountFill {
                at: when,
                instrument: AssetId::new(id.as_str()).map_err(|e| format!("`{id}`: {e}"))?,
                asset_class: *class,
                side,
                qty: qty_gross,
            })?;
            vec![Input::Fill(Execution {
                fill_id: format!("step_{n}"),
                client_order_id,
                instrument: id.clone(),
                asset_class: *class,
                side,
                qty_gross,
                price: num(Price::parse(dec_at(data, "price")?.as_str()), "price")?,
                liquidity,
                executed_at: when,
            })]
        }
        "mark" => {
            fields(data, "mark data", &["instrument", "price", "source"])?;
            let source = str_at(data, "source")?;
            ensure(
                ["quote", "trade", "official_close", "bar_close"].contains(&source),
                || format!("unknown mark source `{source}`"),
            )?;
            let (_, id, _, _) = instrument(instruments, str_at(data, "instrument")?)?;
            vec![Input::Mark {
                instrument: id.clone(),
                price: num(Price::parse(dec_at(data, "price")?.as_str()), "price")?,
            }]
        }
        "fees_charged" => {
            let (family, day_key) = match str_at(data, "family")? {
                "equities" => (FeeFamily::Equities, "trade_date"),
                "crypto" => (FeeFamily::Crypto, "day"),
                other => return Err(format!("unknown fee family `{other}`")),
            };
            fields(data, "fees_charged data", &["family", day_key])?;
            vec![Input::FeesCharged {
                family,
                day: date(at(data, day_key)?, day_key)?,
            }]
        }
        "advance_clock" => {
            fields(data, "advance_clock data", &["note"])?;
            acct(account.due(when))?
        }
        "corporate_action_applied" => {
            vec![Input::CorporateAction(corporate_action(data, instruments)?)]
        }
        "broker_cash_posting" => {
            fields(
                data,
                "broker_cash_posting data",
                &["type", "instrument", "amount"],
            )?;
            expect_eq(
                "broker_cash_posting type",
                str_at(data, "type")?,
                "cash_in_lieu",
            )?;
            let (_, id, _, _) = instrument(instruments, str_at(data, "instrument")?)?;
            vec![Input::CashInLieuPosted {
                instrument: id.clone(),
                amount: num(Usd::parse(dec_at(data, "amount")?.as_str()), "amount")?,
            }]
        }
        other => return Err(format!("unknown event `{other}`")),
    };
    let mut record = None;
    for input in inputs {
        let applied = acct(account.apply(&input, config))?;
        *account = applied.account;
        record = Some(applied.record);
    }
    Ok((record, None))
}

/// Values at the first state of the case where every one is defined (the start, or after the
/// first mark of a position opened without one), for the cumulative `conservation` expectation.
struct Baseline {
    equity: Usd,
    realized: Usd,
    unrealized: Usd,
    income: Usd,
    fees: Usd,
}

impl Baseline {
    fn of(account: &Account) -> Option<Self> {
        Some(Self {
            equity: account.equity().ok()?,
            realized: account.realized_gross(),
            unrealized: account.unrealized().ok()?,
            income: account.income(),
            fees: account.fees_total().ok()?,
        })
    }
}

/// A `corporate_action_applied` step: a split (to whole shares unless the instrument snapshot is
/// `fractionable`, which a split requires) or a cash dividend (spec §8.5).
fn corporate_action(data: &Json, instruments: &Instruments) -> Result<CorporateAction, String> {
    let (name, id, _, fractionable) = instrument(instruments, str_at(data, "instrument")?)?;
    let ex_date = date(at(data, "ex_date")?, "ex_date")?;
    match str_at(data, "type")? {
        "split" => {
            fields(
                data,
                "split data",
                &[
                    "instrument",
                    "type",
                    "ratio",
                    "ex_date",
                    "cash_in_lieu_price_per_new_share",
                ],
            )?;
            let ratio = at(data, "ratio")?;
            fields(ratio, "split ratio", &["new", "old"])?;
            let ratio = num(
                SplitRatio::new(u64_at(ratio, "new")?, u64_at(ratio, "old")?),
                "ratio",
            )?;
            let increment = match fractionable {
                Some(true) => ShareIncrement::Fractional,
                Some(false) => ShareIncrement::Whole,
                None => {
                    return Err(format!(
                        "a split needs `fractionable` on instrument `{name}`"
                    ));
                }
            };
            let cash_in_lieu_price = match data.get("cash_in_lieu_price_per_new_share") {
                None => None,
                Some(p) => Some(num(
                    Price::parse(dec(p, "cash_in_lieu_price_per_new_share")?.as_str()),
                    "cash_in_lieu_price_per_new_share",
                )?),
            };
            Ok(CorporateAction::Split(Split {
                instrument: id.clone(),
                ex_date,
                ratio,
                increment,
                cash_in_lieu_price,
            }))
        }
        "cash_dividend" => {
            fields(
                data,
                "cash_dividend data",
                &[
                    "instrument",
                    "type",
                    "amount_per_share",
                    "ex_date",
                    "pay_date",
                ],
            )?;
            let per_share = num(
                Price::parse(dec_at(data, "amount_per_share")?.as_str()),
                "amount_per_share",
            )?;
            let pay_date = date(at(data, "pay_date")?, "pay_date")?;
            Ok(CorporateAction::CashDividend(acct(CashDividend::new(
                id.clone(),
                ex_date,
                pay_date,
                per_share,
            ))?))
        }
        other => Err(format!("unknown corporate action type `{other}`")),
    }
}

fn check_dec(what: &str, actual: impl Display, expected: &Json) -> Result<(), String> {
    let expected = dec(expected, what)?;
    let actual = actual.to_string();
    ensure(actual == expected.as_str(), || {
        format!("{what}: expected {}, got {actual}", expected.as_str())
    })
}

fn delta(what: &str, now: Result<Usd, AccountingError>, then: Usd) -> Result<Usd, String> {
    num(acct(now)?.checked_sub(then), what)
}

fn check(
    account: &Account,
    (record, decision): (Option<&Record>, Option<&Decision>),
    (gate, config, when): (&gate::Gate, &Config, UtcNanos),
    baseline: Option<&Baseline>,
    instruments: &Instruments,
    expect: &Json,
) -> Result<(), String> {
    let expect = fields(
        expect,
        "expect",
        &[
            "decision",
            "positions",
            "cash",
            "fees",
            "realized_pnl_gross",
            "realized_pnl_net",
            "unrealized_pnl",
            "equity",
            "marks",
            "conservation",
            "trade_date",
            "settles_on",
            "receivables",
            "income",
            "buying_power",
            "agent_mode",
            "day_trade_count",
        ],
    )?;
    for (key, expected) in expect {
        match key.as_str() {
            "decision" => gate.check_decision(decision, expected)?,
            "day_trade_count" => {
                gate.check_day_trade_count(account, config, instruments, when, expected)?
            }
            "agent_mode" => {
                return Err(format!(
                    "expectation `agent_mode` not interpreted until {MODE_HOLDER}"
                ));
            }
            "positions" => {
                let listed = expected.as_object().ok_or("`positions` is not an object")?;
                for (name, e) in listed {
                    let what = format!("positions.{name}");
                    let p = fields(e, &what, &["qty", "cost_basis", "avg_cost"])?;
                    let (_, id, _, _) = instrument(instruments, name)?;
                    let position = account.position(id);
                    for (field, value) in p {
                        let what = format!("{what}.{field}");
                        match field.as_str() {
                            "qty" => check_dec(&what, position.qty(), value)?,
                            "cost_basis" => check_dec(&what, position.basis(), value)?,
                            _ => check_dec(&what, avg_cost(position)?, value)?,
                        }
                    }
                }
            }
            "cash" => {
                let c = fields(expected, "cash", &["settled", "unsettled", "total"])?;
                for (field, value) in c {
                    match field.as_str() {
                        "settled" => check_dec("cash.settled", account.settled(), value)?,
                        "total" => check_dec("cash.total", acct(account.cash_total())?, value)?,
                        _ => check_unsettled(account, value)?,
                    }
                }
            }
            "fees" => {
                let f = fields(expected, "fees", &["accrued", "charged", "total_usd"])?;
                for (field, value) in f {
                    match field.as_str() {
                        "accrued" => {
                            check_dec("fees.accrued", acct(account.fees_accrued())?, value)?
                        }
                        "charged" => check_dec("fees.charged", account.fees_charged(), value)?,
                        _ => check_dec("fees.total_usd", acct(account.fees_total())?, value)?,
                    }
                }
            }
            "realized_pnl_gross" => check_dec(key, account.realized_gross(), expected)?,
            "realized_pnl_net" => check_dec(key, acct(account.realized_net())?, expected)?,
            "unrealized_pnl" => check_dec(key, acct(account.unrealized())?, expected)?,
            "equity" => check_dec(key, acct(account.equity())?, expected)?,
            "receivables" => check_dec(key, acct(account.net_receivables())?, expected)?,
            "income" => check_dec(key, account.income(), expected)?,
            "buying_power" => check_dec(
                key,
                acct(account.buying_power(Reservations::NONE))?,
                expected,
            )?,
            "marks" => {
                let listed = expected.as_object().ok_or("`marks` is not an object")?;
                for (name, value) in listed {
                    let (_, id, _, _) = instrument(instruments, name)?;
                    let mark = account
                        .mark(id)
                        .ok_or_else(|| format!("marks.{name}: no mark"))?;
                    check_dec(&format!("marks.{name}"), mark, value)?;
                }
            }
            "conservation" => {
                let baseline =
                    baseline.ok_or("`conservation` before every value it compares is defined")?;
                check_conservation(account, baseline, expected)?
            }
            "trade_date" | "settles_on" => {
                let Some(Record::Fill {
                    trade_date,
                    settles_on,
                    ..
                }) = record
                else {
                    return Err(format!("`{key}` expected on a step that applied no fill"));
                };
                let actual = if key == "trade_date" {
                    trade_date
                } else {
                    settles_on
                };
                expect_eq(
                    key,
                    actual.map(|d| d.to_string()),
                    Some(date(expected, key)?.to_string()),
                )?;
            }
            other => return Err(format!("unknown expectation `{other}`")),
        }
    }
    Ok(())
}

/// A = B ÷ Q, derived (spec §8.1).
fn avg_cost(position: Position) -> Result<CostBasis, String> {
    let one = num(Qty::parse("1"), "1")?;
    let magnitude = num(
        position.basis().portion(
            one,
            position.qty().abs(),
            AVG_COST_SCALE,
            Rounding::HalfEven,
        ),
        "avg_cost",
    )?;
    Ok(if position.qty().is_negative() {
        num(CostBasis::ZERO.checked_sub(magnitude), "avg_cost")?
    } else {
        magnitude
    })
}

fn check_unsettled(account: &Account, expected: &Json) -> Result<(), String> {
    let mut wanted = Vec::new();
    for bucket in expected
        .as_array()
        .ok_or("`cash.unsettled` is not a list")?
    {
        fields(bucket, "cash.unsettled[]", &["settles_on", "amount"])?;
        wanted.push((
            date(at(bucket, "settles_on")?, "settles_on")?.to_string(),
            dec_at(bucket, "amount")?.as_str().to_owned(),
        ));
    }
    wanted.sort();
    let actual: Vec<(String, String)> = account
        .unsettled()
        .map(|(d, amount)| (d.to_string(), amount.to_string()))
        .collect();
    expect_eq("cash.unsettled", actual, wanted)
}

fn check_conservation(
    account: &Account,
    baseline: &Baseline,
    expected: &Json,
) -> Result<(), String> {
    let c = fields(
        expected,
        "conservation",
        &[
            "delta_equity",
            "realized_gross",
            "unrealized",
            "income",
            "fees",
        ],
    )?;
    let delta_equity = delta("equity", account.equity(), baseline.equity)?;
    let realized = delta("realized", Ok(account.realized_gross()), baseline.realized)?;
    let unrealized = delta("unrealized", account.unrealized(), baseline.unrealized)?;
    let income = delta("income", Ok(account.income()), baseline.income)?;
    let fees = delta("fees", account.fees_total(), baseline.fees)?;
    let identity = num(
        realized
            .checked_add(unrealized)
            .and_then(|v| v.checked_add(income))
            .and_then(|v| v.checked_sub(fees)),
        "identity",
    )?;
    expect_eq(
        "conservation identity (Δequity = realized + unrealized + income − fees)",
        delta_equity,
        identity,
    )?;
    for (field, value) in c {
        let actual = match field.as_str() {
            "delta_equity" => delta_equity,
            "realized_gross" => realized,
            "unrealized" => unrealized,
            "income" => income,
            _ => fees,
        };
        check_dec(&format!("conservation.{field}"), actual, value)?;
    }
    Ok(())
}

/// The backtest cases (trading-domain spec §6.4; RC-10, RC-12, RC-19; DEC-106 item 11). A backtest
/// case has `bars` and `orders` instead of `steps`: every order carries its own expectation, and
/// `isolation: per_order` simulates each one alone against a fresh volume cap.
///
/// Sessions, session starts, auction bars, and trading dates are the labels `mandate-sim` reads from
/// each bar (spec §4.1). The harness derives them from the bar's instant: the New York hour comes
/// from `mandate-time`, and the minute of an instant is its UTC minute, because every New York
/// offset is a whole number of hours. A bar starting exactly at the regular session's open is the
/// auction bar (DEC-106 item 5).
mod backtest {
    use super::*;

    /// 04:00, 09:30, 16:00, 13:00 on an early close, and 20:00 ET, as minutes from New York midnight
    /// (spec §4.3).
    const PRE_MARKET_OPEN: i64 = 4 * 60;
    const REGULAR_OPEN: i64 = 9 * 60 + 30;
    const EARLY_CLOSE: i64 = 13 * 60;
    const REGULAR_CLOSE: i64 = 16 * 60;
    const OVERNIGHT_OPEN: i64 = 20 * 60;
    const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

    const PURPOSES: [&str; 5] = [
        "open",
        "protective",
        "risk_exit",
        "discretionary_exit",
        "owner_exit",
    ];

    /// One 20-session median, from the case's `first_bar_reference_volume`, for every bar that opens
    /// a session (spec §6.4 rule 3).
    struct Medians(Option<Qty>);

    impl FirstBarVolumes for Medians {
        fn median_at(&self, _bar_start: UtcNanos) -> Option<Qty> {
            self.0
        }
    }

    fn sim<T>(result: Result<T, SimError>) -> Result<T, String> {
        result.map_err(|e| format!("{e} ({})", e.code()))
    }

    pub(super) fn run(fixture: &Json, case: &Json) -> Result<(), String> {
        fields(
            case,
            "backtest case",
            &[
                "id",
                "title",
                "scope",
                "broker_profile",
                "config",
                "config_overrides",
                "calendar",
                "instruments",
                "isolation",
                "bars",
                "orders",
            ],
        )?;
        let config = config_of(fixture, case)?;
        let name = str_at(case, "calendar")?;
        let calendar = calendar(fixture, name)?;
        let early = early_closes(fixture, name)?;
        let listed = instruments(case)?;
        let (_, _, class, fractionable) = match listed.as_slice() {
            [only] => only,
            _ => return Err("a backtest case names exactly one instrument".to_owned()),
        };
        let instrument = Instrument {
            asset_class: *class,
            increment: match fractionable {
                Some(true) => ShareIncrement::Fractional,
                _ => ShareIncrement::Whole,
            },
        };
        let per_order = match case.get("isolation").map(Json::as_str) {
            None => false,
            Some(Some("per_order")) => true,
            Some(other) => return Err(format!("unknown isolation {other:?}")),
        };
        let shared = case.get("bars");
        let mut orders = Vec::new();
        for listed in list_at(case, "orders")? {
            let name = str_at(listed, "name")?.to_owned();
            fields(
                listed,
                &format!("order `{name}`"),
                &[
                    "name",
                    "initial_position",
                    "decided_at",
                    "order",
                    "bars",
                    "first_bar_reference_volume",
                    "expect",
                ],
            )
            .and_then(|_| {
                let bars = listed
                    .get("bars")
                    .or(shared)
                    .ok_or("neither the case nor the order has `bars`")?;
                let bars: Vec<SimBar> = list_at(&json!({ "bars": bars }), "bars")?
                    .iter()
                    .map(|bar| bar_of(bar, *class, &calendar, &early))
                    .collect::<Result<_, String>>()?;
                let median = match listed.get("first_bar_reference_volume") {
                    None => None,
                    Some(volume) => Some(num(
                        Qty::parse(dec(volume, "first_bar_reference_volume")?.as_str()),
                        "first_bar_reference_volume",
                    )?),
                };
                let order = order_of(listed)?;
                Ok((bars, median, order, listed.get("expect")))
            })
            .map_err(|e: String| format!("order `{name}`: {e}"))
            .map(|built| orders.push((name, built)))?;
        }
        ensure(per_order || orders.len() <= 1 || shared.is_some(), || {
            "orders without `isolation: per_order` need the case's own `bars`".to_owned()
        })?;
        if per_order {
            for (name, (bars, median, order, expect)) in &orders {
                let outcome = simulated(&config, &instrument, bars, &Medians(*median), &[*order])?;
                if let Some(expect) = expect {
                    check(expect, &outcome, OrderRef::new(0))
                        .map_err(|e| format!("order `{name}`: {e}"))?;
                }
            }
            return Ok(());
        }
        let bars = orders
            .first()
            .map(|(_, (bars, _, _, _))| bars.clone())
            .unwrap_or_default();
        let median = orders.first().and_then(|(_, (_, median, _, _))| *median);
        let submitted: Vec<SimOrder> = orders.iter().map(|(_, (_, _, o, _))| *o).collect();
        let outcome = simulated(&config, &instrument, &bars, &Medians(median), &submitted)?;
        for (index, (name, (_, _, _, expect))) in orders.iter().enumerate() {
            if let Some(expect) = expect {
                check(expect, &outcome, OrderRef::new(index))
                    .map_err(|e| format!("order `{name}`: {e}"))?;
            }
        }
        Ok(())
    }

    fn simulated(
        config: &SimConfig,
        instrument: &Instrument,
        bars: &[SimBar],
        medians: &Medians,
        orders: &[SimOrder],
    ) -> Result<SimOutcome, String> {
        let coverage_start = bars
            .first()
            .map(|b| b.start)
            .ok_or("a backtest case needs at least one bar")?;
        sim(simulate(
            config,
            instrument,
            bars,
            coverage_start,
            medians,
            orders,
        ))
    }

    /// The `backtest` block of the case's configuration (spec §6.4). A configuration that states a
    /// reference, a first-bar source, or a tick rounding other than the spec's fixed ones fails.
    fn config_of(fixture: &Json, case: &Json) -> Result<SimConfig, String> {
        let resolved = resolved_config(fixture, case)?;
        let backtest = at(&resolved, "backtest")?;
        fields(
            backtest,
            "config backtest",
            &[
                "decision_latency_ms",
                "approval_latency_ms",
                "slippage",
                "volume_cap",
                "fill_price_tick_rounding",
            ],
        )?;
        fixed(backtest, "fill_price_tick_rounding", &json!("none"))?;
        let slippage = at(backtest, "slippage")?;
        fields(
            slippage,
            "config backtest.slippage",
            &[
                "half_spread_bps",
                "impact_model",
                "impact_bps",
                "impact_coefficient_bps",
            ],
        )?;
        let half_spread_bps = num(
            Bps::parse(dec_at(slippage, "half_spread_bps")?.as_str()),
            "half_spread_bps",
        )?;
        let slippage = match str_at(slippage, "impact_model")? {
            "fixed" => Slippage::Fixed {
                half_spread_bps,
                impact_bps: num(
                    Bps::parse(dec_at(slippage, "impact_bps")?.as_str()),
                    "impact_bps",
                )?,
            },
            "sqrt" => Slippage::Sqrt {
                half_spread_bps,
                coefficient_bps: num(
                    Bps::parse(dec_at(slippage, "impact_coefficient_bps")?.as_str()),
                    "impact_coefficient_bps",
                )?,
            },
            other => return Err(format!("unknown impact_model `{other}`")),
        };
        let cap = at(backtest, "volume_cap")?;
        fields(
            cap,
            "config backtest.volume_cap",
            &["fraction", "reference", "first_bar"],
        )?;
        fixed(cap, "reference", &json!("previous_bar_same_session"))?;
        fixed(cap, "first_bar", &json!("median_same_minute_20_sessions"))?;
        Ok(SimConfig {
            decision_latency: sim(Nanos::from_millis(u64_at(backtest, "decision_latency_ms")?))?,
            approval_latency: sim(Nanos::from_millis(u64_at(backtest, "approval_latency_ms")?))?,
            slippage,
            volume_cap_fraction: num(
                Fraction::parse(dec_at(cap, "fraction")?.as_str()),
                "fraction",
            )?,
        })
    }

    /// The calendar's early closes, on which the regular session ends at 13:00 ET (spec §4.3).
    fn early_closes(fixture: &Json, name: &str) -> Result<Vec<Date>, String> {
        let listed = at(fixture, "calendars")?
            .get(name)
            .ok_or_else(|| format!("no calendar `{name}`"))?;
        dates(at(listed, "early_closes")?, "early_closes")
    }

    /// One bar, with the session labels the fill model reads (spec §4.1, §4.3).
    fn bar_of(
        bar: &Json,
        class: AssetClass,
        calendar: &TradingCalendar,
        early: &[Date],
    ) -> Result<SimBar, String> {
        fields(
            bar,
            "bar",
            &["start", "open", "high", "low", "close", "volume", "session"],
        )?;
        let start = UtcNanos::parse_rfc3339(str_at(bar, "start")?)
            .map_err(|e| format!("bar `start`: {e}"))?;
        let (session, session_start, trade_date) = label(start, class, calendar, early)?;
        if let Some(stated) = bar.get("session") {
            let stated = stated
                .as_str()
                .ok_or("a bar's `session` is not a string")?
                .to_owned();
            expect_eq("bar session", name_of(session), stated.as_str())?;
        }
        let price = |key: &str| num(Price::parse(dec_at(bar, key)?.as_str()), key);
        Ok(SimBar {
            start,
            open: price("open")?,
            high: price("high")?,
            low: price("low")?,
            close: price("close")?,
            volume: num(Qty::parse(dec_at(bar, "volume")?.as_str()), "volume")?,
            trade_date,
            session,
            session_start,
            auction: session == Session::Regular && start == session_start,
        })
    }

    fn name_of(session: Session) -> &'static str {
        match session {
            Session::Overnight => "overnight",
            Session::PreMarket => "pre_market",
            Session::Regular => "regular",
            Session::AfterHours => "after_hours",
            Session::Continuous => "continuous",
        }
    }

    /// The session, its first instant, and the trading day of an instant. Crypto trades continuously,
    /// in a day that ends at 00:00 UTC (spec §2.2, §4.3).
    fn label(
        start: UtcNanos,
        class: AssetClass,
        calendar: &TradingCalendar,
        early: &[Date],
    ) -> Result<(Session, UtcNanos, Date), String> {
        if class == AssetClass::Crypto {
            let midnight = start
                .secs()
                .checked_sub(start.secs().rem_euclid(SECONDS_PER_DAY))
                .ok_or("a bar's start is out of range")?;
            let session_start =
                UtcNanos::from_parts(midnight, 0).map_err(|e| format!("bar `start`: {e}"))?;
            return Ok((Session::Continuous, session_start, start.date()));
        }
        let (date, hour) =
            new_york_date_and_hour(start).map_err(|e| format!("bar `start`: {e}"))?;
        let minute = start.secs().rem_euclid(3600) / 60;
        let minutes = i64::from(hour)
            .checked_mul(60)
            .and_then(|h| h.checked_add(minute))
            .ok_or("a bar's start is out of range")?;
        let close = if early.contains(&date) {
            EARLY_CLOSE
        } else {
            REGULAR_CLOSE
        };
        let (session, session_minutes) = match minutes {
            m if m < PRE_MARKET_OPEN => {
                (Session::Overnight, -(SECONDS_PER_DAY / 60) + OVERNIGHT_OPEN)
            }
            m if m < REGULAR_OPEN => (Session::PreMarket, PRE_MARKET_OPEN),
            m if m < close => (Session::Regular, REGULAR_OPEN),
            m if m < OVERNIGHT_OPEN => (Session::AfterHours, close),
            _ => (Session::Overnight, OVERNIGHT_OPEN),
        };
        let midnight = new_york_midnight(date).map_err(|e| format!("bar `start`: {e}"))?;
        let session_start = midnight
            .secs()
            .checked_add(session_minutes.saturating_mul(60))
            .ok_or("a bar's session start is out of range")
            .and_then(|secs| {
                UtcNanos::from_parts(secs, 0).map_err(|_| "a bar's session start is out of range")
            })?;
        let trade_date = calendar
            .equity_trade_date(start)
            .map_err(|e| format!("bar `start`: {e}"))?;
        Ok((session, session_start, trade_date))
    }

    /// One order, and the position it exits. A sell may never exceed the position held: v1 has no
    /// short sales (DEC-32), so a case cannot describe one.
    ///
    /// No backtest case states a time in force or `extended_hours`, so neither is interpreted
    /// (DEC-85): every order is a day order that trades only the regular session, the most
    /// restrictive reading, and a case that states either key fails as an unknown one.
    fn order_of(listed: &Json) -> Result<SimOrder, String> {
        let held = match listed.get("initial_position") {
            None => Qty::ZERO,
            Some(position) => {
                fields(position, "initial_position", &["qty"])?;
                num(Qty::parse(dec_at(position, "qty")?.as_str()), "qty")?
            }
        };
        let order = at(listed, "order")?;
        fields(
            order,
            "order",
            &[
                "side",
                "type",
                "qty",
                "limit_price",
                "stop_price",
                "legs",
                "purpose",
                "resting_since_bar",
                "resting_since",
            ],
        )?;
        let eligible_from = match (
            order.get("resting_since_bar"),
            order.get("resting_since"),
            listed.get("decided_at"),
        ) {
            (Some(bar), None, None) => Eligibility::Resting {
                from_bar: usize::try_from(u64_at(order, "resting_since_bar")?)
                    .map_err(|_| format!("`resting_since_bar` {bar} is not a bar index"))?,
            },
            (None, Some(since), None) => {
                expect_eq(
                    "resting_since",
                    since.as_str(),
                    Some("previous_trading_day"),
                )?;
                Eligibility::Resting { from_bar: 0 }
            }
            (None, None, Some(decided)) => Eligibility::DecidedAt {
                at: UtcNanos::parse_rfc3339(
                    decided.as_str().ok_or("`decided_at` is not a string")?,
                )
                .map_err(|e| format!("`decided_at`: {e}"))?,
                approval_required: false,
            },
            _ => {
                return Err(
                    "an order needs exactly one of `decided_at`, `resting_since_bar`, and \
                     `resting_since`"
                        .to_owned(),
                );
            }
        };
        let price = |key: &str| num(Price::parse(dec_at(order, key)?.as_str()), key);
        let (side, qty, kind) = match str_at(order, "type")? {
            "oco" => oco(order)?,
            other => {
                let side = side_of(str_at(order, "side")?)?;
                let qty = num(Qty::parse(dec_at(order, "qty")?.as_str()), "qty")?;
                let kind = match other {
                    "market" => OrderKind::Market,
                    "limit" => OrderKind::Limit {
                        limit: price("limit_price")?,
                    },
                    "stop" => OrderKind::Stop {
                        stop: price("stop_price")?,
                    },
                    "stop_limit" => OrderKind::StopLimit {
                        stop: price("stop_price")?,
                        limit: price("limit_price")?,
                    },
                    unknown => return Err(format!("unknown order type `{unknown}`")),
                };
                (side, qty, kind)
            }
        };
        if let Some(purpose) = order.get("purpose") {
            let purpose = purpose.as_str().ok_or("`purpose` is not a string")?;
            ensure(PURPOSES.contains(&purpose), || {
                format!("unknown purpose `{purpose}`")
            })?;
            ensure(purpose == "open" || !held.is_zero(), || {
                format!("a `{purpose}` order has no position to exit")
            })?;
        }
        ensure(side == Side::Buy || qty <= held, || {
            "a sell exceeds the position held, which v1 never does (DEC-32)".to_owned()
        })?;
        Ok(SimOrder {
            side,
            qty,
            kind,
            tif: TimeInForce::Day,
            extended_hours: false,
            eligible_from,
        })
    }

    /// A protective pair: one limit leg and one stop leg, the same side and quantity (spec §5.4).
    fn oco(order: &Json) -> Result<(Side, Qty, OrderKind), String> {
        let legs = list_at(order, "legs")?;
        let [first, second] = legs else {
            return Err("an OCO has exactly two legs".to_owned());
        };
        let mut limit = None;
        let mut stop = None;
        let mut sides = Vec::new();
        let mut quantities = Vec::new();
        for leg in [first, second] {
            fields(
                leg,
                "OCO leg",
                &["side", "type", "qty", "limit_price", "stop_price"],
            )?;
            sides.push(side_of(str_at(leg, "side")?)?);
            quantities.push(num(Qty::parse(dec_at(leg, "qty")?.as_str()), "qty")?);
            match str_at(leg, "type")? {
                "limit" => {
                    limit = Some(num(
                        Price::parse(dec_at(leg, "limit_price")?.as_str()),
                        "limit_price",
                    )?);
                }
                "stop" => {
                    stop = Some(num(
                        Price::parse(dec_at(leg, "stop_price")?.as_str()),
                        "stop_price",
                    )?);
                }
                other => return Err(format!("an OCO leg is limit or stop, not `{other}`")),
            }
        }
        match (limit, stop, sides.as_slice(), quantities.as_slice()) {
            (Some(limit), Some(stop), [a, b], [x, y]) if a == b && x == y => {
                Ok((*a, *x, OrderKind::Oco { limit, stop }))
            }
            (Some(_), Some(_), _, _) => {
                Err("an OCO's legs have the same side and quantity".to_owned())
            }
            _ => Err("an OCO has one limit leg and one stop leg".to_owned()),
        }
    }

    fn side_of(text: &str) -> Result<Side, String> {
        match text {
            "buy" => Ok(Side::Buy),
            "sell" => Ok(Side::Sell),
            other => Err(format!("unknown side `{other}`")),
        }
    }

    /// The `fills` and `canceled_legs` expectations of one order. Every key a case states is
    /// compared; liquidity is compared only where the case states it (DEC-106 item 11).
    fn check(expect: &Json, outcome: &SimOutcome, order: OrderRef) -> Result<(), String> {
        let stated = fields(expect, "expect", &["fills", "canceled_legs"])?;
        for (key, value) in stated {
            match key.as_str() {
                "fills" => check_fills(value, outcome, order)?,
                _ => {
                    let expected: Vec<&str> = value
                        .as_array()
                        .ok_or("`canceled_legs` is not a list")?
                        .iter()
                        .map(|leg| leg.as_str().ok_or("a canceled leg is not a string"))
                        .collect::<Result<_, _>>()?;
                    let actual: Vec<&str> = outcome
                        .canceled_legs
                        .iter()
                        .filter(|canceled| canceled.order == order)
                        .map(|canceled| leg_name(canceled.leg))
                        .collect();
                    expect_eq("canceled_legs", actual, expected)?;
                }
            }
        }
        Ok(())
    }

    fn check_fills(expected: &Json, outcome: &SimOutcome, order: OrderRef) -> Result<(), String> {
        let expected = expected.as_array().ok_or("`fills` is not a list")?;
        let actual: Vec<&SimFill> = outcome.fills_of(order).collect();
        expect_eq("fills: count", actual.len(), expected.len())?;
        for (n, (fill, wanted)) in actual.iter().zip(expected).enumerate() {
            let what = |key: &str| format!("fills[{n}].{key}");
            let stated = fields(
                wanted,
                &format!("fills[{n}]"),
                &["bar", "qty", "price", "leg", "liquidity"],
            )?;
            for (key, value) in stated {
                match key.as_str() {
                    "bar" => expect_eq(
                        &what("bar"),
                        u64::try_from(fill.bar).unwrap_or(u64::MAX),
                        u64_at(wanted, "bar")?,
                    )?,
                    "qty" => check_dec(&what("qty"), fill.qty, value)?,
                    "price" => check_dec(&what("price"), fill.price, value)?,
                    "leg" => expect_eq(&what("leg"), fill.leg.map(leg_name), value.as_str())?,
                    _ => expect_eq(
                        &what("liquidity"),
                        fill.liquidity.map(liquidity_name),
                        value.as_str(),
                    )?,
                }
            }
        }
        Ok(())
    }

    fn leg_name(leg: OcoLeg) -> &'static str {
        match leg {
            OcoLeg::Limit => "limit",
            OcoLeg::Stop => "stop",
        }
    }

    fn liquidity_name(liquidity: Liquidity) -> &'static str {
        match liquidity {
            Liquidity::Maker => "maker",
            Liquidity::Taker => "taker",
        }
    }
}
