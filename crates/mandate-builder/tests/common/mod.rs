//! Fixtures for the E6-2 tests: the two base mandates of the mandate reference cases as the order
//! builder and the autonomy policy see them (`reference/mandate/bases.py`), plus small builders so a
//! test states only what it changes.
//!
//! Every value here is transcribed from the committed bases, not invented, so a hand test's expected
//! figure is the reference case's figure.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_builder::{
    AccountSnapshot, AccumulateGoal, ActionContext, AutonomyPolicy, BuilderMandate, Condition,
    ContentHash, Decision, Direction, Field, GoalKind, Limits, Market, ModelId, ModelOutput,
    ModelVersion, Op, OrderShape, Purpose, RiskContext, Rule, RuleId, Session, SignalModel, Sizing,
    SizingMethod, Value,
};
use mandate_num::{
    Conviction, CostBasis, FeeRate, MarkPrice, Price, Qty, ShareIncrement, Signed, SizeFraction,
    Unit, Usd,
};
use mandate_time::UtcNanos;

pub const BTC: &str = "7b4a1c2e-1111-4a2b-9c3d-000000000001";
pub const XYZ: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
pub const QRS: &str = "7b4a1c2e-3333-4a2b-9c3d-000000000003";
pub const HASH_MEAN_REVERSION: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
pub const HASH_MOMENTUM: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";
pub const HASH_NEWS: &str =
    "sha256:3333333333333333333333333333333333333333333333333333333333333333";

/// The instant every builder case evaluates at.
pub const NOW: &str = "2026-09-22T14:00:00.000000000Z";
/// One minute before [`NOW`]: the `as_of` of a fresh output.
pub const MINUTE_AGO: &str = "2026-09-22T13:59:00.000000000Z";
/// An hour after [`NOW`]: the `expires_at` of a fresh output.
pub const HOUR_AHEAD: &str = "2026-09-22T15:00:00.000000000Z";

#[track_caller]
pub fn instrument(id: &str) -> InstrumentId {
    InstrumentId::new(id).unwrap()
}

#[track_caller]
pub fn time(text: &str) -> UtcNanos {
    UtcNanos::parse(text).unwrap()
}

#[track_caller]
pub fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap()
}

#[track_caller]
pub fn price(text: &str) -> Price {
    Price::parse(text).unwrap()
}

#[track_caller]
pub fn mark(text: &str) -> MarkPrice {
    MarkPrice::parse(text).unwrap()
}

#[track_caller]
pub fn qty(text: &str) -> Qty {
    Qty::parse(text).unwrap()
}

#[track_caller]
pub fn fraction(text: &str) -> SizeFraction {
    SizeFraction::parse(text).unwrap()
}

#[track_caller]
pub fn unit(text: &str) -> Unit {
    Unit::parse(text).unwrap()
}

#[track_caller]
pub fn conviction(text: &str) -> Conviction {
    Conviction::parse(text).unwrap()
}

#[track_caller]
pub fn basis(text: &str) -> CostBasis {
    CostBasis::parse(text).unwrap()
}

#[track_caller]
pub fn rate(text: &str) -> FeeRate {
    FeeRate::parse(text).unwrap()
}

/// `bases.py` RULES: the three rules every base mandate carries, in order.
pub fn base_rules() -> Vec<Rule> {
    vec![
        Rule {
            id: RuleId::new("large_orders"),
            when: Condition::Compare {
                field: Field::OrderUsd,
                op: Op::Gt,
                value: Value::Money(usd("900")),
            },
            then: Decision::Ask,
        },
        Rule {
            id: RuleId::new("low_score"),
            when: Condition::Compare {
                field: Field::CombinedScore,
                op: Op::Lt,
                value: Value::Unit(unit("0.65")),
            },
            then: Decision::Ask,
        },
        Rule {
            id: RuleId::new("routine"),
            when: Condition::Compare {
                field: Field::Purpose,
                op: Op::In,
                value: Value::Set(
                    ["increase", "open"]
                        .iter()
                        .map(|s| (*s).to_owned())
                        .collect(),
                ),
            },
            then: Decision::Auto,
        },
    ]
}

