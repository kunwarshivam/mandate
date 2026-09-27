//! The report: both metric blocks, the digests of what produced them, and the canonical bytes whose
//! SHA-256 is a run's identity (FR-4.5, DEC-127 item 15).

use core::fmt;

use crate::metrics::Metrics;
use crate::{BacktestError, BacktestInput, RunConfig};
use mandate_accounting::{AccountType, Config as FeeConfig, TafCapBasis};
use mandate_canon::{Digest, Int, Key, Object, Value};
use mandate_num::{NumError, Ratio, TickRule};
use mandate_sim::{Session, SimBar, SimConfig, Slippage};

use crate::strategy::{Strategy, StrategyConfig};

/// The sign of a Sharpe's excess mean, reported beside the squared figure because squaring loses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sign {
    Negative,
    Zero,
    Positive,
}

impl Sign {
    /// The canonical spelling in a report: `negative`, `zero`, or `positive`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Negative => "negative",
            Self::Zero => "zero",
            Self::Positive => "positive",
        }
    }
}

/// Why a block's dispersion or Sharpe figures are absent (DEC-127 item 7). `ZeroVariance` covers a
/// positive exact variance that rounds to zero at 12 places, not only a genuinely flat series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbsentStatistics {
    FewerThanTwoPeriods,
    ZeroVariance,
}

impl AbsentStatistics {
    /// The canonical spelling in a report.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FewerThanTwoPeriods => "fewer_than_two_periods",
            Self::ZeroVariance => "zero_variance",
        }
    }
}

/// The digests that say which snapshot and which configuration a report came from (FR-4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputDigests {
    pub bars: Digest,
    pub config: Digest,
}

impl InputDigests {
    /// The digest of the bar input's canonical form and of the configuration's, each over
    /// `mandate_canon::to_canonical` of a value holding every field as canonical text.
    ///
    /// The bar input is everything the fill model reads about the snapshot: the rows, the coverage
    /// start, and the 20-session median at each bar's start. The last two change which bars can fill
    /// and how much (spec §6.4 rules 1 and 3), so a report that omitted them could claim two runs
    /// came from the same inputs while their fills differed.
    pub fn of(input: &BacktestInput<'_>) -> Result<Self, BacktestError> {
        Ok(Self {
            bars: Digest::of(&mandate_canon::to_canonical(&bar_input(input)?)),
            config: Digest::of(&mandate_canon::to_canonical(&configuration(input.config)?)),
        })
    }
}

/// One run's report: the strategy's figures, the buy-and-hold benchmark's, and the exact difference
/// of their total returns. `report_version` rises whenever a definition changes, as the fold's
/// `fold_version` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub report_version: u32,
    pub inputs: InputDigests,
    pub strategy: Metrics,
    pub benchmark: Metrics,
    pub excess_total_return: Ratio,
}

/// The version this crate writes. A change to any figure's definition raises it, so a report always
/// says which rules produced it.
pub const REPORT_VERSION: u32 = 1;

impl Report {
    /// The report as canonical JSON: decimals as the canonical text their `Display` produces, counts
    /// and versions as integers, dates as `YYYY-MM-DD`, the sign and the absence reason as their
    /// strings, and an absent figure as `null`. Keys sort themselves, because a canonical object is a
    /// `BTreeMap`.
    pub fn canonical(&self) -> Result<Value, BacktestError> {
        object(vec![
            ("report_version", int(self.report_version)?),
            (
                "inputs",
                object(vec![
                    ("bars", text(&self.inputs.bars.to_hex())),
                    ("config", text(&self.inputs.config.to_hex())),
                ])?,
            ),
            ("strategy", metrics(&self.strategy)?),
            ("benchmark", metrics(&self.benchmark)?),
            ("excess_total_return", decimal(self.excess_total_return)),
        ])
    }

    /// `to_canonical` of [`Report::canonical`]: the bytes two runs of the same inputs must match
    /// byte for byte (the story's acceptance criterion).
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, BacktestError> {
        Ok(mandate_canon::to_canonical(&self.canonical()?))
    }

    /// The SHA-256 of those bytes: a run's identity, and what a `BacktestRunRecorded` event will
    /// carry once a runner journals it (journal spec §12).
    pub fn digest(&self) -> Result<Digest, BacktestError> {
        Ok(Digest::of(&self.canonical_bytes()?))
    }
}

