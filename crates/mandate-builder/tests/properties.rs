//! Property tests for every invariant and every "never" or "always" of
//! [mandate spec §6](../../../docs/specs/mandate.md#6-autonomy-dec-42-dec-48-dec-58) and
//! [§8.3](../../../docs/specs/mandate.md#83-order-builder-conviction_linear-dec-47-dec-60), against
//! **three oracles that share no code with the crate**.
//!
//! 1. **Combine** is recomputed as `i128` integer arithmetic at fixed scales with its own half-even
//!    rounding: the weights, convictions and confidences are scaled to integers, the numerator and
//!    the denominator are summed exactly, and one division rounds at the end. It never calls
//!    `mandate_num`.
//! 2. **Sizing** is recomputed as a rational ([`Rat`]): a numerator and a denominator carried as
//!    integers, compared by cross-multiplication, so no quotient is ever taken and no intermediate
//!    is ever rounded. The one truncation, to the share increment, is a floor of a rational.
//! 3. **Autonomy** is a naive recursive walk that re-reads the rule list from the start for every
//!    action and evaluates a condition by structural recursion over its own copy of the field
//!    table.
//!
//! **Non-vacuity.** Every property that calls `propose` goes through [`agree`], which first asserts
//! that the oracle and the crate return the same **action kind** and the same hold reason, so no
//! property can pass on a hold the crate returned for the wrong reason. The three bands of §8.3
//! step 2 are checked by a generator that sweeps the conviction line rather than by asking the
//! crate which band it chose, and each generated scenario is closed with a figure the property
//! derives from the inputs rather than from the crate's answer.
//!
//! **The generator's range is narrower than the crate's.** Weights, convictions and confidences are
//! generated at three fractional places and prices and quantities at two, because that is what an
//! `i128` oracle carries exactly; the 12-place and 18-place bounds the crate promises are tested by
//! [`no_input_within_the_stated_bounds_overflows`], which needs no oracle because the answer it
//! asserts is "not an overflow".
//!
//! Every property is pending until the implementation PR and fails on `BuilderError::Unimplemented`
//! (DEC-77, DEC-83, DEC-110).

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU32, Ordering};

use common::{
    BTC_INSTRUMENT, NOW, SWING_INSTRUMENT, asset, basis, compare, decimal, flag, frac, mark,
    momentum, news, policy, price, qty, rule_id, text, timed_output, unit, usd,
};
use mandate_builder::{
    AccountSnapshot, AccumulateGoal, Action, ActionContext, BuilderMandate, GateVerdict, GoalKind,
    HoldReason, Limits, Market, ModelOutput, Outcome, Proposal, RiskContext, SignalModel, Sizing,
    classify, decide, propose,
};
use mandate_domain::{AssetClass, AutonomyDecision, MarketSession, Purpose};
use mandate_num::{Conviction, CostBasis, FeeRate, Signed, Unit};
use mandate_spec::condition::{Condition, ConditionField, ConditionValue, Operator};
use mandate_spec::document::{Autonomy, Rule, SizingMethod};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::RngSeed;

/// What the generated scenarios reached, counted by the **oracle** rather than by the crate, so the
/// evidence is about the generator and cannot be manufactured by the code under test.
///
/// A fuzz that never proposed a buy is not evidence for a property about buys, which is what the
/// independent review of this PR found: every sizing property ran 256 cases of which none reached a
/// buy, so four planted bugs in the after-values survived all 125 tests.
/// [`zz_the_generated_scenarios_reach_every_action_and_every_clip`] is the gate that keeps that
/// from coming back.
static SAW_BUY: AtomicU32 = AtomicU32::new(0);
static SAW_SELL: AtomicU32 = AtomicU32::new(0);
static SAW_NO_FRESH_OUTPUTS: AtomicU32 = AtomicU32::new(0);
static SAW_BETWEEN_THRESHOLDS: AtomicU32 = AtomicU32::new(0);
static SAW_NO_POSITION: AtomicU32 = AtomicU32::new(0);
static SAW_EXITS_DISABLED: AtomicU32 = AtomicU32::new(0);
static SAW_AT_OR_ABOVE_TARGET: AtomicU32 = AtomicU32::new(0);
static SAW_WITHIN_BAND: AtomicU32 = AtomicU32::new(0);
static SAW_BELOW_BAND: AtomicU32 = AtomicU32::new(0);
static SAW_BELOW_MINIMUM: AtomicU32 = AtomicU32::new(0);
static SAW_MAX_AVG_PRICE: AtomicU32 = AtomicU32::new(0);
static SAW_CLIP_LIMITS: AtomicU32 = AtomicU32::new(0);
static SAW_CLIP_GOAL: AtomicU32 = AtomicU32::new(0);
static EVERY_COUNTER: [&AtomicU32; 13] = [
    &SAW_BUY,
    &SAW_SELL,
    &SAW_NO_FRESH_OUTPUTS,
    &SAW_BETWEEN_THRESHOLDS,
    &SAW_NO_POSITION,
    &SAW_EXITS_DISABLED,
    &SAW_AT_OR_ABOVE_TARGET,
    &SAW_WITHIN_BAND,
    &SAW_BELOW_BAND,
    &SAW_BELOW_MINIMUM,
    &SAW_MAX_AVG_PRICE,
    &SAW_CLIP_LIMITS,
    &SAW_CLIP_GOAL,
];

fn record(proposal: &OracleProposal) {
    let counter = match &proposal.action {
        OracleAction::Buy { .. } => &SAW_BUY,
        OracleAction::Sell { .. } => &SAW_SELL,
        OracleAction::Hold(reason) => match *reason {
            "no_fresh_outputs" => &SAW_NO_FRESH_OUTPUTS,
            "between_thresholds" => &SAW_BETWEEN_THRESHOLDS,
            "no_position" => &SAW_NO_POSITION,
            "discretionary_exits_disabled" => &SAW_EXITS_DISABLED,
            "at_or_above_target" => &SAW_AT_OR_ABOVE_TARGET,
            "within_rebalance_band" => &SAW_WITHIN_BAND,
            "below_band_after_clipping" => &SAW_BELOW_BAND,
            "below_minimum_after_clipping" => &SAW_BELOW_MINIMUM,
            "would_exceed_max_avg_price" => &SAW_MAX_AVG_PRICE,
            other => panic!("the oracle returned a hold reason the counters do not know: {other}"),
        },
    };
    counter.fetch_add(1, Ordering::Relaxed);
    if proposal.clips.contains("limits") {
        SAW_CLIP_LIMITS.fetch_add(1, Ordering::Relaxed);
    }
    if proposal.clips.contains("goal") {
        SAW_CLIP_GOAL.fetch_add(1, Ordering::Relaxed);
    }
}

/// An exact rational on `i128`, reduced after every operation, compared by cross-multiplication.
///
/// Nothing here rounds, and nothing here calls `mandate_num`. Every operation is checked: an
/// overflow is a bug in this oracle's own range and panics saying so, rather than wrapping and
/// quietly agreeing with whatever the crate returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rat {
    numerator: i128,
    denominator: i128,
}

fn gcd(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    if a == 0 { 1 } else { a }
}

fn checked(value: Option<i128>, what: &str) -> i128 {
    value.unwrap_or_else(|| panic!("the oracle's own {what} left i128; tighten the generator"))
}

impl Rat {
    fn new(numerator: i128, denominator: i128) -> Self {
        assert!(denominator != 0, "the oracle never divides by zero");
        let sign = if denominator < 0 { -1 } else { 1 };
        let divisor = gcd(numerator, denominator);
        Self {
            numerator: checked(numerator.checked_div(divisor), "reduction").saturating_mul(sign),
            denominator: checked(denominator.checked_div(divisor), "reduction").abs(),
        }
    }

    fn int(value: i128) -> Self {
        Self {
            numerator: value,
            denominator: 1,
        }
    }

    const ZERO: Self = Self {
        numerator: 0,
        denominator: 1,
    };

    fn add(self, other: Self) -> Self {
        let left = checked(self.numerator.checked_mul(other.denominator), "addition");
        let right = checked(other.numerator.checked_mul(self.denominator), "addition");
        Self::new(
            checked(left.checked_add(right), "addition"),
            checked(self.denominator.checked_mul(other.denominator), "addition"),
        )
    }

    fn sub(self, other: Self) -> Self {
        self.add(Self {
            numerator: checked(other.numerator.checked_neg(), "negation"),
            denominator: other.denominator,
        })
    }

    fn mul(self, other: Self) -> Self {
        let reduce_a = gcd(self.numerator, other.denominator);
        let reduce_b = gcd(other.numerator, self.denominator);
        Self::new(
            checked(
                (self.numerator / reduce_a).checked_mul(other.numerator / reduce_b),
                "multiplication",
            ),
            checked(
                (self.denominator / reduce_b).checked_mul(other.denominator / reduce_a),
                "multiplication",
            ),
        )
    }

    /// `self < other`, by cross-multiplication: no quotient is taken.
    fn below(self, other: Self) -> bool {
        let left = checked(self.numerator.checked_mul(other.denominator), "comparison");
        let right = checked(other.numerator.checked_mul(self.denominator), "comparison");
        left < right
    }

    fn min(self, other: Self) -> Self {
        if other.below(self) { other } else { self }
    }

    fn is_positive(self) -> bool {
        Self::ZERO.below(self)
    }

    /// `truncate(self ÷ divisor)` toward zero as an integer count, which is what §8.3's "truncate
    /// to the increment" means and what `reference/mandate/ref.py` does with `ROUND_DOWN`. Toward
    /// zero and not toward minus infinity: the two differ on the negative quantity the
    /// `max_avg_price` clip can produce.
    fn trunc_div(self, divisor: Self) -> i128 {
        let numerator = checked(self.numerator.checked_mul(divisor.denominator), "division");
        let denominator = checked(divisor.numerator.checked_mul(self.denominator), "division");
        assert!(denominator != 0, "the oracle never divides by zero");
        let (numerator, denominator) = if denominator < 0 {
            (-numerator, -denominator)
        } else {
            (numerator, denominator)
        };
        numerator / denominator
    }

    /// The canonical decimal text of the value, at the fewest places that hold it exactly.
    ///
    /// The scale comes from the reduced denominator's factors of two and five, so a value that is
    /// not a finite decimal is a bug in the oracle's own arithmetic and says so rather than
    /// rounding quietly.
    fn to_text(self) -> String {
        let places = decimal_places(self.denominator);
        let factor = checked(
            10i128
                .checked_pow(places)
                .and_then(|p| p.checked_div(self.denominator)),
            "scaling",
        );
        let scaled = checked(self.numerator.checked_mul(factor), "scaling");
        let sign = if scaled < 0 { "-" } else { "" };
        let magnitude = scaled.unsigned_abs();
        let divisor = 10u128.pow(places);
        let whole = magnitude / divisor;
        let fraction = format!("{:0width$}", magnitude % divisor, width = places as usize);
        let fraction = fraction.trim_end_matches('0');
        if fraction.is_empty() {
            format!("{sign}{whole}")
        } else {
            format!("{sign}{whole}.{fraction}")
        }
    }
}

/// How many fractional places a reduced denominator needs, from its factors of two and five.
fn decimal_places(denominator: i128) -> u32 {
    let mut rest = denominator;
    let mut twos = 0;
    while rest % 2 == 0 {
        rest /= 2;
        twos += 1;
    }
    let mut fives = 0;
    while rest % 5 == 0 {
        rest /= 5;
        fives += 1;
    }
    assert_eq!(rest, 1, "the oracle prints only finite decimals");
    twos.max(fives)
}

/// Half-even rounding of `numerator ÷ denominator` to an integer, written out rather than borrowed.
fn round_half_even(numerator: i128, denominator: i128) -> i128 {
    assert!(denominator > 0, "the oracle rounds a positive denominator");
    let quotient = numerator.div_euclid(denominator);
    let remainder = numerator.rem_euclid(denominator);
    let twice = checked(remainder.checked_mul(2), "rounding");
    if twice > denominator || (twice == denominator && quotient % 2 != 0) {
        checked(quotient.checked_add(1), "rounding")
    } else {
        quotient
    }
}

const MODEL_NAMES: [&str; 3] = [
    "quant.momentum",
    "llm.news_research",
    "quant.mean_reversion",
];
/// Every generated instant is an offset in seconds from this one, so the oracle counts seconds and
/// never parses a timestamp.
const EPOCH_SECS: i64 = 1_789_048_800;

#[derive(Debug, Clone)]
struct ModelSpec {
    /// The fixed weight, in thousandths.
    weight_milli: i128,
    max_age_s: u32,
}

#[derive(Debug, Clone)]
struct OutputSpec {
    model: usize,
    conviction_milli: i128,
    confidence_milli: i128,
    as_of_s: i64,
    expires_s: i64,
    /// False makes the output carry another model's content hash, which §8.1 ignores.
    pinned: bool,
}

