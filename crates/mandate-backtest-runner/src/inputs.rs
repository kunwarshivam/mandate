//! The runner's five input files ([DEC-1144](../../../docs/project/decisions/DEC-1144.md) item 3),
//! each one JSON object that `mandate-canon` parses: decimals are strings, counts are integers,
//! instants are RFC 3339 UTC text, and dates are `YYYY-MM-DD`. Every member is required, `null`
//! only where a member is nullable, and a member no reader names is refused, so a misspelt setting
//! fails closed rather than falling back to anything.

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{
    AccountType, AssetClass, Config, CryptoFees, EquityFees, FeeRounding, InstrumentId, TafCapBasis,
};
use mandate_backtest::MetricsConfig;
use mandate_builder::{Direction, ModelOutput, ModelVersion};
use mandate_canon::{Digest, Value, parse};
use mandate_domain::{
    AssetId, CapabilityProfile, Cell, Idempotency, MarketSession, OrderType, ProtectionForm,
    QuantityForm, Retry, Row, TimeInForce,
};
use mandate_num::{
    Bps, Conviction, FeeCap, FeePerShare, FeeRate, Fraction, Price, Qty, Ratio, ShareIncrement,
    TickRule, Unit, Usd,
};
use mandate_risk::{DayTradeRegime, EtpClass, Exchange, GateConfig};
use mandate_sim::{FirstBarVolumes, Instrument, Nanos, Session, SimBar, SimConfig, Slippage};
use mandate_spec::document::ModelId;
use mandate_time::{Date, TradingCalendar, UtcNanos};
use mandate_version_backtest::{
    EtpClassification, InstrumentReference, RecordedOutput, RecordedThesis, VersionSettings,
};

use crate::RunnerError;

/// The five files' bytes, by name, as the directory holds them.
#[derive(Debug, Clone, Copy)]
pub struct InputFiles<'a> {
    pub bars: &'a [u8],
    pub outputs: &'a [u8],
    pub theses: &'a [u8],
    pub instrument: &'a [u8],
    pub settings: &'a [u8],
}

/// The file names [`InputFiles`] are read from, in its order.
pub const FILE_NAMES: [&str; 5] = [
    "bars.json",
    "outputs.json",
    "theses.json",
    "instrument.json",
    "settings.json",
];

/// Every input of one run but the mandate, decoded.
#[derive(Debug, Clone)]
pub struct Inputs {
    pub bars: Vec<SimBar>,
    pub coverage_start: UtcNanos,
    pub medians: Medians,
    pub corporate_actions: Vec<Date>,
    pub outputs: Vec<RecordedOutput>,
    pub theses: Vec<RecordedThesis>,
    pub instrument: InstrumentReference,
    pub settings: VersionSettings,
    /// The asset classes the profile protects with a stop-limit, which validation reads (V-008).
    pub stop_limit_classes: BTreeSet<AssetClass>,
}

/// Each bar's first-bar median, by the bar's start, as `bars.json` states it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Medians(BTreeMap<UtcNanos, Qty>);

impl FirstBarVolumes for Medians {
    fn median_at(&self, bar_start: UtcNanos) -> Option<Qty> {
        self.0.get(&bar_start).copied()
    }
}

