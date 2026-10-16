//! The reviewed production inputs, each read once from the confirmed version and the effective
//! registrations and judged on the bytes hashed into the executor's config references
//! (DEC-470 item 2; E19-11, DEC-505).

use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_alpaca::Exchange as BrokerExchange;
use mandate_canon::{Digest, Key, Value};
use mandate_domain::{AssetClass as DomainAssetClass, AssetId};
use mandate_executor::{BindingGateConfigRefs, ExecutorConfig};
use mandate_num::{Fraction, Qty, ShareIncrement};
use mandate_risk::{EtpClass, Exchange as GateExchange, GateConfig};
use mandate_runtime::{AgentId, ConnectionId, Deployment, WorkspaceId};
use mandate_time::{Date, TradingCalendar, UtcNanos};

use super::{absent, usd};
use crate::control::{Configuration, ConfirmedVersion, SnapshotExchange};
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
    gate_config: GateConfig,
    executor_config: ExecutorConfig,
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

/// Effective gate and executor configuration read from the content-addressed rule set.
pub struct ProductionConfiguration<'a> {
    pub gate: &'a GateConfig,
    pub executor: ExecutorConfig,
}

/// Validated opaque deployment identity for one production cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentInput {
    deployment: Deployment,
    account_ref: String,
}

impl DeploymentInput {
    /// The mandate-bound runtime deployment identity.
    pub fn deployment(&self) -> &Deployment {
        &self.deployment
    }

    /// The opaque account-stream subject.
    pub fn account_ref(&self) -> &str {
        &self.account_ref
    }
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
    /// The run's artifacts from the confirmed version and the effective registrations on the
    /// control stream (E19-11, DEC-505), never from a file: the mandate and its one pinned model,
    /// whose registered content is a DEC-504 content object (`kind` `quant_model_content`) naming
    /// the pin's own `model_id` and `model_version` under the pinned hash; the registered fee
    /// schedule, calendar and rule set, each judged as its file
    /// was; and the instrument from the DEC-523 snapshot, which must
    /// be the mandate's pinned asset and symbol. The config references hash the registered bytes,
    /// so the executor journals the objects the stream names (E7-19 slice 2 remainder, the brief's
    /// Q1, X-8). It needs no credential, so it runs before the preflight.
    ///
    /// # Errors
    /// [`Cause::Absent`] for a registered object the run cannot use, as for its file.
    pub fn from_registered(
        confirmed: &ConfirmedVersion,
        configuration: &Configuration,
    ) -> Result<Self, Cause> {
        let mandate = confirmed.mandate().clone();
        if mandate.environment != mandate_domain::Environment::Paper {
            return Err(absent("a paper mandate"));
        }
        let [pinned] = mandate.universe.pinned_instruments.as_slice() else {
            return Err(absent("one pinned production instrument"));
        };
        let [model] = mandate.behavior.signal_models.as_slice() else {
            return Err(absent("the one reviewed signal model"));
        };
        let registered = &configuration.model_version;
        let content = mandate_canon::parse(&registered.bytes)
            .map_err(|_| absent("the registered model content"))?;
        let member = |name: &str| content.get(name).and_then(Value::as_str);
        if model.content_hash != registered.content_hash
            || member("kind") != Some("quant_model_content")
            || member("model_id") != Some(model.id.as_str())
            || member("model_version") != Some(model.version.as_str())
        {
            return Err(absent("the registered model"));
        }
        let snapshot = &configuration.instrument;
        if pinned.asset_class != DomainAssetClass::UsEquity
            || snapshot.instrument_id != pinned.asset_id.as_str()
            || snapshot.symbol != pinned.symbol
        {
            return Err(absent("the registered instrument"));
        }
        let (broker_exchange, gate_exchange) = exchanges(match snapshot.exchange {
            SnapshotExchange::Arca => "arca",
            SnapshotExchange::Nasdaq => "nasdaq",
        })?;
        let instrument = ReviewedInstrument {
            asset_id: pinned.asset_id.clone(),
            symbol: InstrumentId::new(&pinned.symbol)
                .map_err(|_| absent("the registered instrument symbol"))?,
            asset_class: DomainAssetClass::UsEquity,
            broker_exchange,
            gate_exchange,
            increment: ShareIncrement::Whole,
            increment_qty: Qty::parse(WHOLE_SHARE)?,
            etp: EtpClass::Plain,
            etp_classified_at: snapshot.etp_classified_at,
        };
        let fee = artifact_from(configuration.fee_config.bytes.clone(), FEE_MEMBERS)?;
        let calendar = artifact_from(
            configuration.trading_calendar.bytes.clone(),
            CALENDAR_MEMBERS,
        )?;
        let rules = artifact_from(configuration.rule_set.bytes.clone(), RULE_MEMBERS)?;
        let fees = fees(&fee, &calendar)?;
        let (quote_max_age, gate_config, executor_config) = rule_set(&rules)?;
        let canonical_mandate = mandate
            .canonical_bytes()
            .map_err(|_| absent("the mandate's canonical artifact"))?;
        let config_refs = BindingGateConfigRefs::complete(
            reference(&fee.bytes),
            reference(&calendar.bytes),
            reference(&configuration.instrument_snapshot.bytes),
            reference(&rules.bytes),
            reference(&canonical_mandate),
        );
        Ok(Self {
            model_id: model.id.as_str().to_owned(),
            model_version: model.version.clone(),
            model_hash: model.content_hash,
            mandate,
            fees,
            instrument,
            quote_max_age,
            config_refs,
            gate_config,
            executor_config,
        })
    }