/// `bases.py` autonomy: the base rules, `default: ask`, `admission: ask`, no two-approver threshold.
#[track_caller]
pub fn base_policy() -> AutonomyPolicy {
    AutonomyPolicy::new(base_rules(), Decision::Ask, Decision::Ask, None).unwrap()
}

/// [`base_policy`] with one field replaced, which is how the `MC-A` cases patch it.
#[track_caller]
pub fn policy_with(
    rules: Vec<Rule>,
    default: Decision,
    admission: Decision,
    two_approver_above_usd: Option<Usd>,
) -> AutonomyPolicy {
    AutonomyPolicy::new(rules, default, admission, two_approver_above_usd).unwrap()
}

pub fn news_model() -> SignalModel {
    SignalModel {
        id: ModelId::new("llm.news_research"),
        version: ModelVersion::new("0.3.0"),
        content_hash: ContentHash::new(HASH_NEWS),
        weight: fraction("0.4"),
        max_output_age_s: 3600,
    }
}

pub fn momentum_model() -> SignalModel {
    SignalModel {
        id: ModelId::new("quant.momentum"),
        version: ModelVersion::new("1.0.0"),
        content_hash: ContentHash::new(HASH_MOMENTUM),
        weight: fraction("0.6"),
        max_output_age_s: 900,
    }
}

pub fn mean_reversion_model() -> SignalModel {
    SignalModel {
        id: ModelId::new("quant.mean_reversion"),
        version: ModelVersion::new("1.0.0"),
        content_hash: ContentHash::new(HASH_MEAN_REVERSION),
        weight: fraction("1"),
        max_output_age_s: 1800,
    }
}

/// `behavior.sizing` of both bases: thresholds 0.3 and a 5% rebalance band.
pub fn base_sizing() -> Sizing {
    Sizing {
        method: SizingMethod::ConvictionLinear,
        entry_threshold: fraction("0.3"),
        exit_threshold: fraction("0.3"),
        rebalance_band: fraction("0.05"),
    }
}

/// `two_stock_swing`: two models weighted 0.6 and 0.4, a 1500 USD position cap, and a 2000 USD gross
/// limit, on a `profit_stop` goal.
pub fn swing_mandate() -> BuilderMandate {
    BuilderMandate {
        models: vec![news_model(), momentum_model()],
        sizing: base_sizing(),
        limits: Limits {
            max_position_usd: usd("1500"),
            max_position_fraction: fraction("0.2"),
            max_order_usd: usd("1000"),
            max_gross_exposure_usd: usd("2000"),
        },
        goal: GoalKind::ProfitStop,
    }
}

/// `btc_accumulator`: one model at full weight on an `accumulate` goal, whose universe is pinned to
/// the goal instrument.
pub fn btc_mandate() -> BuilderMandate {
    BuilderMandate {
        models: vec![mean_reversion_model()],
        sizing: base_sizing(),
        limits: Limits {
            max_position_usd: usd("10000"),
            max_position_fraction: fraction("1"),
            max_order_usd: usd("1000"),
            max_gross_exposure_usd: usd("10000"),
        },
        goal: GoalKind::Accumulate(AccumulateGoal {
            instrument: instrument(BTC),
            target_qty: qty("0.15"),
            max_avg_price: Some(price("58000")),
            max_spend_usd: usd("9000"),
        }),
    }
}

/// A flat account with 10000 USD of agent equity, the state most `MC-B` cases start from.
pub fn flat_account() -> AccountSnapshot {
    AccountSnapshot {
        agent_equity: usd("10000"),
        position_qty: Qty::ZERO,
        cost_basis: CostBasis::ZERO,
        risk_mark: mark("99.9"),
        gross_usd: Usd::ZERO,
        working_opening_cost: Usd::ZERO,
        goal_spent_usd: Usd::ZERO,
    }
}