/// Decodes the five files.
///
/// # Errors
/// [`RunnerError::Input`] naming the first file and member that is missing, unknown, or malformed.
pub fn decode(files: &InputFiles<'_>) -> Result<Inputs, RunnerError> {
    let [
        bars_file,
        outputs_file,
        theses_file,
        instrument_file,
        settings_file,
    ] = FILE_NAMES;
    let bars = document(bars_file, files.bars)?;
    let [coverage_start, corporate_actions, rows] = fields(
        &bars,
        bars_file,
        ["coverage_start", "corporate_actions", "bars"],
    )?;
    let mut decoded = Vec::new();
    let mut medians = BTreeMap::new();
    for (index, row) in list(rows, &format!("{bars_file}/bars"))?.iter().enumerate() {
        let (bar, median) = bar(row, &format!("{bars_file}/bars/{index}"))?;
        if let Some(median) = median {
            medians.insert(bar.start, median);
        }
        decoded.push(bar);
    }
    let corporate_actions = list(corporate_actions, &format!("{bars_file}/corporate_actions"))?
        .iter()
        .enumerate()
        .map(|(i, d)| date(d, &format!("{bars_file}/corporate_actions/{i}")))
        .collect::<Result<_, _>>()?;
    let outputs = document(outputs_file, files.outputs)?;
    let [outputs] = fields(&outputs, outputs_file, ["outputs"])?;
    let outputs = list(outputs, &format!("{outputs_file}/outputs"))?
        .iter()
        .enumerate()
        .map(|(i, o)| output(o, &format!("{outputs_file}/outputs/{i}")))
        .collect::<Result<_, _>>()?;
    let theses = document(theses_file, files.theses)?;
    let [theses] = fields(&theses, theses_file, ["theses"])?;
    let theses = list(theses, &format!("{theses_file}/theses"))?
        .iter()
        .enumerate()
        .map(|(i, t)| thesis(t, &format!("{theses_file}/theses/{i}")))
        .collect::<Result<_, _>>()?;
    let instrument = reference(
        &document(instrument_file, files.instrument)?,
        instrument_file,
    )?;
    let (settings, stop_limit_classes) =
        settings(&document(settings_file, files.settings)?, settings_file)?;
    Ok(Inputs {
        bars: decoded,
        coverage_start: instant(coverage_start, &format!("{bars_file}/coverage_start"))?,
        medians: Medians(medians),
        corporate_actions,
        outputs,
        theses,
        instrument,
        settings,
        stop_limit_classes,
    })
}

fn invalid(path: &str) -> RunnerError {
    RunnerError::Input(path.to_owned())
}

fn document(name: &str, bytes: &[u8]) -> Result<Value, RunnerError> {
    parse(bytes).map_err(|_| invalid(name))
}

/// `value`'s members `names`, in that order, when it is an object holding exactly those.
fn fields<'a, const N: usize>(
    value: &'a Value,
    path: &str,
    names: [&str; N],
) -> Result<[&'a Value; N], RunnerError> {
    let object = value.as_object().ok_or_else(|| invalid(path))?;
    if let Some(unknown) = object.keys().find(|k| !names.contains(&k.as_str())) {
        return Err(invalid(&format!("{path}/{}", unknown.as_str())));
    }
    let found = names
        .iter()
        .map(|name| {
            object
                .get(*name)
                .ok_or_else(|| invalid(&format!("{path}/{name}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    found.try_into().map_err(|_| invalid(path))
}

fn text<'a>(value: &'a Value, path: &str) -> Result<&'a str, RunnerError> {
    value.as_str().ok_or_else(|| invalid(path))
}

fn parsed<T, E>(
    value: &Value,
    path: &str,
    read: fn(&str) -> Result<T, E>,
) -> Result<T, RunnerError> {
    read(text(value, path)?).map_err(|_| invalid(path))
}

fn list<'a>(value: &'a Value, path: &str) -> Result<&'a [Value], RunnerError> {
    value.as_array().ok_or_else(|| invalid(path))
}

fn instant(value: &Value, path: &str) -> Result<UtcNanos, RunnerError> {
    parsed(value, path, UtcNanos::parse_rfc3339)
}

fn date(value: &Value, path: &str) -> Result<Date, RunnerError> {
    parsed(value, path, Date::parse)
}

fn flag(value: &Value, path: &str) -> Result<bool, RunnerError> {
    match value {
        Value::Bool(b) => Ok(*b),
        _ => Err(invalid(path)),
    }
}

fn count(value: &Value, path: &str) -> Result<u32, RunnerError> {
    value
        .as_int()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| invalid(path))
}