    /// The content-addressed references the executor journals with every gate decision.
    pub fn config_refs(&self) -> &BindingGateConfigRefs {
        &self.config_refs
    }

    /// The rule set's `iex_quote_max_age_s`: the age bound of the opening's quotes (DEC-470
    /// item 2), which the runner's cross-check also holds the broker's own quote to (DEC-945).
    pub fn quote_max_age(&self) -> Duration {
        self.quote_max_age
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

    /// Returns the effective gate and executor configuration from the reviewed rule set.
    ///
    /// # Errors
    /// This loaded artifact has already validated both configurations, so this cannot fail.
    pub fn production_configuration(&self) -> Result<ProductionConfiguration<'_>, Cause> {
        Ok(ProductionConfiguration {
            gate: &self.gate_config,
            executor: self.executor_config,
        })
    }

    /// Validates caller-supplied opaque deployment ids and binds the connection from the mandate.
    ///
    /// # Errors
    /// [`Cause::Absent`] when an opaque id is empty, too long, or off-pattern.
    pub fn deployment(
        &self,
        workspace: String,
        agent: String,
        account_ref: String,
    ) -> Result<DeploymentInput, Cause> {
        require_opaque_id(&workspace, "a valid workspace id")?;
        require_opaque_id(&agent, "a valid agent id")?;
        require_opaque_id(&account_ref, "a valid account reference")?;
        Ok(DeploymentInput {
            deployment: Deployment {
                agent: AgentId(agent),
                connection: ConnectionId(self.mandate.connection_id.as_str().to_owned()),
                workspace: WorkspaceId(workspace),
            },
            account_ref,
        })
    }
}

