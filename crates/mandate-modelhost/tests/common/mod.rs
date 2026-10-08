//! The model host's test fixtures (E15-13): the content object written out by hand, and an
//! agreeing pin, registry entry, calendar and closes to change one input of at a time.

use std::collections::BTreeMap;
use std::fs;

use mandate_accounting::InstrumentId;
use mandate_canon::Digest;
use mandate_modelhost::{DailyCloses, Evaluation, Pin, Refusal, evaluate};
use mandate_num::{Conviction, Price, Unit};
use mandate_runtime::{ModelDirection, ModelOutput, RiskClock};
use mandate_spec::document::{ModelId, ModelParam, ParamValue, SignalModel};
use mandate_spec::validate::RegisteredModel;
use mandate_spec::{DecGrammar, SchemaDec};
use mandate_time::{Date, ExchangeCalendar, UtcNanos};

pub const ID: &str = "quant.ma_crossover";
pub const VERSION: &str = "1.0.0";
pub const AGE: u32 = 64_800;
pub const WEEK: [&str; 3] = ["2026-10-05", "2026-10-06", "2026-10-07"];
pub const RISING: [&str; 3] = ["100", "101", "103"];
const LISTED: [&str; 2] = [
    "crates/mandate-backtest/src/strategy/ma_crossover.rs",
    "crates/mandate-modelhost/src/ma_crossover.rs",
];

/// DEC-504 item 1's object for `quant.ma_crossover` 1.0.0 as DEC-518 fixes it, in canonical form
/// (members sorted by key, no whitespace), each listed file hashed from the bytes on disk.
pub fn oracle_content() -> String {
    let code = LISTED.map(|path| {
        let bytes = fs::read(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        format!(r#"{{"path":"{path}","sha256":"{}"}}"#, Digest::of(&bytes))
    });
    format!(
        concat!(
            r#"{{"authorship":"platform","code":[{},{}],"content_version":1,"inputs":{{"bars":"#,
            r#""daily_close","instrument":"pinned","minimum_count":"slow_periods","trading_day":"#,
            r#""us_equity_regular_session"}},"kind":"quant_model_content","methodology":"Long "#,
            r#"when the mean of the last fast_periods daily closes is strictly above the mean of "#,
            r#"the last slow_periods daily closes, compared by cross-multiplying the window sums; "#,
            r#"otherwise no output.","model_id":"quant.ma_crossover","model_version":"1.0.0","#,
            r#""output":{{"flat":"none","long":{{"confidence":"1","conviction":"1","direction":"#,
            r#""long"}},"undecided":"none"}},"params_schema":{{"constraints":["fast_periods < "#,
            r#"slow_periods"],"params":[{{"max":1000,"min":1,"name":"fast_periods","type":"#,
            r#""integer"}},{{"max":1000,"min":2,"name":"slow_periods","type":"integer"}}]}}}}"#
        ),
        code[0], code[1]
    )
}

pub fn hash() -> Digest {
    Digest::of(oracle_content().as_bytes())
}

pub fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

pub fn dec(text: &str) -> ParamValue {
    ParamValue::Decimal(SchemaDec::parse(text, DecGrammar::Decimal).unwrap())
}

pub fn params(pairs: &[(&str, ParamValue)]) -> Vec<ModelParam> {
    let param = |(key, value): &(&str, ParamValue)| ModelParam {
        key: (*key).to_owned(),
        value: value.clone(),
    };
    pairs.iter().map(param).collect()
}

pub fn id(text: &str) -> InstrumentId {
    InstrumentId::new(text).unwrap()
}

pub struct Case {
    pub pin: Pin,
    pub registry: BTreeMap<ModelId, RegisteredModel>,
    pub calendar: ExchangeCalendar,
    pub closes: DailyCloses,
    pub now: UtcNanos,
}

impl Case {
    /// A `quant.ma_crossover` pin (fast 2, slow 3) on SPY that agrees with its registry entry and
    /// the host, over `closes` as `(date, price)`, at `now`.
    pub fn new(days: [&str; 3], prices: [&str; 3], now: &str) -> Self {
        let model_id = ModelId::parse(ID).unwrap();
        let entry = RegisteredModel {
            version: VERSION.to_owned(),
            content_hash: hash(),
            params: ["fast_periods", "slow_periods"].map(str::to_owned).into(),
            admits_instruments: false,
        };
        let model = SignalModel {
            id: model_id.clone(),
            version: VERSION.to_owned(),
            content_hash: hash(),
            params: params(&[("fast_periods", dec("2")), ("slow_periods", dec("3"))]),
            weight: SchemaDec::parse("1", DecGrammar::UnitPositive).unwrap(),
            max_output_age_s: AGE,
            admits_instruments: false,
        };
        let day = |(d, p): (&str, &str)| (Date::parse(d).unwrap(), Price::parse(p).unwrap());
        let closes = days.into_iter().zip(prices).map(day).collect();
        Self {
            pin: Pin {
                model,
                instrument_id: id("SPY"),
            },
            registry: BTreeMap::from([(model_id, entry)]),
            calendar: ExchangeCalendar::us_equities().unwrap(),
            closes: DailyCloses {
                instrument_id: id("SPY"),
                closes,
            },
            now: at(now),
        }
    }

    /// 100, 101, 103 on Monday to Wednesday 2026-10-05 to 07, an hour after Wednesday's close:
    /// the fast mean 102 is above the slow mean 304/3, so `Long`.
    pub fn rising() -> Self {
        Self::new(WEEK, RISING, "2026-10-07T21:00:00Z")
    }

    pub fn run(&self) -> Result<Evaluation, Refusal> {
        evaluate(
            &self.pin,
            &self.registry,
            &self.calendar,
            &self.closes,
            self.now,
        )
    }
}

/// The one output a `Long` maps to: the pin's identity, conviction and confidence 1 (DEC-157
/// item 4), `as_of` the last close's end (§8.2, X-3), `expires_at` that plus `max_output_age_s`.
pub fn long(as_of: &str) -> Evaluation {
    let as_of = at(as_of).secs();
    Evaluation::Long(Box::new(ModelOutput {
        model_id: ID.to_owned(),
        model_version: VERSION.to_owned(),
        content_hash: hash(),
        instrument_id: id("SPY"),
        as_of: RiskClock::from_secs(as_of),
        expires_at: RiskClock::from_secs(as_of + i64::from(AGE)),
        direction: ModelDirection::Long,
        conviction: Conviction::parse("1").unwrap(),
        confidence: Unit::ONE,
        horizon_s: u64::from(AGE),
        thesis_ref: None,
        evidence: Vec::new(),
        invalidation: None,
        thesis_id: None,
        lineage_id: None,
        ignored: None,
    }))
}
