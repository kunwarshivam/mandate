use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_alpaca::{
    Asset, AssetSnapshot, Exchange as BrokerExchange, Feed, LatestQuote, MinuteBar, MinuteBars,
    alpaca_account_rules,
};
use mandate_builder::buy_action;
use mandate_canon::Digest;
use mandate_domain::AssetId;
use mandate_executor::{
    AgentId as ExecutorAgentId, BindingGateInput, BindingGateRequest, BindingGateSource,
    BrokerAccount, BrokerOrder, BrokerPosition, InstrumentSnapshot, MandateView, Purpose,
};
use mandate_num::{Fraction, Price, Qty, SignedQty, Unit, Usd};
use mandate_risk::{Exchange as GateExchange, GateConfig, SaneQuote};
use mandate_runtime::{AgentId, ProtectionPrices};
use mandate_time::{Date, UtcNanos};

use super::artifacts::reference;
use super::context::{PaperClock, TrustedPaperContext, protection_prices};
use super::gate::{gate_mandate, gate_template};
use super::{Artifacts, BrokerFacts, INSTRUMENT_ID, LiquidityFacts, PaperFacts, load_contexts};
use crate::{Cause, Setup};

const NOW: &str = "2026-09-28T17:00:00Z";
const AGENT: &str = "tracer-aapl";
const OTHER_ASSET: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e416";

fn text<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tracer")
}

fn at(rfc3339: &str) -> Result<UtcNanos, String> {
    UtcNanos::parse_rfc3339(rfc3339).map_err(text)
}

fn price(value: &str) -> Result<Price, String> {
    Price::parse(value).map_err(text)
}

fn usd(value: &str) -> Result<Usd, String> {
    Usd::parse(value).map_err(text)
}

fn qty(value: &str) -> Result<Qty, String> {
    Qty::parse(value).map_err(text)
}

fn symbol(value: &str) -> Result<InstrumentId, String> {
    InstrumentId::new(value).map_err(text)
}

fn artifacts() -> Result<Artifacts, String> {
    Artifacts::load(&fixtures().join("mandate.json"), &fixtures().join("config")).map_err(text)
}

fn reviewed_gate_config() -> Result<GateConfig, String> {
    artifacts()?
        .production_configuration()
        .map(|configuration| configuration.gate.clone())
        .map_err(text)
}

fn production_artifacts(name: &str) -> Result<(Scratch, Artifacts), String> {
    let scratch = Scratch::new(name)?;
    scratch.replace("mandate.json", INSTRUMENT_ID, OTHER_ASSET)?;
    scratch.replace("mandate.json", "AAPL", "MSFT")?;
    scratch.replace("mandate.json", "quant.ma_crossover", "quant.other_model")?;
    scratch.replace(
        "mandate.json",
        "\"version\": \"1.0.0\"",
        "\"version\": \"2.0.0\"",
    )?;
    scratch.replace(
        "config/instrument-snapshot.json",
        INSTRUMENT_ID,
        OTHER_ASSET,
    )?;
    scratch.replace("config/instrument-snapshot.json", "AAPL", "MSFT")?;
    scratch.replace("config/instrument-snapshot.json", "\"nasdaq\"", "\"nyse\"")?;
    let model = br#"{"id":"quant.other_model","version":"2.0.0"}"#;
    fs::write(scratch.0.join("config/model-artifact.json"), model).map_err(text)?;
    scratch.replace(
        "mandate.json",
        "sha256:4f3559229f89b27c04b43ec773b0ff0b884895362622964b78dfd791d67e18fc",
        &format!("sha256:{}", Digest::of(model).to_hex()),
    )?;
    let artifacts =
        Artifacts::load_production(&scratch.0.join("mandate.json"), &scratch.0.join("config"))
            .map_err(text)?;
    Ok((scratch, artifacts))
}

fn production_deployment_artifacts(name: &str) -> Result<(Scratch, Artifacts), String> {
    let scratch = Scratch::new(name)?;
    scratch.replace(
        "mandate.json",
        "conn_alpaca_paper_01",
        "conn_owner_paper_42",
    )?;
    let artifacts =
        Artifacts::load_production(&scratch.0.join("mandate.json"), &scratch.0.join("config"))
            .map_err(text)?;
    Ok((scratch, artifacts))
}

fn production_configuration_artifacts(name: &str) -> Result<(Scratch, Artifacts), String> {
    let scratch = Scratch::new(name)?;
    for (old, new) in [
        (
            "\"bracket_partial_fill_timeout_s\":60",
            "\"bracket_partial_fill_timeout_s\":62",
        ),
        ("\"exit_step_s\":5", "\"exit_step_s\":7"),
        ("\"gtc_expiry_days\":90", "\"gtc_expiry_days\":91"),
        ("\"max_intent_age_s\":120", "\"max_intent_age_s\":121"),
        ("\"max_unprotected_s\":60", "\"max_unprotected_s\":63"),
        (
            "\"protective_replace_buffer_trading_days\":5",
            "\"protective_replace_buffer_trading_days\":6",
        ),
        (
            "\"restriction_403_threshold\":3",
            "\"restriction_403_threshold\":7",
        ),
        ("\"stop_watchdog_s\":60", "\"stop_watchdog_s\":64"),
        (
            "\"unknown_absent_lookups\":3",
            "\"unknown_absent_lookups\":4",
        ),
        (
            "\"unknown_absent_window_s\":15",
            "\"unknown_absent_window_s\":16",
        ),
        ("\"close_window_minutes\":10", "\"close_window_minutes\":11"),
        (
            "\"collar_crypto_x\":\"0.02\"",
            "\"collar_crypto_x\":\"0.033\"",
        ),
        (
            "\"collar_liquid_threshold_usd\":\"50000000\"",
            "\"collar_liquid_threshold_usd\":\"51000000\"",
        ),
        (
            "\"collar_liquid_x\":\"0.01\"",
            "\"collar_liquid_x\":\"0.011\"",
        ),
        (
            "\"collar_other_x\":\"0.02\"",
            "\"collar_other_x\":\"0.022\"",
        ),
        (
            "\"collar_passive_band\":\"0.2\"",
            "\"collar_passive_band\":\"0.24\"",
        ),
        (
            "\"crypto_liquidity_floor_usd\":\"1000000\"",
            "\"crypto_liquidity_floor_usd\":\"1100000\"",
        ),
        (
            "\"daily_participation\":\"0.05\"",
            "\"daily_participation\":\"0.066\"",
        ),
        (
            "\"etp_classification_max_age_s\":604800",
            "\"etp_classification_max_age_s\":604801",
        ),
        (
            "\"legacy_pdt_equity_threshold\":\"25000\"",
            "\"legacy_pdt_equity_threshold\":\"25001\"",
        ),
        (
            "\"liquidity_floor_usd\":\"1000000\"",
            "\"liquidity_floor_usd\":\"1000001\"",
        ),
        ("\"min_resting_time_s\":2", "\"min_resting_time_s\":3"),
        (
            "\"opposite_fill_interval_s\":60",
            "\"opposite_fill_interval_s\":61",
        ),
        (
            "\"order_size_participation\":\"0.05\"",
            "\"order_size_participation\":\"0.055\"",
        ),
        ("\"order_to_fill_max\":10", "\"order_to_fill_max\":12"),
        (
            "\"order_to_fill_min_orders\":20",
            "\"order_to_fill_min_orders\":21",
        ),
        ("\"price_floor\":\"5\"", "\"price_floor\":\"6\""),
    ] {
        scratch.replace("config/rule-set.json", old, new)?;
    }
    let artifacts =
        Artifacts::load_production(&scratch.0.join("mandate.json"), &scratch.0.join("config"))
            .map_err(text)?;
    Ok((scratch, artifacts))
}