fn gate_configuration(rule_set: &Value) -> Result<GateConfig, Cause> {
    let value = required_exact_object(
        rule_set,
        "gate_config",
        &[
            "close_window_minutes",
            "collar_crypto_x",
            "collar_liquid_threshold_usd",
            "collar_liquid_x",
            "collar_other_x",
            "collar_passive_band",
            "crypto_liquidity_floor_usd",
            "daily_participation",
            "etp_classification_max_age_s",
            "legacy_pdt_equity_threshold",
            "liquidity_floor_usd",
            "min_resting_time_s",
            "opposite_fill_interval_s",
            "order_size_participation",
            "order_to_fill_max",
            "order_to_fill_min_orders",
            "price_floor",
        ],
    )?;
    Ok(GateConfig {
        price_floor: usd(required_text(value, "price_floor")?)?,
        liquidity_floor_usd: usd(required_text(value, "liquidity_floor_usd")?)?,
        crypto_liquidity_floor_usd: usd(required_text(value, "crypto_liquidity_floor_usd")?)?,
        collar_liquid_threshold_usd: usd(required_text(value, "collar_liquid_threshold_usd")?)?,
        collar_liquid_x: Fraction::parse(required_text(value, "collar_liquid_x")?)?,
        collar_other_x: Fraction::parse(required_text(value, "collar_other_x")?)?,
        collar_crypto_x: Fraction::parse(required_text(value, "collar_crypto_x")?)?,
        collar_passive_band: Fraction::parse(required_text(value, "collar_passive_band")?)?,
        opposite_fill_interval_s: required_positive_u32(value, "opposite_fill_interval_s")?,
        min_resting_time_s: required_positive_u32(value, "min_resting_time_s")?,
        order_to_fill_max: required_positive_u32(value, "order_to_fill_max")?,
        order_to_fill_min_orders: required_positive_u32(value, "order_to_fill_min_orders")?,
        order_size_participation: Fraction::parse(required_text(
            value,
            "order_size_participation",
        )?)?,
        daily_participation: Fraction::parse(required_text(value, "daily_participation")?)?,
        close_window_minutes: required_positive_u32(value, "close_window_minutes")?,
        legacy_pdt_equity_threshold: usd(required_text(value, "legacy_pdt_equity_threshold")?)?,
        etp_classification_max_age_s: required_positive_u32(value, "etp_classification_max_age_s")?,
    })
}

fn executor_configuration(rule_set: &Value) -> Result<ExecutorConfig, Cause> {
    let value = required_exact_object(
        rule_set,
        "executor",
        &[
            "bracket_partial_fill_timeout_s",
            "exit_step_s",
            "gtc_expiry_days",
            "max_intent_age_s",
            "max_unprotected_s",
            "protective_replace_buffer_trading_days",
            "restriction_403_threshold",
            "stop_watchdog_s",
            "unknown_absent_lookups",
            "unknown_absent_window_s",
        ],
    )?;
    Ok(ExecutorConfig {
        max_intent_age_s: required_positive_i64(value, "max_intent_age_s")?,
        unknown_absent_lookups: required_positive_u32(value, "unknown_absent_lookups")?,
        unknown_absent_window_s: required_positive_i64(value, "unknown_absent_window_s")?,
        protective_replace_buffer_trading_days: required_positive_u32(
            value,
            "protective_replace_buffer_trading_days",
        )?,
        restriction_403_threshold: required_positive_u32(value, "restriction_403_threshold")?,
        bracket_partial_fill_timeout_s: required_positive_i64(
            value,
            "bracket_partial_fill_timeout_s",
        )?,
        max_unprotected_s: required_positive_i64(value, "max_unprotected_s")?,
        stop_watchdog_s: required_positive_i64(value, "stop_watchdog_s")?,
        exit_step_s: required_positive_i64(value, "exit_step_s")?,
        gtc_expiry_days: required_positive_u32(value, "gtc_expiry_days")?,
    })
}

fn required_exact_object<'a>(
    value: &'a Value,
    name: &'static str,
    members: &[&str],
) -> Result<&'a Value, Cause> {
    let nested = value.get(name).ok_or_else(|| absent(name))?;
    let exact = nested
        .as_object()
        .is_some_and(|object| object.keys().map(Key::as_str).eq(members.iter().copied()));
    if exact { Ok(nested) } else { Err(absent(name)) }
}

fn required_positive_u64(value: &Value, name: &'static str) -> Result<u64, Cause> {
    value
        .get(name)
        .and_then(Value::as_int)
        .filter(|number| *number > 0)
        .ok_or_else(|| absent(name))
}

fn required_positive_u32(value: &Value, name: &'static str) -> Result<u32, Cause> {
    u32::try_from(required_positive_u64(value, name)?).map_err(|_| absent(name))
}

fn required_positive_i64(value: &Value, name: &'static str) -> Result<i64, Cause> {
    i64::try_from(required_positive_u64(value, name)?).map_err(|_| absent(name))
}

fn require_opaque_id(text: &str, fact: &'static str) -> Result<(), Cause> {
    if (1..=64).contains(&text.len())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        Ok(())
    } else {
        Err(absent(fact))
    }
}

