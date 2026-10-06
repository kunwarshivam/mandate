//! The reviewed E7-7 artifacts, each read once and judged on the bytes hashed into the executor's
//! config references (DEC-470 item 2).

use std::fs;
use std::path::Path;
use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_alpaca::Exchange as BrokerExchange;
use mandate_canon::{Digest, Key, Value};
use mandate_domain::{AssetClass as DomainAssetClass, AssetId};
use mandate_executor::BindingGateConfigRefs;
use mandate_num::{Qty, ShareIncrement};
use mandate_risk::{EtpClass, Exchange as GateExchange};
use mandate_time::{Date, TradingCalendar, UtcNanos};

use super::{INSTRUMENT_ID, MODEL_ID, MODEL_VERSION, SYMBOL, absent};
use crate::error::Cause;

/// The largest quote-age bound the rule-set artifact may name: a minute-old quote is already older
/// than any marketable decision should rest on.
const MAX_QUOTE_AGE_S: u64 = 60;
/// One whole share, the quantity grid the instrument artifact admits.
const WHOLE_SHARE: &str = "1";

/// The reviewed E7-7 artifacts, each read once and judged on the bytes that are hashed into
/// `config_refs`.
pub struct Artifacts {
    pub(super) mandate: mandate_spec::Mandate,
    pub(super) model_id: String,
    pub(super) model_version: String,
    pub(super) model_hash: Digest,
    pub(super) fees: mandate_accounting::Config,
    pub(super) instrument: ReviewedInstrument,
    pub(super) quote_max_age: Duration,
    pub(super) config_refs: BindingGateConfigRefs,
}

/// Instrument and model identity derived from the confirmed mandate and reviewed snapshots.
pub struct ProductionIdentity<'a> {
    pub asset_id: &'a AssetId,
    pub symbol: &'a InstrumentId,
    pub asset_class: DomainAssetClass,
    pub broker_exchange: BrokerExchange,
    pub gate_exchange: GateExchange,
    pub model_id: &'a str,
    pub model_version: &'a str,
    pub model_hash: Digest,
}

/// What the instrument artifact states, each field read by the run: the symbol the broker is read
/// by, the quantity grid, and the ETP classification.
pub(super) struct ReviewedInstrument {
    pub(super) asset_id: AssetId,
    pub(super) symbol: InstrumentId,
    pub(super) asset_class: DomainAssetClass,
    pub(super) broker_exchange: BrokerExchange,
    pub(super) gate_exchange: GateExchange,
    pub(super) increment: ShareIncrement,
    pub(super) increment_qty: Qty,
    pub(super) etp: EtpClass,
    pub(super) etp_classified_at: UtcNanos,
}

/// One artifact's bytes and their parse: the refs hash `bytes`, and every check reads `value`.
struct Artifact {
    bytes: Vec<u8>,
    value: Value,
}

impl Artifacts {
    /// Loads production artifacts without selecting an instrument or model in the shell.
    ///
    /// # Errors
    /// [`Cause::Absent`] for an incomplete, mismatched, or unsupported reviewed input.
    pub fn load_production(mandate_path: &Path, config_dir: &Path) -> Result<Self, Cause> {
        Self::load_inputs(mandate_path, config_dir)
    }

    /// Loads the temporary E7-7 AAPL adapter's reviewed artifacts.
    ///
    /// # Errors
    /// [`Cause::Absent`] unless the production inputs also match the legacy E7-7 deployment.
    pub fn load(mandate_path: &Path, config_dir: &Path) -> Result<Self, Cause> {
        let loaded = Self::load_inputs(mandate_path, config_dir)?;
        let identity = loaded.production_identity();
        if !legacy_identity(&identity) {
            return Err(absent("the reviewed E7-7 production inputs"));
        }
        Ok(loaded)
    }

