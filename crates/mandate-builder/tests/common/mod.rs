//! Inputs the hand tests and the property tests build on: the two mandate bases the reference cases
//! use, typed, plus the narrow constructors that turn canonical decimal text into the crate's types.
//!
//! Every constructor panics on bad text rather than returning a `Result`, and the message says what
//! it was reading. A test that cannot build its own input has a bug in the test, and a pending test
//! must fail on the crate's `Unimplemented`, never on a fixture or a harness panic (DEC-110), so
//! the panics here are the loud kind that would show up immediately.

#![allow(dead_code, reason = "each test file uses its own part of this module")]

use std::collections::BTreeSet;

use mandate_builder::{
    AccountSnapshot, AccumulateGoal, BuilderMandate, Direction, GoalKind, Limits, Market,
    ModelOutput, ModelVersion, RiskContext, SignalModel, Sizing,
};
use mandate_canon::Digest;
use mandate_domain::{AssetClass, AssetId, AutonomyDecision, MarketSession};
use mandate_num::{
    Conviction, CostBasis, FeeRate, MarkPrice, Price, Qty, Signed, SizeFraction, Unit, Usd,
};
use mandate_spec::condition::{Condition, ConditionField, ConditionValue, Operator};
use mandate_spec::document::{
    Approval, ApproverRef, Autonomy, ModelId, OnTimeout, Rule, RuleId, SizingMethod,
};
use mandate_spec::{DecGrammar, SchemaDec};
use mandate_time::{Date, UtcNanos};

pub fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap_or_else(|e| panic!("`{text}` is a USD amount: {e}"))
}

pub fn price(text: &str) -> Price {
    Price::parse(text).unwrap_or_else(|e| panic!("`{text}` is a price: {e}"))
}

pub fn qty(text: &str) -> Qty {
    Qty::parse(text).unwrap_or_else(|e| panic!("`{text}` is a quantity: {e}"))
}

pub fn mark(text: &str) -> MarkPrice {
    MarkPrice::parse(text).unwrap_or_else(|e| panic!("`{text}` is a mark: {e}"))
}

pub fn frac(text: &str) -> SizeFraction {
    SizeFraction::parse(text).unwrap_or_else(|e| panic!("`{text}` is a size fraction: {e}"))
}

pub fn unit(text: &str) -> Unit {
    Unit::parse(text).unwrap_or_else(|e| panic!("`{text}` is a unit value: {e}"))
}

pub fn conv(text: &str) -> Conviction {
    Conviction::parse(text).unwrap_or_else(|e| panic!("`{text}` is a conviction: {e}"))
}

pub fn signed(text: &str) -> Signed {
    Signed::parse(text).unwrap_or_else(|e| panic!("`{text}` is a signed value: {e}"))
}

pub fn fee(text: &str) -> FeeRate {
    FeeRate::parse(text).unwrap_or_else(|e| panic!("`{text}` is a fee rate: {e}"))
}

pub fn basis(text: &str) -> CostBasis {
    CostBasis::parse(text).unwrap_or_else(|e| panic!("`{text}` is a cost basis: {e}"))
}

pub fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap_or_else(|e| panic!("`{text}` is a timestamp: {e}"))
}

/// A calendar date, `YYYY-MM-DD`.
pub fn day(text: &str) -> Date {
    Date::parse(text).unwrap_or_else(|e| panic!("`{text}` is a date: {e}"))
}

/// The risk day the reference cases' builder and autonomy inputs are evaluated on (MC-B's `now`
/// is 2026-09-22, and family A states no time). Nothing reads it unless a policy has a review
/// date (§6.2 step 5b).
pub const REFERENCE_DAY: &str = "2026-09-22";

pub fn asset(text: &str) -> AssetId {
    AssetId::parse(text).unwrap_or_else(|e| panic!("`{text}` is an asset id: {e}"))
}

pub fn model(text: &str) -> ModelId {
    ModelId::parse(text).unwrap_or_else(|e| panic!("`{text}` is a model id: {e}"))
}

pub fn version(text: &str) -> ModelVersion {
    ModelVersion::parse(text).unwrap_or_else(|e| panic!("`{text}` is a model version: {e:?}"))
}

/// The fixture writes a content hash as `sha256:` and 64 hexadecimal characters.
pub fn digest(text: &str) -> Digest {
    let hex = text.strip_prefix("sha256:").unwrap_or(text);
    Digest::from_hex(hex).unwrap_or_else(|| panic!("`{text}` is a sha256 content reference"))
}

pub fn rule_id(text: &str) -> RuleId {
    RuleId::parse(text).unwrap_or_else(|e| panic!("`{text}` is a rule id: {e}"))
}

pub fn dec(text: &str, grammar: DecGrammar) -> SchemaDec {
    SchemaDec::parse(text, grammar)
        .unwrap_or_else(|e| panic!("`{text}` is in the {} grammar: {e}", grammar.as_str()))
}