fn agent() -> AgentId {
    AgentId(AGENT.to_owned())
}

#[derive(Clone)]
struct TestClock(Rc<Cell<UtcNanos>>);

impl PaperClock for TestClock {
    fn now(&self) -> Option<UtcNanos> {
        Some(self.0.get())
    }
}

/// A scratch copy of the reviewed artifacts, under a name no other test uses.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("mandate-e77-{name}-{}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).map_err(text)?;
        }
        fs::create_dir_all(path.join("config")).map_err(text)?;
        fs::copy(fixtures().join("mandate.json"), path.join("mandate.json")).map_err(text)?;
        for name in [
            "fee-config.json",
            "instrument-snapshot.json",
            "model-artifact.json",
            "rule-set.json",
            "trading-calendar.json",
        ] {
            fs::copy(
                fixtures().join("config").join(name),
                path.join("config").join(name),
            )
            .map_err(text)?;
        }
        Ok(Self(path))
    }

    fn load(&self) -> Result<Artifacts, Cause> {
        Artifacts::load(&self.0.join("mandate.json"), &self.0.join("config"))
    }

    fn replace(&self, file: &str, from: &str, to: &str) -> Result<(), String> {
        let path = self.0.join(file);
        let original = fs::read_to_string(&path).map_err(text)?;
        let changed = original.replace(from, to);
        if changed == original {
            return Err(format!("fixture text not found in {file}: {from}"));
        }
        fs::write(path, changed).map_err(text)
    }

    fn remove(self) -> Result<(), String> {
        fs::remove_dir_all(&self.0).map_err(text)
    }
}

fn account() -> Result<BrokerAccount, String> {
    Ok(BrokerAccount {
        status: "ACTIVE".to_owned(),
        crypto_status: "ACTIVE".to_owned(),
        trading_blocked: false,
        account_blocked: false,
        trade_suspended_by_user: false,
        multiplier: 4,
        equity: usd("1000000")?,
        cash: usd("1000000")?,
        buying_power: usd("3999997.98")?,
        non_marginable_buying_power: usd("999998.99")?,
        accrued_fees: Usd::ZERO,
        last_equity: usd("1000000")?,
        maintenance_margin: Usd::ZERO,
    })
}

/// A fresh snapshot read just before `now`: the asset a second before and the quote half a second
/// before, both well inside the reviewed ten-second bound.
fn facts_at(asset_at: &str, quote_at: &str) -> Result<PaperFacts, String> {
    Ok(PaperFacts {
        broker: BrokerFacts {
            account: account()?,
            account_rules: alpaca_account_rules(),
            positions: Vec::new(),
            open_orders: Vec::new(),
            asset: AssetSnapshot {
                asset: Asset {
                    asset_id: INSTRUMENT_ID.to_owned(),
                    instrument: symbol("AAPL")?,
                    class: AssetClass::UsEquity,
                    exchange: BrokerExchange::Nasdaq,
                    active: true,
                    tradable: true,
                    fractionable: true,
                    ipo: false,
                    ptp_no_exception: false,
                    min_order_size: None,
                    min_trade_increment: None,
                    price_increment: None,
                },
                loaded_at: at(asset_at)?,
            },
            quote: LatestQuote {
                instrument: symbol("AAPL")?,
                at: at(quote_at)?,
                bid: price("255.1")?,
                bid_size: qty("2")?,
                ask: price("255.2")?,
                ask_size: qty("1")?,
                feed: Feed::Iex,
            },
            minute_bars: MinuteBars {
                instrument: symbol("AAPL")?,
                bars: vec![MinuteBar {
                    start: at("2026-09-28T16:59:00Z")?,
                    volume: qty("515")?,
                }],
            },
        },
        liquidity: LiquidityFacts {
            prior_close: price("255.2")?,
            median_dollar_volume_20d: usd("245200000")?,
            adv_20d: qty("1000000")?,
            trailing_5m_volume: qty("515")?,
        },
    })
}

fn facts() -> Result<PaperFacts, String> {
    facts_at("2026-09-28T16:59:59Z", "2026-09-28T16:59:59.5Z")
}

fn refusal(result: Result<super::Contexts, Cause>) -> Result<&'static str, String> {
    match result {
        Err(Cause::Absent { what }) => Ok(what),
        Err(other) => Err(format!("expected an absent fact, got {other}")),
        Ok(_) => Err("the assembly accepted the snapshot".to_owned()),
    }
}

#[test]
fn the_reviewed_artifacts_load_and_bind_the_bytes_they_checked() -> Result<(), String> {
    let loaded = artifacts()?;
    let config = fixtures().join("config");
    let file = |name: &str| fs::read(config.join(name)).map_err(text);
    let refs = loaded.config_refs();
    assert_eq!(refs.fee_config, Some(reference(&file("fee-config.json")?)));
    assert_eq!(
        refs.trading_calendar,
        Some(reference(&file("trading-calendar.json")?))
    );
    assert_eq!(
        refs.instrument_snapshot,
        Some(reference(&file("instrument-snapshot.json")?))
    );
    assert_eq!(refs.rule_set, Some(reference(&file("rule-set.json")?)));
    let canonical = loaded.mandate.canonical_bytes().map_err(text)?;
    assert_eq!(refs.mandate_version, Some(reference(&canonical)));
    assert_eq!(
        loaded.quote_max_age,
        std::time::Duration::from_secs(10),
        "the reviewed rule set's quote bound"
    );
    assert_eq!(
        loaded.instrument.etp_classified_at,
        at("2026-09-25T00:00:00Z")?
    );
    Ok(())
}