/// The variant `table` pairs with `value`'s text.
fn word<T: Copy>(value: &Value, path: &str, table: &[(&str, T)]) -> Result<T, RunnerError> {
    let spelled = text(value, path)?;
    table
        .iter()
        .find(|(name, _)| *name == spelled)
        .map(|(_, variant)| *variant)
        .ok_or_else(|| invalid(path))
}

/// The set of variants `value`'s list of texts names.
fn words<T: Copy + Ord>(
    value: &Value,
    path: &str,
    table: &[(&str, T)],
) -> Result<BTreeSet<T>, RunnerError> {
    list(value, path)?
        .iter()
        .enumerate()
        .map(|(i, v)| word(v, &format!("{path}/{i}"), table))
        .collect()
}

fn bar(value: &Value, path: &str) -> Result<(SimBar, Option<Qty>), RunnerError> {
    let at = |name: &str| format!("{path}/{name}");
    let [
        start,
        open,
        high,
        low,
        close,
        volume,
        trade_date,
        session,
        session_start,
        auction,
        median,
    ] = fields(
        value,
        path,
        [
            "start",
            "open",
            "high",
            "low",
            "close",
            "volume",
            "trade_date",
            "session",
            "session_start",
            "auction",
            "first_bar_median",
        ],
    )?;
    let sessions = [
        ("overnight", Session::Overnight),
        ("pre_market", Session::PreMarket),
        ("regular", Session::Regular),
        ("after_hours", Session::AfterHours),
        ("continuous", Session::Continuous),
    ];
    let median = match median {
        Value::Null => None,
        other => Some(parsed(other, &at("first_bar_median"), Qty::parse)?),
    };
    let bar = SimBar {
        start: instant(start, &at("start"))?,
        open: parsed(open, &at("open"), Price::parse)?,
        high: parsed(high, &at("high"), Price::parse)?,
        low: parsed(low, &at("low"), Price::parse)?,
        close: parsed(close, &at("close"), Price::parse)?,
        volume: parsed(volume, &at("volume"), Qty::parse)?,
        trade_date: date(trade_date, &at("trade_date"))?,
        session: word(session, &at("session"), &sessions)?,
        session_start: instant(session_start, &at("session_start"))?,
        auction: flag(auction, &at("auction"))?,
    };
    Ok((bar, median))
}

fn output(value: &Value, path: &str) -> Result<RecordedOutput, RunnerError> {
    let at = |name: &str| format!("{path}/{name}");
    let [
        received_at,
        model_id,
        model_version,
        content_hash,
        instrument,
        as_of,
        expires_at,
        direction,
        conviction,
        confidence,
    ] = fields(
        value,
        path,
        [
            "received_at",
            "model_id",
            "model_version",
            "content_hash",
            "instrument",
            "as_of",
            "expires_at",
            "direction",
            "conviction",
            "confidence",
        ],
    )?;
    let hash = text(content_hash, &at("content_hash"))?
        .strip_prefix("sha256:")
        .and_then(Digest::from_hex)
        .ok_or_else(|| invalid(&at("content_hash")))?;
    Ok(RecordedOutput {
        received_at: instant(received_at, &at("received_at"))?,
        output: ModelOutput {
            model_id: parsed(model_id, &at("model_id"), ModelId::parse)?,
            model_version: parsed(model_version, &at("model_version"), ModelVersion::parse)?,
            content_hash: hash,
            instrument: parsed(instrument, &at("instrument"), AssetId::parse)?,
            as_of: instant(as_of, &at("as_of"))?,
            expires_at: instant(expires_at, &at("expires_at"))?,
            direction: word(direction, &at("direction"), &[("long", Direction::Long)])?,
            conviction: parsed(conviction, &at("conviction"), Conviction::parse)?,
            confidence: parsed(confidence, &at("confidence"), Unit::parse)?,
        },
    })
}