#[derive(Debug, Clone)]
enum GoalSpec {
    Continuous,
    /// Target quantity and spend in the scenario's own units, and the average bound in dollars.
    Accumulate {
        target_units: i128,
        max_avg_dollars: Option<i128>,
        max_spend_dollars: i128,
    },
}

/// Quantities are generated at two fractional places, so a unit is a hundredth.
const QTY_UNITS: i128 = 100;
/// Prices are generated at one fractional place.
const PRICE_UNITS: i128 = 10;

#[derive(Debug, Clone)]
struct Scenario {
    models: Vec<ModelSpec>,
    outputs: Vec<OutputSpec>,
    entry_milli: i128,
    exit_milli: i128,
    band_milli: i128,
    size_factor_milli: i128,
    max_position_dollars: i128,
    position_fraction_milli: i128,
    max_order_dollars: i128,
    max_gross_dollars: i128,
    equity_dollars: i128,
    position_units: i128,
    mark_tenths: i128,
    bid_tenths: i128,
    ask_tenths: i128,
    increment_units: i128,
    min_order_dollars: i128,
    gross_dollars: i128,
    working_dollars: i128,
    basis_dollars: i128,
    goal_spent_dollars: i128,
    goal: GoalSpec,
    crypto: bool,
    in_close_window: bool,
    now_s: i64,
}

impl Scenario {
    fn money(dollars: i128) -> Rat {
        Rat::int(dollars)
    }

    fn quantity(units: i128) -> Rat {
        Rat::new(units, QTY_UNITS)
    }

    fn price(tenths: i128) -> Rat {
        Rat::new(tenths, PRICE_UNITS)
    }

    fn milli(value: i128) -> Rat {
        Rat::new(value, 1000)
    }

    fn instrument(&self) -> &'static str {
        if self.crypto {
            BTC_INSTRUMENT
        } else {
            SWING_INSTRUMENT
        }
    }

    fn signal_models(&self) -> Vec<SignalModel> {
        self.models
            .iter()
            .enumerate()
            .map(|(index, spec)| SignalModel {
                id: common::model(MODEL_NAMES[index]),
                version: common::version("1.0.0"),
                content_hash: common::digest(hash_of(index)),
                weight: frac(&Scenario::milli(spec.weight_milli).to_text()),
                max_output_age_s: spec.max_age_s,
            })
            .collect()
    }

    fn model_outputs(&self) -> Vec<ModelOutput> {
        self.outputs
            .iter()
            .map(|spec| {
                let model = &self.signal_models()[spec.model];
                let hash = if spec.pinned {
                    hash_of(spec.model)
                } else {
                    hash_of((spec.model + 1) % MODEL_NAMES.len())
                };
                ModelOutput {
                    content_hash: common::digest(hash),
                    ..timed_output(
                        model,
                        self.instrument(),
                        &Scenario::milli(spec.conviction_milli).to_text(),
                        &Scenario::milli(spec.confidence_milli).to_text(),
                        &stamp(spec.as_of_s),
                        &stamp(spec.expires_s),
                    )
                }
            })
            .collect()
    }

    fn mandate(&self) -> BuilderMandate {
        BuilderMandate {
            models: self.signal_models(),
            sizing: Sizing {
                method: SizingMethod::ConvictionLinear,
                entry_threshold: frac(&Scenario::milli(self.entry_milli).to_text()),
                exit_threshold: frac(&Scenario::milli(self.exit_milli).to_text()),
                rebalance_band: frac(&Scenario::milli(self.band_milli).to_text()),
            },
            limits: Limits {
                max_position_usd: usd(&self.max_position_dollars.to_string()),
                max_position_fraction: frac(
                    &Scenario::milli(self.position_fraction_milli).to_text(),
                ),
                max_order_usd: usd(&self.max_order_dollars.to_string()),
                max_gross_exposure_usd: usd(&self.max_gross_dollars.to_string()),
            },
            goal: match &self.goal {
                GoalSpec::Continuous => GoalKind::Continuous,
                GoalSpec::Accumulate {
                    target_units,
                    max_avg_dollars,
                    max_spend_dollars,
                } => GoalKind::Accumulate(AccumulateGoal {
                    instrument: asset(self.instrument()),
                    target_qty: qty(&Scenario::quantity(*target_units).to_text()),
                    max_avg_price: max_avg_dollars.map(|d| price(&d.to_string())),
                    max_spend_usd: usd(&max_spend_dollars.to_string()),
                }),
            },
        }
    }

    fn account(&self) -> AccountSnapshot {
        AccountSnapshot {
            agent_equity: usd(&self.equity_dollars.to_string()),
            position_qty: qty(&Scenario::quantity(self.position_units).to_text()),
            cost_basis: basis(&self.basis_dollars.to_string()),
            risk_mark: mark(&Scenario::price(self.mark_tenths).to_text()),
            gross_usd: usd(&self.gross_dollars.to_string()),
            working_opening_cost: usd(&self.working_dollars.to_string()),
            goal_spent_usd: usd(&self.goal_spent_dollars.to_string()),
        }
    }

    fn market(&self) -> Market {
        Market {
            instrument: asset(self.instrument()),
            asset_class: if self.crypto {
                AssetClass::Crypto
            } else {
                AssetClass::UsEquity
            },
            session: if self.crypto {
                MarketSession::Crypto
            } else {
                MarketSession::Regular
            },
            in_close_window: self.in_close_window,
            bid: price(&Scenario::price(self.bid_tenths).to_text()),
            ask: price(&Scenario::price(self.ask_tenths).to_text()),
            increment: qty(&Scenario::quantity(self.increment_units).to_text()),
            min_order_usd: usd(&self.min_order_dollars.to_string()),
            fee_rate_cash: FeeRate::parse("0").unwrap_or_else(|e| panic!("a zero fee rate: {e}")),
            fee_rate_asset: FeeRate::parse("0").unwrap_or_else(|e| panic!("a zero fee rate: {e}")),
        }
    }

    fn risk(&self) -> RiskContext {
        RiskContext {
            size_factor: frac(&Scenario::milli(self.size_factor_milli).to_text()),
            drawdown: Unit::ZERO,
            daily_pnl_fraction: Signed::ZERO,
            position_pnl_fraction: Signed::ZERO,
            bought_today_usd: usd("0"),
            has_prior_fill: self.position_units > 0,
            new_instrument: false,
            thesis_confidence: Unit::ZERO,
        }
    }

    fn now(&self) -> UtcNanos {
        common::at(&stamp(self.now_s))
    }

    fn propose(&self) -> Result<Proposal, mandate_builder::BuilderError> {
        propose(
            &self.mandate(),
            &self.account(),
            &self.market(),
            &self.risk(),
            &self.model_outputs(),
            self.now(),
        )
    }
}

fn hash_of(index: usize) -> &'static str {
    match index {
        0 => common::MOMENTUM_HASH,
        1 => common::NEWS_HASH,
        _ => common::MEAN_REVERSION_HASH,
    }
}

/// An RFC 3339 instant from a second offset, written out rather than taken from `mandate_time`.
fn stamp(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.000000000Z",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

/// Howard Hinnant's `civil_from_days`, so the oracle's calendar is its own.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OracleCombined {
    used: BTreeSet<String>,
    /// The three figures at 12 fractional places, each one half-even rounding of one exact quotient.
    exit: i128,
    buy: i128,
    score: i128,
}

/// §8.3 step 1 as integer arithmetic: the weights at a thousandth, the convictions and confidences
/// at a thousandth, the numerator and the denominator summed exactly, and one half-even division at
/// the end. It shares nothing with the crate but the input.
fn oracle_combine(scenario: &Scenario) -> OracleCombined {
    let mut latest: BTreeMap<usize, (usize, &OutputSpec)> = BTreeMap::new();
    for (index, spec) in scenario.outputs.iter().enumerate() {
        if !spec.pinned {
            continue;
        }
        let model = &scenario.models[spec.model];
        let fresh_window = spec.as_of_s <= scenario.now_s && scenario.now_s < spec.expires_s;
        let age = scenario.now_s - spec.as_of_s;
        if !fresh_window || age > i64::from(model.max_age_s) {
            continue;
        }
        let takes = match latest.get(&spec.model) {
            None => true,
            Some((held_index, held)) => (spec.as_of_s, index) > (held.as_of_s, *held_index),
        };
        if takes {
            latest.insert(spec.model, (index, spec));
        }
    }
    let weight_sum: i128 = scenario.models.iter().map(|m| m.weight_milli).sum();
    let mut numerator = 0i128;
    let mut score_numerator = 0i128;
    for (model, (_, spec)) in &latest {
        let weight = scenario.models[*model].weight_milli;
        numerator += weight * spec.conviction_milli * spec.confidence_milli;
        score_numerator += weight * spec.confidence_milli;
    }
    let missing: i128 = scenario
        .models
        .iter()
        .enumerate()
        .filter(|(index, _)| !latest.contains_key(index))
        .map(|(_, m)| m.weight_milli)
        .sum();
    OracleCombined {
        used: latest
            .keys()
            .map(|index| MODEL_NAMES[*index].to_owned())
            .collect(),
        exit: round_half_even(numerator * 1_000_000, weight_sum),
        buy: round_half_even((numerator - missing * 1_000_000) * 1_000_000, weight_sum),
        score: round_half_even(score_numerator * 1_000_000_000, weight_sum),
    }
}