const FEE_MEMBERS: &[&str] = &["effective_from", "environment", "schedule"];
const CALENDAR_MEMBERS: &[&str] = &["first", "holidays", "last", "special_sessions"];
const RULE_MEMBERS: &[&str] = &[
    "executor",
    "gate",
    "gate_config",
    "iex_quote_max_age_s",
    "ruleset_version",
    "story",
];

/// The reviewed fee schedule over the reviewed calendar.
fn fees(fee: &Artifact, calendar: &Artifact) -> Result<mandate_accounting::Config, Cause> {
    require_text(&fee.value, "schedule", "conservative_v1")?;
    let first = Date::parse(required_text(&calendar.value, "first")?)
        .map_err(|_| absent("the trading calendar's first date"))?;
    let last = Date::parse(required_text(&calendar.value, "last")?)
        .map_err(|_| absent("the trading calendar's last date"))?;
    require_empty_array(&calendar.value, "holidays")?;
    require_empty_array(&calendar.value, "special_sessions")?;
    let calendar = TradingCalendar::new(first, last, [], [])
        .map_err(|_| absent("the effective trading calendar"))?;
    mandate_executor::paper_only_fee_config(
        required_text(&fee.value, "environment")?,
        calendar,
        required_text(&fee.value, "effective_from")?,
    )
    .map_err(Cause::Executor)
}

/// The reviewed rule set's quote age bound and its gate and executor configurations.
fn rule_set(rules: &Artifact) -> Result<(Duration, GateConfig, ExecutorConfig), Cause> {
    require_text(&rules.value, "gate", "trading-domain-9.1")?;
    require_text(&rules.value, "ruleset_version", "v1")?;
    require_text(&rules.value, "story", "E7-7")?;
    let quote_max_age_s = rules
        .value
        .get("iex_quote_max_age_s")
        .and_then(Value::as_int)
        .filter(|seconds| (1..=MAX_QUOTE_AGE_S).contains(seconds))
        .ok_or_else(|| absent("a reviewed IEX quote age of 1 to 60 seconds"))?;
    Ok((
        Duration::from_secs(quote_max_age_s),
        gate_configuration(&rules.value)?,
        executor_configuration(&rules.value)?,
    ))
}

fn artifact_from(bytes: Vec<u8>, members: &[&str]) -> Result<Artifact, Cause> {
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
    use mandate_alpaca::Exchange as BrokerExchange;
    use mandate_risk::Exchange as GateExchange;
    use mandate_runtime::{AgentId, ConnectionId, Deployment, WorkspaceId};

    use super::{DeploymentInput, exchanges, require_opaque_id, required_positive_u64};

    fn text<E: std::fmt::Display>(error: E) -> String {
        error.to_string()
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

    #[test]
    fn deployment_input_exposes_only_its_validated_values() {
        let input = DeploymentInput {
            deployment: Deployment {
                agent: AgentId("agent-deployment-9".to_owned()),
                connection: ConnectionId("conn-owner-paper-42".to_owned()),
                workspace: WorkspaceId("workspace-owner-42".to_owned()),
            },
            account_ref: "account-ref-7".to_owned(),
        };
        assert_eq!(input.deployment().workspace.0, "workspace-owner-42");
        assert_eq!(input.deployment().agent.0, "agent-deployment-9");
        assert_eq!(input.deployment().connection.0, "conn-owner-paper-42");
        assert_eq!(input.account_ref(), "account-ref-7");
    }

    #[test]
    fn opaque_deployment_ids_use_the_schema_id_grammar() {
        assert!(require_opaque_id("A_z-9", "id").is_ok());
        assert!(require_opaque_id(&"a".repeat(64), "id").is_ok());
        let too_long = "a".repeat(65);
        for invalid in ["", "A.z", "A z", too_long.as_str()] {
            assert!(require_opaque_id(invalid, "id").is_err(), "{invalid}");
        }
    }

    #[test]
    fn configured_counts_and_durations_are_strictly_positive() -> Result<(), String> {
        let zero = mandate_canon::parse(br#"{"n":0}"#).map_err(text)?;
        let one = mandate_canon::parse(br#"{"n":1}"#).map_err(text)?;
        assert!(required_positive_u64(&zero, "n").is_err());
        assert_eq!(required_positive_u64(&one, "n").map_err(text)?, 1);
        Ok(())
    }
}