fn thesis(value: &Value, path: &str) -> Result<RecordedThesis, RunnerError> {
    let at = |name: &str| format!("{path}/{name}");
    let [
        thesis_id,
        received_at,
        instrument,
        as_of,
        expires_at,
        admitted,
        confidence,
        invalidated_at,
    ] = fields(
        value,
        path,
        [
            "thesis_id",
            "received_at",
            "instrument",
            "as_of",
            "expires_at",
            "admitted",
            "confidence",
            "invalidated_at",
        ],
    )?;
    let thesis_id = text(thesis_id, &at("thesis_id"))?;
    if thesis_id.is_empty() {
        return Err(invalid(&at("thesis_id")));
    }
    Ok(RecordedThesis {
        thesis_id: thesis_id.to_owned(),
        received_at: instant(received_at, &at("received_at"))?,
        instrument: parsed(instrument, &at("instrument"), AssetId::parse)?,
        as_of: instant(as_of, &at("as_of"))?,
        expires_at: instant(expires_at, &at("expires_at"))?,
        admitted: flag(admitted, &at("admitted"))?,
        confidence: parsed(confidence, &at("confidence"), Unit::parse)?,
        invalidated_at: match invalidated_at {
            Value::Null => None,
            other => Some(instant(other, &at("invalidated_at"))?),
        },
    })
}

const ASSET_CLASSES: [(&str, AssetClass); 2] = [
    ("us_equity", AssetClass::UsEquity),
    ("crypto", AssetClass::Crypto),
];

fn reference(value: &Value, path: &str) -> Result<InstrumentReference, RunnerError> {
    let at = |name: &str| format!("{path}/{name}");
    let [
        asset,
        ledger_id,
        asset_class,
        increment,
        tick,
        qty_increment,
        min_order_size,
        exchange,
        classifications,
        fractionable,
        ipo,
        ptp_no_exception,
    ] = fields(
        value,
        path,
        [
            "asset",
            "ledger_id",
            "asset_class",
            "increment",
            "tick",
            "qty_increment",
            "min_order_size",
            "exchange",
            "etp_classifications",
            "fractionable",
            "ipo",
            "ptp_no_exception",
        ],
    )?;
    let exchanges = [
        ("nasdaq", Exchange::Nasdaq),
        ("nyse", Exchange::Nyse),
        ("arca", Exchange::Arca),
        ("amex", Exchange::Amex),
        ("bats", Exchange::Bats),
        ("otc", Exchange::Otc),
        ("other", Exchange::Other),
    ];
    let classes = [
        ("plain", EtpClass::Plain),
        ("complex", EtpClass::Complex),
        ("unclassified", EtpClass::Unclassified),
    ];
    let etp_classifications = list(classifications, &at("etp_classifications"))?
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let path = format!("{path}/etp_classifications/{i}");
            let [made, class] = fields(entry, &path, ["at", "class"])?;
            Ok(EtpClassification {
                at: instant(made, &format!("{path}/at"))?,
                class: word(class, &format!("{path}/class"), &classes)?,
            })
        })
        .collect::<Result<_, RunnerError>>()?;
    Ok(InstrumentReference {
        asset: parsed(asset, &at("asset"), AssetId::parse)?,
        ledger_id: parsed(ledger_id, &at("ledger_id"), InstrumentId::new)?,
        instrument: Instrument {
            asset_class: word(asset_class, &at("asset_class"), &ASSET_CLASSES)?,
            increment: word(
                increment,
                &at("increment"),
                &[
                    ("whole", ShareIncrement::Whole),
                    ("fractional", ShareIncrement::Fractional),
                ],
            )?,
        },
        tick: tick_rule(tick, &at("tick"))?,
        qty_increment: parsed(qty_increment, &at("qty_increment"), Qty::parse)?,
        min_order_size: parsed(min_order_size, &at("min_order_size"), Qty::parse)?,
        exchange: word(exchange, &at("exchange"), &exchanges)?,
        etp_classifications,
        fractionable: flag(fractionable, &at("fractionable"))?,
        ipo: flag(ipo, &at("ipo"))?,
        ptp_no_exception: flag(ptp_no_exception, &at("ptp_no_exception"))?,
    })
}

