//! `BacktestRunRecorded`'s payload (journal spec §9.18, DEC-1001 item 4) and the configuration
//! digest it names.
//!
//! The payload names the report by artifact reference and carries every digest that reproduces it,
//! so the record holds no figure of its own. The caller stores the report's canonical bytes as the
//! artifact before it appends the record (journal before acting, `AGENTS.md` rule 5).

use mandate_canon::{Digest, Value};
use mandate_risk::{DayTradeRegime, EtpClass, Exchange, GateConfig};

use super::report::{VersionDigests, VersionReport, reference};
use super::{VersionInput, half_spread};
use crate::report::{decimal, int, object, text};
use crate::{BacktestError, MetricsConfig};

/// What one `BacktestRunRecorded` says: the run's id, the version, the input digests, the
/// runner's build, and either the report or the run's refusal.
#[derive(Debug, Clone, Copy)]
pub struct RunRecord<'a> {
    pub backtest_id: &'a str,
    pub mandate_version: Digest,
    pub inputs: VersionDigests,
    pub code_build: Digest,
    pub outcome: Result<&'a VersionReport, &'a BacktestError>,
}

/// The `BacktestRunRecorded` payload at `schema_version` 1 (journal spec §9.18): `report` and
/// `refusal` are `null` exactly as rule 143 says, and `marker` is always `backtest`.
pub fn backtest_run_recorded(record: &RunRecord<'_>) -> Result<Value, BacktestError> {
    let (report, result, refusal) = match record.outcome {
        Ok(report) => (reference(&report.digest()?), "completed", Value::Null),
        Err(error) => (Value::Null, "refused", text(error.code())),
    };
    object(vec![
        ("backtest_id", text(record.backtest_id)),
        ("mandate_version", reference(&record.mandate_version)),
        ("data_snapshot", reference(&record.inputs.bars)),
        ("model_outputs", reference(&record.inputs.model_outputs)),
        ("theses", reference(&record.inputs.theses)),
        ("configuration", reference(&record.inputs.config)),
        ("code_build", reference(&record.code_build)),
        ("report", report),
        ("report_version", int(super::VERSION_REPORT_VERSION)?),
        ("marker", text("backtest")),
        ("result", text(result)),
        ("refusal", refusal),
    ])
}

