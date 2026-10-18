//! The evaluation's four input files ([DEC-2244](../../../docs/project/decisions/DEC-2244.md) item
//! 3), each one JSON object that `mandate-canon` parses: decimals are strings, counts are integers,
//! and instants are RFC 3339 UTC text. Every member is required and a member no reader names is
//! refused, so a misspelt input fails closed rather than falling back to anything, as the backtest
//! runner's inputs do (DEC-1144 item 3).

use std::collections::BTreeMap;

use mandate_accounting::{Config, CryptoFees, EquityFees, FeeRounding, TafCapBasis};
use mandate_canon::{Value, parse};
use mandate_num::{Bps, FeeCap, FeePerShare, FeeRate, Price, Ratio, ShareIncrement, Usd};
use mandate_research::AssetId;
use mandate_research::score::{EvaluationDecision, EvaluationWindow, ObservedClose};
use mandate_time::{Date, TradingCalendar, UtcNanos};

use crate::{Closes, CostModel, EvaluationError};

/// The four files' bytes, by name, as the directory holds them.
#[derive(Debug, Clone, Copy)]
pub struct InputFiles<'a> {
    pub decision: &'a [u8],
    pub closes: &'a [u8],
    pub cost: &'a [u8],
    pub baselines: &'a [u8],
}

/// The file names [`InputFiles`] are read from, in its order.
pub const INPUT_FILE_NAMES: [&str; 4] = [
    "decision.json",
    "closes.json",
    "cost.json",
    "baselines.json",
];

/// Every input of one evaluation but the thesis records and the mandate documents, decoded.
#[derive(Debug, Clone)]
pub struct EvaluationInputs {
    /// The recorded decision that registered the window, metric and threshold (DEC-122).
    pub decision_id: String,
    pub decision: EvaluationDecision,
    pub closes: Closes,
    pub cost: CostModel,
    pub basket: Vec<AssetId>,
    pub index: AssetId,
}

/// Decodes the four files.
///
/// # Errors
/// [`EvaluationError::Input`] naming the first file and member that is missing, unknown, or
/// malformed: an instrument with two series, and a window that ends before it starts, included.
pub fn decode(files: &InputFiles<'_>) -> Result<EvaluationInputs, EvaluationError> {
    let [decision_name, closes_name, cost_name, baselines_name] = INPUT_FILE_NAMES;
    let (decision_id, decision) =
        decision(&document(decision_name, files.decision)?, decision_name)?;
    let closes = series(&document(closes_name, files.closes)?, closes_name)?;
    let cost = cost(&document(cost_name, files.cost)?, cost_name)?;
    let baselines = document(baselines_name, files.baselines)?;
    let [basket, index] = fields(&baselines, baselines_name, ["basket", "index"])?;
    let basket_path = format!("{baselines_name}/basket");
    let basket = list(basket, &basket_path)?
        .iter()
        .enumerate()
        .map(|(i, member)| asset(member, &format!("{basket_path}/{i}")))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EvaluationInputs {
        decision_id,
        decision,
        closes,
        cost,
        basket,
        index: asset(index, &format!("{baselines_name}/index"))?,
    })
}

fn invalid(path: &str) -> EvaluationError {
    EvaluationError::Input(path.to_owned())
}

fn document(name: &str, bytes: &[u8]) -> Result<Value, EvaluationError> {
    parse(bytes).map_err(|_| invalid(name))
}

/// `value`'s members `names`, in that order, when it is an object holding exactly those.
fn fields<'a, const N: usize>(
    value: &'a Value,
    path: &str,
    names: [&str; N],
) -> Result<[&'a Value; N], EvaluationError> {
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

fn text<'a>(value: &'a Value, path: &str) -> Result<&'a str, EvaluationError> {
    value.as_str().ok_or_else(|| invalid(path))
}

fn parsed<T, E>(
    value: &Value,
    path: &str,
    read: fn(&str) -> Result<T, E>,
) -> Result<T, EvaluationError> {
    read(text(value, path)?).map_err(|_| invalid(path))
}

fn list<'a>(value: &'a Value, path: &str) -> Result<&'a [Value], EvaluationError> {
    value.as_array().ok_or_else(|| invalid(path))
}