fn tick_rule(value: &Value, path: &str) -> Result<TickRule, RunnerError> {
    let rule = value
        .get("rule")
        .ok_or_else(|| invalid(&format!("{path}/rule")))?;
    match text(rule, &format!("{path}/rule"))? {
        "reg_nms_equity" => {
            fields(value, path, ["rule"])?;
            Ok(TickRule::RegNmsEquity)
        }
        "increment" => {
            let [_, increment] = fields(value, path, ["rule", "increment"])?;
            let step = parsed(increment, &format!("{path}/increment"), Price::parse)?;
            Ok(TickRule::Increment(step))
        }
        _ => Err(invalid(&format!("{path}/rule"))),
    }
}

fn settings(
    value: &Value,
    path: &str,
) -> Result<(VersionSettings, BTreeSet<AssetClass>), RunnerError> {
    let at = |name: &str| format!("{path}/{name}");
    let [
        sim,
        fees,
        gate,
        profile,
        account_type,
        regime,
        metrics,
        min_order_usd,
        mark_max_age_s,
        benchmark_collar_bps,
    ] = fields(
        value,
        path,
        [
            "sim",
            "fees",
            "gate",
            "profile",
            "account_type",
            "regime",
            "metrics",
            "min_order_usd",
            "mark_max_age_s",
            "benchmark_collar_bps",
        ],
    )?;
    let (profile, stop_limit_classes) = capability_profile(profile, &at("profile"))?;
    let accounts = [("cash", AccountType::Cash), ("margin", AccountType::Margin)];
    let settings = VersionSettings {
        sim: sim_config(sim, &at("sim"))?,
        fees: fee_config(fees, &at("fees"))?,
        gate: gate_config(gate, &at("gate"))?,
        profile,
        account_type: word(account_type, &at("account_type"), &accounts)?,
        regime: day_trade_regime(regime, &at("regime"))?,
        metrics: metrics_config(metrics, &at("metrics"))?,
        min_order_usd: parsed(min_order_usd, &at("min_order_usd"), Usd::parse)?,
        mark_max_age_s: count(mark_max_age_s, &at("mark_max_age_s"))?,
        benchmark_collar: parsed(
            benchmark_collar_bps,
            &at("benchmark_collar_bps"),
            Bps::parse,
        )?,
    };
    Ok((settings, stop_limit_classes))
}

fn sim_config(value: &Value, path: &str) -> Result<SimConfig, RunnerError> {
    let at = |name: &str| format!("{path}/{name}");
    let [decision, approval, slippage, cap] = fields(
        value,
        path,
        [
            "decision_latency_ms",
            "approval_latency_ms",
            "slippage",
            "volume_cap_fraction",
        ],
    )?;
    let latency = |value: &Value, name: &str| {
        value
            .as_int()
            .and_then(|ms| Nanos::from_millis(ms).ok())
            .ok_or_else(|| invalid(&at(name)))
    };
    let slip = at("slippage");
    let model = slippage
        .get("model")
        .ok_or_else(|| invalid(&format!("{slip}/model")))?;
    let slippage = match text(model, &format!("{slip}/model"))? {
        "fixed" => {
            let [_, half, impact] =
                fields(slippage, &slip, ["model", "half_spread_bps", "impact_bps"])?;
            Slippage::Fixed {
                half_spread_bps: parsed(half, &format!("{slip}/half_spread_bps"), Bps::parse)?,
                impact_bps: parsed(impact, &format!("{slip}/impact_bps"), Bps::parse)?,
            }
        }
        "sqrt" => {
            let [_, half, coefficient] = fields(
                slippage,
                &slip,
                ["model", "half_spread_bps", "coefficient_bps"],
            )?;
            Slippage::Sqrt {
                half_spread_bps: parsed(half, &format!("{slip}/half_spread_bps"), Bps::parse)?,
                coefficient_bps: parsed(
                    coefficient,
                    &format!("{slip}/coefficient_bps"),
                    Bps::parse,
                )?,
            }
        }
        _ => return Err(invalid(&format!("{slip}/model"))),
    };
    Ok(SimConfig {
        decision_latency: latency(decision, "decision_latency_ms")?,
        approval_latency: latency(approval, "approval_latency_ms")?,
        slippage,
        volume_cap_fraction: parsed(cap, &at("volume_cap_fraction"), Fraction::parse)?,
    })
}