/// One metric block, every decimal as the canonical text its `Display` produces, every count as an
/// integer, and every undefined figure as `Null` beside the reason it is absent (DEC-127 item 7).
fn metrics(block: &Metrics) -> Result<Value, BacktestError> {
    object(vec![
        ("period_count", int(block.period_count)?),
        ("first_date", decimal(block.first_date)),
        ("last_date", decimal(block.last_date)),
        ("starting_equity", decimal(block.starting_equity)),
        ("ending_equity", decimal(block.ending_equity)),
        ("net_pnl", decimal(block.net_pnl)),
        ("total_return", decimal(block.total_return)),
        ("return_sum", decimal(block.return_sum)),
        (
            "return_sum_of_squares",
            decimal(block.return_sum_of_squares),
        ),
        ("mean_return", decimal(block.mean_return)),
        ("variance", optional(block.variance)),
        ("volatility", optional(block.volatility)),
        ("periods_per_year", int(block.periods_per_year)?),
        ("variance_annualized", optional(block.variance_annualized)),
        (
            "volatility_annualized",
            optional(block.volatility_annualized),
        ),
        ("risk_free_per_period", decimal(block.risk_free_per_period)),
        ("sharpe_squared", optional(block.sharpe_squared)),
        ("sharpe_sign", text(block.sharpe_sign.as_str())),
        ("sharpe", optional(block.sharpe)),
        (
            "sharpe_squared_annualized",
            optional(block.sharpe_squared_annualized),
        ),
        ("sharpe_annualized", optional(block.sharpe_annualized)),
        ("max_drawdown", decimal(block.max_drawdown)),
        ("max_drawdown_usd", decimal(block.max_drawdown_usd)),
        (
            "max_drawdown_peak_period",
            int(block.max_drawdown_peak_period)?,
        ),
        (
            "max_drawdown_trough_period",
            int(block.max_drawdown_trough_period)?,
        ),
        ("buy_notional", decimal(block.buy_notional)),
        ("sell_notional", decimal(block.sell_notional)),
        ("traded_notional", decimal(block.traded_notional)),
        ("fill_count", int(block.fill_count)?),
        ("turnover", decimal(block.turnover)),
        ("fees_total", decimal(block.fees_total)),
        ("fees_charged", decimal(block.fees_charged)),
        ("fees_accrued", decimal(block.fees_accrued)),
        ("fees_asset", decimal(block.fees_asset)),
        ("submitted_qty", decimal(block.submitted_qty)),
        ("filled_qty", decimal(block.filled_qty)),
        (
            "absent",
            block
                .absent
                .map_or(Value::Null, |reason| text(reason.as_str())),
        ),
    ])
}

/// Every bar the run was given, the coverage start the fill model measures a session's first covered
/// bar from (DEC-106 item 4), and the median volume it would read at each bar's start, `null` where
/// there is none (spec §6.4 rule 3). The medians are taken for every bar rather than only for the
/// bars that open a session, so this digest does not restate the model's rule for which bars ask.
fn bar_input(input: &BacktestInput<'_>) -> Result<Value, BacktestError> {
    let rows = input
        .bars
        .iter()
        .map(bar_row)
        .collect::<Result<Vec<Value>, BacktestError>>()?;
    let medians = input
        .bars
        .iter()
        .map(|bar| optional(input.first_bar_volumes.median_at(bar.start)))
        .collect();
    object(vec![
        ("rows", Value::Array(rows)),
        ("coverage_start", decimal(input.coverage_start)),
        ("first_bar_medians", Value::Array(medians)),
    ])
}

/// One bar as the digest of a run's inputs sees it: every field the fill model reads, so a changed
/// bar changes the digest that says which snapshot produced a report (FR-4.5). The session's spelling
/// is written here rather than in a function of its own, because no case changes a bar's session
/// alone, so a function that answered with one constant name would be indistinguishable from it.
fn bar_row(bar: &SimBar) -> Result<Value, BacktestError> {
    object(vec![
        ("start", decimal(bar.start)),
        ("open", decimal(bar.open)),
        ("high", decimal(bar.high)),
        ("low", decimal(bar.low)),
        ("close", decimal(bar.close)),
        ("volume", decimal(bar.volume)),
        ("trade_date", decimal(bar.trade_date)),
        (
            "session",
            text(match bar.session {
                Session::Overnight => "overnight",
                Session::PreMarket => "pre_market",
                Session::Regular => "regular",
                Session::AfterHours => "after_hours",
                Session::Continuous => "continuous",
            }),
        ),
        ("session_start", decimal(bar.session_start)),
        ("auction", Value::Bool(bar.auction)),
    ])
}

/// Everything a run is pinned to, so a reader knows which configuration produced a report
/// (FR-4.5). The fee configuration's trading calendar exposes no fields to read, so it is not part
/// of this digest; the journal pins the whole fee configuration by content hash where a run is
/// recorded (journal spec §12), which is the story that appends `BacktestRunRecorded`.
fn configuration(config: &RunConfig) -> Result<Value, BacktestError> {
    object(vec![
        ("sim", sim(&config.sim)?),
        ("fees", fees(&config.fees)?),
        (
            "account_type",
            text(match config.account_type {
                AccountType::Cash => "cash",
                AccountType::Margin => "margin",
            }),
        ),
        ("instrument_id", text(config.instrument_id.as_str())),
        (
            "instrument",
            object(vec![
                ("asset_class", text(config.instrument.asset_class.as_str())),
                (
                    "increment",
                    text(match config.instrument.increment {
                        mandate_num::ShareIncrement::Fractional => "fractional",
                        mandate_num::ShareIncrement::Whole => "whole",
                    }),
                ),
            ])?,
        ),
        ("tick", tick(config.tick)?),
        ("starting_cash", decimal(config.starting_cash)),
        ("strategy", strategy(config.strategy)?),
        (
            "metrics",
            object(vec![
                ("periods_per_year", int(config.metrics.periods_per_year)?),
                (
                    "risk_free_per_period",
                    decimal(config.metrics.risk_free_per_period),
                ),
            ])?,
        ),
    ])
}