    fn load_inputs(mandate_path: &Path, config_dir: &Path) -> Result<Self, Cause> {
        let mandate_bytes = fs::read(mandate_path).map_err(|_| absent("the mandate artifact"))?;
        let mandate_value = mandate_canon::parse(&mandate_bytes)
            .map_err(|_| absent("a canonical mandate artifact"))?;
        let mandate = mandate_spec::Mandate::parse(&mandate_value)
            .map_err(|_| absent("a parsed mandate artifact"))?;
        if mandate.environment != mandate_domain::Environment::Paper {
            return Err(absent("a paper mandate artifact"));
        }
        let [pinned] = mandate.universe.pinned_instruments.as_slice() else {
            return Err(absent("one pinned production instrument"));
        };
        if pinned.asset_class != DomainAssetClass::UsEquity {
            return Err(absent("a supported production instrument"));
        }
        let [model] = mandate.behavior.signal_models.as_slice() else {
            return Err(absent("the one reviewed signal model"));
        };
        let model_artifact = artifact(config_dir, "model-artifact.json", &["id", "version"])?;
        require_text(&model_artifact.value, "id", model.id.as_str())?;
        require_text(&model_artifact.value, "version", &model.version)?;
        if model.content_hash != Digest::of(&model_artifact.bytes) {
            return Err(absent("the reviewed model artifact"));
        }
        let model_id = model.id.as_str().to_owned();
        let model_version = model.version.clone();
        let model_hash = model.content_hash;

        let fee = artifact(
            config_dir,
            "fee-config.json",
            &["effective_from", "environment", "schedule"],
        )?;
        require_text(&fee.value, "schedule", "conservative_v1")?;
        let calendar_artifact = artifact(
            config_dir,
            "trading-calendar.json",
            &["first", "holidays", "last", "special_sessions"],
        )?;
        let first = Date::parse(required_text(&calendar_artifact.value, "first")?)
            .map_err(|_| absent("the trading calendar's first date"))?;
        let last = Date::parse(required_text(&calendar_artifact.value, "last")?)
            .map_err(|_| absent("the trading calendar's last date"))?;
        require_empty_array(&calendar_artifact.value, "holidays")?;
        require_empty_array(&calendar_artifact.value, "special_sessions")?;
        let calendar = TradingCalendar::new(first, last, [], [])
            .map_err(|_| absent("the effective trading calendar"))?;
        let fees = mandate_executor::paper_only_fee_config(
            required_text(&fee.value, "environment")?,
            calendar,
            required_text(&fee.value, "effective_from")?,
        )
        .map_err(Cause::Executor)?;

        let instrument_artifact = artifact(
            config_dir,
            "instrument-snapshot.json",
            &[
                "asset_class",
                "etp",
                "etp_classified_at",
                "exchange",
                "increment",
                "instrument_id",
                "symbol",
            ],
        )?;
        let reviewed = &instrument_artifact.value;
        require_text(reviewed, "instrument_id", pinned.asset_id.as_str())?;
        require_text(reviewed, "symbol", &pinned.symbol)?;
        require_text(reviewed, "asset_class", "us_equity")?;
        require_text(reviewed, "increment", "whole")?;
        require_text(reviewed, "etp", "plain")?;
        let (broker_exchange, gate_exchange) = exchanges(required_text(reviewed, "exchange")?)?;
        let etp_classified_at =
            UtcNanos::parse_rfc3339(required_text(reviewed, "etp_classified_at")?)
                .map_err(|_| absent("the instrument's ETP classification date"))?;
        let instrument = ReviewedInstrument {
            asset_id: pinned.asset_id.clone(),
            symbol: InstrumentId::new(&pinned.symbol)
                .map_err(|_| absent("the reviewed instrument symbol"))?,
            asset_class: DomainAssetClass::UsEquity,
            broker_exchange,
            gate_exchange,
            increment: ShareIncrement::Whole,
            increment_qty: Qty::parse(WHOLE_SHARE)?,
            etp: EtpClass::Plain,
            etp_classified_at,
        };

        let rules = artifact(
            config_dir,
            "rule-set.json",
            &["gate", "iex_quote_max_age_s", "ruleset_version", "story"],
        )?;
        require_text(&rules.value, "gate", "trading-domain-9.1")?;
        require_text(&rules.value, "ruleset_version", "v1")?;
        require_text(&rules.value, "story", "E7-7")?;
        let quote_max_age_s = rules
            .value
            .get("iex_quote_max_age_s")
            .and_then(Value::as_int)
            .filter(|seconds| (1..=MAX_QUOTE_AGE_S).contains(seconds))
            .ok_or_else(|| absent("a reviewed IEX quote age of 1 to 60 seconds"))?;

        let canonical_mandate = mandate
            .canonical_bytes()
            .map_err(|_| absent("the mandate's canonical artifact"))?;
        let config_refs = BindingGateConfigRefs::complete(
            reference(&fee.bytes),
            reference(&calendar_artifact.bytes),
            reference(&instrument_artifact.bytes),
            reference(&rules.bytes),
            reference(&canonical_mandate),
        );
        Ok(Self {
            mandate,
            model_id,
            model_version,
            model_hash,
            fees,
            instrument,
            quote_max_age: Duration::from_secs(quote_max_age_s),
            config_refs,
        })
    }

    /// The content-addressed references the executor journals with every gate decision.
    pub fn config_refs(&self) -> &BindingGateConfigRefs {
        &self.config_refs
    }

    /// The production identity this reviewed input binds.
    pub fn production_identity(&self) -> ProductionIdentity<'_> {
        ProductionIdentity {
            asset_id: &self.instrument.asset_id,
            symbol: &self.instrument.symbol,
            asset_class: self.instrument.asset_class,
            broker_exchange: self.instrument.broker_exchange,
            gate_exchange: self.instrument.gate_exchange,
            model_id: &self.model_id,
            model_version: &self.model_version,
            model_hash: self.model_hash,
        }
    }
}

/// Reads `name` once. Its members must be exactly `members`, which are listed in canonical order.
fn artifact(config_dir: &Path, name: &str, members: &[&str]) -> Result<Artifact, Cause> {
    let bytes =
        fs::read(config_dir.join(name)).map_err(|_| absent("a required config artifact"))?;
    let value = mandate_canon::parse(&bytes).map_err(|_| absent("a canonical config artifact"))?;
    let exact = value
        .as_object()
        .is_some_and(|object| object.keys().map(Key::as_str).eq(members.iter().copied()));
    if !exact {
        return Err(absent(
            "a config artifact with exactly its reviewed members",
        ));
    }
    Ok(Artifact { bytes, value })
}