fn fee_config(value: &Value, path: &str) -> Result<Config, RunnerError> {
    let [equities, crypto, calendar] = fields(value, path, ["equities", "crypto", "calendar"])?;
    let eq = format!("{path}/equities");
    let at = |name: &str| format!("{eq}/{name}");
    let [
        sec_rate,
        taf_per_share,
        taf_cap,
        basis,
        cat_per_share,
        rounding,
    ] = fields(
        equities,
        &eq,
        [
            "sec_rate",
            "taf_per_share",
            "taf_cap",
            "taf_cap_basis",
            "cat_per_share",
            "fee_rounding",
        ],
    )?;
    let bases = [
        ("per_execution", TafCapBasis::PerExecution),
        ("per_order", TafCapBasis::PerOrder),
    ];
    let roundings = [
        ("daily_total", FeeRounding::DailyTotal),
        ("per_fee_per_fill", FeeRounding::PerFeePerFill),
    ];
    let cr = format!("{path}/crypto");
    let [maker, taker] = fields(crypto, &cr, ["maker_bps", "taker_bps"])?;
    let cal = format!("{path}/calendar");
    let [from, to, trading, settlement] = fields(
        calendar,
        &cal,
        [
            "valid_from",
            "valid_to",
            "trading_holidays",
            "settlement_holidays",
        ],
    )?;
    let dates = |value: &Value, name: &str| -> Result<Vec<Date>, RunnerError> {
        list(value, &format!("{cal}/{name}"))?
            .iter()
            .enumerate()
            .map(|(i, d)| date(d, &format!("{cal}/{name}/{i}")))
            .collect()
    };
    let calendar = TradingCalendar::new(
        date(from, &format!("{cal}/valid_from"))?,
        date(to, &format!("{cal}/valid_to"))?,
        dates(trading, "trading_holidays")?,
        dates(settlement, "settlement_holidays")?,
    )
    .map_err(|_| invalid(&cal))?;
    Ok(Config {
        equities: EquityFees {
            sec_rate: parsed(sec_rate, &at("sec_rate"), FeeRate::parse)?,
            taf_per_share: parsed(taf_per_share, &at("taf_per_share"), FeePerShare::parse)?,
            taf_cap: parsed(taf_cap, &at("taf_cap"), FeeCap::parse)?,
            taf_cap_basis: word(basis, &at("taf_cap_basis"), &bases)?,
            cat_per_share: parsed(cat_per_share, &at("cat_per_share"), FeePerShare::parse)?,
            fee_rounding: word(rounding, &at("fee_rounding"), &roundings)?,
        },
        crypto: CryptoFees {
            maker: parsed(maker, &format!("{cr}/maker_bps"), Bps::parse)?,
            taker: parsed(taker, &format!("{cr}/taker_bps"), Bps::parse)?,
        },
        calendar,
    })
}