#[test]
fn production_artifacts_derive_the_instrument_and_model_without_shell_literals()
-> Result<(), String> {
    let (scratch, loaded) = production_artifacts("production-inputs")?;
    let identity = loaded.production_identity();
    assert_eq!(identity.asset_id.as_str(), OTHER_ASSET);
    assert_eq!(identity.symbol.as_str(), "MSFT");
    assert_eq!(identity.asset_class, mandate_domain::AssetClass::UsEquity);
    assert_eq!(identity.broker_exchange, BrokerExchange::Nyse);
    assert_eq!(identity.gate_exchange, GateExchange::Nyse);
    assert_eq!(identity.model_id, "quant.other_model");
    assert_eq!(identity.model_version, "2.0.0");
    let model = fs::read(scratch.0.join("config/model-artifact.json")).map_err(text)?;
    assert_eq!(identity.model_hash, Digest::of(&model));
    scratch.remove()
}

#[test]
fn production_contexts_use_the_reviewed_instrument_and_model_end_to_end() -> Result<(), String> {
    let (scratch, loaded) = production_artifacts("production-contexts")?;
    let mut snapshot = facts()?;
    snapshot.broker.asset.asset.asset_id = OTHER_ASSET.to_owned();
    snapshot.broker.asset.asset.instrument = symbol("MSFT")?;
    snapshot.broker.asset.asset.exchange = BrokerExchange::Nyse;
    snapshot.broker.quote.instrument = symbol("MSFT")?;
    snapshot.broker.minute_bars.instrument = symbol("MSFT")?;

    let contexts = load_contexts(&loaded, &snapshot, at(NOW)?, &agent()).map_err(text)?;
    let decision = contexts
        .run
        .decision
        .ok_or("the production context carries a decision")?;
    let builder = decision.builder.ok_or("a production builder context")?;
    assert_eq!(
        builder.market.instrument,
        AssetId::parse(OTHER_ASSET).map_err(text)?
    );
    assert_eq!(
        builder
            .model_content_hashes
            .get(&("quant.other_model".to_owned(), "2.0.0".to_owned())),
        Some(&loaded.model_hash)
    );
    let gate = decision.gate.ok_or("a production gate context")?;
    assert_eq!(gate.instrument.instrument.as_str(), OTHER_ASSET);
    assert_eq!(gate.instrument.exchange, Some(GateExchange::Nyse));

    let template =
        gate_template(&loaded, &snapshot, at(NOW)?, reviewed_gate_config()?).map_err(text)?;
    let trusted = TrustedPaperContext {
        template,
        agent: AGENT.to_owned(),
        increment: loaded.instrument.increment,
        fees: loaded.fees.clone(),
        quote_at: snapshot.broker.quote.at,
        quote_max_age: loaded.quote_max_age,
        clock: Rc::new(TestClock(Rc::new(Cell::new(at(NOW)?)))),
    };
    let executor_agent = ExecutorAgentId(AGENT.to_owned());
    let reviewed = symbol(OTHER_ASSET)?;
    assert!(trusted.covers(&executor_agent, &reviewed));
    assert_eq!(trusted.asset_class(&reviewed), Some(AssetClass::UsEquity));
    assert_eq!(
        trusted.increment(&reviewed),
        Some(mandate_num::ShareIncrement::Whole)
    );
    let request = BindingGateRequest {
        agent: &executor_agent,
        instrument: &reviewed,
        side: Side::Buy,
        qty: qty("1")?,
        limit: price("255.2")?,
        purpose: Purpose::Open,
        tif: mandate_executor::TimeInForce::Day,
        protection: None,
    };
    assert!(trusted.input(&request).is_some());
    scratch.remove()
}

#[test]
fn production_configuration_comes_from_the_content_addressed_rule_set() -> Result<(), String> {
    let (scratch, loaded) = production_configuration_artifacts("production-configuration")?;
    let configuration = loaded.production_configuration().map_err(text)?;
    let gate = configuration.gate;
    assert_eq!(
        (
            gate.price_floor,
            gate.liquidity_floor_usd,
            gate.crypto_liquidity_floor_usd,
            gate.collar_liquid_threshold_usd,
            gate.legacy_pdt_equity_threshold,
        ),
        (
            usd("6")?,
            usd("1000001")?,
            usd("1100000")?,
            usd("51000000")?,
            usd("25001")?,
        )
    );
    assert_eq!(
        (
            gate.collar_liquid_x,
            gate.collar_other_x,
            gate.collar_crypto_x,
            gate.collar_passive_band,
            gate.order_size_participation,
            gate.daily_participation,
        ),
        (
            Fraction::parse("0.011").map_err(text)?,
            Fraction::parse("0.022").map_err(text)?,
            Fraction::parse("0.033").map_err(text)?,
            Fraction::parse("0.24").map_err(text)?,
            Fraction::parse("0.055").map_err(text)?,
            Fraction::parse("0.066").map_err(text)?,
        )
    );
    assert_eq!(
        (
            gate.opposite_fill_interval_s,
            gate.min_resting_time_s,
            gate.order_to_fill_max,
            gate.order_to_fill_min_orders,
            gate.close_window_minutes,
            gate.etp_classification_max_age_s,
        ),
        (61, 3, 12, 21, 11, 604_801)
    );
    let executor = configuration.executor;
    assert_eq!(
        (
            executor.max_intent_age_s,
            executor.unknown_absent_lookups,
            executor.unknown_absent_window_s,
            executor.protective_replace_buffer_trading_days,
            executor.restriction_403_threshold,
        ),
        (121, 4, 16, 6, 7)
    );
    assert_eq!(
        (
            executor.bracket_partial_fill_timeout_s,
            executor.max_unprotected_s,
            executor.stop_watchdog_s,
            executor.exit_step_s,
            executor.gtc_expiry_days,
        ),
        (62, 63, 64, 7, 91)
    );
    let bytes = fs::read(scratch.0.join("config/rule-set.json")).map_err(text)?;
    let expected_ref = reference(&bytes);
    assert_eq!(
        loaded.config_refs.rule_set.as_deref(),
        Some(expected_ref.as_str())
    );
    scratch.remove()
}

