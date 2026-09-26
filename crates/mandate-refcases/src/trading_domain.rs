//! The `trading_domain` suite: `fixtures/refcases/trading-domain.json`, the JSON form of
//! [the trading-domain reference cases](../../../docs/specs/reference-cases/trading-domain.yaml)
//! (spec §14). One named test per case (`trading_domain::RC-01`) and per variant
//! (`trading_domain::RC-03::gate_rejects_zero_crossing_order`).
//!
//! E3-1 interprets `fill`, `mark`, `fees_charged`, and `advance_clock` (settlement) steps and the
//! accounting expectation keys; E3-2 adds `corporate_action_applied` (splits and cash dividends),
//! `broker_cash_posting` (cash in lieu), dividend payment on `advance_clock`, and the `receivables`
//! and `income` expectations (DEC-96); E3-3 adds the account type (`initial.account.type`) and the
//! `buying_power` expectation, the fold's model buying power with no reservations (DEC-105). A
//! case that uses anything owned by a later story fails with "not interpreted until <story>" for
//! each such item; anything the vocabulary does not know fails as unknown. Every key of every
//! interpreted expectation is checked.

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
    Bps, CostBasis, FeeCap, FeePerShare, FeeRate, Price, Qty, Rounding, ShareIncrement, SignedQty,
    SplitRatio, Usd,
};
use mandate_time::{Date, TradingCalendar, UtcNanos};
use serde_json::{Map, json};

use crate::{Case, Json, at, ensure, expect_eq, list_at, str_at, u64_at};

const SUITE: &str = "trading_domain";
const SCHEMA_VERSION: u64 = 3;
const CALENDAR_RULE: &str = "weekdays not listed as holidays are trading days";
const MAX_EXTENDS: usize = 8;
/// Average cost is compared as round(B ÷ Q, 12, half_even).
const AVG_COST_SCALE: u32 = 12;

/// Step events owned by later stories.
const PENDING_EVENTS: &[(&str, &str)] = &[
    ("propose_order", "E6-3"),
    ("broker_order_update", "E7-2"),
    ("corporate_action_prepare", "E7-4"),
    ("reconciliation", "E7-3"),
    ("broker_position_update", "E7-3"),
    ("broker_account_update", "E6-9"),
    ("deploy_agent", "E7-5"),
    ("owner_ack", "E7-5"),
    ("kill_switch", "E6-5"),
    ("conduct_breach", "E6-8"),
];

/// Expectation keys owned by later stories.
const PENDING_EXPECT: &[(&str, &str)] = &[
    ("decision", "E6-3"),
    ("orders", "E7-2"),
    ("actions", "E7-4"),
    ("agent_mode", "E6-9"),
    ("day_trade_count", "E6-6"),
    ("fills", "E4-1"),
    ("canceled_legs", "E4-1"),
    ("reconciliation", "E7-3"),
    ("protective_sell_qty", "E7-4"),
];

/// Case-level keys of backtest cases.
const PENDING_CASE_KEYS: &[(&str, &str)] =
    &[("bars", "E4-1"), ("orders", "E4-1"), ("isolation", "E4-1")];

const PENDING_INITIAL: &[(&str, &str)] = &[("open_orders", "E7-4"), ("agents", "E7-5")];

const PENDING_ACCOUNT: &[(&str, &str)] = &[
    ("regime", "E6-6"),
    ("prior_day_trades", "E6-6"),
    ("last_equity", "E6-6"),
    ("multiplier", "E6-6"),
    ("status", "E6-9"),
    ("crypto_status", "E6-9"),
    ("trading_blocked", "E6-9"),
    ("account_blocked", "E6-9"),
    ("trade_suspended_by_user", "E6-9"),
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
    let members = case.as_object().unwrap_or(&empty);
    for key in members.keys() {
        if let Some(story) = owner(PENDING_CASE_KEYS, key) {
            note(format!("case key `{key}`"), story);
        }
    }
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
        if let Some(story) = owner(PENDING_ACCOUNT, key) {
            note(format!("initial account `{key}`"), story);
        }
    }
    let steps = case.get("steps").and_then(Json::as_array);
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
    }
    out
}

fn run_case(fixture: &Json, case: &Json) -> Result<(), String> {
    let pending = pending(case);
    if !pending.is_empty() {
        return Err(pending.into_iter().collect::<Vec<_>>().join("; "));
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
    let mut baseline = Baseline::of(&account);
    for (n, step) in list_at(case, "steps")?.iter().enumerate() {
        let label = format!("step {}", n.saturating_add(1));
        let record = run_step(&mut account, step, n, &instruments, &config)
            .map_err(|e| format!("{label}: {e}"))?;
        if let Some(expect) = step.get("expect") {
            check(
                &account,
                record.as_ref(),
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
    fields(account, "initial account", &["cash", "type"])?;
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

fn run_step(
    account: &mut Account,
    step: &Json,
    n: usize,
    instruments: &Instruments,
    config: &Config,
) -> Result<Option<Record>, String> {
    fields(step, "step", &["at", "event", "data", "expect"])?;
    let when = UtcNanos::parse_rfc3339(str_at(step, "at")?).map_err(|e| format!("`at`: {e}"))?;
    let empty = Json::Object(Map::new());
    let data = step.get("data").unwrap_or(&empty);
    let inputs = match str_at(step, "event")? {
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
            vec![Input::Fill(Execution {
                fill_id: format!("step_{n}"),
                client_order_id,
                instrument: id.clone(),
                asset_class: *class,
                side,
                qty_gross: num(Qty::parse(dec_at(data, "qty_gross")?.as_str()), "qty_gross")?,
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
    Ok(record)
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
    record: Option<&Record>,
    baseline: Option<&Baseline>,
    instruments: &Instruments,
    expect: &Json,
) -> Result<(), String> {
    let expect = fields(
        expect,
        "expect",
        &[
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
        ],
    )?;
    for (key, expected) in expect {
        match key.as_str() {
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