fn gate_config(value: &Value, path: &str) -> Result<GateConfig, RunnerError> {
    let at = |name: &str| format!("{path}/{name}");
    let names = [
        "price_floor",
        "liquidity_floor_usd",
        "crypto_liquidity_floor_usd",
        "collar_liquid_threshold_usd",
        "collar_liquid_x",
        "collar_other_x",
        "collar_crypto_x",
        "collar_passive_band",
        "opposite_fill_interval_s",
        "min_resting_time_s",
        "order_to_fill_max",
        "order_to_fill_min_orders",
        "order_size_participation",
        "daily_participation",
        "close_window_minutes",
        "legacy_pdt_equity_threshold",
        "etp_classification_max_age_s",
    ];
    let [
        price_floor,
        liquidity_floor,
        crypto_liquidity_floor,
        collar_threshold,
        collar_liquid_x,
        collar_other_x,
        collar_crypto_x,
        collar_passive_band,
        opposite_fill_interval,
        min_resting_time,
        order_to_fill_max,
        order_to_fill_min_orders,
        order_size_participation,
        daily_participation,
        close_window,
        legacy_pdt,
        etp_max_age,
    ] = fields(value, path, names)?;
    let usd = |v: &Value, name: &str| parsed(v, &at(name), Usd::parse);
    let fraction = |v: &Value, name: &str| parsed(v, &at(name), Fraction::parse);
    let int = |v: &Value, name: &str| count(v, &at(name));
    Ok(GateConfig {
        price_floor: usd(price_floor, "price_floor")?,
        liquidity_floor_usd: usd(liquidity_floor, "liquidity_floor_usd")?,
        crypto_liquidity_floor_usd: usd(crypto_liquidity_floor, "crypto_liquidity_floor_usd")?,
        collar_liquid_threshold_usd: usd(collar_threshold, "collar_liquid_threshold_usd")?,
        collar_liquid_x: fraction(collar_liquid_x, "collar_liquid_x")?,
        collar_other_x: fraction(collar_other_x, "collar_other_x")?,
        collar_crypto_x: fraction(collar_crypto_x, "collar_crypto_x")?,
        collar_passive_band: fraction(collar_passive_band, "collar_passive_band")?,
        opposite_fill_interval_s: int(opposite_fill_interval, "opposite_fill_interval_s")?,
        min_resting_time_s: int(min_resting_time, "min_resting_time_s")?,
        order_to_fill_max: int(order_to_fill_max, "order_to_fill_max")?,
        order_to_fill_min_orders: int(order_to_fill_min_orders, "order_to_fill_min_orders")?,
        order_size_participation: fraction(order_size_participation, "order_size_participation")?,
        daily_participation: fraction(daily_participation, "daily_participation")?,
        close_window_minutes: int(close_window, "close_window_minutes")?,
        legacy_pdt_equity_threshold: usd(legacy_pdt, "legacy_pdt_equity_threshold")?,
        etp_classification_max_age_s: int(etp_max_age, "etp_classification_max_age_s")?,
    })
}

/// The profile, and the asset classes any of its cells protects with a stop-limit.
fn capability_profile(
    value: &Value,
    path: &str,
) -> Result<(CapabilityProfile, BTreeSet<AssetClass>), RunnerError> {
    let [version, rows, idempotency] =
        fields(value, path, ["profile_version", "rows", "idempotency"])?;
    let sessions = [
        ("overnight", MarketSession::Overnight),
        ("pre_market", MarketSession::PreMarket),
        ("regular", MarketSession::Regular),
        ("after_hours", MarketSession::AfterHours),
        ("crypto", MarketSession::Crypto),
    ];
    let mut decoded = Vec::new();
    let mut stop_limit = BTreeSet::new();
    for (r, row) in list(rows, &format!("{path}/rows"))?.iter().enumerate() {
        let row_path = format!("{path}/rows/{r}");
        let [asset_class, session, cells] =
            fields(row, &row_path, ["asset_class", "session", "cells"])?;
        let asset_class = word(
            asset_class,
            &format!("{row_path}/asset_class"),
            &ASSET_CLASSES,
        )?;
        let mut decoded_cells = Vec::new();
        for (c, value) in list(cells, &format!("{row_path}/cells"))?
            .iter()
            .enumerate()
        {
            let cell = cell(value, &format!("{row_path}/cells/{c}"))?;
            if cell.protection_forms.contains(&ProtectionForm::StopLimit) {
                stop_limit.insert(asset_class);
            }
            decoded_cells.push(cell);
        }
        decoded.push(Row {
            asset_class,
            session: word(session, &format!("{row_path}/session"), &sessions)?,
            cells: decoded_cells,
        });
    }
    let idem = format!("{path}/idempotency");
    let [client_order_id, retry, query] = fields(
        idempotency,
        &idem,
        ["client_order_id", "retry", "query_by_client_order_id"],
    )?;
    let retries = [
        ("idempotent", Retry::Idempotent),
        ("not_idempotent", Retry::NotIdempotent),
        ("unknown", Retry::Unknown),
    ];
    let idempotency = Idempotency {
        client_order_id: flag(client_order_id, &format!("{idem}/client_order_id"))?,
        retry: word(retry, &format!("{idem}/retry"), &retries)?,
        query_by_client_order_id: flag(query, &format!("{idem}/query_by_client_order_id"))?,
    };
    let version = count(version, &format!("{path}/profile_version"))?;
    let profile =
        CapabilityProfile::new(version, decoded, idempotency).map_err(|_| invalid(path))?;
    Ok((profile, stop_limit))
}