#[test]
fn production_contexts_use_only_the_reviewed_gate_and_executor_configuration() -> Result<(), String>
{
    let (scratch, loaded) = production_configuration_artifacts("production-context-configuration")?;
    let _configuration = loaded.production_configuration().map_err(text)?;
    let source = include_str!("context.rs");
    let body = source
        .split_once("pub fn load_contexts_with_clock(")
        .and_then(|(_, after)| after.split_once("\n}\n\n/// The mandate's protection"))
        .map(|(body, _)| body)
        .ok_or_else(|| "the production context assembly is missing".to_owned())?;
    assert!(!body.contains("platform_gate_config"));
    assert!(!body.contains("ExecutorConfig::PROPOSED"));
    for required in [
        "let configuration = artifacts.production_configuration()?;",
        "let config = configuration.gate.clone();",
        "configuration.executor,",
    ] {
        assert!(
            body.contains(required),
            "production context assembly must use `{required}`"
        );
    }
    scratch.remove()
}

#[test]
fn production_deployment_takes_opaque_ids_and_binds_the_confirmed_connection() -> Result<(), String>
{
    let (scratch, loaded) = production_deployment_artifacts("production-deployment")?;
    let input = loaded
        .deployment(
            "workspace-owner-42".to_owned(),
            "agent-deployment-9".to_owned(),
            "account-ref-7".to_owned(),
        )
        .map_err(text)?;
    assert_eq!(input.deployment().workspace.0, "workspace-owner-42");
    assert_eq!(input.deployment().agent.0, "agent-deployment-9");
    assert_ne!(
        loaded.mandate.connection_id.as_str(),
        "conn_alpaca_paper_01"
    );
    assert_eq!(
        input.deployment().connection.0,
        loaded.mandate.connection_id.as_str()
    );
    assert_eq!(input.account_ref(), "account-ref-7");
    for (name, workspace, agent, account_ref) in [
        ("workspace", "", "agent-deployment-9", "account-ref-7"),
        ("agent", "workspace-owner-42", "", "account-ref-7"),
        ("account", "workspace-owner-42", "agent-deployment-9", ""),
    ] {
        assert!(
            loaded
                .deployment(
                    workspace.to_owned(),
                    agent.to_owned(),
                    account_ref.to_owned()
                )
                .is_err(),
            "{name} must not be empty"
        );
    }
    scratch.remove()
}

#[test]
fn shipping_paper_adapter_uses_only_the_validated_deployment_input() -> Result<(), String> {
    let (scratch, loaded) = production_deployment_artifacts("shipping-deployment")?;
    let input = loaded
        .deployment(
            "workspace-owner-42".to_owned(),
            "agent-deployment-9".to_owned(),
            "account-ref-7".to_owned(),
        )
        .map_err(text)?;
    let setup = Setup {
        deployment: input.deployment().clone(),
        account_ref: input.account_ref().to_owned(),
        now: at(NOW)?,
        place_one_order: false,
        new_cycle: false,
    };
    let source_agent = input.deployment().agent.clone();
    let source_workspace = input.deployment().workspace.0.clone();
    let source_account_ref = input.account_ref().to_owned();
    assert_eq!(setup.deployment.agent, source_agent);
    assert_eq!(setup.deployment.workspace.0, source_workspace);
    assert_eq!(
        setup.deployment.connection.0,
        loaded.mandate.connection_id.as_str()
    );
    assert_eq!(setup.account_ref, source_account_ref);
    let source = include_str!("../bin/mandate-tracer.rs");
    let after_signature = source
        .split_once("fn tracer()")
        .map(|(_, after)| after)
        .ok_or_else(|| "the shipping tracer function is missing".to_owned())?;
    let body = after_signature
        .split_once("\n}\n\nstruct SystemClock")
        .map(|(body, _)| body)
        .ok_or_else(|| "the shipping tracer function boundary is missing".to_owned())?;
    assert!(!body.contains("//") && !body.contains("/*"));
    let cli_source = include_str!("../cli.rs");
    for forbidden in [
        "WORKSPACE",
        "AGENT",
        "CONNECTION",
        "ACCOUNT_REF",
        "conn_alpaca_paper_01",
        "tracer-aapl",
        "tracer-paper",
        "\"tracer\"",
    ] {
        assert!(
            !body.contains(forbidden) && !cli_source.contains(forbidden),
            "the shipping adapter still selects {forbidden}"
        );
    }
    for required in [
        "cli::parse_production(",
        "Artifacts::load_production(",
        "artifacts.deployment(",
    ] {
        assert!(
            body.contains(required),
            "the shipping adapter must use {required}"
        );
    }
    assert!(!body.contains("Artifacts::load("));
    let deployment_block = body
        .split_once("let deployment_input = artifacts.deployment(")
        .and_then(|(_, after)| after.split_once(".map_err"))
        .map(|(block, _)| block)
        .ok_or_else(|| "the deployment-input call is missing".to_owned())?;
    assert!(!deployment_block.contains('"'));
    assert!(deployment_block.contains("args.workspace.clone()"));
    assert!(deployment_block.contains("args.agent.clone()"));
    assert!(deployment_block.contains("args.account_ref.clone()"));
    assert!(body.contains("let deployment_input = artifacts"));
    assert_eq!(
        body.matches("let agent = deployment_input.deployment().agent.clone();")
            .count(),
        1
    );
    let context_block = body
        .split_once("load_contexts_with_clock(")
        .and_then(|(_, after)| after.split_once(")"))
        .map(|(block, _)| block)
        .ok_or_else(|| "the trusted-context call is missing".to_owned())?;
    assert!(context_block.contains("&agent,"));
    let setup_block = body
        .split_once("let setup = Setup {")
        .and_then(|(_, after)| after.split_once("\n    };"))
        .map(|(block, _)| block)
        .ok_or_else(|| "the production setup block is missing".to_owned())?;
    assert!(setup_block.contains("deployment: deployment_input.deployment().clone(),"));
    assert!(setup_block.contains("account_ref: deployment_input.account_ref().to_owned(),"));
    let sources_block = body
        .split_once("production(Sources {")
        .and_then(|(_, after)| after.split_once("\n    });"))
        .map(|(block, _)| block)
        .ok_or_else(|| "the production sources block is missing".to_owned())?;
    assert!(sources_block.contains("agent,"));
    assert!(
        sources_block.contains("workspace: deployment_input.deployment().workspace.0.clone(),")
    );
    assert!(sources_block.contains("account_ref: deployment_input.account_ref().to_owned(),"));
    scratch.remove()
}