pub(super) fn reference(bytes: &[u8]) -> String {
    format!("sha256:{}", Digest::of(bytes).to_hex())
}

fn required_text<'a>(value: &'a Value, name: &'static str) -> Result<&'a str, Cause> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| absent(name))
}

fn require_text(value: &Value, name: &'static str, expected: &str) -> Result<(), Cause> {
    if required_text(value, name)? == expected {
        Ok(())
    } else {
        Err(absent(name))
    }
}

fn legacy_identity(identity: &ProductionIdentity<'_>) -> bool {
    identity.asset_id.as_str() == INSTRUMENT_ID
        && identity.symbol.as_str() == SYMBOL
        && identity.broker_exchange == BrokerExchange::Nasdaq
        && identity.model_id == MODEL_ID
        && identity.model_version == MODEL_VERSION
}

fn exchanges(text: &str) -> Result<(BrokerExchange, GateExchange), Cause> {
    if text == "nasdaq" {
        Ok((BrokerExchange::Nasdaq, GateExchange::Nasdaq))
    } else if text == "nyse" {
        Ok((BrokerExchange::Nyse, GateExchange::Nyse))
    } else if text == "arca" {
        Ok((BrokerExchange::Arca, GateExchange::Arca))
    } else if text == "amex" {
        Ok((BrokerExchange::Amex, GateExchange::Amex))
    } else if text == "bats" {
        Ok((BrokerExchange::Bats, GateExchange::Bats))
    } else {
        Err(absent("an eligible reviewed exchange"))
    }
}

fn require_empty_array(value: &Value, name: &'static str) -> Result<(), Cause> {
    match value.get(name).and_then(Value::as_array) {
        Some([]) => Ok(()),
        Some(_) | None => Err(absent(name)),
    }
}

#[cfg(test)]
mod tests {
    use mandate_accounting::InstrumentId;
    use mandate_alpaca::Exchange as BrokerExchange;
    use mandate_canon::Digest;
    use mandate_domain::{AssetClass, AssetId};
    use mandate_risk::Exchange as GateExchange;

    use super::{ProductionIdentity, exchanges, legacy_identity};

    fn text<E: std::fmt::Display>(error: E) -> String {
        error.to_string()
    }

    #[test]
    fn every_legacy_identity_field_is_required_independently() -> Result<(), String> {
        let aapl = AssetId::parse("b0b6dd9d-8b9b-48a9-ba46-b9d54906e415").map_err(text)?;
        let other = AssetId::parse("b0b6dd9d-8b9b-48a9-ba46-b9d54906e416").map_err(text)?;
        let aapl_symbol = InstrumentId::new("AAPL").map_err(text)?;
        let other_symbol = InstrumentId::new("MSFT").map_err(text)?;
        let matches = |asset_id, symbol, exchange, model_id, model_version| {
            legacy_identity(&ProductionIdentity {
                asset_id,
                symbol,
                asset_class: AssetClass::UsEquity,
                broker_exchange: exchange,
                gate_exchange: GateExchange::Nasdaq,
                model_id,
                model_version,
                model_hash: Digest::of(b"model"),
            })
        };
        assert!(matches(
            &aapl,
            &aapl_symbol,
            BrokerExchange::Nasdaq,
            "quant.ma_crossover",
            "1.0.0"
        ));
        assert!(!matches(
            &other,
            &aapl_symbol,
            BrokerExchange::Nasdaq,
            "quant.ma_crossover",
            "1.0.0"
        ));
        assert!(!matches(
            &aapl,
            &other_symbol,
            BrokerExchange::Nasdaq,
            "quant.ma_crossover",
            "1.0.0"
        ));
        assert!(!matches(
            &aapl,
            &aapl_symbol,
            BrokerExchange::Nyse,
            "quant.ma_crossover",
            "1.0.0"
        ));
        assert!(!matches(
            &aapl,
            &aapl_symbol,
            BrokerExchange::Nasdaq,
            "quant.other_model",
            "1.0.0"
        ));
        assert!(!matches(
            &aapl,
            &aapl_symbol,
            BrokerExchange::Nasdaq,
            "quant.ma_crossover",
            "2.0.0"
        ));
        Ok(())
    }

    #[test]
    fn every_eligible_exchange_maps_to_both_consumers() -> Result<(), String> {
        let cases = [
            ("nasdaq", BrokerExchange::Nasdaq, GateExchange::Nasdaq),
            ("nyse", BrokerExchange::Nyse, GateExchange::Nyse),
            ("arca", BrokerExchange::Arca, GateExchange::Arca),
            ("amex", BrokerExchange::Amex, GateExchange::Amex),
            ("bats", BrokerExchange::Bats, GateExchange::Bats),
        ];
        for (name, broker, gate) in cases {
            assert_eq!(exchanges(name).map_err(text)?, (broker, gate), "{name}");
        }
        assert!(exchanges("otc").is_err());
        assert!(exchanges("other").is_err());
        Ok(())
    }
}