fn scaled_text(value: i128, places: u32) -> String {
    Rat::new(value, 10i128.pow(places)).to_text()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OracleAction {
    Hold(&'static str),
    Sell { units: i128 },
    Buy { units: i128 },
}

impl OracleAction {
    fn kind(&self) -> &'static str {
        match self {
            Self::Hold(reason) => reason,
            Self::Sell { .. } => "sell",
            Self::Buy { .. } => "buy",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OracleProposal {
    combined: OracleCombined,
    cap: Rat,
    current_mv: Rat,
    target: Option<Rat>,
    delta: Option<Rat>,
    action: OracleAction,
    clips: BTreeSet<&'static str>,
}

/// §8.3 steps 2 to 5 as rationals: every bound is a numerator over a denominator, every comparison
/// is a cross-multiplication, and the only division is the truncation to the share increment.
///
/// The share count is clamped at zero, as `UsdExact::shares_at` clamps it: the gross headroom can be
/// negative, and a negative count would make the oracle answer `below_band_after_clipping` where the
/// crate answers `below_minimum_after_clipping` at a zero band. The clamp is the crate's documented
/// behaviour, not a convenience — an oracle that disagreed with it here would have failed
/// `identical_inputs_give_identical_proposals` on correct code.
fn oracle_propose(scenario: &Scenario) -> OracleProposal {
    let answer = oracle_sized(scenario);
    record(&answer);
    answer
}

fn oracle_sized(scenario: &Scenario) -> OracleProposal {
    let combined = oracle_combine(scenario);
    let cap = Scenario::money(scenario.max_position_dollars).min(
        Scenario::milli(scenario.position_fraction_milli)
            .mul(Scenario::money(scenario.equity_dollars)),
    );
    let current_mv =
        Scenario::quantity(scenario.position_units).mul(Scenario::price(scenario.mark_tenths));
    let working = Scenario::money(scenario.working_dollars);
    let mut clips: BTreeSet<&'static str> = BTreeSet::new();
    let held = |reason, clips: &BTreeSet<&'static str>| OracleProposal {
        combined: combined.clone(),
        cap,
        current_mv,
        target: None,
        delta: None,
        action: OracleAction::Hold(reason),
        clips: clips.clone(),
    };
    if combined.used.is_empty() {
        return held("no_fresh_outputs", &clips);
    }
    let exit_conviction = Rat::new(combined.exit, 1_000_000_000_000);
    let buy_conviction = Rat::new(combined.buy, 1_000_000_000_000);
    let exit_threshold = Scenario::milli(scenario.exit_milli);
    let below_exit = !Rat::ZERO.sub(exit_threshold).below(exit_conviction);
    if below_exit {
        if matches!(scenario.goal, GoalSpec::Accumulate { .. }) {
            return held("discretionary_exits_disabled", &clips);
        }
        if scenario.position_units == 0 {
            return held("no_position", &clips);
        }
        return OracleProposal {
            action: OracleAction::Sell {
                units: scenario.position_units,
            },
            ..held("", &clips)
        };
    }
    if buy_conviction.below(Scenario::milli(scenario.entry_milli)) {
        return held("between_thresholds", &clips);
    }
    let target = buy_conviction
        .mul(cap)
        .mul(Scenario::milli(scenario.size_factor_milli));
    let delta = target.sub(current_mv).sub(working);
    let with_sizes = |action, clips: &BTreeSet<&'static str>| OracleProposal {
        combined: combined.clone(),
        cap,
        current_mv,
        target: Some(target),
        delta: Some(delta),
        action,
        clips: clips.clone(),
    };
    if !delta.is_positive() {
        return with_sizes(OracleAction::Hold("at_or_above_target"), &clips);
    }
    let band = Scenario::milli(scenario.band_milli).mul(cap);
    if delta.below(band) {
        return with_sizes(OracleAction::Hold("within_rebalance_band"), &clips);
    }
    let gross_bound = Scenario::money(scenario.max_gross_dollars)
        .min(Scenario::money(scenario.equity_dollars))
        .sub(Scenario::money(scenario.gross_dollars));
    let budget = delta
        .min(Scenario::money(scenario.max_order_dollars))
        .min(cap.sub(current_mv).sub(working))
        .min(gross_bound);
    if budget.below(delta) {
        clips.insert("limits");
    }
    let ask = Scenario::price(scenario.ask_tenths);
    let increment = Scenario::quantity(scenario.increment_units);
    let mut units = (budget.trunc_div(ask.mul(increment)) * scenario.increment_units).max(0);
    if Scenario::quantity(units).mul(ask).below(band) {
        return with_sizes(OracleAction::Hold("below_band_after_clipping"), &clips);
    }
    if let GoalSpec::Accumulate {
        target_units,
        max_avg_dollars,
        max_spend_dollars,
    } = &scenario.goal
    {
        let per_unit_cost = ask;
        let received_per_unit = Rat::int(1);
        let remaining = Scenario::quantity(target_units - scenario.position_units);
        let mut bound =
            remaining.trunc_div(received_per_unit.mul(increment)) * scenario.increment_units;
        let spend_left = Scenario::money(max_spend_dollars - scenario.goal_spent_dollars);
        bound = bound
            .min(spend_left.trunc_div(per_unit_cost.mul(increment)) * scenario.increment_units);
        if let Some(max_avg) = max_avg_dollars {
            let denominator = per_unit_cost.sub(Scenario::money(*max_avg).mul(received_per_unit));
            if denominator.is_positive() {
                let headroom = Scenario::money(*max_avg)
                    .mul(Scenario::quantity(scenario.position_units))
                    .sub(Scenario::money(scenario.basis_dollars));
                bound = bound
                    .min(headroom.trunc_div(denominator.mul(increment)) * scenario.increment_units);
            }
        }
        if bound < units {
            clips.insert("goal");
            units = bound.max(0);
        }
        if let (Some(max_avg), true) = (max_avg_dollars, units > 0) {
            {
                let spend = Scenario::money(scenario.basis_dollars)
                    .add(Scenario::quantity(units).mul(per_unit_cost));
                let allowed = Scenario::money(*max_avg).mul(
                    Scenario::quantity(scenario.position_units)
                        .add(Scenario::quantity(units).mul(received_per_unit)),
                );
                if allowed.below(spend) {
                    return with_sizes(OracleAction::Hold("would_exceed_max_avg_price"), &clips);
                }
            }
        }
    }
    let value = Scenario::quantity(units).mul(ask);
    if units <= 0 || value.below(Scenario::money(scenario.min_order_dollars)) {
        return with_sizes(OracleAction::Hold("below_minimum_after_clipping"), &clips);
    }
    with_sizes(OracleAction::Buy { units }, &clips)
}

/// §6.2 steps 3 to 5 as a naive walk: the rule list is re-read from the start for every action, the
/// conditions are evaluated by structural recursion over this file's own field table, and the
/// ceiling is applied afterwards by an explicit strictness ranking.
fn oracle_classify(policy: &Autonomy, action: &ActionContext) -> (String, String, Option<u8>) {
    let strictness = |decision: AutonomyDecision| match decision {
        AutonomyDecision::Auto => 0u8,
        AutonomyDecision::Ask => 1,
        AutonomyDecision::Deny => 2,
    };
    let name = |decision: AutonomyDecision| match decision {
        AutonomyDecision::Auto => "auto".to_owned(),
        AutonomyDecision::Ask => "ask".to_owned(),
        AutonomyDecision::Deny => "deny".to_owned(),
    };
    if action.purpose != Purpose::Open && action.purpose != Purpose::Increase {
        return ("auto".to_owned(), "builtin_risk_reducing".to_owned(), None);
    }
    let mut decision = policy.default;
    let mut by = "default".to_owned();
    for index in 0..policy.rules.len() {
        let mut walked = None;
        for candidate in policy.rules.iter().take(index + 1) {
            if oracle_matches(&candidate.when, action) {
                walked = Some(candidate);
                break;
            }
        }
        if let Some(rule) = walked {
            decision = rule.then;
            by = format!("rule:{}", rule.id.as_str());
            break;
        }
    }
    if action.new_instrument && strictness(policy.admission) > strictness(decision) {
        decision = policy.admission;
        by = "admission_ceiling".to_owned();
    }
    let approvers = if decision == AutonomyDecision::Ask {
        let two = policy
            .approval
            .two_approver_above_usd
            .as_ref()
            .is_some_and(|limit| {
                rat_of_text(limit.as_str()).below(rat_of_text(&action.order_usd.to_string()))
            });
        Some(if two { 2 } else { 1 })
    } else {
        None
    };
    (name(decision), by, approvers)
}

fn oracle_matches(condition: &Condition, action: &ActionContext) -> bool {
    match condition {
        Condition::All(children) => children.iter().all(|c| oracle_matches(c, action)),
        Condition::Any(children) => children.iter().any(|c| oracle_matches(c, action)),
        Condition::Not(child) => !oracle_matches(child, action),
        Condition::Compare { field, op, value } => oracle_compare(*field, *op, value, action),
    }
}

fn oracle_compare(
    field: ConditionField,
    op: Operator,
    value: &ConditionValue,
    action: &ActionContext,
) -> bool {
    match value {
        ConditionValue::Decimal(decimal) => {
            let left = rat_of_text(&oracle_decimal(field, action));
            let right = rat_of_text(decimal.as_str());
            match op {
                Operator::Eq => left == right,
                Operator::Ne => left != right,
                Operator::Gt => right.below(left),
                Operator::Gte => !left.below(right),
                Operator::Lt => left.below(right),
                Operator::Lte => !right.below(left),
                Operator::In | Operator::NotIn => false,
            }
        }
        ConditionValue::Bool(flag) => {
            let left = oracle_bool(field, action);
            match op {
                Operator::Eq => left == *flag,
                Operator::Ne => left != *flag,
                _ => false,
            }
        }
        ConditionValue::Text(wanted) => {
            let left = oracle_text(field, action);
            match op {
                Operator::Eq => left == *wanted,
                Operator::Ne => left != *wanted,
                _ => false,
            }
        }
        ConditionValue::List(members) => {
            let left = oracle_text(field, action);
            match op {
                Operator::In => members.contains(&left),
                Operator::NotIn => !members.contains(&left),
                _ => false,
            }
        }
    }
}

fn oracle_text(field: ConditionField, action: &ActionContext) -> String {
    match field {
        ConditionField::Purpose => match action.purpose {
            Purpose::Open => "open",
            Purpose::Increase => "increase",
            Purpose::DiscretionaryExit => "discretionary_exit",
            Purpose::OwnerExit => "owner_exit",
            Purpose::RiskExit => "risk_exit",
            Purpose::Protective => "protective",
        }
        .to_owned(),
        ConditionField::AssetClass => match action.asset_class {
            AssetClass::Crypto => "crypto",
            AssetClass::UsEquity => "us_equity",
        }
        .to_owned(),
        ConditionField::Session => match action.session {
            MarketSession::PreMarket => "pre_market",
            MarketSession::Regular => "regular",
            MarketSession::AfterHours => "after_hours",
            MarketSession::Crypto => "crypto",
            MarketSession::Overnight => "overnight",
        }
        .to_owned(),
        ConditionField::Instrument => action.instrument.as_str().to_owned(),
        other => panic!("{other:?} is not a text field"),
    }
}

fn oracle_bool(field: ConditionField, action: &ActionContext) -> bool {
    match field {
        ConditionField::FirstTradeInInstrument => action.first_trade_in_instrument,
        ConditionField::NewInstrument => action.new_instrument,
        other => panic!("{other:?} is not a boolean field a rule may read"),
    }
}

fn oracle_decimal(field: ConditionField, action: &ActionContext) -> String {
    match field {
        ConditionField::OrderUsd => action.order_usd.to_string(),
        ConditionField::CombinedScore => action.combined_score.to_string(),
        ConditionField::ThesisConfidence => action.thesis_confidence.to_string(),
        ConditionField::Drawdown => action.drawdown.to_string(),
        ConditionField::DailyPnlFraction => action.daily_pnl_fraction.to_string(),
        ConditionField::PositionUsdAfter => action.position_usd_after.to_string(),
        ConditionField::GrossUsdAfter => action.gross_usd_after.to_string(),
        ConditionField::BoughtTodayUsd => action.bought_today_usd.to_string(),
        ConditionField::PositionPnlFraction => action.position_pnl_fraction.to_string(),
        other => panic!("{other:?} is not a decimal field"),
    }
}

/// Canonical decimal text to a rational, written out here so the oracle never parses through
/// `mandate_num`.
fn rat_of_text(text: &str) -> Rat {
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (whole, fraction) = rest.split_once('.').unwrap_or((rest, ""));
    let digits: String = format!("{whole}{fraction}");
    let magnitude: i128 = digits
        .parse()
        .unwrap_or_else(|e| panic!("`{text}` is canonical decimal text: {e}"));
    let denominator = 10i128
        .checked_pow(u32::try_from(fraction.len()).unwrap_or(0))
        .unwrap_or_else(|| panic!("`{text}` is inside the oracle's range"));
    Rat::new(if negative { -magnitude } else { magnitude }, denominator)
}

/// V-023's type rules, restated: which operators and value kinds a field admits.
/// Which kind of value a field takes, restated here rather than read from
/// `ConditionField::kind()`, so this oracle shares no table with the crate under test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OracleKind {
    Bool,
    Decimal,
    Text,
}

fn oracle_kind(field: ConditionField) -> OracleKind {
    match field {
        ConditionField::Purpose
        | ConditionField::AssetClass
        | ConditionField::Session
        | ConditionField::Instrument => OracleKind::Text,
        ConditionField::FirstTradeInInstrument
        | ConditionField::NewInstrument
        | ConditionField::UnusualInput => OracleKind::Bool,
        ConditionField::OrderUsd
        | ConditionField::CombinedScore
        | ConditionField::ThesisConfidence
        | ConditionField::Drawdown
        | ConditionField::DailyPnlFraction
        | ConditionField::PositionUsdAfter
        | ConditionField::GrossUsdAfter
        | ConditionField::BoughtTodayUsd
        | ConditionField::PositionPnlFraction => OracleKind::Decimal,
    }
}

/// The two fields §6.3 bounds to the closed unit interval, restated for the same reason.
fn oracle_is_unit_bounded(field: ConditionField) -> bool {
    matches!(
        field,
        ConditionField::CombinedScore | ConditionField::Drawdown
    )
}

/// The one refusal `classify` owes a rule, computed the oracle's own way: the reserved field is
/// checked before the type rules, so the expected code is exactly one value and never a choice.
fn oracle_refusal(condition: &Condition) -> Option<&'static str> {
    for (comparison, depth) in condition.comparisons() {
        if depth > 4 {
            return Some("condition_too_deep");
        }
        let Condition::Compare { field, op, value } = comparison else {
            continue;
        };
        if *field == ConditionField::UnusualInput {
            return Some("reserved_field");
        }
        if !oracle_type_ok(*field, *op, value) {
            return Some("condition_type_mismatch");
        }
    }
    None
}

fn oracle_type_ok(field: ConditionField, op: Operator, value: &ConditionValue) -> bool {
    match oracle_kind(field) {
        OracleKind::Bool => {
            matches!(op, Operator::Eq | Operator::Ne) && matches!(value, ConditionValue::Bool(_))
        }
        OracleKind::Decimal => {
            if matches!(op, Operator::In | Operator::NotIn) {
                return false;
            }
            let ConditionValue::Decimal(decimal) = value else {
                return false;
            };
            if !oracle_is_unit_bounded(field) {
                return true;
            }
            let held = rat_of_text(decimal.as_str());
            !held.below(Rat::ZERO) && !Rat::int(1).below(held)
        }
        OracleKind::Text => {
            if matches!(
                op,
                Operator::Gt | Operator::Gte | Operator::Lt | Operator::Lte
            ) {
                return false;
            }
            match (op, value) {
                (Operator::In | Operator::NotIn, ConditionValue::List(members)) => {
                    !members.is_empty() && members.iter().all(|m| oracle_enum_ok(field, m))
                }
                (Operator::Eq | Operator::Ne, ConditionValue::Text(member)) => {
                    oracle_enum_ok(field, member)
                }
                _ => false,
            }
        }
    }
}