#[test]
fn a_missing_effective_dated_artifact_refuses_the_load() {
    let loaded = Artifacts::load(
        &fixtures().join("mandate.json"),
        &fixtures().join("no-config"),
    );
    assert!(
        matches!(loaded, Err(Cause::Absent { .. })),
        "a missing artifact must refuse"
    );
}

#[test]
fn every_reviewed_mandate_identity_field_is_required_independently() -> Result<(), String> {
    let cases = [
        ("asset-id", INSTRUMENT_ID, OTHER_ASSET),
        ("symbol", "\"symbol\": \"AAPL\"", "\"symbol\": \"MSFT\""),
        (
            "asset-class",
            "\"asset_class\": \"us_equity\",\n        \"asset_id\"",
            "\"asset_class\": \"crypto_spot\",\n        \"asset_id\"",
        ),
        ("model-id", "quant.ma_crossover", "quant.other_model"),
        (
            "model-version",
            "\"version\": \"1.0.0\"",
            "\"version\": \"2.0.0\"",
        ),
        (
            "environment",
            "\"environment\": \"paper\"",
            "\"environment\": \"live\"",
        ),
    ];
    for (name, from, to) in cases {
        let scratch = Scratch::new(name)?;
        scratch.replace("mandate.json", from, to)?;
        assert!(
            scratch.load().is_err(),
            "{name} must be checked independently"
        );
        scratch.remove()?;
    }
    let scratch = Scratch::new("model-hash")?;
    fs::write(
        scratch.0.join("config/model-artifact.json"),
        b"{\"id\":\"quant.ma_crossover\",\"version\":\"1.0.0\"} ",
    )
    .map_err(text)?;
    assert!(
        scratch.load().is_err(),
        "the model content hash must be checked"
    );
    scratch.remove()
}

/// Every member an artifact carries is one the run reads, and every value but the reviewed one is
/// refused: an unknown member, a missing one, and each reviewed value changed.
#[test]
fn each_artifact_member_is_read_and_each_unreviewed_value_refused() -> Result<(), String> {
    let cases = [
        (
            "extra-member",
            "config/instrument-snapshot.json",
            "{\"asset_class\"",
            "{\"aa_note\":\"x\",\"asset_class\"",
        ),
        (
            "missing-quote-age",
            "config/rule-set.json",
            "\"iex_quote_max_age_s\":10,",
            "",
        ),
        (
            "zero-quote-age",
            "config/rule-set.json",
            "\"iex_quote_max_age_s\":10",
            "\"iex_quote_max_age_s\":0",
        ),
        (
            "long-quote-age",
            "config/rule-set.json",
            "\"iex_quote_max_age_s\":10",
            "\"iex_quote_max_age_s\":61",
        ),
        (
            "text-quote-age",
            "config/rule-set.json",
            "\"iex_quote_max_age_s\":10",
            "\"iex_quote_max_age_s\":\"10\"",
        ),
        (
            "gate",
            "config/rule-set.json",
            "trading-domain-9.1",
            "trading-domain-9.2",
        ),
        ("ruleset", "config/rule-set.json", "\"v1\"", "\"v2\""),
        ("story", "config/rule-set.json", "E7-7", "E7-8"),
        (
            "etp",
            "config/instrument-snapshot.json",
            "\"plain\"",
            "\"complex\"",
        ),
        (
            "etp-date",
            "config/instrument-snapshot.json",
            "2026-09-25T00:00:00Z",
            "2026-09-25",
        ),
        (
            "exchange",
            "config/instrument-snapshot.json",
            "\"nasdaq\"",
            "\"nyse\"",
        ),
        (
            "increment",
            "config/instrument-snapshot.json",
            "\"whole\"",
            "\"fractional\"",
        ),
        (
            "instrument-class",
            "config/instrument-snapshot.json",
            "\"us_equity\"",
            "\"crypto\"",
        ),
        (
            "instrument-id",
            "config/instrument-snapshot.json",
            INSTRUMENT_ID,
            OTHER_ASSET,
        ),
        (
            "instrument-symbol",
            "config/instrument-snapshot.json",
            "\"AAPL\"",
            "\"MSFT\"",
        ),
        (
            "fee-environment",
            "config/fee-config.json",
            "\"paper\"",
            "\"live\"",
        ),
        (
            "fee-schedule",
            "config/fee-config.json",
            "conservative_v1",
            "conservative_v2",
        ),
        (
            "fee-date",
            "config/fee-config.json",
            "2026-01-01",
            "2026-01-xx",
        ),
        (
            "holidays",
            "config/trading-calendar.json",
            "\"holidays\":[]",
            "\"holidays\":[\"2026-11-26\"]",
        ),
        (
            "special-sessions",
            "config/trading-calendar.json",
            "\"special_sessions\":[]",
            "\"special_sessions\":[\"2026-11-27\"]",
        ),
        (
            "calendar-first",
            "config/trading-calendar.json",
            "2026-01-01",
            "2026-13-01",
        ),
    ];
    for (name, file, from, to) in cases {
        let scratch = Scratch::new(name)?;
        scratch.replace(file, from, to)?;
        assert!(scratch.load().is_err(), "{name} must be refused");
        scratch.remove()?;
    }
    let scratch = Scratch::new("max-quote-age")?;
    scratch.replace(
        "config/rule-set.json",
        "\"iex_quote_max_age_s\":10",
        "\"iex_quote_max_age_s\":60",
    )?;
    let loaded = scratch.load().map_err(text)?;
    assert_eq!(loaded.quote_max_age, std::time::Duration::from_secs(60));
    scratch.remove()
}