fn sim(config: &SimConfig) -> Result<Value, BacktestError> {
    let latency = |nanos: u64| -> Result<Value, BacktestError> {
        Int::new(nanos)
            .map(Value::Int)
            .ok_or(BacktestError::Num(NumError::Overflow))
    };
    object(vec![
        (
            "decision_latency_nanos",
            latency(config.decision_latency.nanos())?,
        ),
        (
            "approval_latency_nanos",
            latency(config.approval_latency.nanos())?,
        ),
        (
            "slippage",
            match config.slippage {
                Slippage::Fixed {
                    half_spread_bps,
                    impact_bps,
                } => object(vec![
                    ("model", text("fixed")),
                    ("half_spread_bps", decimal(half_spread_bps)),
                    ("impact_bps", decimal(impact_bps)),
                ])?,
                Slippage::Sqrt {
                    half_spread_bps,
                    coefficient_bps,
                } => object(vec![
                    ("model", text("sqrt")),
                    ("half_spread_bps", decimal(half_spread_bps)),
                    ("coefficient_bps", decimal(coefficient_bps)),
                ])?,
            },
        ),
        ("volume_cap_fraction", decimal(config.volume_cap_fraction)),
    ])
}

fn fees(config: &FeeConfig) -> Result<Value, BacktestError> {
    let equities = object(vec![
        ("sec_rate", decimal(config.equities.sec_rate)),
        ("taf_per_share", decimal(config.equities.taf_per_share)),
        ("taf_cap", decimal(config.equities.taf_cap.to_usd())),
        (
            "taf_cap_basis",
            text(match config.equities.taf_cap_basis {
                TafCapBasis::PerExecution => "per_execution",
                TafCapBasis::PerOrder => "per_order",
            }),
        ),
        ("cat_per_share", decimal(config.equities.cat_per_share)),
    ])?;
    let crypto = object(vec![
        ("maker_bps", decimal(config.crypto.maker)),
        ("taker_bps", decimal(config.crypto.taker)),
    ])?;
    object(vec![("equities", equities), ("crypto", crypto)])
}

fn tick(rule: TickRule) -> Result<Value, BacktestError> {
    match rule {
        TickRule::RegNmsEquity => object(vec![("rule", text("reg_nms_equity"))]),
        TickRule::Increment(increment) => object(vec![
            ("rule", text("increment")),
            ("increment", decimal(increment)),
        ]),
    }
}

fn strategy(strategy: Strategy) -> Result<Value, BacktestError> {
    match strategy {
        Strategy::MovingAverageCrossover(StrategyConfig {
            fast_periods,
            slow_periods,
            collar,
            target_notional,
        }) => object(vec![
            ("kind", text("moving_average_crossover")),
            ("fast_periods", int(fast_periods)?),
            ("slow_periods", int(slow_periods)?),
            ("collar_bps", decimal(collar)),
            ("target_notional", decimal(target_notional)),
        ]),
        Strategy::BuyAndHold { collar } => object(vec![
            ("kind", text("buy_and_hold")),
            ("collar_bps", decimal(collar)),
        ]),
    }
}

/// An object of `members`, or `not_canonical` for a key outside the canonical key grammar, which
/// none of this crate's literal keys is (journal spec §4.1).
fn object(members: Vec<(&str, Value)>) -> Result<Value, BacktestError> {
    members
        .into_iter()
        .map(|(name, value)| {
            Key::new(name)
                .map(|key| (key, value))
                .map_err(|_| BacktestError::Num(NumError::NotCanonical))
        })
        .collect::<Result<Object, BacktestError>>()
        .map(Value::Object)
}

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

/// A decimal, a date, or an instant as the canonical text its `Display` produces (spec §2.1).
fn decimal<T: fmt::Display>(value: T) -> Value {
    Value::Str(value.to_string())
}

/// An absent figure is `Null`; a present one is its canonical text.
fn optional<T: fmt::Display>(value: Option<T>) -> Value {
    value.map_or(Value::Null, decimal)
}

fn int(count: u32) -> Result<Value, BacktestError> {
    Int::new(u64::from(count))
        .map(Value::Int)
        .ok_or(BacktestError::Num(NumError::Overflow))
}