fn oracle_enum_ok(field: ConditionField, member: &str) -> bool {
    match field {
        ConditionField::Purpose => matches!(member, "open" | "increase"),
        ConditionField::AssetClass => matches!(member, "us_equity" | "crypto"),
        ConditionField::Session => {
            matches!(member, "pre_market" | "regular" | "after_hours" | "crypto")
        }
        _ => true,
    }
}

/// 256 cases a property, as ES-11 requires, with a fixed seed source and no persistence file, so a
/// run on one machine is a run on every other (ES-21).
fn check<S>(strategy: S, body: impl Fn(S::Value) -> Result<(), TestCaseError>)
where
    S: Strategy,
    S::Value: std::fmt::Debug,
{
    let config = ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    let mut runner = proptest::test_runner::TestRunner::new(config);
    if let Err(failure) = runner.run(&strategy, body) {
        panic!("{failure}");
    }
}

/// Weights that sum to exactly 1000 thousandths, so W is one and the sizing oracle's rationals stay
/// inside `i128`. The combine properties use [`free_weights`] instead, where W is anything.
fn unit_weights() -> impl Strategy<Value = Vec<i128>> {
    prop_oneof![
        Just(vec![1000i128]),
        (1i128..1000).prop_map(|a| vec![a, 1000 - a]),
        (1i128..998, 1i128..998).prop_filter_map("three positive weights", |(a, b)| {
            (a + b < 1000).then(|| vec![a, b, 1000 - a - b])
        }),
    ]
}

fn free_weights() -> impl Strategy<Value = Vec<i128>> {
    prop::collection::vec(1i128..1000, 1..=3)
}

fn model_specs(weights: Vec<i128>) -> impl Strategy<Value = Vec<ModelSpec>> {
    let count = weights.len();
    prop::collection::vec(60u32..3600, count).prop_map(move |ages| {
        weights
            .iter()
            .zip(ages)
            .map(|(weight_milli, max_age_s)| ModelSpec {
                weight_milli: *weight_milli,
                max_age_s,
            })
            .collect()
    })
}

fn output_specs(models: usize) -> impl Strategy<Value = Vec<OutputSpec>> {
    prop::collection::vec(
        (
            0usize..models,
            -1000i128..=1000,
            0i128..=1000,
            -4000i64..=120i64,
            0i64..=7200,
            prop::bool::weighted(0.85),
        )
            .prop_map(
                |(model, conviction_milli, confidence_milli, as_of_delta, span, pinned)| {
                    OutputSpec {
                        model,
                        conviction_milli,
                        confidence_milli,
                        as_of_s: EPOCH_SECS + as_of_delta,
                        expires_s: EPOCH_SECS + as_of_delta + span,
                        pinned,
                    }
                },
            ),
        0..=5,
    )
}

/// Which region of §8.3 a generated scenario is aimed at.
///
/// Without this the generator was **vacuous on the buy path**: 256 uniformly drawn cases produced
/// about 125 `no_fresh_outputs`, about 110 `between_thresholds`, a handful of sells and
/// **no buys at all**, so every property past step 2 asserted nothing. Each shape narrows only what
/// it must to land in its region and leaves every other field to the generator, so the fuzzing power
/// is kept where it matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// Uniform and mostly stale: `no_fresh_outputs` and `between_thresholds`.
    Stale,
    /// Fresh and bullish with room to buy.
    FreshBuy,
    /// Fresh and bullish with the gross headroom cut to half the band: `below_band_after_clipping`.
    Crowded,
    /// The position placed so Delta lands inside the band: `within_rebalance_band`.
    NearTarget,
    /// The position placed above the target: `at_or_above_target`.
    AboveTarget,
    /// A zero band with a minimum above any affordable order: `below_minimum_after_clipping`.
    Dust,
    /// Fresh and bearish over a position: a discretionary exit.
    Exit,
    /// An `accumulate` goal whose bounds bite: the goal clip and `would_exceed_max_avg_price`.
    Accumulate,
}

const EVERY_SHAPE: [Shape; 8] = [
    Shape::Stale,
    Shape::FreshBuy,
    Shape::Crowded,
    Shape::NearTarget,
    Shape::AboveTarget,
    Shape::Dust,
    Shape::Exit,
    Shape::Accumulate,
];

fn cap_dollars(scenario: &Scenario) -> i128 {
    scenario
        .max_position_dollars
        .min(scenario.position_fraction_milli * scenario.equity_dollars / 1000)
}

fn band_dollars(scenario: &Scenario) -> i128 {
    scenario.band_milli * cap_dollars(scenario) / 1000
}

/// The target value in whole dollars, from the oracle's own buy conviction. Used only to **place**
/// a position, never to assert anything.
fn target_dollars(scenario: &Scenario) -> i128 {
    let buy = oracle_combine(scenario).buy.max(0);
    buy * cap_dollars(scenario) / 1_000_000_000_000 * scenario.size_factor_milli / 1000
}

/// One fresh, pinned output per configured model, so nothing counts as missing and b equals c.
fn fresh_outputs(scenario: &Scenario, bearish: bool) -> Vec<OutputSpec> {
    scenario
        .models
        .iter()
        .enumerate()
        .map(|(model, _)| {
            let seed = scenario.outputs.get(model % scenario.outputs.len().max(1));
            let conviction = seed
                .map(|o| o.conviction_milli.abs())
                .unwrap_or(900)
                .max(600);
            let confidence = seed.map(|o| o.confidence_milli).unwrap_or(900).max(700);
            OutputSpec {
                model,
                conviction_milli: if bearish { -conviction } else { conviction },
                confidence_milli: confidence,
                as_of_s: EPOCH_SECS - 30,
                expires_s: EPOCH_SECS + 3600,
                pinned: true,
            }
        })
        .collect()
}

fn shaped(mut scenario: Scenario, shape: Shape) -> Scenario {
    if shape == Shape::Stale {
        return scenario;
    }
    scenario.outputs = fresh_outputs(&scenario, shape == Shape::Exit);
    scenario.entry_milli = scenario.entry_milli.min(300);
    scenario.exit_milli = scenario.exit_milli.min(300);
    scenario.size_factor_milli = 1000;
    scenario.working_dollars = 0;
    scenario.gross_dollars = 0;
    scenario.position_units = 0;
    scenario.equity_dollars = scenario.equity_dollars.max(1000);
    scenario.max_position_dollars = scenario.max_position_dollars.max(100);
    scenario.position_fraction_milli = scenario.position_fraction_milli.max(100);
    match shape {
        Shape::Stale => scenario,
        Shape::FreshBuy => {
            scenario.band_milli = scenario.band_milli.min(50);
            scenario.min_order_dollars = scenario.min_order_dollars.min(10);
            scenario
        }
        Shape::Crowded => {
            scenario.band_milli = scenario.band_milli.clamp(50, 200);
            let bound = scenario.max_gross_dollars.min(scenario.equity_dollars);
            let half = (band_dollars(&scenario) / 2).max(1);
            scenario.gross_dollars = (bound - half).max(0);
            scenario
        }
        Shape::NearTarget => {
            scenario.band_milli = scenario.band_milli.clamp(50, 200);
            scenario.mark_tenths = scenario.mark_tenths.clamp(1, 1000);
            scenario.bid_tenths = scenario.mark_tenths;
            scenario.ask_tenths = scenario.mark_tenths;
            let wanted = target_dollars(&scenario) - band_dollars(&scenario) / 2;
            scenario.position_units = (wanted.max(0) * 1000 / scenario.mark_tenths).max(0);
            scenario
        }
        Shape::AboveTarget => {
            scenario.mark_tenths = scenario.mark_tenths.clamp(1, 1000);
            scenario.bid_tenths = scenario.mark_tenths;
            scenario.ask_tenths = scenario.mark_tenths;
            scenario.position_units = (cap_dollars(&scenario) + 1) * 1000 / scenario.mark_tenths;
            scenario
        }
        Shape::Dust => {
            scenario.band_milli = 0;
            scenario.min_order_dollars = scenario.min_order_dollars.max(50);
            scenario.max_order_dollars = 1 + scenario.max_order_dollars % 40;
            scenario
        }
        Shape::Exit => {
            scenario.mark_tenths = scenario.mark_tenths.clamp(1, 1000);
            scenario.bid_tenths = scenario.mark_tenths;
            scenario.ask_tenths = scenario.mark_tenths;
            scenario.position_units = 1 + scenario.max_order_dollars % 10_000;
            scenario
        }
        Shape::Accumulate => {
            scenario.crypto = true;
            scenario.band_milli = scenario.band_milli.min(50);
            scenario.min_order_dollars = scenario.min_order_dollars.min(1);
            scenario.mark_tenths = scenario.mark_tenths.clamp(10, 1000);
            scenario.bid_tenths = scenario.mark_tenths;
            scenario.ask_tenths = scenario.mark_tenths;
            scenario.position_units = 100 + scenario.max_order_dollars % 10_000;
            let ask = scenario.ask_tenths / 10;
            let position_value = scenario.position_units * scenario.ask_tenths / 1000;
            let dear_basis = scenario.max_order_dollars % 2 == 0;
            scenario.basis_dollars = if dear_basis {
                position_value * 11 / 10 + 1
            } else {
                position_value / 2
            };
            scenario.goal_spent_dollars = scenario.basis_dollars;
            scenario.goal = GoalSpec::Accumulate {
                target_units: scenario.position_units + 1 + scenario.max_position_dollars % 200,
                max_avg_dollars: Some(ask.max(1)),
                max_spend_dollars: scenario.goal_spent_dollars
                    + 1
                    + scenario.max_gross_dollars % 2000,
            };
            scenario
        }
    }
}

prop_compose! {
    /// The uniform base: weights that sum to one, which is the range the rational sizing oracle
    /// covers, and every other field over its whole admissible range.
    fn base_scenario()(
        weights in unit_weights(),
    )(
        models in model_specs(weights.clone()),
        outputs in output_specs(weights.len()),
        entry_milli in 0i128..=1000,
        exit_milli in 0i128..=1000,
        band_milli in 0i128..=200,
        size_factor_milli in 1i128..=1000,
        max_position_dollars in 1i128..=100_000,
        position_fraction_milli in 1i128..=1000,
        max_order_dollars in 1i128..=100_000,
        max_gross_dollars in 1i128..=100_000,
        equity_dollars in 100i128..=100_000,
        position_units in 0i128..=100_000,
        mark_tenths in 1i128..=100_000,
        bid_tenths in 1i128..=100_000,
        spread_tenths in 0i128..=100,
        increment_units in prop::sample::select(vec![1i128, 10, 100]),
        min_order_dollars in 0i128..=100,
        gross_dollars in 0i128..=100_000,
        working_dollars in 0i128..=1_000,
        crypto in any::<bool>(),
        in_close_window in any::<bool>(),
    ) -> Scenario {
        Scenario {
            models,
            outputs,
            entry_milli,
            exit_milli,
            band_milli,
            size_factor_milli,
            max_position_dollars,
            position_fraction_milli,
            max_order_dollars,
            max_gross_dollars,
            equity_dollars,
            position_units,
            mark_tenths,
            bid_tenths,
            ask_tenths: bid_tenths + spread_tenths,
            increment_units,
            min_order_dollars,
            gross_dollars,
            working_dollars,
            basis_dollars: 0,
            goal_spent_dollars: 0,
            goal: GoalSpec::Continuous,
            crypto,
            in_close_window,
            now_s: EPOCH_SECS,
        }
    }
}

/// The generator every sizing property runs: the uniform base aimed at one of the eight regions.
fn sizing_scenario() -> impl Strategy<Value = Scenario> {
    (base_scenario(), prop::sample::select(EVERY_SHAPE.to_vec()))
        .prop_map(|(base, shape)| shaped(base, shape))
}

prop_compose! {
    /// A scenario whose weights are free, so W is anything: the range the integer combine oracle
    /// covers, and the one that exercises the denominator.
    fn combine_scenario()(
        weights in free_weights(),
    )(
        models in model_specs(weights.clone()),
        outputs in output_specs(weights.len()),
    ) -> Scenario {
        Scenario {
            models,
            outputs,
            ..blank_scenario()
        }
    }
}

/// A scenario with every non-combine field at a value that proposes nothing surprising, for the
/// generators that only vary the outputs.
fn blank_scenario() -> Scenario {
    Scenario {
        models: vec![ModelSpec {
            weight_milli: 1000,
            max_age_s: 900,
        }],
        outputs: Vec::new(),
        entry_milli: 300,
        exit_milli: 300,
        band_milli: 50,
        size_factor_milli: 1000,
        max_position_dollars: 1500,
        position_fraction_milli: 200,
        max_order_dollars: 1000,
        max_gross_dollars: 2000,
        equity_dollars: 10_000,
        position_units: 500,
        mark_tenths: 999,
        bid_tenths: 999,
        ask_tenths: 1000,
        increment_units: 100,
        min_order_dollars: 1,
        gross_dollars: 0,
        working_dollars: 0,
        basis_dollars: 0,
        goal_spent_dollars: 0,
        goal: GoalSpec::Continuous,
        crypto: false,
        in_close_window: false,
        now_s: EPOCH_SECS,
    }
}