#[test]
fn the_reviewed_snapshot_assembles_both_contexts_from_its_own_facts() -> Result<(), String> {
    let loaded = artifacts()?;
    let contexts = load_contexts(&loaded, &facts()?, at(NOW)?, &agent()).map_err(text)?;
    let decision = contexts
        .run
        .decision
        .ok_or("the run context carries a decision context")?;
    let builder = decision.builder.ok_or("a builder context")?;
    assert_eq!(builder.market.bid, price("255.1")?);
    assert_eq!(builder.market.ask, price("255.2")?);
    assert_eq!(builder.market.increment, qty("1")?);
    assert_eq!(builder.account.agent_equity, usd("1000")?);
    assert_eq!(builder.action.order_usd, usd("255.2")?);
    assert_eq!(
        builder.action,
        buy_action(
            &builder.account,
            &builder.market,
            &builder.risk,
            Unit::ONE,
            qty("1")?
        )
        .map_err(text)?,
        "the classification facts are the builder's own for one share at the ask"
    );
    assert_eq!(
        builder.risk.risk_day,
        Date::parse("2026-09-28").map_err(text)?
    );
    assert!(builder.execution.protection_required);
    assert_eq!(
        builder.execution.protection,
        Some(ProtectionPrices {
            stop: price("242.44")?,
            take_profit: Some(price("280.72")?),
        })
    );
    assert_eq!(contexts.run.validation.account_equity_usd, usd("1000000")?);
    let gate = decision.gate.ok_or("an advisory gate context")?;
    assert_eq!(gate.now, at(NOW)?);
    assert_eq!(gate.instrument.prior_close, Some(price("255.2")?));
    assert_eq!(
        gate.market.quote,
        Some(SaneQuote {
            bid: price("255.1")?,
            ask: price("255.2")?,
            at: at("2026-09-28T16:59:59.5Z")?,
        })
    );
    Ok(())
}

#[test]
fn the_gate_template_carries_the_snapshot_and_nothing_invented() -> Result<(), String> {
    let loaded = artifacts()?;
    let mut snapshot = facts()?;
    let config = reviewed_gate_config()?;
    let template = gate_template(&loaded, &snapshot, at(NOW)?, config.clone()).map_err(text)?;
    assert_eq!(template.config_refs, loaded.config_refs);
    assert_eq!(template.risk.agent_equity, usd("1000")?);
    assert_eq!(template.account.equity, usd("1000000")?);
    assert_eq!(
        template.account.model_buying_power,
        usd("999998.99")?,
        "the lesser of cash and non-marginable buying power"
    );
    assert_eq!(template.account.broker_buying_power, usd("3999997.98")?);
    assert_eq!(
        template.account.broker_non_marginable_buying_power,
        usd("999998.99")?
    );
    assert!(template.account.crypto_active);
    assert_eq!(template.instrument.prior_close, Some(price("255.2")?));
    assert_eq!(
        template.instrument.median_dollar_volume_20d,
        Some(usd("245200000")?)
    );
    assert_eq!(template.instrument.median_dollar_volume_30d, None);
    assert_eq!(
        template.instrument.etp_classified_at,
        Some(at("2026-09-25T00:00:00Z")?)
    );
    assert!(template.instrument.fractionable);
    assert!(!template.instrument.halted);
    assert!(
        !template.instrument.status_feed_current,
        "no status feed is read, so market orders stay barred"
    );
    assert_eq!(template.instrument.qty_increment, qty("1")?);
    assert_eq!(template.market.last_trade, None);
    assert_eq!(template.market.trailing_5m_volume, Some(qty("515")?));
    assert_eq!(template.market.adv_20d, Some(qty("1000000")?));
    assert_eq!(
        template.fee_reservation,
        usd("0.01")?,
        "the planned one-share buy owes the paper schedule's CAT fee of 0.01 a share"
    );

    snapshot.broker.account.cash = usd("500")?;
    snapshot.broker.account.crypto_status = "INACTIVE".to_owned();
    snapshot.broker.asset.asset.fractionable = false;
    let poorer = gate_template(&loaded, &snapshot, at(NOW)?, config).map_err(text)?;
    assert_eq!(poorer.account.model_buying_power, usd("500")?);
    assert!(!poorer.account.crypto_active);
    assert!(!poorer.instrument.fractionable);
    Ok(())
}

/// Each fact the assembly needs, broken alone, refuses it with its own reason.
#[test]
fn each_fact_that_does_not_hold_refuses_the_assembly() -> Result<(), String> {
    type Breaks = fn(&mut PaperFacts) -> Result<(), String>;
    let account = "an active, unblocked paper account";
    let asset = "the bound instrument's asset record";
    let current_asset = "a current asset record";
    let quote = "a current, uncrossed IEX quote";
    let cases: [(&str, &str, Breaks); 18] = [
        (
            "a position",
            "a paper account with no position and no open order",
            |f| {
                f.broker.positions.push(BrokerPosition {
                    instrument: symbol(INSTRUMENT_ID)?,
                    qty: SignedQty::parse("1").map_err(text)?,
                    avg_entry_price: price("255.2")?,
                });
                Ok(())
            },
        ),
        (
            "an open order",
            "a paper account with no position and no open order",
            |f| {
                f.broker.open_orders.push(BrokerOrder {
                    broker_order_id: "61e69015-8549-4bfd-b9c3-01e75843f47d".to_owned(),
                    client_order_id: None,
                    instrument: symbol("AAPL")?,
                    side: Side::Buy,
                    qty: qty("1")?,
                    filled_qty: Qty::ZERO,
                    limit_price: Some(price("255.2")?),
                    stop_price: None,
                    status: "new".to_owned(),
                    reject_code: None,
                    replaced_by_broker_order_id: None,
                    legs: Vec::new(),
                    created_on: None,
                });
                Ok(())
            },
        ),
        ("an inactive account", account, |f| {
            f.broker.account.status = "ACCOUNT_UPDATED".to_owned();
            Ok(())
        }),
        ("trading blocked", account, |f| {
            f.broker.account.trading_blocked = true;
            Ok(())
        }),
        ("account blocked", account, |f| {
            f.broker.account.account_blocked = true;
            Ok(())
        }),
        ("suspended by the user", account, |f| {
            f.broker.account.trade_suspended_by_user = true;
            Ok(())
        }),
        (
            "accrued fees",
            "a paper account without accrued fees",
            |f| {
                f.broker.account.accrued_fees = usd("0.01")?;
                Ok(())
            },
        ),
        ("another asset id", asset, |f| {
            f.broker.asset.asset.asset_id = OTHER_ASSET.to_owned();
            Ok(())
        }),
        ("another symbol", asset, |f| {
            f.broker.asset.asset.instrument = symbol("MSFT")?;
            Ok(())
        }),
        ("another class", asset, |f| {
            f.broker.asset.asset.class = AssetClass::Crypto;
            Ok(())
        }),
        ("another exchange", asset, |f| {
            f.broker.asset.asset.exchange = BrokerExchange::Nyse;
            Ok(())
        }),
        ("a stale asset record", current_asset, |f| {
            f.broker.asset.loaded_at = at("2026-09-28T16:59:49.999999999Z")?;
            Ok(())
        }),
        ("an asset record from after now", current_asset, |f| {
            f.broker.asset.loaded_at = at("2026-09-28T17:00:00.000000001Z")?;
            Ok(())
        }),
        ("a quote for another symbol", quote, |f| {
            f.broker.quote.instrument = symbol("MSFT")?;
            Ok(())
        }),
        ("a quote off the IEX feed", quote, |f| {
            f.broker.quote.feed = Feed::Crypto;
            Ok(())
        }),
        ("a quote from after now", quote, |f| {
            f.broker.quote.at = at("2026-09-28T17:00:00.000000001Z")?;
            Ok(())
        }),
        ("a stale quote", quote, |f| {
            f.broker.quote.at = at("2026-09-28T16:59:49.999999999Z")?;
            Ok(())
        }),
        ("a crossed quote", quote, |f| {
            f.broker.quote.bid = price("255.21")?;
            Ok(())
        }),
    ];
    let loaded = artifacts()?;
    for (name, expected, breaks) in cases {
        let mut snapshot = facts()?;
        breaks(&mut snapshot)?;
        let what = refusal(load_contexts(&loaded, &snapshot, at(NOW)?, &agent()))?;
        assert_eq!(what, expected, "{name}");
    }
    Ok(())
}

