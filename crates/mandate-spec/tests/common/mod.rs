//! A mandate as a canonical value, built by hand so the parse can be tested without a fixture.
//!
//! The reference cases cover the 31 MC-S rejections from `fixtures/refcases/mandate.json` through
//! `mandate-refcases`. What this builder is for is the other half: the **pointer and the code** each
//! rejection carries, which a case's `schema_valid: false` does not pin.
//!
//! The shape follows `two_stock_swing`, the two-instrument equity base, because it is the one base
//! with protection on, no research agent, and no goal instrument.

use mandate_canon::{Int, Key, Value};

pub fn s(text: &str) -> Value {
    Value::Str(text.to_owned())
}

pub fn i(n: u64) -> Value {
    Value::Int(Int::new(n).expect("a canonical integer"))
}

pub fn b(flag: bool) -> Value {
    Value::Bool(flag)
}

pub fn arr(items: Vec<Value>) -> Value {
    Value::Array(items)
}

pub fn obj(members: Vec<(&str, Value)>) -> Value {
    Value::Object(
        members
            .into_iter()
            .map(|(k, v)| (Key::new(k).expect("a canonical key"), v))
            .collect(),
    )
}

const SHA: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
pub const ASSET_A: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
const ASSET_B: &str = "7b4a1c2e-3333-4a2b-9c3d-000000000003";

/// A document that should parse. Every test below starts here and changes one thing.
pub fn base() -> Value {
    obj(vec![
        ("mandate_schema_version", i(1)),
        ("name", s("two-stock-swing")),
        ("source_text_ref", Value::Null),
        ("environment", s("paper")),
        ("connection_id", s("conn_alpaca_paper_01")),
        (
            "capital",
            obj(vec![
                ("allocation_usd", s("10000")),
                ("max_loss_from_allocation", s("0.1")),
            ]),
        ),
        (
            "goal",
            obj(vec![
                ("type", s("continuous")),
                ("end_date", Value::Null),
                ("on_complete", s("hold_protected")),
            ]),
        ),
        (
            "universe",
            obj(vec![
                ("pinned", b(true)),
                (
                    "pinned_instruments",
                    arr(vec![instrument(ASSET_A, "AAA"), instrument(ASSET_B, "BBB")]),
                ),
                ("max_instruments", i(2)),
                ("asset_classes", arr(vec![s("us_equity")])),
                ("leveraged_etps_enabled", b(false)),
                ("leveraged_etp_disclosure_version", Value::Null),
            ]),
        ),
        (
            "behavior",
            obj(vec![
                ("description", s("Buy strength; exit on weakness.")),
                ("signal_models", arr(vec![model()])),
                ("research", Value::Null),
                (
                    "cadence",
                    obj(vec![
                        ("interval_s", i(900)),
                        ("event_sources", arr(vec![s("price")])),
                    ]),
                ),
                (
                    "sizing",
                    obj(vec![
                        ("method", s("conviction_linear")),
                        ("entry_threshold", s("0.3")),
                        ("exit_threshold", s("0.3")),
                        ("rebalance_band", s("0.05")),
                    ]),
                ),
            ]),
        ),
        (
            "protection",
            obj(vec![
                ("enabled", b(true)),
                ("stop_distance", s("0.05")),
                ("take_profit_distance", Value::Null),
                ("crypto_stop_limit_offset", Value::Null),
            ]),
        ),
        (
            "risk",
            obj(vec![
                ("max_position_usd", s("1500")),
                ("max_position_fraction", s("0.5")),
                ("max_gross_exposure_usd", s("3000")),
                ("max_order_usd", s("1000")),
                ("max_orders_per_day", i(50)),
                ("max_daily_loss", s("0.02")),
                ("daily_loss_action", s("exits_only")),
                ("max_drawdown", s("0.08")),
                (
                    "drawdown_ladder",
                    arr(vec![
                        rung("0.03", "scale_sizes", Some("0.5")),
                        rung("0.06", "exits_only", None),
                        rung("0.08", "flatten_and_pause", None),
                    ]),
                ),
                ("hysteresis", s("0.01")),
                ("breach_confirm_s", i(60)),
                ("daily_breach_min_s", i(3600)),
                ("scale_lift_after_s", i(600)),
                ("reentry_cooldown_s", i(3600)),
                ("scale_action", s("limit_buys")),
            ]),
        ),
        (
            "autonomy",
            obj(vec![
                (
                    "rules",
                    arr(vec![obj(vec![
                        ("id", s("large_orders")),
                        (
                            "when",
                            obj(vec![
                                ("field", s("order_usd")),
                                ("op", s("gt")),
                                ("value", s("900")),
                            ]),
                        ),
                        ("then", s("ask")),
                    ])]),
                ),
                ("default", s("ask")),
                ("admission", s("ask")),
                (
                    "approval",
                    obj(vec![
                        ("timeout_s", i(600)),
                        ("on_timeout", s("skip")),
                        ("approvers", arr(vec![s("role:approver")])),
                        ("two_approver_above_usd", Value::Null),
                    ]),
                ),
            ]),
        ),
        (
            "notifications",
            obj(vec![
                ("channels", arr(vec![s("email")])),
                ("quiet_hours", Value::Null),
            ]),
        ),
    ])
}