/// The `accumulate` shape on its own, for the goal properties.
fn accumulate_scenario() -> impl Strategy<Value = Scenario> {
    base_scenario().prop_map(|base| shaped(base, Shape::Accumulate))
}

fn action_kind(action: &Action) -> &'static str {
    match action {
        Action::Hold { reason } => reason.as_str(),
        Action::Sell { .. } => "sell",
        Action::Buy { .. } => "buy",
    }
}

fn proposed_units(action: &Action) -> Option<i128> {
    let quantity = match action {
        Action::Hold { .. } => return None,
        Action::Sell { qty, .. } | Action::Buy { qty, .. } => qty,
    };
    Some(
        rat_of_text(&quantity.to_string())
            .mul(Rat::int(QTY_UNITS))
            .to_text()
            .parse()
            .unwrap_or(-1),
    )
}

/// Runs the crate and the oracle over one scenario and asserts they agree on **everything the
/// proposal reports**, starting with the action kind.
///
/// The kind comes first deliberately: a hold for the wrong reason and a hold for the right one are
/// different answers, and a property that only checked a bound would accept either.
fn agree(scenario: &Scenario) -> Result<(Proposal, OracleProposal), TestCaseError> {
    let oracle = oracle_propose(scenario);
    let proposal = scenario
        .propose()
        .map_err(|e| TestCaseError::fail(format!("propose returns a proposal, not {e}")))?;

    let used: BTreeSet<String> = proposal
        .combined
        .outputs_used
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();
    prop_assert_eq!(&used, &oracle.combined.used, "the models counted");
    prop_assert_eq!(
        proposal.combined.exit_conviction.to_string(),
        scaled_text(oracle.combined.exit, 12),
        "the exit conviction c"
    );
    prop_assert_eq!(
        proposal.combined.buy_conviction.to_string(),
        scaled_text(oracle.combined.buy, 12),
        "the buy conviction b"
    );
    prop_assert_eq!(
        proposal.combined.score.to_string(),
        scaled_text(oracle.combined.score, 12),
        "the combined score s"
    );

    prop_assert_eq!(
        action_kind(&proposal.action),
        oracle.action.kind(),
        "the action the two reached"
    );
    match &oracle.action {
        OracleAction::Hold(_) => {}
        OracleAction::Sell { units } | OracleAction::Buy { units } => {
            prop_assert_eq!(
                proposed_units(&proposal.action),
                Some(*units),
                "the quantity"
            );
        }
    }

    prop_assert_eq!(proposal.sizes.cap.to_string(), oracle.cap.to_text(), "cap");
    prop_assert_eq!(
        proposal.sizes.current_mv.to_string(),
        oracle.current_mv.to_text(),
        "MV at the risk mark"
    );
    prop_assert_eq!(
        proposal
            .sizes
            .target_value
            .as_ref()
            .map(ToString::to_string),
        oracle.target.map(Rat::to_text),
        "the target value T"
    );
    prop_assert_eq!(
        proposal.sizes.delta.as_ref().map(ToString::to_string),
        oracle.delta.map(Rat::to_text),
        "Delta"
    );
    let clips: BTreeSet<&'static str> = proposal.clipped_by.iter().map(|c| c.as_str()).collect();
    prop_assert_eq!(&clips, &oracle.clips, "the clips reported");
    Ok((proposal, oracle))
}

fn order_value(action: &Action) -> Option<Rat> {
    match action {
        Action::Hold { .. } => None,
        Action::Sell { order_usd, .. } | Action::Buy { order_usd, .. } => {
            Some(rat_of_text(&order_usd.to_string()))
        }
    }
}

/// §8.3 step 1: the three figures are each one half-even rounding of one exact quotient, computed
/// against an integer oracle that scales the weights, convictions and confidences and divides once.
#[test]
#[ignore = "pending E6-2"]
fn the_three_combined_figures_match_the_integer_oracle() {
    check(combine_scenario(), |scenario| {
        let oracle = oracle_combine(&scenario);
        let combined = mandate_builder::combine(
            &scenario.signal_models(),
            &scenario.model_outputs(),
            &asset(scenario.instrument()),
            scenario.now(),
        )
        .map_err(|e| TestCaseError::fail(format!("combine returns figures, not {e}")))?;
        let used: BTreeSet<String> = combined
            .outputs_used
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect();
        prop_assert_eq!(&used, &oracle.used);
        prop_assert_eq!(
            combined.exit_conviction.to_string(),
            scaled_text(oracle.exit, 12)
        );
        prop_assert_eq!(
            combined.buy_conviction.to_string(),
            scaled_text(oracle.buy, 12)
        );
        prop_assert_eq!(combined.score.to_string(), scaled_text(oracle.score, 12));
        prop_assert!(
            oracle.exit >= -1_000_000_000_000 && oracle.exit <= 1_000_000_000_000,
            "the oracle's own c stays in [-1, 1], which is what the crate's type promises"
        );
        Ok(())
    });
}

/// §8.2: an output counts exactly when `as_of ≤ now < expires_at` and `now − as_of ≤
/// max_output_age_s`, and its pinned triple matches. The interval is recomputed here from the
/// generated offsets, so the property never asks the crate which outputs it took.
#[test]
#[ignore = "pending E6-2"]
fn freshness_matches_the_interval_oracle() {
    check(combine_scenario(), |scenario| {
        let combined = mandate_builder::combine(
            &scenario.signal_models(),
            &scenario.model_outputs(),
            &asset(scenario.instrument()),
            scenario.now(),
        )
        .map_err(|e| TestCaseError::fail(format!("combine returns figures, not {e}")))?;
        let mut expected: BTreeSet<String> = BTreeSet::new();
        for spec in &scenario.outputs {
            let model = &scenario.models[spec.model];
            let inside = spec.as_of_s <= scenario.now_s
                && scenario.now_s < spec.expires_s
                && scenario.now_s - spec.as_of_s <= i64::from(model.max_age_s);
            if inside && spec.pinned {
                expected.insert(MODEL_NAMES[spec.model].to_owned());
            }
        }
        let used: BTreeSet<String> = combined
            .outputs_used
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect();
        prop_assert_eq!(
            &used,
            &expected,
            "the models with at least one fresh pinned output"
        );
        Ok(())
    });
}

/// §8.2, DEC-130 item 10: at most one output per model is used, and it is the one with the latest
/// `as_of`, ties by journal position. Feeding only that output back gives an identical answer,
/// which is what "only the latest counts" means.
#[test]
#[ignore = "pending E6-2"]
fn one_output_per_model_is_used_and_it_is_the_latest() {
    check(combine_scenario(), |scenario| {
        let all = mandate_builder::combine(
            &scenario.signal_models(),
            &scenario.model_outputs(),
            &asset(scenario.instrument()),
            scenario.now(),
        )
        .map_err(|e| TestCaseError::fail(format!("combine returns figures, not {e}")))?;
        prop_assert!(
            all.outputs_used.len() <= scenario.models.len(),
            "no model is counted twice"
        );
        let oracle = oracle_combine(&scenario);
        let used: BTreeSet<String> = all
            .outputs_used
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect();
        prop_assert_eq!(
            &used,
            &oracle.used,
            "the models counted, so a run that counted none is not a run that agreed"
        );
        prop_assert_eq!(
            all.exit_conviction.to_string(),
            scaled_text(oracle.exit, 12)
        );
        let mut latest: BTreeMap<usize, (i64, usize)> = BTreeMap::new();
        for (index, spec) in scenario.outputs.iter().enumerate() {
            let model = &scenario.models[spec.model];
            let fresh = spec.pinned
                && spec.as_of_s <= scenario.now_s
                && scenario.now_s < spec.expires_s
                && scenario.now_s - spec.as_of_s <= i64::from(model.max_age_s);
            if fresh {
                let key = (spec.as_of_s, index);
                latest
                    .entry(spec.model)
                    .and_modify(|held| {
                        if key > *held {
                            *held = key;
                        }
                    })
                    .or_insert(key);
            }
        }
        let kept: Vec<usize> = latest.values().map(|(_, index)| *index).collect();
        let trimmed = Scenario {
            outputs: scenario
                .outputs
                .iter()
                .enumerate()
                .filter(|(index, _)| kept.contains(index))
                .map(|(_, spec)| spec.clone())
                .collect(),
            ..scenario.clone()
        };
        let only_latest = mandate_builder::combine(
            &trimmed.signal_models(),
            &trimmed.model_outputs(),
            &asset(trimmed.instrument()),
            trimmed.now(),
        )
        .map_err(|e| TestCaseError::fail(format!("combine returns figures, not {e}")))?;
        prop_assert_eq!(
            all.exit_conviction.to_string(),
            only_latest.exit_conviction.to_string(),
            "the outputs this property dropped were never counted"
        );
        prop_assert_eq!(all.score.to_string(), only_latest.score.to_string());
        Ok(())
    });
}

/// §8.3 step 1: a model without a fresh output counts as **0** for the exit conviction. Dropping an
/// output therefore moves c toward zero and never past it: a bullish output's removal lowers c and
/// a bearish one's raises it.
#[test]
#[ignore = "pending E6-2"]
fn removing_a_fresh_output_never_lowers_the_exit_conviction_below_the_rest() {
    check((combine_scenario(), 0usize..5), |(scenario, drop)| {
        if drop >= scenario.outputs.len() {
            return Ok(());
        }
        let dropped = &scenario.outputs[drop];
        let with_all = oracle_combine(&scenario);
        let mut without = scenario.clone();
        without.outputs.remove(drop);
        let rest = oracle_combine(&without);
        let full = mandate_builder::combine(
            &scenario.signal_models(),
            &scenario.model_outputs(),
            &asset(scenario.instrument()),
            scenario.now(),
        )
        .map_err(|e| TestCaseError::fail(format!("combine returns figures, not {e}")))?;
        let partial = mandate_builder::combine(
            &without.signal_models(),
            &without.model_outputs(),
            &asset(without.instrument()),
            without.now(),
        )
        .map_err(|e| TestCaseError::fail(format!("combine returns figures, not {e}")))?;
        prop_assert_eq!(
            full.exit_conviction.to_string(),
            scaled_text(with_all.exit, 12)
        );
        prop_assert_eq!(
            partial.exit_conviction.to_string(),
            scaled_text(rest.exit, 12)
        );
        let counted = with_all.used.contains(MODEL_NAMES[dropped.model])
            && !rest.used.contains(MODEL_NAMES[dropped.model]);
        if counted && dropped.conviction_milli > 0 && dropped.confidence_milli > 0 {
            prop_assert!(
                rest.exit <= with_all.exit,
                "dropping a bullish output lowers c toward zero, it does not raise it"
            );
        }
        if counted && dropped.conviction_milli < 0 && dropped.confidence_milli > 0 {
            prop_assert!(
                rest.exit >= with_all.exit,
                "dropping a bearish output raises c toward zero, so an outage never forces a sell"
            );
        }
        Ok(())
    });
}

/// §8.3 step 1, MI-10: a model without a fresh output counts as fully bearish, so **any** removal
/// lowers the buy conviction or leaves it alone. This one holds whatever the removed output said.
#[test]
#[ignore = "pending E6-2"]
fn a_missing_model_never_raises_the_buy_conviction() {
    check((combine_scenario(), 0usize..5), |(scenario, drop)| {
        if drop >= scenario.outputs.len() {
            return Ok(());
        }
        let with_all = oracle_combine(&scenario);
        let mut without = scenario.clone();
        without.outputs.remove(drop);
        let rest = oracle_combine(&without);
        let full = mandate_builder::combine(
            &scenario.signal_models(),
            &scenario.model_outputs(),
            &asset(scenario.instrument()),
            scenario.now(),
        )
        .map_err(|e| TestCaseError::fail(format!("combine returns figures, not {e}")))?;
        let partial = mandate_builder::combine(
            &without.signal_models(),
            &without.model_outputs(),
            &asset(without.instrument()),
            without.now(),
        )
        .map_err(|e| TestCaseError::fail(format!("combine returns figures, not {e}")))?;
        prop_assert_eq!(
            full.buy_conviction.to_string(),
            scaled_text(with_all.buy, 12)
        );
        prop_assert_eq!(
            partial.buy_conviction.to_string(),
            scaled_text(rest.buy, 12)
        );
        let name = MODEL_NAMES[scenario.outputs[drop].model];
        if !rest.used.contains(name) {
            prop_assert!(
                rest.buy <= with_all.buy,
                "a model that is now missing counts as fully bearish, so b can only fall"
            );
        }
        Ok(())
    });
}