fn cell(value: &Value, path: &str) -> Result<Cell, RunnerError> {
    let [order_type, quantity_form, times_in_force, protection_forms] = fields(
        value,
        path,
        [
            "order_type",
            "quantity_form",
            "times_in_force",
            "protection_forms",
        ],
    )?;
    let types = [
        ("market", OrderType::Market),
        ("limit", OrderType::Limit),
        ("stop", OrderType::Stop),
        ("stop_limit", OrderType::StopLimit),
    ];
    let forms = [
        ("whole", QuantityForm::Whole),
        ("fractional", QuantityForm::Fractional),
        ("notional", QuantityForm::Notional),
    ];
    let tifs = [
        ("day", TimeInForce::Day),
        ("gtc", TimeInForce::Gtc),
        ("ioc", TimeInForce::Ioc),
    ];
    let protections = [
        ("bracket", ProtectionForm::Bracket),
        ("oco", ProtectionForm::Oco),
        ("stop_limit", ProtectionForm::StopLimit),
    ];
    Ok(Cell {
        order_type: word(order_type, &format!("{path}/order_type"), &types)?,
        quantity_form: word(quantity_form, &format!("{path}/quantity_form"), &forms)?,
        times_in_force: words(times_in_force, &format!("{path}/times_in_force"), &tifs)?,
        protection_forms: words(
            protection_forms,
            &format!("{path}/protection_forms"),
            &protections,
        )?,
    })
}

fn day_trade_regime(value: &Value, path: &str) -> Result<DayTradeRegime, RunnerError> {
    let kind = value
        .get("kind")
        .ok_or_else(|| invalid(&format!("{path}/kind")))?;
    match text(kind, &format!("{path}/kind"))? {
        "legacy_pdt" => {
            fields(value, path, ["kind"])?;
            Ok(DayTradeRegime::LegacyPdt)
        }
        "intraday_margin" => {
            let [_, excess] = fields(value, path, ["kind", "maintenance_excess"])?;
            Ok(DayTradeRegime::IntradayMargin {
                maintenance_excess: parsed(
                    excess,
                    &format!("{path}/maintenance_excess"),
                    Usd::parse,
                )?,
            })
        }
        _ => Err(invalid(&format!("{path}/kind"))),
    }
}

fn metrics_config(value: &Value, path: &str) -> Result<MetricsConfig, RunnerError> {
    let [periods, risk_free] = fields(value, path, ["periods_per_year", "risk_free_per_period"])?;
    Ok(MetricsConfig {
        periods_per_year: count(periods, &format!("{path}/periods_per_year"))?,
        risk_free_per_period: parsed(
            risk_free,
            &format!("{path}/risk_free_per_period"),
            Ratio::parse,
        )?,
    })
}