fn instrument(asset_id: &str, symbol: &str) -> Value {
    obj(vec![
        ("asset_id", s(asset_id)),
        ("symbol", s(symbol)),
        ("asset_class", s("us_equity")),
    ])
}

fn model() -> Value {
    obj(vec![
        ("id", s("quant.momentum")),
        ("version", s("1.0.0")),
        ("content_hash", s(SHA)),
        (
            "params",
            arr(vec![obj(vec![
                ("key", s("lookback_bars")),
                ("value", s("20")),
            ])]),
        ),
        ("weight", s("1")),
        ("max_output_age_s", i(1800)),
        ("admits_instruments", b(false)),
    ])
}

fn rung(at: &str, action: &str, factor: Option<&str>) -> Value {
    obj(vec![
        ("at", s(at)),
        ("action", s(action)),
        ("factor", factor.map_or(Value::Null, s)),
    ])
}

/// The base with one JSON Pointer replaced, added, or (with `None`) removed.
pub fn with(path: &str, value: Option<Value>) -> Value {
    with_all(&[(path, value)])
}

/// The base with several pointers changed at once, applied in order.
pub fn with_all(changes: &[(&str, Option<Value>)]) -> Value {
    let mut document = base();
    for (path, value) in changes {
        set(&mut document, path, value.clone());
    }
    document
}

fn set(document: &mut Value, path: &str, value: Option<Value>) {
    let tokens: Vec<&str> = path
        .strip_prefix('/')
        .unwrap_or_default()
        .split('/')
        .collect();
    let Some((last, parents)) = tokens.split_last() else {
        return;
    };
    let mut node = document;
    for token in parents {
        let next = match node {
            Value::Object(members) => Key::new(token).ok().and_then(|k| members.get_mut(&k)),
            Value::Array(items) => token.parse::<usize>().ok().and_then(|n| items.get_mut(n)),
            _ => None,
        };
        match next {
            Some(child) => node = child,
            None => return,
        }
    }
    match (node, value) {
        (Value::Object(members), Some(v)) => {
            if let Ok(key) = Key::new(last) {
                members.insert(key, v);
            }
        }
        (Value::Object(members), None) => {
            if let Ok(key) = Key::new(last) {
                members.remove(&key);
            }
        }
        (Value::Array(items), Some(v)) => {
            if let Ok(index) = last.parse::<usize>()
                && let Some(slot) = items.get_mut(index)
            {
                *slot = v;
            }
        }
        (Value::Array(items), None) => {
            if let Ok(index) = last.parse::<usize>()
                && index < items.len()
            {
                items.remove(index);
            }
        }
        _ => {}
    }
}