/// §8.3 step 2: the conviction line is partitioned by the two thresholds, and which band a scenario
/// falls in is computed here from c, b and the thresholds — never read back from the crate.
#[test]
#[ignore = "pending E6-2"]
fn the_three_bands_partition_the_conviction_line() {
    check(sizing_scenario(), |scenario| {
        let (proposal, _) = agree(&scenario)?;
        let combined = oracle_combine(&scenario);
        if combined.used.is_empty() {
            prop_assert_eq!(action_kind(&proposal.action), "no_fresh_outputs");
            return Ok(());
        }
        let exit_band = combined.exit <= -scenario.exit_milli * 1_000_000_000;
        let entry_band = combined.buy >= scenario.entry_milli * 1_000_000_000;
        let kind = action_kind(&proposal.action);
        if exit_band {
            prop_assert!(
                matches!(
                    kind,
                    "sell" | "discretionary_exits_disabled" | "no_position"
                ),
                "c is at or below the exit threshold, so the exit branch is the one taken"
            );
        } else if entry_band {
            prop_assert_ne!(
                kind,
                "between_thresholds",
                "b is at or above the entry threshold, so the buy branch is the one taken"
            );
            prop_assert_ne!(kind, "sell", "and a buy branch never sells");
        } else {
            prop_assert_eq!(kind, "between_thresholds");
        }
        Ok(())
    });
}

/// §8.3 step 3: Delta is T − MV at the risk mark − the working opening cost, recomputed here as a
/// rational from the generated inputs.
#[test]
#[ignore = "pending E6-2"]
fn delta_matches_the_independent_target_oracle() {
    check(sizing_scenario(), |scenario| {
        let (proposal, oracle) = agree(&scenario)?;
        if let Some(delta) = oracle.delta {
            let target = oracle.target.unwrap_or(Rat::ZERO);
            let recomputed = target
                .sub(
                    Scenario::quantity(scenario.position_units)
                        .mul(Scenario::price(scenario.mark_tenths)),
                )
                .sub(Scenario::money(scenario.working_dollars));
            prop_assert_eq!(
                delta,
                recomputed,
                "Delta is the target less the mark value and the working cost"
            );
            prop_assert_eq!(
                proposal.sizes.delta.as_ref().map(ToString::to_string),
                Some(recomputed.to_text())
            );
        }
        Ok(())
    });
}

/// §8.3 step 3, DEC-65: a positive conviction never produces a sell. There are no signal trims in
/// v1, so a position above its target holds.
#[test]
#[ignore = "pending E6-2"]
fn a_positive_conviction_never_produces_a_sell() {
    check(sizing_scenario(), |scenario| {
        let (proposal, _) = agree(&scenario)?;
        let combined = oracle_combine(&scenario);
        if combined.exit > 0 {
            prop_assert!(
                !matches!(proposal.action, Action::Sell { .. }),
                "c is above zero, so nothing is sold"
            );
        }
        Ok(())
    });
}

/// §8.3 step 5, DEC-130 item 21: every proposed quantity is strictly positive, and its value is at
/// or above the minimum order.
#[test]
#[ignore = "pending E6-2"]
fn every_proposed_quantity_is_strictly_positive() {
    check(sizing_scenario(), |scenario| {
        let (proposal, _) = agree(&scenario)?;
        if let Some(units) = proposed_units(&proposal.action) {
            prop_assert!(units > 0, "a proposal of nothing is not a proposal");
        }
        if matches!(proposal.action, Action::Buy { .. }) {
            let value = order_value(&proposal.action).unwrap_or(Rat::ZERO);
            prop_assert!(
                !value.below(Scenario::money(scenario.min_order_dollars)),
                "a buy is never below the minimum order"
            );
        }
        Ok(())
    });
}

/// §8.3 step 3: a proposal never exceeds Delta, `max_order_usd`, the cap headroom, or the gross
/// headroom, each recomputed here from the inputs rather than taken from the oracle's own budget.
#[test]
#[ignore = "pending E6-2"]
fn a_proposal_never_exceeds_any_of_the_four_bounds() {
    check(sizing_scenario(), |scenario| {
        let (proposal, oracle) = agree(&scenario)?;
        let Action::Buy { .. } = &proposal.action else {
            return Ok(());
        };
        let value = order_value(&proposal.action).unwrap_or(Rat::ZERO);
        let mv =
            Scenario::quantity(scenario.position_units).mul(Scenario::price(scenario.mark_tenths));
        let working = Scenario::money(scenario.working_dollars);
        let delta = oracle.delta.unwrap_or(Rat::ZERO);
        prop_assert!(!delta.below(value), "a buy never exceeds Delta");
        prop_assert!(
            !Scenario::money(scenario.max_order_dollars).below(value),
            "nor max_order_usd"
        );
        prop_assert!(
            !oracle.cap.sub(mv).sub(working).below(value),
            "nor the cap headroom"
        );
        prop_assert!(
            !Scenario::money(scenario.max_gross_dollars)
                .min(Scenario::money(scenario.equity_dollars))
                .sub(Scenario::money(scenario.gross_dollars))
                .below(value),
            "nor the gross headroom"
        );
        Ok(())
    });
}

/// §8.3 and §5.3: because the proposal is already clipped to the limits, the gate's §5.3 checks
/// cannot deny it for a limit the builder owns — the position after, the order value, and the gross
/// after all stay at or below their limits.
#[test]
#[ignore = "pending E6-2"]
fn a_proposal_never_fails_the_position_order_or_gross_limit() {
    check(sizing_scenario(), |scenario| {
        let (proposal, oracle) = agree(&scenario)?;
        let Action::Buy { .. } = &proposal.action else {
            return Ok(());
        };
        let value = order_value(&proposal.action).unwrap_or(Rat::ZERO);
        let mv =
            Scenario::quantity(scenario.position_units).mul(Scenario::price(scenario.mark_tenths));
        let working = Scenario::money(scenario.working_dollars);
        let position_after = mv.add(working).add(value);
        prop_assert!(
            !oracle.cap.below(position_after),
            "the position after the order is at or below the cap, which is at or below both §5.3 position limits"
        );
        prop_assert!(
            !Scenario::money(scenario.max_order_dollars).below(value),
            "the order value is at or below max_order_usd"
        );
        let gross_after = Scenario::money(scenario.gross_dollars).add(value);
        prop_assert!(
            !Scenario::money(scenario.max_gross_dollars).below(gross_after),
            "and the gross after is at or below max_gross_exposure_usd"
        );
        Ok(())
    });
}

/// §8.3 step 4: an `accumulate` buy never breaks the goal's own bounds — the remaining quantity,
/// the remaining spend, and the projected average.
#[test]
#[ignore = "pending E6-2"]
fn an_accumulate_buy_never_breaks_a_goal_bound() {
    check(accumulate_scenario(), |scenario| {
        let (proposal, _) = agree(&scenario)?;
        let Action::Buy { .. } = &proposal.action else {
            return Ok(());
        };
        let GoalSpec::Accumulate {
            target_units,
            max_avg_dollars,
            max_spend_dollars,
        } = &scenario.goal
        else {
            return Ok(());
        };
        let units = proposed_units(&proposal.action).unwrap_or(0);
        prop_assert!(
            scenario.position_units + units <= *target_units,
            "the position after the fill never passes the target quantity"
        );
        let spend = Scenario::quantity(units).mul(Scenario::price(scenario.ask_tenths));
        prop_assert!(
            !Scenario::money(max_spend_dollars - scenario.goal_spent_dollars).below(spend),
            "nor the remaining spend"
        );
        if let Some(max_avg) = max_avg_dollars {
            let basis_after = Scenario::money(scenario.basis_dollars).add(spend);
            let allowed =
                Scenario::money(*max_avg).mul(Scenario::quantity(scenario.position_units + units));
            prop_assert!(!allowed.below(basis_after), "nor the projected average");
        }
        Ok(())
    });
}

/// §6.1 and DEC-32: no proposal crosses zero, and no sell exceeds the position. A buy is long-only
/// and a sell is the whole position or nothing.
#[test]
#[ignore = "pending E6-2"]
fn no_proposal_crosses_zero() {
    check(sizing_scenario(), |scenario| {
        let (proposal, _) = agree(&scenario)?;
        match &proposal.action {
            Action::Hold { .. } => Ok(()),
            Action::Sell {
                qty: _, purpose, ..
            } => {
                let units = proposed_units(&proposal.action).unwrap_or(0);
                prop_assert!(
                    units <= scenario.position_units,
                    "a sell never exceeds the position"
                );
                prop_assert_eq!(*purpose, Purpose::DiscretionaryExit);
                Ok(())
            }
            Action::Buy { purpose, .. } => {
                let expected = if scenario.position_units == 0 {
                    Purpose::Open
                } else {
                    Purpose::Increase
                };
                prop_assert_eq!(*purpose, expected, "the label follows the position");
                Ok(())
            }
        }
    });
}

/// §6.1: the builder never proposes a sell above the position, stated on its own because it is the
/// one thing a sizing mistake could turn into a short.
#[test]
#[ignore = "pending E6-2"]
fn the_builder_never_proposes_a_sell_above_the_position() {
    check(sizing_scenario(), |scenario| {
        let (proposal, _) = agree(&scenario)?;
        if let Action::Sell { .. } = &proposal.action {
            let units = proposed_units(&proposal.action).unwrap_or(i128::MAX);
            prop_assert_eq!(
                units,
                scenario.position_units,
                "a discretionary exit is the whole position, never more"
            );
            prop_assert!(scenario.position_units > 0, "and never from a flat agent");
        }
        Ok(())
    });
}

/// §6.3: the exposure fields a rule reads are the order's **after** values, which is what makes a
/// `bought_today_usd` bound something order splitting cannot evade.
#[test]
#[ignore = "pending E6-2"]
fn position_and_gross_after_include_this_order() {
    check(sizing_scenario(), |scenario| {
        let (proposal, _) = agree(&scenario)?;
        let Action::Buy { action, .. } = &proposal.action else {
            return Ok(());
        };
        let value = order_value(&proposal.action).unwrap_or(Rat::ZERO);
        let mv =
            Scenario::quantity(scenario.position_units).mul(Scenario::price(scenario.mark_tenths));
        prop_assert_eq!(
            action.position_usd_after.to_string(),
            mv.add(Scenario::money(scenario.working_dollars))
                .add(value)
                .to_text()
        );
        prop_assert_eq!(
            action.gross_usd_after.to_string(),
            Scenario::money(scenario.gross_dollars).add(value).to_text()
        );
        prop_assert_eq!(action.bought_today_usd.to_string(), value.to_text());
        prop_assert_eq!(action.order_usd.to_string(), value.to_text());
        Ok(())
    });
}

/// ES-21: identical inputs give an identical proposal. No clock, no randomness, no iteration order
/// that two runs could differ on.
#[test]
#[ignore = "pending E6-2"]
fn identical_inputs_give_identical_proposals() {
    check(sizing_scenario(), |scenario| {
        let (first, oracle) = agree(&scenario)?;
        let second = scenario
            .propose()
            .map_err(|e| TestCaseError::fail(format!("propose returns a proposal, not {e}")))?;
        prop_assert_eq!(&first, &second, "two calls on one input");
        prop_assert_eq!(
            action_kind(&first.action),
            oracle.action.kind(),
            "and both agree with the oracle, so the equality is not two identical mistakes"
        );
        Ok(())
    });
}