pub const SWING_INSTRUMENT: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
pub const BTC_INSTRUMENT: &str = "7b4a1c2e-1111-4a2b-9c3d-000000000001";
pub const OTHER_INSTRUMENT: &str = "7b4a1c2e-3333-4a2b-9c3d-000000000003";
pub const MOMENTUM_HASH: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";
pub const NEWS_HASH: &str =
    "sha256:3333333333333333333333333333333333333333333333333333333333333333";
pub const MEAN_REVERSION_HASH: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
/// The instant every builder reference case evaluates at.
pub const NOW: &str = "2026-09-22T14:00:00.000000000Z";

/// `quant.momentum`, weight 0.6, fresh for 900 seconds — the `two_stock_swing` base's second model.
pub fn momentum() -> SignalModel {
    SignalModel {
        id: model("quant.momentum"),
        version: version("1.0.0"),
        content_hash: digest(MOMENTUM_HASH),
        weight: frac("0.6"),
        max_output_age_s: 900,
    }
}

/// `llm.news_research`, weight 0.4, fresh for 3600 seconds.
pub fn news() -> SignalModel {
    SignalModel {
        id: model("llm.news_research"),
        version: version("0.3.0"),
        content_hash: digest(NEWS_HASH),
        weight: frac("0.4"),
        max_output_age_s: 3600,
    }
}

/// `quant.mean_reversion`, weight 1 — the `btc_accumulator` base's only model.
pub fn mean_reversion() -> SignalModel {
    SignalModel {
        id: model("quant.mean_reversion"),
        version: version("1.0.0"),
        content_hash: digest(MEAN_REVERSION_HASH),
        weight: frac("1"),
        max_output_age_s: 1800,
    }
}

/// The `two_stock_swing` base's §8 view: two models summing to one, thresholds 0.3 and 0.3, band
/// 0.05, and the four limits `MC-B01` to `MC-B25` are recomputed against.
pub fn two_stock_swing() -> BuilderMandate {
    BuilderMandate {
        models: vec![news(), momentum()],
        sizing: Sizing {
            method: SizingMethod::ConvictionLinear,
            entry_threshold: frac("0.3"),
            exit_threshold: frac("0.3"),
            rebalance_band: frac("0.05"),
        },
        limits: Limits {
            max_position_usd: usd("1500"),
            max_position_fraction: frac("0.2"),
            max_order_usd: usd("1000"),
            max_gross_exposure_usd: usd("2000"),
        },
        goal: GoalKind::ProfitStop,
    }
}

/// The `btc_accumulator` base's §8 view: one model at weight 1 and the accumulate goal of `MC-B26`
/// to `MC-B29`.
pub fn btc_accumulator() -> BuilderMandate {
    BuilderMandate {
        models: vec![mean_reversion()],
        sizing: Sizing {
            method: SizingMethod::ConvictionLinear,
            entry_threshold: frac("0.3"),
            exit_threshold: frac("0.3"),
            rebalance_band: frac("0.05"),
        },
        limits: Limits {
            max_position_usd: usd("10000"),
            max_position_fraction: frac("1"),
            max_order_usd: usd("1000"),
            max_gross_exposure_usd: usd("10000"),
        },
        goal: GoalKind::Accumulate(AccumulateGoal {
            instrument: asset(BTC_INSTRUMENT),
            target_qty: qty("0.15"),
            max_avg_price: Some(price("58000")),
            max_spend_usd: usd("9000"),
        }),
    }
}

/// The equity market every `two_stock_swing` case quotes: bid 99.9, ask 100, whole shares, a
/// one-dollar minimum, no fees, the regular session, outside the close window.
pub fn swing_market() -> Market {
    Market {
        instrument: asset(SWING_INSTRUMENT),
        asset_class: AssetClass::UsEquity,
        session: MarketSession::Regular,
        in_close_window: false,
        bid: price("99.9"),
        ask: price("100"),
        increment: qty("1"),
        min_order_usd: usd("1"),
        fee_rate_cash: fee("0"),
        fee_rate_asset: fee("0"),
    }
}

/// The crypto market the accumulate cases quote, at the bid and ask the caller passes.
pub fn crypto_market(bid: &str, ask: &str, increment: &str) -> Market {
    Market {
        instrument: asset(BTC_INSTRUMENT),
        asset_class: AssetClass::Crypto,
        session: MarketSession::Crypto,
        in_close_window: false,
        bid: price(bid),
        ask: price(ask),
        increment: qty(increment),
        min_order_usd: usd("1"),
        fee_rate_cash: fee("0"),
        fee_rate_asset: fee("0"),
    }
}