fn instant(value: &Value, path: &str) -> Result<UtcNanos, EvaluationError> {
    parsed(value, path, UtcNanos::parse_rfc3339)
}

fn date(value: &Value, path: &str) -> Result<Date, EvaluationError> {
    parsed(value, path, Date::parse)
}

fn asset(value: &Value, path: &str) -> Result<AssetId, EvaluationError> {
    parsed(value, path, AssetId::new)
}

/// The variant `table` pairs with `value`'s text.
fn word<T: Copy>(value: &Value, path: &str, table: &[(&str, T)]) -> Result<T, EvaluationError> {
    let spelled = text(value, path)?;
    table
        .iter()
        .find(|(name, _)| *name == spelled)
        .map(|(_, variant)| *variant)
        .ok_or_else(|| invalid(path))
}

fn decision(value: &Value, path: &str) -> Result<(String, EvaluationDecision), EvaluationError> {
    let [id, window, minimum, z] = fields(
        value,
        path,
        ["decision_id", "window", "minimum_scoreable", "z"],
    )?;
    let window_path = format!("{path}/window");
    let [from, to] = fields(window, &window_path, ["from", "to"])?;
    let window = EvaluationWindow {
        from: instant(from, &format!("{window_path}/from"))?,
        to: instant(to, &format!("{window_path}/to"))?,
    };
    if window.to < window.from {
        return Err(invalid(&window_path));
    }
    let minimum_path = format!("{path}/minimum_scoreable");
    let minimum_scoreable = minimum
        .as_int()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| invalid(&minimum_path))?;
    Ok((
        text(id, &format!("{path}/decision_id"))?.to_owned(),
        EvaluationDecision {
            window,
            minimum_scoreable,
            z: parsed(z, &format!("{path}/z"), Ratio::parse)?,
        },
    ))
}

fn series(value: &Value, path: &str) -> Result<Closes, EvaluationError> {
    let [all] = fields(value, path, ["series"])?;
    let all_path = format!("{path}/series");
    let mut closes = BTreeMap::new();
    for (i, one) in list(all, &all_path)?.iter().enumerate() {
        let at = format!("{all_path}/{i}");
        let [instrument, points] = fields(one, &at, ["instrument", "closes"])?;
        let instrument_path = format!("{at}/instrument");
        let id = asset(instrument, &instrument_path)?;
        let points_path = format!("{at}/closes");
        let observed = list(points, &points_path)?
            .iter()
            .enumerate()
            .map(|(j, point)| {
                let point_path = format!("{points_path}/{j}");
                let [when, price] = fields(point, &point_path, ["at", "price"])?;
                Ok(ObservedClose {
                    at: instant(when, &format!("{point_path}/at"))?,
                    price: parsed(price, &format!("{point_path}/price"), Price::parse)?,
                })
            })
            .collect::<Result<Vec<_>, EvaluationError>>()?;
        if closes.insert(id, observed).is_some() {
            return Err(invalid(&instrument_path));
        }
    }
    Ok(closes)
}

fn cost(value: &Value, path: &str) -> Result<CostModel, EvaluationError> {
    let [slippage, notional, increment, fees] = fields(
        value,
        path,
        [
            "slippage_per_leg_bps",
            "reference_notional",
            "increment",
            "fees",
        ],
    )?;
    let increments = [
        ("whole", ShareIncrement::Whole),
        ("fractional", ShareIncrement::Fractional),
    ];
    Ok(CostModel {
        slippage_per_leg: parsed(
            slippage,
            &format!("{path}/slippage_per_leg_bps"),
            Bps::parse,
        )?,
        fees: fee_config(fees, &format!("{path}/fees"))?,
        reference_notional: parsed(notional, &format!("{path}/reference_notional"), Usd::parse)?,
        increment: word(increment, &format!("{path}/increment"), &increments)?,
    })
}

/// The fee configuration in the backtest runner's `settings.json` `fees` shape (DEC-1144 item 3).
fn fee_config(value: &Value, path: &str) -> Result<Config, EvaluationError> {
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
    let dates = |value: &Value, name: &str| -> Result<Vec<Date>, EvaluationError> {
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