/// DEC-130 items 7 and 8: nothing inside the stated bounds overflows. A 12-place weight, an
/// 18-place confidence and conviction, a 12-place mark, 9-place quantities and prices and a value
/// ceiling of 10¹² must produce a proposal or a **typed** refusal, never a wrapped number.
#[test]
#[ignore = "pending E6-2"]
fn no_input_within_the_stated_bounds_overflows() {
    let widest = BuilderMandate {
        models: vec![
            SignalModel {
                weight: frac("0.999999999999"),
                ..news()
            },
            SignalModel {
                weight: frac("0.000000000001"),
                ..momentum()
            },
        ],
        sizing: Sizing {
            method: SizingMethod::ConvictionLinear,
            entry_threshold: frac("0.000000000001"),
            exit_threshold: frac("0.000000000001"),
            rebalance_band: frac("0.000000000001"),
        },
        limits: Limits {
            max_position_usd: usd("1000000000000"),
            max_position_fraction: frac("0.999999999999"),
            max_order_usd: usd("1000000000000"),
            max_gross_exposure_usd: usd("1000000000000"),
        },
        goal: GoalKind::Continuous,
    };
    let account = AccountSnapshot {
        agent_equity: usd("1000000000000"),
        position_qty: qty("999999999.999999999"),
        cost_basis: CostBasis::parse("0").unwrap_or_else(|e| panic!("a zero basis: {e}")),
        risk_mark: mark("999.999999999999"),
        gross_usd: usd("0"),
        working_opening_cost: usd("0"),
        goal_spent_usd: usd("0"),
    };
    let market = Market {
        bid: price("999.999999999"),
        ask: price("1000"),
        increment: qty("0.000000001"),
        ..common::swing_market()
    };
    let risk = RiskContext {
        size_factor: frac("0.999999999999"),
        ..common::quiet_risk()
    };
    let outputs = [
        ModelOutput {
            conviction: Conviction::parse("0.999999999999999999")
                .unwrap_or_else(|e| panic!("an 18-place conviction: {e}")),
            confidence: Unit::parse("0.999999999999999999")
                .unwrap_or_else(|e| panic!("an 18-place confidence: {e}")),
            ..common::output(&news(), SWING_INSTRUMENT, "1", "1")
        },
        common::output(&momentum(), SWING_INSTRUMENT, "1", "1"),
    ];
    let held = propose(&widest, &account, &market, &risk, &outputs, common::at(NOW))
        .unwrap_or_else(|e| panic!("the widest market value carries exactly, and gave {e}"));
    assert_eq!(
        held.sizes.cap.to_string(),
        "999999999999",
        "cap is the 12-place fraction of a 10¹² equity, carried exactly"
    );
    assert_eq!(
        held.sizes.current_mv.to_string(),
        "999999999999.998999000000000000001",
        "a 9-place quantity at a 12-place mark is 21 fractional places over 12 integer ones"
    );
    assert!(
        matches!(
            held.action,
            Action::Hold {
                reason: HoldReason::AtOrAboveTarget
            }
        ),
        "a position that large is above any target this cap allows, and says so rather than overflowing"
    );

    let flat = AccountSnapshot {
        position_qty: qty("0"),
        ..account
    };
    let bought = propose(&widest, &flat, &market, &risk, &outputs, common::at(NOW))
        .unwrap_or_else(|e| panic!("the widest sizing chain carries exactly, and gave {e}"));
    match bought.action {
        Action::Buy {
            qty: ref quantity, ..
        } => assert!(
            !quantity.is_zero(),
            "a buy at full conviction and a 10¹² cap is not a buy of nothing"
        ),
        ref other => panic!("the widest stated inputs still propose a buy, not {other:?}"),
    }
}

const PURPOSES: [Purpose; 6] = [
    Purpose::Open,
    Purpose::Increase,
    Purpose::DiscretionaryExit,
    Purpose::OwnerExit,
    Purpose::RiskExit,
    Purpose::Protective,
];

prop_compose! {
    fn action_context()(
        purpose in prop::sample::select(PURPOSES.to_vec()),
        order_dollars in 0i128..=5_000,
        score_milli in 0i128..=1000,
        crypto in any::<bool>(),
        first_trade in any::<bool>(),
        new_instrument in any::<bool>(),
        thesis_milli in 0i128..=1000,
        position_after in 0i128..=50_000,
        gross_after in 0i128..=50_000,
        bought_today in 0i128..=50_000,
        pnl_milli in -1000i128..=1000,
    ) -> ActionContext {
        ActionContext {
            purpose,
            order_usd: usd(&order_dollars.to_string()),
            combined_score: unit(&Scenario::milli(score_milli).to_text()),
            instrument: asset(if crypto { BTC_INSTRUMENT } else { SWING_INSTRUMENT }),
            asset_class: if crypto { AssetClass::Crypto } else { AssetClass::UsEquity },
            session: if crypto { MarketSession::Crypto } else { MarketSession::Regular },
            first_trade_in_instrument: first_trade,
            new_instrument,
            thesis_confidence: unit(&Scenario::milli(thesis_milli).to_text()),
            drawdown: Unit::ZERO,
            daily_pnl_fraction: Signed::ZERO,
            position_usd_after: usd(&position_after.to_string()),
            gross_usd_after: usd(&gross_after.to_string()),
            bought_today_usd: usd(&bought_today.to_string()),
            position_pnl_fraction: Signed::parse(&Scenario::milli(pnl_milli).to_text())
                .unwrap_or(Signed::ZERO),
        }
    }
}

/// A well-typed comparison over the fields §6.3 lets a rule read.
fn well_typed_leaf() -> impl Strategy<Value = Condition> {
    prop_oneof![
        (
            prop::sample::select(vec![
                ConditionField::OrderUsd,
                ConditionField::BoughtTodayUsd,
                ConditionField::PositionUsdAfter,
                ConditionField::GrossUsdAfter,
            ]),
            prop::sample::select(vec![
                Operator::Gt,
                Operator::Gte,
                Operator::Lt,
                Operator::Lte,
                Operator::Eq,
                Operator::Ne
            ]),
            0i128..=5_000i128,
        )
            .prop_map(|(field, op, dollars)| compare(
                field,
                op,
                decimal(&dollars.to_string())
            )),
        (
            prop::sample::select(vec![
                ConditionField::CombinedScore,
                ConditionField::ThesisConfidence,
            ]),
            prop::sample::select(vec![
                Operator::Gt,
                Operator::Gte,
                Operator::Lt,
                Operator::Lte
            ]),
            0i128..=1000i128,
        )
            .prop_map(|(field, op, milli)| {
                compare(field, op, decimal(&Scenario::milli(milli).to_text()))
            }),
        (
            prop::sample::select(vec![
                ConditionField::NewInstrument,
                ConditionField::FirstTradeInInstrument,
            ]),
            prop::sample::select(vec![Operator::Eq, Operator::Ne]),
            any::<bool>(),
        )
            .prop_map(|(field, op, value)| compare(field, op, flag(value))),
        (
            prop::sample::select(vec![Operator::Eq, Operator::Ne]),
            prop::sample::select(vec!["open".to_owned(), "increase".to_owned()]),
        )
            .prop_map(|(op, member)| compare(ConditionField::Purpose, op, text(&member))),
        (
            prop::sample::select(vec![Operator::In, Operator::NotIn]),
            prop::sample::select(vec![
                vec!["open".to_owned()],
                vec!["increase".to_owned()],
                vec!["open".to_owned(), "increase".to_owned()],
            ]),
        )
            .prop_map(|(op, members)| Condition::Compare {
                field: ConditionField::Purpose,
                op,
                value: ConditionValue::List(members),
            }),
        (
            prop::sample::select(vec![Operator::Eq, Operator::Ne]),
            prop::sample::select(vec!["crypto".to_owned(), "us_equity".to_owned()]),
        )
            .prop_map(|(op, member)| compare(
                ConditionField::AssetClass,
                op,
                text(&member)
            )),
    ]
}

/// Trees nesting to V-017's four levels: a leaf is depth one, and three combinators above it reach
/// the limit.
fn well_typed_condition() -> impl Strategy<Value = Condition> {
    well_typed_leaf().prop_recursive(3, 12, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 1..=2).prop_map(Condition::All),
            prop::collection::vec(inner.clone(), 1..=2).prop_map(Condition::Any),
            inner.prop_map(|c| Condition::Not(Box::new(c))),
        ]
    })
}

const RULE_IDS: [&str; 4] = ["first", "second", "third", "fourth"];

fn decisions() -> impl Strategy<Value = AutonomyDecision> {
    prop::sample::select(vec![
        AutonomyDecision::Auto,
        AutonomyDecision::Ask,
        AutonomyDecision::Deny,
    ])
}

prop_compose! {
    fn well_typed_policy()(
        conditions in prop::collection::vec(well_typed_condition(), 0..=4),
        outcomes in prop::collection::vec(decisions(), 4),
        default in decisions(),
        admission in decisions(),
        threshold in prop::option::of(1i128..=5_000i128),
    ) -> Autonomy {
        let rules = conditions
            .into_iter()
            .enumerate()
            .map(|(index, when)| Rule {
                id: rule_id(RULE_IDS[index]),
                when,
                then: outcomes[index],
            })
            .collect();
        policy(rules, default, admission, threshold.map(|t| t.to_string()).as_deref())
    }
}

/// §6.2 steps 3 to 5, against the naive walk: the first matching rule, then the default, then the
/// ceiling. The oracle re-reads the list from the start for every action, so an implementation that
/// cached a match or took the last one disagrees with it.
#[test]
#[ignore = "pending E6-2"]
fn the_decision_matches_the_first_match_oracle() {
    check(
        (well_typed_policy(), action_context()),
        |(policy, action)| {
            let (decision, by, approvers) = oracle_classify(&policy, &action);
            let classified = classify(&policy, &action).map_err(|e| {
                TestCaseError::fail(format!("classify returns a decision, not {e}"))
            })?;
            let named = match classified.decision {
                AutonomyDecision::Auto => "auto",
                AutonomyDecision::Ask => "ask",
                AutonomyDecision::Deny => "deny",
            };
            prop_assert_eq!(named, decision.as_str(), "the decision");
            prop_assert_eq!(classified.by.label(), by, "what decided it");
            prop_assert_eq!(
                classified.approval.map(|a| a.approvers_required.get()),
                approvers,
                "the approvers an ASK needs, and none otherwise"
            );
            Ok(())
        },
    );
}

/// §6.2 step 3, MI-1, DEC-05: no rule set ever denies or asks a risk-reducing purpose, and the
/// anchor is the same rule set reaching a non-AUTO answer for an opening action.
#[test]
#[ignore = "pending E6-2"]
fn no_rule_set_ever_denies_or_asks_a_reducing_purpose() {
    check(
        (well_typed_policy(), action_context()),
        |(policy, action)| {
            let reducing = ActionContext {
                purpose: Purpose::RiskExit,
                ..action.clone()
            };
            let decided = classify(&policy, &reducing).map_err(|e| {
                TestCaseError::fail(format!("classify returns a decision, not {e}"))
            })?;
            prop_assert_eq!(decided.decision, AutonomyDecision::Auto);
            prop_assert_eq!(decided.by.label(), "builtin_risk_reducing");
            prop_assert_eq!(decided.approval, None, "and asks nobody");

            let opening = ActionContext {
                purpose: Purpose::Open,
                ..action
            };
            let (expected, _, _) = oracle_classify(&policy, &opening);
            let opened = classify(&policy, &opening).map_err(|e| {
                TestCaseError::fail(format!("classify returns a decision, not {e}"))
            })?;
            let named = match opened.decision {
                AutonomyDecision::Auto => "auto",
                AutonomyDecision::Ask => "ask",
                AutonomyDecision::Deny => "deny",
            };
            prop_assert_eq!(
                named,
                expected.as_str(),
                "the same rules do decide an opening action, so the AUTO above is the built-in"
            );
            Ok(())
        },
    );
}

/// §6.2 step 5, MI-17: the ceiling only tightens. Raising `autonomy.admission` never loosens the
/// answer for a new instrument, and never changes it at all for a held one.
#[test]
#[ignore = "pending E6-2"]
fn the_admission_ceiling_is_monotone_in_strictness() {
    check(
        (well_typed_policy(), action_context()),
        |(policy, action)| {
            let rank = |decision: AutonomyDecision| match decision {
                AutonomyDecision::Auto => 0u8,
                AutonomyDecision::Ask => 1,
                AutonomyDecision::Deny => 2,
            };
            let admitted = ActionContext {
                purpose: Purpose::Open,
                new_instrument: true,
                ..action.clone()
            };
            let held = ActionContext {
                new_instrument: false,
                ..admitted.clone()
            };
            let denying = Autonomy {
                rules: vec![Rule {
                    id: rule_id("deny_all"),
                    when: compare(ConditionField::OrderUsd, Operator::Gte, decimal("0")),
                    then: AutonomyDecision::Deny,
                }],
                admission: AutonomyDecision::Auto,
                ..policy.clone()
            };
            let survives = classify(&denying, &admitted).map_err(|e| {
                TestCaseError::fail(format!("classify returns a decision, not {e}"))
            })?;
            prop_assert_eq!(
                survives.decision,
                AutonomyDecision::Deny,
                "an `auto` admission never loosens a deny rule, which is the control the loop needs"
            );
            let loose = Autonomy {
                admission: AutonomyDecision::Auto,
                ..policy.clone()
            };
            let rule_only = classify(&loose, &admitted).map_err(|e| {
                TestCaseError::fail(format!("classify returns a decision, not {e}"))
            })?;
            let mut previous: Option<u8> = None;
            let mut held_answer: Option<u8> = None;
            for admission in [
                AutonomyDecision::Auto,
                AutonomyDecision::Ask,
                AutonomyDecision::Deny,
            ] {
                let tightened = Autonomy {
                    admission,
                    ..policy.clone()
                };
                let new_instrument = classify(&tightened, &admitted).map_err(|e| {
                    TestCaseError::fail(format!("classify returns a decision, not {e}"))
                })?;
                let same_instrument = classify(&tightened, &held).map_err(|e| {
                    TestCaseError::fail(format!("classify returns a decision, not {e}"))
                })?;
                let (expected, _, _) = oracle_classify(&tightened, &admitted);
                let named = match new_instrument.decision {
                    AutonomyDecision::Auto => "auto",
                    AutonomyDecision::Ask => "ask",
                    AutonomyDecision::Deny => "deny",
                };
                prop_assert_eq!(named, expected.as_str(), "and each answer is the oracle's");
                let rank_new = rank(new_instrument.decision);
                if let Some(before) = previous {
                    prop_assert!(
                        rank_new >= before,
                        "a stricter admission never loosens the answer"
                    );
                }
                prop_assert!(
                    rank_new >= rank(rule_only.decision),
                    "and the ceiling never loosens the rules' own answer for the same action"
                );
                previous = Some(rank_new);
                match held_answer {
                    None => held_answer = Some(rank(same_instrument.decision)),
                    Some(first) => prop_assert_eq!(
                        rank(same_instrument.decision),
                        first,
                        "an order in a held instrument does not see the ceiling at all"
                    ),
                }
            }
            Ok(())
        },
    );
}