/// The bounds are inclusive at exactly the reviewed age and a locked quote is sane, so each strict
/// comparison is pinned from the side that passes.
#[test]
fn a_quote_and_record_exactly_at_the_bound_and_a_locked_quote_pass() -> Result<(), String> {
    let loaded = artifacts()?;
    let mut snapshot = facts_at("2026-09-28T16:59:50Z", "2026-09-28T16:59:50Z")?;
    snapshot.broker.quote.bid = snapshot.broker.quote.ask;
    snapshot.broker.account.cash = usd("1")?;
    load_contexts(&loaded, &snapshot, at(NOW)?, &agent()).map_err(text)?;
    let fresh = facts_at(NOW, NOW)?;
    load_contexts(&loaded, &fresh, at(NOW)?, &agent()).map_err(text)?;
    Ok(())
}

#[test]
fn the_clock_must_be_inside_the_regular_session_and_before_its_close_window() -> Result<(), String>
{
    let loaded = artifacts()?;
    let cases = [
        (
            "pre-market",
            "2026-09-28T13:29:59Z",
            Some("a run clock inside the regular session"),
        ),
        ("the open", "2026-09-28T13:30:00Z", None),
        (
            "a Sunday",
            "2026-09-27T17:00:00Z",
            Some("a run clock inside the regular session"),
        ),
        (
            "just before the close window",
            "2026-09-28T19:49:59.999999999Z",
            None,
        ),
        (
            "the close window",
            "2026-09-28T19:50:00Z",
            Some("a run clock before the close window"),
        ),
        (
            "after the close",
            "2026-09-28T20:00:00Z",
            Some("a run clock inside the regular session"),
        ),
        (
            "an ETP classification from after now",
            "2026-09-24T17:00:00Z",
            Some("an ETP classification dated before the run"),
        ),
        (
            "an ETP classification exactly at now",
            "2026-09-25T00:00:00Z",
            Some("a run clock inside the regular session"),
        ),
    ];
    for (name, now, expected) in cases {
        let snapshot = facts_at(now, now)?;
        let result = load_contexts(&loaded, &snapshot, at(now)?, &agent());
        match expected {
            Some(expected) => assert_eq!(refusal(result)?, expected, "{name}"),
            None => assert!(result.is_ok(), "{name}"),
        }
    }
    Ok(())
}

#[test]
fn protection_derives_from_the_entry_and_rounds_each_sell_price_up() -> Result<(), String> {
    let mandate = artifacts()?.mandate;
    let protection = mandate.protection.clone();
    let prices = |entry: &str| -> Result<Option<ProtectionPrices>, String> {
        protection_prices(&protection, price(entry)?).map_err(text)
    };
    assert_eq!(
        prices("255.2")?,
        Some(ProtectionPrices {
            stop: price("242.44")?,
            take_profit: Some(price("280.72")?),
        })
    );
    assert_eq!(
        prices("199.99")?,
        Some(ProtectionPrices {
            stop: price("190")?,
            take_profit: Some(price("219.99")?),
        }),
        "189.9905 rounds up to 190.00 and 219.989 up to 219.99"
    );

    let mut without_take_profit = protection.clone();
    without_take_profit.take_profit_distance = None;
    assert_eq!(
        protection_prices(&without_take_profit, price("255.2")?).map_err(text)?,
        Some(ProtectionPrices {
            stop: price("242.44")?,
            take_profit: None,
        })
    );
    let mut disabled = protection.clone();
    disabled.enabled = false;
    assert_eq!(
        protection_prices(&disabled, price("255.2")?).map_err(text)?,
        None
    );
    let mut without_stop = protection;
    without_stop.stop_distance = None;
    assert!(
        protection_prices(&without_stop, price("255.2")?).is_err(),
        "enabled protection without a stop distance refuses"
    );
    Ok(())
}

#[test]
fn the_gate_uses_the_reviewed_config_and_the_mandates_exact_limits() -> Result<(), String> {
    let config = reviewed_gate_config()?;
    assert_eq!(config.price_floor, usd("5")?);
    assert_eq!(config.liquidity_floor_usd, usd("1000000")?);
    assert_eq!(config.close_window_minutes, 10);
    assert_eq!(config.legacy_pdt_equity_threshold, usd("25000")?);
    let mandate = gate_mandate(&artifacts()?.mandate).map_err(text)?;
    assert_eq!(mandate.risk().max_order_usd, usd("300")?);
    assert_eq!(mandate.risk().max_position_usd, usd("1000")?);
    assert_eq!(mandate.risk().drawdown_ladder.len(), 3);
    Ok(())
}