/// An account with 10000 of equity, the stated position at the stated mark, and nothing else: no
/// working orders, no gross exposure beyond the position, no goal spend.
pub fn account(position: &str, risk_mark: &str) -> AccountSnapshot {
    AccountSnapshot {
        agent_equity: usd("10000"),
        position_qty: qty(position),
        cost_basis: basis("0"),
        risk_mark: mark(risk_mark),
        gross_usd: usd("0"),
        working_opening_cost: usd("0"),
        goal_spent_usd: usd("0"),
    }
}

/// A flat agent at the swing bid.
pub fn flat_account() -> AccountSnapshot {
    account("0", "99.9")
}

/// No drawdown, no ladder, no thesis: the risk context every unscaled case runs under.
pub fn quiet_risk() -> RiskContext {
    RiskContext {
        size_factor: SizeFraction::ONE,
        drawdown: Unit::ZERO,
        daily_pnl_fraction: Signed::ZERO,
        position_pnl_fraction: Signed::ZERO,
        bought_today_usd: usd("0"),
        has_prior_fill: false,
        new_instrument: false,
        thesis_confidence: Unit::ZERO,
        risk_day: day(REFERENCE_DAY),
    }
}

/// An output of `m` on `instrument`, valid from 13:59 to 15:00 on the reference cases' day.
pub fn output(
    m: &SignalModel,
    instrument: &str,
    conviction: &str,
    confidence: &str,
) -> ModelOutput {
    timed_output(
        m,
        instrument,
        conviction,
        confidence,
        "2026-09-22T13:59:00.000000000Z",
        "2026-09-22T15:00:00.000000000Z",
    )
}

pub fn timed_output(
    m: &SignalModel,
    instrument: &str,
    conviction: &str,
    confidence: &str,
    as_of: &str,
    expires_at: &str,
) -> ModelOutput {
    ModelOutput {
        model_id: m.id.clone(),
        model_version: m.version.clone(),
        content_hash: m.content_hash,
        instrument: asset(instrument),
        as_of: at(as_of),
        expires_at: at(expires_at),
        direction: Direction::Long,
        conviction: conv(conviction),
        confidence: unit(confidence),
    }
}

/// The three rules every reference-case base carries, in the order the first match walks them:
/// `large_orders` above 900 asks, `low_score` below 0.65 asks, `routine` on an opening or
/// increasing purpose is automatic.
pub fn base_rules() -> Vec<Rule> {
    vec![
        Rule {
            id: rule_id("large_orders"),
            when: compare(ConditionField::OrderUsd, Operator::Gt, decimal("900")),
            then: AutonomyDecision::Ask,
        },
        Rule {
            id: rule_id("low_score"),
            when: compare(ConditionField::CombinedScore, Operator::Lt, decimal("0.65")),
            then: AutonomyDecision::Ask,
        },
        Rule {
            id: rule_id("routine"),
            when: Condition::Compare {
                field: ConditionField::Purpose,
                op: Operator::In,
                value: ConditionValue::List(vec!["increase".to_owned(), "open".to_owned()]),
            },
            then: AutonomyDecision::Auto,
        },
    ]
}

/// The `two_stock_swing` and `btc_accumulator` autonomy block: the three rules, `ask` by default,
/// `ask` for an admission, one approver at any size.
pub fn base_policy() -> Autonomy {
    policy(
        base_rules(),
        AutonomyDecision::Ask,
        AutonomyDecision::Ask,
        None,
    )
}

pub fn policy(
    rules: Vec<Rule>,
    default: AutonomyDecision,
    admission: AutonomyDecision,
    two_approver_above_usd: Option<&str>,
) -> Autonomy {
    Autonomy {
        rules,
        default,
        admission,
        approval: Approval {
            timeout_s: 600,
            on_timeout: OnTimeout::Skip,
            approvers: vec![approver("role:approver")],
            two_approver_above_usd: two_approver_above_usd
                .map(|t| dec(t, DecGrammar::PositiveDecimal)),
        },
        review_by: None,
        delegations: Vec::new(),
        tripwires: Vec::new(),
    }
}

pub fn approver(text: &str) -> ApproverRef {
    ApproverRef::parse(text).unwrap_or_else(|e| panic!("`{text}` is an approver reference: {e}"))
}

pub fn compare(field: ConditionField, op: Operator, value: ConditionValue) -> Condition {
    Condition::Compare { field, op, value }
}

pub fn decimal(text: &str) -> ConditionValue {
    ConditionValue::Decimal(dec(text, DecGrammar::Decimal))
}

pub fn text(value: &str) -> ConditionValue {
    ConditionValue::Text(value.to_owned())
}

pub fn flag(value: bool) -> ConditionValue {
    ConditionValue::Bool(value)
}

pub fn ids(names: &[&str]) -> BTreeSet<ModelId> {
    names.iter().map(|n| model(n)).collect()
}