/// §6.4: an ASK needs two approvers exactly when its order value is **above**
/// `two_approver_above_usd`, and one otherwise. The comparison is recomputed here from the
/// generated numbers.
#[test]
fn the_approver_count_is_two_exactly_above_the_threshold() {
    check(
        (well_typed_policy(), action_context(), 0i128..=5_000i128),
        |(policy, action, threshold)| {
            let asking = Autonomy {
                rules: Vec::new(),
                default: AutonomyDecision::Ask,
                ..policy
            };
            let asking = Autonomy {
                approval: mandate_spec::document::Approval {
                    two_approver_above_usd: Some(common::dec(
                        &threshold.max(1).to_string(),
                        mandate_spec::DecGrammar::PositiveDecimal,
                    )),
                    ..asking.approval.clone()
                },
                ..asking
            };
            let opening = ActionContext {
                purpose: Purpose::Open,
                new_instrument: false,
                ..action
            };
            let decided = classify(&asking, &opening).map_err(|e| {
                TestCaseError::fail(format!("classify returns a decision, not {e}"))
            })?;
            prop_assert_eq!(decided.decision, AutonomyDecision::Ask);
            let order = rat_of_text(&opening.order_usd.to_string());
            let expected = if Scenario::money(threshold.max(1)).below(order) {
                2
            } else {
                1
            };
            prop_assert_eq!(
                decided.approval.map(|a| a.approvers_required.get()),
                Some(expected),
                "two approvers strictly above the threshold, one at it"
            );
            Ok(())
        },
    );
}

/// §6.3: `all`, `any`, `not` and the comparisons evaluate as the recursive oracle says, at every
/// depth V-017 allows.
#[test]
#[ignore = "pending E6-2"]
fn conditions_match_the_recursive_oracle() {
    check(
        (well_typed_condition(), action_context()),
        |(condition, action)| {
            let opening = ActionContext {
                purpose: Purpose::Open,
                new_instrument: false,
                ..action
            };
            let single = policy(
                vec![Rule {
                    id: rule_id("only"),
                    when: condition.clone(),
                    then: AutonomyDecision::Deny,
                }],
                AutonomyDecision::Auto,
                AutonomyDecision::Auto,
                None,
            );
            let decided = classify(&single, &opening).map_err(|e| {
                TestCaseError::fail(format!("classify returns a decision, not {e}"))
            })?;
            let matched = oracle_matches(&condition, &opening);
            prop_assert_eq!(
                decided.decision == AutonomyDecision::Deny,
                matched,
                "the rule fires exactly when the oracle says the tree holds"
            );
            prop_assert_eq!(
                decided.by.label(),
                if matched {
                    "rule:only".to_owned()
                } else {
                    "default".to_owned()
                }
            );
            Ok(())
        },
    );
}

/// §6.3, V-018 and V-023: a rule whose value is not of its field's type, or which reads the
/// reserved `unusual_input`, is refused; every other rule is evaluated. The type rules and the field
/// table are restated in this file rather than borrowed from the crate under test.
///
/// The expected code is [`oracle_refusal`]'s single answer, not a choice between two: the reserved
/// field is checked before the type rules, so "either code will do" would have hidden a swap of that
/// order.
#[test]
#[ignore = "pending E6-2"]
fn every_loaded_rule_is_type_correct() {
    let maybe_typed = prop_oneof![
        well_typed_leaf(),
        (
            prop::sample::select(vec![
                ConditionField::OrderUsd,
                ConditionField::Purpose,
                ConditionField::NewInstrument,
                ConditionField::UnusualInput,
                ConditionField::Session,
            ]),
            prop::sample::select(vec![
                Operator::Eq,
                Operator::Gt,
                Operator::In,
                Operator::Lte,
                Operator::NotIn
            ]),
            prop_oneof![
                Just(ConditionValue::Bool(true)),
                Just(ConditionValue::Text("open".to_owned())),
                Just(ConditionValue::Text("not_a_member".to_owned())),
                Just(ConditionValue::List(vec!["open".to_owned()])),
                Just(ConditionValue::List(Vec::new())),
                Just(decimal("100")),
                Just(decimal("2")),
            ],
        )
            .prop_map(|(field, op, value)| Condition::Compare { field, op, value }),
    ];
    check((maybe_typed, action_context()), |(condition, action)| {
        let opening = ActionContext {
            purpose: Purpose::Open,
            new_instrument: false,
            ..action
        };
        let single = policy(
            vec![Rule {
                id: rule_id("only"),
                when: condition.clone(),
                then: AutonomyDecision::Deny,
            }],
            AutonomyDecision::Auto,
            AutonomyDecision::Auto,
            None,
        );
        let expected = oracle_refusal(&condition);
        prop_assert_eq!(
            classify(&single, &opening).err().map(|e| e.code()),
            expected,
            "a rule is refused exactly when the oracle says so, with exactly that code"
        );
        Ok(())
    });
}

/// §6.2 step 2, DEC-05 and DEC-48: a denied proposal never reaches an approval, and a deferred one
/// is neither a deny nor a skip. The three verdicts give three outcomes for every policy.
#[test]
#[ignore = "pending E6-2"]
fn no_denied_proposal_ever_reaches_an_approval() {
    check(
        (well_typed_policy(), action_context()),
        |(policy, action)| {
            let proposal = proposal_for(&action);
            let denied = decide(&policy, &proposal, GateVerdict::Deny)
                .map_err(|e| TestCaseError::fail(format!("decide returns an outcome, not {e}")))?;
            prop_assert_eq!(denied, Outcome::Skipped, "a deny is skipped");
            let allowed = decide(&policy, &proposal, GateVerdict::Allow)
                .map_err(|e| TestCaseError::fail(format!("decide returns an outcome, not {e}")))?;
            let opening = ActionContext {
                purpose: Purpose::Open,
                ..action
            };
            let (expected, _, _) = oracle_classify(&policy, &opening);
            match allowed {
                Outcome::Classified(decision) => {
                    let named = match decision.decision {
                        AutonomyDecision::Auto => "auto",
                        AutonomyDecision::Ask => "ask",
                        AutonomyDecision::Deny => "deny",
                    };
                    prop_assert_eq!(
                        named,
                        expected.as_str(),
                        "an allowed proposal reaches the rules, which is what the deny must not do"
                    );
                }
                other => prop_assert!(false, "an allowed proposal is classified, not {:?}", other),
            }
            Ok(())
        },
    );
}

/// §6.2 step 2, DEC-130 item 15: `decide` returns `Deferred` exactly when the verdict defers, and
/// the builder never derives a defer of its own — it takes no market and cannot see the session.
#[test]
#[ignore = "pending E6-2"]
fn decide_returns_deferred_exactly_when_the_verdict_defers() {
    check(
        (well_typed_policy(), action_context()),
        |(policy, action)| {
            let proposal = proposal_for(&action);
            for verdict in [GateVerdict::Allow, GateVerdict::Deny, GateVerdict::Defer] {
                let outcome = decide(&policy, &proposal, verdict).map_err(|e| {
                    TestCaseError::fail(format!("decide returns an outcome, not {e}"))
                })?;
                prop_assert_eq!(
                    outcome == Outcome::Deferred,
                    verdict == GateVerdict::Defer,
                    "only a defer defers"
                );
            }
            Ok(())
        },
    );
}

/// A proposal carrying the generated action as an opening buy, for the two `decide` properties.
fn proposal_for(action: &ActionContext) -> Proposal {
    let opening = ActionContext {
        purpose: Purpose::Open,
        ..action.clone()
    };
    Proposal {
        action: Action::Buy {
            purpose: Purpose::Open,
            qty: qty("1"),
            limit_price: price("100"),
            order_usd: opening.order_usd,
            action: opening.clone(),
        },
        combined: mandate_builder::Combined {
            outputs_used: BTreeSet::new(),
            exit_conviction: Conviction::ZERO,
            buy_conviction: Conviction::ZERO,
            score: opening.combined_score,
        },
        sizes: mandate_builder::Sizes {
            cap: mandate_num::UsdExact::parse("1500").unwrap_or(mandate_num::UsdExact::zero()),
            current_mv: mandate_num::UsdExact::zero(),
            target_value: None,
            delta: None,
        },
        clipped_by: BTreeSet::new(),
    }
}

/// The seeds the coverage gate runs: the first five, by rule, not chosen for passing.
const COVERAGE_SEEDS: std::ops::RangeInclusive<u64> = 1..=5;

/// The coverage gate: [`agree`] run over [`sizing_scenario`] under each of [`COVERAGE_SEEDS`], with
/// the counters reset before each, and every run required to reach **every** action and **both**
/// clips.
///
/// This exists because the independent review of this PR found the sizing generator vacuous: 256
/// uniformly drawn cases reached no buy at all, so twelve properties past §8.3 step 2 asserted
/// nothing and four planted bugs in the after-values survived every test. A fuzz that never proposed
/// a buy is not evidence for a property about buys, and must not report success.
///
/// The seeds are fixed so the evidence is deterministic (ES-12), the same reason
/// `mandate-risk`'s own coverage gate pins its five. The properties themselves keep proptest's random
/// seed and their fuzzing power; only the evidence is pinned. The counts come from the **oracle**, so
/// the code under test cannot manufacture them.
#[test]
#[ignore = "pending E6-2"]
fn zz_the_generated_scenarios_reach_every_action_and_every_clip() {
    for seed in COVERAGE_SEEDS {
        for counter in EVERY_COUNTER {
            counter.store(0, Ordering::Relaxed);
        }
        let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
            cases: 256,
            rng_seed: RngSeed::Fixed(seed),
            failure_persistence: None,
            ..ProptestConfig::default()
        });
        runner
            .run(&sizing_scenario(), |scenario| {
                agree(&scenario)?;
                Ok(())
            })
            .unwrap_or_else(|failure| {
                panic!("seed {seed}: the agreement helper failed: {failure}")
            });
        coverage_reached(seed, "the sizing scenarios");
    }
}

fn coverage_reached(seed: u64, what: &str) {
    let seen: Vec<(&str, u32)> = vec![
        ("a buy", SAW_BUY.load(Ordering::Relaxed)),
        ("a discretionary exit", SAW_SELL.load(Ordering::Relaxed)),
        (
            "a hold on no fresh outputs",
            SAW_NO_FRESH_OUTPUTS.load(Ordering::Relaxed),
        ),
        (
            "a hold between the thresholds",
            SAW_BETWEEN_THRESHOLDS.load(Ordering::Relaxed),
        ),
        (
            "a hold at or above the target",
            SAW_AT_OR_ABOVE_TARGET.load(Ordering::Relaxed),
        ),
        (
            "a hold inside the band",
            SAW_WITHIN_BAND.load(Ordering::Relaxed),
        ),
        (
            "a hold below the band after clipping",
            SAW_BELOW_BAND.load(Ordering::Relaxed),
        ),
        (
            "a hold below the minimum order",
            SAW_BELOW_MINIMUM.load(Ordering::Relaxed),
        ),
        (
            "a hold on the projected average",
            SAW_MAX_AVG_PRICE.load(Ordering::Relaxed),
        ),
        ("a limits clip", SAW_CLIP_LIMITS.load(Ordering::Relaxed)),
        ("a goal clip", SAW_CLIP_GOAL.load(Ordering::Relaxed)),
    ];
    assert_eq!(
        EVERY_COUNTER.len(),
        seen.len() + 2,
        "EVERY_COUNTER resets every counter `seen` reads, plus SAW_NO_POSITION and \
         SAW_EXITS_DISABLED, which the `hand` suite pins deterministically and the gate does not \
         require; a counter read here but missing from the reset list would let a later seed pass on \
         an earlier seed's evidence"
    );
    let missing: BTreeSet<&str> = seen
        .iter()
        .filter_map(|(name, count)| (*count == 0).then_some(*name))
        .collect();
    assert!(
        missing.is_empty(),
        "seed {seed}: {what} never reached {missing:?}, so the properties that depend on them are \
         not evidence; counts were {seen:?}"
    );
}