#[test]
fn trusted_context_exposes_only_the_reviewed_agent_and_instrument() -> Result<(), String> {
    let loaded = artifacts()?;
    let template =
        gate_template(&loaded, &facts()?, at(NOW)?, reviewed_gate_config()?).map_err(text)?;
    let trusted = TrustedPaperContext {
        template,
        agent: AGENT.to_owned(),
        increment: mandate_num::ShareIncrement::Whole,
        fees: loaded.fees.clone(),
        quote_at: facts()?.broker.quote.at,
        quote_max_age: loaded.quote_max_age,
        clock: Rc::new(TestClock(Rc::new(Cell::new(at(NOW)?)))),
    };
    let reviewed = ExecutorAgentId(AGENT.to_owned());
    let other_agent = ExecutorAgentId("other".to_owned());
    let instrument = symbol(INSTRUMENT_ID)?;
    let other_instrument = symbol(OTHER_ASSET)?;

    assert_eq!(
        trusted.version(&reviewed).map(|version| version.0),
        loaded.config_refs.mandate_version.clone()
    );
    assert!(trusted.version(&other_agent).is_none());
    assert!(trusted.covers(&reviewed, &instrument));
    assert!(!trusted.covers(&other_agent, &instrument));
    assert!(!trusted.covers(&reviewed, &other_instrument));
    assert_eq!(trusted.asset_class(&instrument), Some(AssetClass::UsEquity));
    assert!(trusted.asset_class(&other_instrument).is_none());
    assert_eq!(
        trusted.increment(&instrument),
        Some(mandate_num::ShareIncrement::Whole)
    );
    assert!(trusted.increment(&other_instrument).is_none());

    let limit = price("255.2")?;
    let one = qty("1")?;
    let request = |agent, instrument, side, qty| BindingGateRequest {
        agent,
        instrument,
        side,
        qty,
        limit,
        purpose: Purpose::Open,
        tif: mandate_executor::TimeInForce::Day,
        protection: None,
    };
    assert!(
        trusted
            .input(&request(&reviewed, &instrument, Side::Buy, one))
            .is_some()
    );
    assert!(
        trusted
            .input(&request(&other_agent, &instrument, Side::Buy, one))
            .is_none()
    );
    assert!(
        trusted
            .input(&request(&reviewed, &other_instrument, Side::Buy, one))
            .is_none()
    );
    Ok(())
}

#[test]
fn binding_gate_rechecks_quote_age_and_close_window_at_its_current_clock() -> Result<(), String> {
    let loaded = artifacts()?;
    let template =
        gate_template(&loaded, &facts()?, at(NOW)?, reviewed_gate_config()?).map_err(text)?;
    let current = Rc::new(Cell::new(at(NOW)?));
    let trusted = TrustedPaperContext {
        template: template.clone(),
        agent: AGENT.to_owned(),
        increment: mandate_num::ShareIncrement::Whole,
        fees: loaded.fees.clone(),
        quote_at: at("2026-09-28T16:59:59.5Z")?,
        quote_max_age: loaded.quote_max_age,
        clock: Rc::new(TestClock(Rc::clone(&current))),
    };
    let reviewed = ExecutorAgentId(AGENT.to_owned());
    let instrument = symbol(INSTRUMENT_ID)?;
    let request = BindingGateRequest {
        agent: &reviewed,
        instrument: &instrument,
        side: Side::Buy,
        qty: qty("1")?,
        limit: price("255.2")?,
        purpose: Purpose::Open,
        tif: mandate_executor::TimeInForce::Day,
        protection: None,
    };

    current.set(at("2026-09-28T17:00:09.5Z")?);
    assert_eq!(
        trusted.input(&request).map(|input| input.now),
        Some(at("2026-09-28T17:00:09.5Z")?),
        "the exact quote-age boundary reaches the gate with the refreshed clock"
    );
    current.set(at("2026-09-28T17:00:09.500000001Z")?);
    assert!(
        trusted.input(&request).is_none(),
        "one nanosecond beyond the quote bound refuses before submission"
    );

    let close_clock = Rc::new(Cell::new(at("2026-09-28T19:50:00Z")?));
    let close_window = TrustedPaperContext {
        quote_at: at("2026-09-28T19:49:59.5Z")?,
        clock: Rc::new(TestClock(close_clock)),
        ..trusted
    };
    assert!(
        close_window.input(&request).is_none(),
        "the first instant of the close window refuses before submission"
    );
    Ok(())
}

#[test]
fn each_binding_request_reserves_its_own_orders_fees() -> Result<(), String> {
    let loaded = artifacts()?;
    let template =
        gate_template(&loaded, &facts()?, at(NOW)?, reviewed_gate_config()?).map_err(text)?;
    let trusted = TrustedPaperContext {
        template: template.clone(),
        agent: AGENT.to_owned(),
        increment: mandate_num::ShareIncrement::Whole,
        fees: loaded.fees.clone(),
        quote_at: facts()?.broker.quote.at,
        quote_max_age: loaded.quote_max_age,
        clock: Rc::new(TestClock(Rc::new(Cell::new(at(NOW)?)))),
    };
    let reviewed = ExecutorAgentId(AGENT.to_owned());
    let instrument = symbol(INSTRUMENT_ID)?;
    let request = |side, qty, limit| BindingGateRequest {
        agent: &reviewed,
        instrument: &instrument,
        side,
        qty,
        limit,
        purpose: Purpose::Open,
        tif: mandate_executor::TimeInForce::Day,
        protection: None,
    };
    let reserved = |side, qty: &str, limit: &str| -> Result<Option<Usd>, String> {
        Ok(trusted
            .input(&request(
                side,
                Qty::parse(qty).map_err(text)?,
                price(limit)?,
            ))
            .map(|input| input.fee_reservation))
    };

    for (shares, limit, cat) in [
        ("1", "255.2", "0.01"),
        ("7", "255.2", "0.07"),
        ("3", "1.01", "0.03"),
        ("250", "255.2", "2.5"),
    ] {
        assert_eq!(
            reserved(Side::Buy, shares, limit)?,
            Some(usd(cat)?),
            "{shares} shares bought owe {cat} of CAT at 0.01 a share, whatever the price"
        );
    }
    assert_eq!(
        reserved(Side::Sell, "1", "255.2")?,
        Some(Usd::ZERO),
        "only openings reach check 7, the reservation's one reader"
    );
    assert_eq!(
        reserved(Side::Buy, "0", "255.2")?,
        None,
        "an order the fee schedule cannot price gets no gate input"
    );
    let input = trusted
        .input(&request(Side::Buy, qty("2")?, price("255.2")?))
        .ok_or("the reviewed request has an input")?;
    assert_eq!(input.fee_reservation, usd("0.02")?);
    assert_eq!(
        format!(
            "{:?}",
            BindingGateInput {
                fee_reservation: template.fee_reservation,
                ..input
            }
        ),
        format!("{template:?}"),
        "the reservation is the only field a request changes"
    );
    Ok(())
}