/// The `MC-B` equity market: bid 99.9, ask 100, whole shares, in the regular session.
pub fn equity_market() -> Market {
    Market {
        instrument: instrument(XYZ),
        asset_class: AssetClass::UsEquity,
        session: Session::Regular,
        in_close_window: false,
        bid: price("99.9"),
        ask: price("100"),
        increment: ShareIncrement::Whole,
        min_order_usd: usd("1"),
        fee_rate_cash: rate("0"),
        fee_rate_asset: rate("0"),
    }
}

/// The `btc_accumulator` market: a fractional crypto instrument that trades continuously.
pub fn crypto_market() -> Market {
    Market {
        instrument: instrument(BTC),
        asset_class: AssetClass::Crypto,
        session: Session::Crypto,
        in_close_window: false,
        bid: price("55000"),
        ask: price("55100"),
        increment: ShareIncrement::Fractional,
        min_order_usd: usd("1"),
        fee_rate_cash: rate("0"),
        fee_rate_asset: rate("0"),
    }
}

/// No drawdown, no prior fill, no thesis: the risk state every `MC-B` case starts from.
pub fn quiet_risk() -> RiskContext {
    RiskContext {
        size_factor: SizeFraction::ONE,
        drawdown: Unit::ZERO,
        daily_pnl_fraction: Signed::ZERO,
        position_pnl_fraction: Signed::ZERO,
        bought_today_usd: Usd::ZERO,
        has_prior_fill: false,
        new_instrument: false,
        thesis_confidence: Unit::ZERO,
    }
}

/// A fresh output for `model`, `as_of` a minute before [`NOW`] and expiring an hour after it.
pub fn fresh_output(
    model: &SignalModel,
    instrument_id: &str,
    conv: &str,
    conf: &str,
) -> ModelOutput {
    output_at(model, instrument_id, conv, conf, MINUTE_AGO, HOUR_AHEAD)
}

pub fn output_at(
    model: &SignalModel,
    instrument_id: &str,
    conv: &str,
    conf: &str,
    as_of: &str,
    expires_at: &str,
) -> ModelOutput {
    ModelOutput {
        model_id: model.id.clone(),
        model_version: model.version.clone(),
        content_hash: model.content_hash.clone(),
        instrument: instrument(instrument_id),
        as_of: time(as_of),
        expires_at: time(expires_at),
        direction: Direction::Long,
        conviction: conviction(conv),
        confidence: unit(conf),
    }
}

/// The `MC-B01` pair: momentum at conviction 0.8 and confidence 0.9, news at 0.2 and 0.5, which give
/// both convictions 0.472 and a score of 0.74.
pub fn swing_outputs() -> Vec<ModelOutput> {
    vec![
        fresh_output(&momentum_model(), XYZ, "0.8", "0.9"),
        fresh_output(&news_model(), XYZ, "0.2", "0.5"),
    ]
}

/// An action context an `MC-A` case classifies: a routine 300 USD open with a 0.8 score, which the
/// base rules make AUTO, and which each test changes one field of.
pub fn routine_open() -> ActionContext {
    ActionContext {
        purpose: Purpose::Open,
        order_usd: usd("300"),
        combined_score: unit("0.8"),
        instrument: instrument(BTC),
        asset_class: AssetClass::Crypto,
        session: Session::Crypto,
        first_trade_in_instrument: false,
        new_instrument: false,
        thesis_confidence: Unit::ZERO,
        drawdown: Unit::ZERO,
        daily_pnl_fraction: Signed::ZERO,
        position_pnl_fraction: Signed::ZERO,
        position_usd_after: Usd::ZERO,
        gross_usd_after: Usd::ZERO,
        bought_today_usd: Usd::ZERO,
    }
}

/// A one-field condition, the shape most rules use.
pub fn compare(field: Field, op: Op, value: Value) -> Condition {
    Condition::Compare { field, op, value }
}

pub fn rule(id: &str, when: Condition, then: Decision) -> Rule {
    Rule {
        id: RuleId::new(id),
        when,
        then,
    }
}

/// The shape a plain limit exit takes, for a test that asserts only the shape.
pub const PLAIN: OrderShape = OrderShape::Limit;