/// Every setting the run reads besides the mandate, which the report names by its version hash,
/// and the bars and records, which have digests of their own: the fill model, fees, the gate's
/// settings, the connection's profile by content hash, the account, the instrument reference, the
/// corporate-action dates, and the report's statistics.
pub(super) fn configuration(input: &VersionInput<'_>) -> Result<Value, BacktestError> {
    let settings = input.settings;
    let instrument = input.instrument;
    object(vec![
        ("sim", crate::report::sim(&settings.sim)?),
        ("half_spread_bps", decimal(half_spread(&settings.sim))),
        ("fees", crate::report::fees(&settings.fees)?),
        ("gate", gate(&settings.gate)?),
        ("profile", reference(&settings.profile.content_hash())),
        (
            "account_type",
            text(match settings.account_type {
                mandate_accounting::AccountType::Cash => "cash",
                mandate_accounting::AccountType::Margin => "margin",
            }),
        ),
        ("regime", regime(settings.regime)?),
        ("metrics", metrics(&settings.metrics)?),
        ("min_order_usd", decimal(settings.min_order_usd)),
        ("mark_max_age_s", int(settings.mark_max_age_s)?),
        ("benchmark_collar_bps", decimal(settings.benchmark_collar)),
        (
            "instrument",
            object(vec![
                ("asset", text(instrument.asset.as_str())),
                ("ledger_id", text(instrument.ledger_id.as_str())),
                (
                    "asset_class",
                    text(instrument.instrument.asset_class.as_str()),
                ),
                (
                    "increment",
                    text(match instrument.instrument.increment {
                        mandate_num::ShareIncrement::Fractional => "fractional",
                        mandate_num::ShareIncrement::Whole => "whole",
                    }),
                ),
                ("tick", crate::report::tick(instrument.tick)?),
                ("qty_increment", decimal(instrument.qty_increment)),
                ("min_order_size", decimal(instrument.min_order_size)),
                ("exchange", text(exchange(instrument.exchange))),
                (
                    "etp_classifications",
                    instrument
                        .etp_classifications
                        .iter()
                        .map(|classification| {
                            object(vec![
                                ("at", decimal(classification.at)),
                                ("class", text(etp(classification.class))),
                            ])
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map(Value::Array)?,
                ),
                ("fractionable", Value::Bool(instrument.fractionable)),
                ("ipo", Value::Bool(instrument.ipo)),
                ("ptp_no_exception", Value::Bool(instrument.ptp_no_exception)),
            ])?,
        ),
        (
            "corporate_actions",
            Value::Array(
                input
                    .corporate_actions
                    .iter()
                    .map(|date| decimal(*date))
                    .collect(),
            ),
        ),
    ])
}

fn gate(config: &GateConfig) -> Result<Value, BacktestError> {
    object(vec![
        ("price_floor", decimal(config.price_floor)),
        ("liquidity_floor_usd", decimal(config.liquidity_floor_usd)),
        (
            "crypto_liquidity_floor_usd",
            decimal(config.crypto_liquidity_floor_usd),
        ),
        (
            "collar_liquid_threshold_usd",
            decimal(config.collar_liquid_threshold_usd),
        ),
        ("collar_liquid_x", decimal(config.collar_liquid_x)),
        ("collar_other_x", decimal(config.collar_other_x)),
        ("collar_crypto_x", decimal(config.collar_crypto_x)),
        ("collar_passive_band", decimal(config.collar_passive_band)),
        (
            "opposite_fill_interval_s",
            int(config.opposite_fill_interval_s)?,
        ),
        ("min_resting_time_s", int(config.min_resting_time_s)?),
        ("order_to_fill_max", int(config.order_to_fill_max)?),
        (
            "order_to_fill_min_orders",
            int(config.order_to_fill_min_orders)?,
        ),
        (
            "order_size_participation",
            decimal(config.order_size_participation),
        ),
        ("daily_participation", decimal(config.daily_participation)),
        ("close_window_minutes", int(config.close_window_minutes)?),
        (
            "legacy_pdt_equity_threshold",
            decimal(config.legacy_pdt_equity_threshold),
        ),
        (
            "etp_classification_max_age_s",
            int(config.etp_classification_max_age_s)?,
        ),
    ])
}

fn regime(regime: DayTradeRegime) -> Result<Value, BacktestError> {
    match regime {
        DayTradeRegime::LegacyPdt => object(vec![("kind", text("legacy_pdt"))]),
        DayTradeRegime::IntradayMargin { maintenance_excess } => object(vec![
            ("kind", text("intraday_margin")),
            ("maintenance_excess", decimal(maintenance_excess)),
        ]),
    }
}

fn metrics(config: &MetricsConfig) -> Result<Value, BacktestError> {
    object(vec![
        ("periods_per_year", int(config.periods_per_year)?),
        ("risk_free_per_period", decimal(config.risk_free_per_period)),
    ])
}

fn exchange(exchange: Exchange) -> &'static str {
    match exchange {
        Exchange::Nasdaq => "nasdaq",
        Exchange::Nyse => "nyse",
        Exchange::Arca => "arca",
        Exchange::Amex => "amex",
        Exchange::Bats => "bats",
        Exchange::Otc => "otc",
        Exchange::Other => "other",
    }
}

fn etp(class: EtpClass) -> &'static str {
    match class {
        EtpClass::Plain => "plain",
        EtpClass::Complex => "complex",
        EtpClass::Unclassified => "unclassified",
    }
}
