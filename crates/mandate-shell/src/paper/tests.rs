use std::cell::Cell;
use std::rc::Rc;

use mandate_accounting::{AccountType, AssetClass, InstrumentId, Side};
use mandate_alpaca::{
    AccountRules, Asset, AssetSnapshot, DeclaredRegime, Exchange as BrokerExchange, Feed,
    LatestQuote, MinuteBar, MinuteBars, alpaca_account_rules,
};
use mandate_builder::buy_action;
use mandate_canon::{Digest, Key, Value};
use mandate_domain::{AssetId, Environment};
use mandate_executor::{
    AgentId as ExecutorAgentId, BindingGateInput, BindingGateRequest, BindingGateSource,
    BrokerAccount, BrokerOrder, BrokerPosition, FillConfigRefs, InstrumentSnapshot, MandateView,
    Purpose,
};
use mandate_num::{Fraction, Price, Qty, SignedQty, Unit, Usd};
use mandate_risk::{DayTradeRegime, Exchange as GateExchange, GateConfig, SaneQuote};
use mandate_robinhood::{
    AccountSnapshot as RobinhoodSnapshot, PreflightFacts, Quote as RobinhoodQuote,
};
use mandate_runtime::{AgentId, ProtectionPrices};
use mandate_time::{Date, UtcNanos};

use crate::Cause;
use crate::control::{
    Configuration, ConfirmedVersion, ControlRecord, LiveAdmission, Origin, Pinned, RunFacts,
    configuration, confirmed_version,
};
use mandate_canon::to_canonical;
use mandate_spec::context::Membership;
use mandate_spec::validate::{Violation, validate};
use proptest::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

use super::artifacts::reference;
use super::context::{
    GateQuote, PaperClock, ReconcilingPaperContext, TrustedPaperContext, protection_prices,
    run_stop_limit_classes, stop_limit_offset,
};
use super::gate::{gate_mandate, gate_template};
use super::{
    Artifacts, BrokerFacts, LiquidityFacts, MarketFacts, PaperFacts, load_contexts, robinhood_facts,
};

const NOW: &str = "2026-09-28T17:00:00Z";
const AGENT: &str = "agent_spy";
const INSTRUMENT_ID: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
const OTHER_ASSET: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e416";
/// The E7-7 paper mandate fixture, whose pinned model hash `aapl_mandate` re-pins.
const MANDATE: &str = include_str!("../../tests/fixtures/tracer/mandate.json");
/// The registered objects beside a E7-7-shaped deployment: the reviewed fee schedule, the
/// calendar, and the rule set, whose bytes are shared with `tests/common/mod.rs`'s streams.
const FEE: &str = include_str!("../../tests/fixtures/tracer/config/fee-config.json");
const CALENDAR: &str = include_str!("../../tests/fixtures/tracer/config/trading-calendar.json");
const RULES: &str = include_str!("../../tests/fixtures/tracer/config/rule-set.json");
const ENVELOPE: &str = "autonomy behavior capital connection_id environment goal name notifications protection risk universe";
/// The E7-7 model artifact's bytes, whose digest is the hash the mandate fixture pins and
/// `aapl_mandate` replaces with the host's (DEC-475 item 6).
const E7_7_MODEL: &[u8] = b"{\"id\":\"quant.ma_crossover\",\"version\":\"1.0.0\"}\n";

fn text<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
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

/// The content object `mandate model register` stores for the pinned model (DEC-504 item 1), the
/// host's own canonical bytes for `quant.ma_crossover` 1.0.0.
fn host_content() -> Result<String, String> {
    let content = mandate_modelhost::content("quant.ma_crossover", "1.0.0").map_err(text)?;
    String::from_utf8(content.canonical).map_err(|_| "the content object is not UTF-8".to_owned())
}

/// The registered model of one deployment: its triple, and the content bytes whose hash the
/// mandate pins.
struct RegisteredModel<'a> {
    id: &'a str,
    version: &'a str,
    content: &'a str,
}

/// The host's own pinned model.
fn host_model<'a>(content: &'a str) -> RegisteredModel<'a> {
    RegisteredModel {
        id: "quant.ma_crossover",
        version: "1.0.0",
        content,
    }
}

/// The control stream of one deployment and the store of every object it names, each stored as
/// written so its reference is the hash of those bytes, as `tests/common/mod.rs`'s `Stream` holds
/// SPY's (DEC-505).
#[derive(Default)]
struct Deployed {
    records: Vec<ControlRecord>,
    store: BTreeMap<Digest, Vec<u8>>,
}

impl Deployed {
    /// Stores `object`'s bytes as written and returns their reference.
    fn put(&mut self, object: &str) -> String {
        self.store
            .insert(Digest::of(object.as_bytes()), object.as_bytes().to_vec());
        reference(object.as_bytes())
    }

    /// Appends one fixture record at the next `seq`. The payload is one this file built, so a
    /// refusal names the fixture, never a product object.
    fn record(&mut self, event_type: &str, payload: &str) -> Result<(), Cause> {
        let parsed = mandate_canon::parse(payload.as_bytes()).map_err(|_| Cause::Absent {
            what: "a fixture record's payload",
        })?;
        let seq = u64::try_from(self.records.len()).map_err(|_| Cause::Absent {
            what: "a fixture record's seq",
        })?;
        self.records.push(ControlRecord {
            seq: seq.saturating_add(1),
            event_time: None,
            event_type: event_type.to_owned(),
            payload: parsed,
            origin: Origin {
                stream_id: "ctl:ws_spy".to_owned(),
                environment: Environment::Paper,
            },
        });
        Ok(())
    }
}

/// One fixture object parsed, to be canonicalized or judged.
fn parsed(text: &str) -> Result<mandate_canon::Value, Cause> {
    mandate_canon::parse(text.as_bytes()).map_err(|_| Cause::Absent {
        what: "a fixture object",
    })
}

/// The E7-7 paper mandate as a registered deployment pins it: AAPL on nasdaq with the model
/// host's own content pinned under its hash, re-pinnable by each case below (DEC-504, DEC-505).
fn aapl_mandate() -> Result<String, String> {
    Ok(MANDATE.replace(
        &Digest::of(E7_7_MODEL).to_hex(),
        &Digest::of(host_content()?.as_bytes()).to_hex(),
    ))
}

/// SPY's registered snapshot (DEC-523's own shape), the base [`aapl_snapshot`] mutates.
const SPY_SNAPSHOT: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-09-21T00:00:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b28f4066-5c6d-479b-a2af-85dc1a8f16fb","symbol":"SPY"}"#;

/// The registered AAPL snapshot, in DEC-523's own shape, dated as the E7-7 file was.
fn aapl_snapshot() -> String {
    SPY_SNAPSHOT
        .replace("b28f4066-5c6d-479b-a2af-85dc1a8f16fb", INSTRUMENT_ID)
        .replace("\"SPY\"", "\"AAPL\"")
        .replace("\"arca\"", "\"nasdaq\"")
        .replace("2026-09-21", "2026-09-25")
}

/// One deployment's control stream and object store, as `tests/common/mod.rs`'s `Stream::of`
/// writes SPY's, generalized to the mandate, the agent, the snapshot and the model registration
/// the case deploys (DEC-505).
fn deployed_stream(
    mandate: &str,
    snapshot: &str,
    rules: &str,
    model: &RegisteredModel<'_>,
) -> Result<Deployed, Cause> {
    let mut stream = Deployed::default();
    let document = to_canonical(&parsed(mandate)?);
    let version = reference(&document);
    let paths: Vec<String> = ENVELOPE.split(' ').map(|path| format!("/{path}")).collect();
    let provenance: Vec<String> = paths
        .iter()
        .map(|path| format!(r#"{{"path":"{path}","source":"user_entered"}}"#))
        .collect();
    let confirmed: Vec<String> = paths.iter().map(|path| format!(r#""{path}""#)).collect();
    let content_hash = stream.put(model.content);
    stream.record(
        "ConfigSnapshotRegistered",
        &format!(
            r#"{{"admits_instruments":false,"content_hash":"{content_hash}","kind":"model_version","model_id":"{}","model_version":"{}","params":["fast_periods","slow_periods"]}}"#,
            model.id,
            model.version
        ),
    )?;
    for (kind, object) in [
        ("fee_config", FEE),
        ("trading_calendar", CALENDAR),
        ("rule_set", rules),
        ("instrument_snapshot", snapshot),
    ] {
        let hash = stream.put(object);
        stream.record(
            "ConfigSnapshotRegistered",
            &format!(
                r#"{{"admits_instruments":null,"content_hash":"{hash}","kind":"{kind}","model_id":null,"model_version":null,"params":[]}}"#
            ),
        )?;
    }
    stream.record(
        "MandateVersionCreated",
        &format!(
            r#"{{"mandate_version":"{version}","provenance":[{}],"record_ref":"sha256:{}"}}"#,
            provenance.join(","),
            "7".repeat(64)
        ),
    )?;
    stream.record(
        "MandateConfirmed",
        &format!(
            r#"{{"confirmed_paths":[{}],"mandate_version":"{version}"}}"#,
            confirmed.join(",")
        ),
    )?;
    stream.record(
        "AgentDeployed",
        &format!(
            r#"{{"agent_id":"{AGENT}","mandate_version":"{version}","record_ref":"sha256:{}"}}"#,
            "6".repeat(64)
        ),
    )?;
    stream.store.insert(Digest::of(&document), document);
    Ok(stream)
}

/// The confirmed version and the effective configuration the stream deploys, pinned as its own
/// mandate's instrument and model are, for a run on [`NOW`]'s day.
fn deployed_inputs(stream: &Deployed) -> Result<(ConfirmedVersion, Configuration), String> {
    let day = at(NOW)?.date();
    let facts = RunFacts {
        agent: mandate_spec::context::AgentId::new(AGENT),
        validation_date: day,
        membership: Membership {
            workspace_users: 1,
            approver_users: 1,
        },
        live: LiveAdmission::refusing(),
    };
    let confirmed = confirmed_version(&stream.records, &stream.store, &facts).map_err(text)?;
    let mandate = confirmed.mandate();
    let ([instrument], [model]) = (
        mandate.universe.pinned_instruments.as_slice(),
        mandate.behavior.signal_models.as_slice(),
    ) else {
        return Err("the deployment does not pin one instrument and one model".to_owned());
    };
    let pinned = Pinned {
        asset_id: instrument.asset_id.as_str().to_owned(),
        symbol: instrument.symbol.clone(),
        model_id: model.id.as_str().to_owned(),
        model_version: model.version.clone(),
        content_hash: model.content_hash,
    };
    let config = configuration(&stream.records, &stream.store, &pinned, day).map_err(text)?;
    Ok((confirmed, config))
}

/// The run's artifacts for the deployment `stream` writes.
fn artifacts_from(stream: &Deployed) -> Result<Artifacts, String> {
    let (confirmed, config) = deployed_inputs(stream)?;
    Artifacts::from_registered(&confirmed, &config).map_err(text)
}

fn artifacts() -> Result<Artifacts, String> {
    let content = host_content()?;
    let stream = deployed_stream(
        &aapl_mandate()?,
        &aapl_snapshot(),
        RULES,
        &host_model(&content),
    )
    .map_err(text)?;
    artifacts_from(&stream)
}

fn reviewed_gate_config() -> Result<GateConfig, String> {
    artifacts()?
        .production_configuration()
        .map(|configuration| configuration.gate.clone())
        .map_err(text)
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
                clock_skew: None,
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
fn the_registered_artifacts_bind_the_bytes_they_checked() -> Result<(), String> {
    let loaded = artifacts()?;
    let refs = loaded.config_refs();
    assert_eq!(refs.fee_config, Some(reference(FEE.as_bytes())));
    assert_eq!(refs.trading_calendar, Some(reference(CALENDAR.as_bytes())));
    assert_eq!(
        refs.instrument_snapshot,
        Some(reference(aapl_snapshot().as_bytes()))
    );
    assert_eq!(refs.rule_set, Some(reference(RULES.as_bytes())));
    let canonical = loaded.mandate.canonical_bytes().map_err(text)?;
    let document = to_canonical(&parsed(&aapl_mandate()?).map_err(text)?);
    assert_eq!(canonical, document);
    assert_eq!(refs.mandate_version, Some(reference(&canonical)));
    assert_eq!(
        loaded.quote_max_age,
        std::time::Duration::from_secs(10),
        "the registered rule set's quote bound"
    );
    assert_eq!(
        loaded.quote_max_age(),
        std::time::Duration::from_secs(10),
        "the runner's cross-check reads the same bound (DEC-945 item 2)"
    );
    assert_eq!(
        loaded.instrument.etp_classified_at,
        at("2026-09-25T00:00:00Z")?
    );
    Ok(())
}

/// The registered deployment of another instrument, model and connection than any shell
/// literal: MSFT on arca, a model registered under `quant.other_model` 2.0.0 whose content is
/// neither the host's nor the E7-7 file's, and the founder's own connection id (DEC-475 item 3;
/// DEC-523's exchange value set is `arca` and `nasdaq`, D3c).
fn msft_deployment() -> Result<Artifacts, String> {
    let content =
        br#"{"content_hash":"set by the registration","kind":"quant_model_content","model_id":"quant.other_model","model_version":"2.0.0"}"#;
    let model = RegisteredModel {
        id: "quant.other_model",
        version: "2.0.0",
        content: std::str::from_utf8(content).map_err(|error| error.to_string())?,
    };
    let mandate = aapl_mandate()?
        .replace(INSTRUMENT_ID, OTHER_ASSET)
        .replace("\"AAPL\"", "\"MSFT\"")
        .replace("quant.ma_crossover", "quant.other_model")
        .replace("\"version\": \"1.0.0\"", "\"version\": \"2.0.0\"")
        .replace("conn_alpaca_paper_01", "conn_owner_paper_42")
        .replace(
            &Digest::of(host_content()?.as_bytes()).to_hex(),
            &Digest::of(content).to_hex(),
        );
    let snapshot = aapl_snapshot()
        .replace(INSTRUMENT_ID, OTHER_ASSET)
        .replace("\"AAPL\"", "\"MSFT\"")
        .replace("\"nasdaq\"", "\"arca\"");
    let stream = deployed_stream(&mandate, &snapshot, RULES, &model).map_err(text)?;
    artifacts_from(&stream)
}

#[test]
fn production_artifacts_derive_the_instrument_and_model_without_shell_literals()
-> Result<(), String> {
    let loaded = msft_deployment()?;
    let identity = loaded.production_identity();
    assert_eq!(identity.asset_id.as_str(), OTHER_ASSET);
    assert_eq!(identity.symbol.as_str(), "MSFT");
    assert_eq!(identity.asset_class, mandate_domain::AssetClass::UsEquity);
    assert_eq!(identity.broker_exchange, BrokerExchange::Arca);
    assert_eq!(identity.gate_exchange, GateExchange::Arca);
    assert_eq!(identity.model_id, "quant.other_model");
    assert_eq!(identity.model_version, "2.0.0");
    let content =
        br#"{"content_hash":"set by the registration","kind":"quant_model_content","model_id":"quant.other_model","model_version":"2.0.0"}"#;
    assert_eq!(identity.model_hash, Digest::of(content));
    Ok(())
}

#[test]
fn production_contexts_use_the_reviewed_instrument_and_model_end_to_end() -> Result<(), String> {
    let loaded = msft_deployment()?;
    let mut snapshot = facts()?;
    snapshot.broker.asset.asset.asset_id = OTHER_ASSET.to_owned();
    snapshot.broker.asset.asset.instrument = symbol("MSFT")?;
    snapshot.broker.asset.asset.exchange = BrokerExchange::Arca;
    snapshot.broker.quote.instrument = symbol("MSFT")?;
    snapshot.broker.minute_bars.instrument = symbol("MSFT")?;

    let contexts =
        load_contexts(&loaded, &snapshot, at(NOW)?, &agent(), Environment::Paper).map_err(text)?;
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
    assert_eq!(gate.instrument.exchange, Some(GateExchange::Arca));

    let template = gate_template(
        &loaded,
        &snapshot,
        at(NOW)?,
        loaded
            .production_configuration()
            .map_err(text)?
            .gate
            .clone(),
    )
    .map_err(text)?;
    let trusted = TrustedPaperContext {
        template,
        agent: AGENT.to_owned(),
        increment: loaded.instrument.increment,
        fees: loaded.fees.clone(),
        quote: GateQuote::of(&snapshot.broker.quote),
        quote_max_age: loaded.quote_max_age,
        clock: Rc::new(TestClock(Rc::new(Cell::new(at(NOW)?)))),
        stop_limit_offset: None,
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
    let refs = loaded.config_refs();
    let registered = (
        refs.fee_config.clone(),
        refs.trading_calendar.clone(),
        refs.instrument_snapshot.clone(),
    );
    let (Some(fee), Some(calendar), Some(snapshot)) = registered else {
        return Err("the registered references".to_owned());
    };
    assert_eq!(
        trusted.fill_refs(&reviewed),
        Some(FillConfigRefs {
            fee_config: fee,
            settlement_calendar: calendar.clone(),
            trading_calendar: calendar,
            instrument_snapshot: snapshot,
        }),
        "the reviewed instrument's fill names the registered objects, settling on its trading \
         calendar (DEC-938 item 2)"
    );
    assert_eq!(
        trusted.fill_refs(&symbol("SPY")?),
        None,
        "another instrument's fill names none"
    );
    Ok(())
}

/// DEC-955 item 5: the own-fill reconciliation's context answers the reviewed agent on the
/// reviewed instrument as the trusted context does, names the registered fill configuration (or
/// none when a reference is missing, which the journal refuses), and answers no binding request,
/// so nothing it carries can let an order through.
#[test]
fn the_reconciling_context_names_the_fill_configuration_and_answers_no_gate_request()
-> Result<(), String> {
    let loaded = msft_deployment()?;
    let refs = loaded.config_refs().clone();
    let context = ReconcilingPaperContext {
        agent: AGENT.to_owned(),
        asset: OTHER_ASSET.to_owned(),
        increment: loaded.instrument.increment,
        config_refs: refs.clone(),
    };
    let ours = ExecutorAgentId(AGENT.to_owned());
    let theirs = ExecutorAgentId("agent_other".to_owned());
    let reviewed = symbol(OTHER_ASSET)?;
    let other = symbol("SPY")?;
    assert!(context.covers(&ours, &reviewed));
    assert!(!context.covers(&theirs, &reviewed));
    assert!(!context.covers(&ours, &other));
    let version = refs
        .mandate_version
        .clone()
        .ok_or("the registered mandate version")?;
    assert_eq!(
        context.version(&ours),
        Some(mandate_executor::MandateVersion(version))
    );
    assert_eq!(context.version(&theirs), None);
    assert_eq!(context.crypto_stop_limit_offset(&ours), None);
    assert_eq!(context.asset_class(&reviewed), Some(AssetClass::UsEquity));
    assert_eq!(context.asset_class(&other), None);
    assert_eq!(
        context.increment(&reviewed),
        Some(mandate_num::ShareIncrement::Whole)
    );
    assert_eq!(context.increment(&other), None);
    assert_eq!(context.exit_tier(&reviewed), None);
    let (Some(fee), Some(calendar), Some(snapshot)) = (
        refs.fee_config.clone(),
        refs.trading_calendar.clone(),
        refs.instrument_snapshot.clone(),
    ) else {
        return Err("the registered references".to_owned());
    };
    assert_eq!(
        context.fill_refs(&reviewed),
        Some(FillConfigRefs {
            fee_config: fee,
            settlement_calendar: calendar.clone(),
            trading_calendar: calendar,
            instrument_snapshot: snapshot,
        })
    );
    assert_eq!(context.fill_refs(&other), None);
    let missing: [fn(&mut mandate_executor::BindingGateConfigRefs); 3] = [
        |refs| refs.fee_config = None,
        |refs| refs.trading_calendar = None,
        |refs| refs.instrument_snapshot = None,
    ];
    for drop in missing {
        let mut partial = refs.clone();
        drop(&mut partial);
        let short = ReconcilingPaperContext {
            config_refs: partial,
            agent: AGENT.to_owned(),
            asset: OTHER_ASSET.to_owned(),
            increment: loaded.instrument.increment,
        };
        assert_eq!(
            short.fill_refs(&reviewed),
            None,
            "a missing reference names none"
        );
    }
    for side in [Side::Buy, Side::Sell] {
        let request = BindingGateRequest {
            agent: &ours,
            instrument: &reviewed,
            side,
            qty: qty("1")?,
            limit: price("255.2")?,
            purpose: Purpose::Open,
            tif: mandate_executor::TimeInForce::Day,
            protection: None,
        };
        assert!(context.input(&request).is_none(), "no request is answered");
    }
    Ok(())
}

/// The registered rule set with every member changed, as the E7-7 file's was: each edit must
/// reach the gate and executor configurations the run assembles, and the reference the executor
/// journals must be the mutated bytes' own hash.
fn mutated_rules() -> String {
    let mut rules = RULES.to_owned();
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
        let changed = rules.replace(old, new);
        assert_ne!(changed, rules, "the rule-set edit did not apply: {old}");
        rules = changed;
    }
    rules
}

#[test]
fn production_configuration_comes_from_the_content_addressed_rule_set() -> Result<(), String> {
    let rules = mutated_rules();
    let content = host_content()?;
    let stream = deployed_stream(
        &aapl_mandate()?,
        &aapl_snapshot(),
        &rules,
        &host_model(&content),
    )
    .map_err(text)?;
    let loaded = artifacts_from(&stream)?;
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
    assert_eq!(
        loaded.config_refs.rule_set.as_deref(),
        Some(reference(rules.as_bytes()).as_str()),
        "the journaled reference is the mutated bytes' own hash"
    );
    Ok(())
}

#[test]
fn production_contexts_use_only_the_reviewed_gate_and_executor_configuration() -> Result<(), String>
{
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
    Ok(())
}

#[test]
fn production_deployment_takes_opaque_ids_and_binds_the_confirmed_connection() -> Result<(), String>
{
    let content = host_content()?;
    let stream = deployed_stream(
        &aapl_mandate()?.replace("conn_alpaca_paper_01", "conn_owner_paper_42"),
        &aapl_snapshot(),
        RULES,
        &host_model(&content),
    )
    .map_err(text)?;
    let loaded = artifacts_from(&stream)?;
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
    Ok(())
}

#[test]
fn the_reviewed_snapshot_assembles_both_contexts_from_its_own_facts() -> Result<(), String> {
    let loaded = artifacts()?;
    let contexts =
        load_contexts(&loaded, &facts()?, at(NOW)?, &agent(), Environment::Paper).map_err(text)?;
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
        let what = refusal(load_contexts(
            &loaded,
            &snapshot,
            at(NOW)?,
            &agent(),
            Environment::Paper,
        ))?;
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
    load_contexts(&loaded, &snapshot, at(NOW)?, &agent(), Environment::Paper).map_err(text)?;
    let fresh = facts_at(NOW, NOW)?;
    load_contexts(&loaded, &fresh, at(NOW)?, &agent(), Environment::Paper).map_err(text)?;
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
        let result = load_contexts(&loaded, &snapshot, at(now)?, &agent(), Environment::Paper);
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

/// DEC-966 item 5: the stop-limit's offset is the mandate's when protection is on, and none when
/// it is off or unset; the executor's view hands it to the reviewed agent alone.
#[test]
fn the_stop_limit_offset_is_the_mandates_and_only_the_reviewed_agents() -> Result<(), String> {
    let loaded = artifacts()?;
    let mut protection = loaded.mandate.protection.clone();
    protection.enabled = true;
    protection.crypto_stop_limit_offset = Some(
        mandate_spec::SchemaDec::parse("0.005", mandate_spec::DecGrammar::OpenFraction)
            .map_err(|e| format!("{e:?}"))?,
    );
    let offset = Fraction::parse("0.005").map_err(text)?;
    assert_eq!(stop_limit_offset(&protection).map_err(text)?, Some(offset));
    let mut disabled = protection.clone();
    disabled.enabled = false;
    assert_eq!(stop_limit_offset(&disabled).map_err(text)?, None);
    let mut unset = protection;
    unset.crypto_stop_limit_offset = None;
    assert_eq!(stop_limit_offset(&unset).map_err(text)?, None);

    let snapshot = facts_at(NOW, NOW)?;
    let template = gate_template(
        &loaded,
        &snapshot,
        at(NOW)?,
        loaded
            .production_configuration()
            .map_err(text)?
            .gate
            .clone(),
    )
    .map_err(text)?;
    let trusted = TrustedPaperContext {
        template,
        agent: AGENT.to_owned(),
        increment: loaded.instrument.increment,
        fees: loaded.fees.clone(),
        quote: GateQuote::of(&snapshot.broker.quote),
        quote_max_age: loaded.quote_max_age,
        clock: Rc::new(TestClock(Rc::new(Cell::new(at(NOW)?)))),
        stop_limit_offset: Some(offset),
    };
    assert_eq!(
        trusted.crypto_stop_limit_offset(&ExecutorAgentId(AGENT.to_owned())),
        Some(offset)
    );
    assert_eq!(
        trusted.crypto_stop_limit_offset(&ExecutorAgentId("another-agent".to_owned())),
        None
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
        quote: GateQuote::of(&facts()?.broker.quote),
        quote_max_age: loaded.quote_max_age,
        clock: Rc::new(TestClock(Rc::new(Cell::new(at(NOW)?)))),
        stop_limit_offset: None,
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
        quote: GateQuote::of(
            &facts_at("2026-09-28T16:59:59Z", "2026-09-28T16:59:59.5Z")?
                .broker
                .quote,
        ),
        quote_max_age: loaded.quote_max_age,
        clock: Rc::new(TestClock(Rc::clone(&current))),
        stop_limit_offset: None,
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
        quote: GateQuote::of(
            &facts_at("2026-09-28T19:49:59Z", "2026-09-28T19:49:59.5Z")?
                .broker
                .quote,
        ),
        clock: Rc::new(TestClock(close_clock)),
        stop_limit_offset: None,
        ..trusted
    };
    assert!(
        close_window.input(&request).is_none(),
        "the first instant of the close window refuses before submission"
    );
    Ok(())
}

/// DEC-943 item 2 (DEC-940 item 6, gap 3): the binding gate judges the IEX quote the run last
/// handed it, at its own clock and under the unchanged ten-second bound. Thirty seconds after
/// the preflight its quote is stale and refuses; a fresh read handed to the gate reaches it,
/// bid, ask and stamp, as the market quote the gate judges; a handed read ten seconds and a
/// nanosecond old refuses again; and a quote stamped after the gate's clock refuses, as the
/// gate's recheck always did (DEC-936 item 4).
#[test]
fn the_binding_gate_judges_the_quote_the_run_last_handed_it() -> Result<(), String> {
    let loaded = artifacts()?;
    let preflight = facts()?;
    let template =
        gate_template(&loaded, &preflight, at(NOW)?, reviewed_gate_config()?).map_err(text)?;
    let quote = GateQuote::of(&preflight.broker.quote);
    let current = Rc::new(Cell::new(at("2026-09-28T17:00:30Z")?));
    let trusted = TrustedPaperContext {
        template,
        agent: AGENT.to_owned(),
        increment: mandate_num::ShareIncrement::Whole,
        fees: loaded.fees.clone(),
        quote: quote.clone(),
        quote_max_age: loaded.quote_max_age,
        clock: Rc::new(TestClock(Rc::clone(&current))),
        stop_limit_offset: None,
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
    assert!(
        trusted.input(&request).is_none(),
        "thirty seconds on, the preflight's quote is past its bound"
    );
    let read = |stamp: &str, bid: &str, ask: &str| -> Result<LatestQuote, String> {
        Ok(LatestQuote {
            at: at(stamp)?,
            bid: price(bid)?,
            ask: price(ask)?,
            ..preflight.broker.quote.clone()
        })
    };
    quote.hand(&read("2026-09-28T17:00:29.5Z", "254.9", "255.3")?);
    assert_eq!(
        trusted.input(&request).and_then(|input| input.market.quote),
        Some(SaneQuote {
            bid: price("254.9")?,
            ask: price("255.3")?,
            at: at("2026-09-28T17:00:29.5Z")?,
        }),
        "the gate judges the read the run handed it"
    );
    quote.hand(&read("2026-09-28T17:00:20Z", "254.9", "255.3")?);
    assert!(
        trusted.input(&request).is_some(),
        "a handed read exactly at the bound is current"
    );
    quote.hand(&read("2026-09-28T17:00:19.999999999Z", "254.9", "255.3")?);
    assert!(
        trusted.input(&request).is_none(),
        "a handed read a nanosecond past the bound refuses: the bound is never relaxed"
    );
    quote.hand(&read("2026-09-28T17:00:30.000000001Z", "254.9", "255.3")?);
    assert!(
        trusted.input(&request).is_none(),
        "a handed read stamped after the gate's clock refuses"
    );
    Ok(())
}

/// A scripted IEX data feed: every quote read answers `body`, and the clock reads `now`.
#[derive(Clone)]
struct QuoteFeed {
    body: String,
    now: UtcNanos,
}

impl mandate_alpaca::DataTransport for QuoteFeed {
    async fn send(
        &self,
        _: &mandate_alpaca::QuoteRequest,
    ) -> Result<mandate_alpaca::Response, mandate_alpaca::TransportError> {
        Ok(mandate_alpaca::Response {
            status: 200,
            body: self.body.clone().into_bytes(),
        })
    }

    async fn send_bars(
        &self,
        _: &mandate_alpaca::BarsRequest,
    ) -> Result<mandate_alpaca::Response, mandate_alpaca::TransportError> {
        Err(mandate_alpaca::TransportError::Timeout)
    }
}

impl mandate_alpaca::Pause for QuoteFeed {
    fn now(&self) -> UtcNanos {
        self.now
    }

    async fn pause(&self, _: std::time::Duration) {}
}

/// DEC-943 item 2: a wake's fresh read is the preflight's own IEX read and the judge's checks
/// of it. A current quote of the reviewed symbol, a locked one among them, is the quote; a
/// crossed quote, another symbol's, and one past the ten-second bound or DEC-936's 250 ms skew
/// tolerance each refuse, naming the wake's quote.
#[test]
fn a_fresh_read_is_the_reviewed_symbols_current_uncrossed_iex_quote() -> Result<(), String> {
    let loaded = artifacts()?;
    let now = at("2026-09-28T17:00:30Z")?;
    let feed = |symbol: &str, bid: &str, ask: &str, stamp: &str| QuoteFeed {
        body: format!(
            r#"{{"quote":{{"ap":{ask},"as":1,"ax":"V","bp":{bid},"bs":2,"bx":"V","c":["R"],"t":"{stamp}","z":"C"}},"symbol":"{symbol}"}}"#
        ),
        now,
    };
    let read = |feed: QuoteFeed| super::fresh_quote(&loaded, feed.clone(), feed);
    let current = read(feed("AAPL", "255.1", "255.2", "2026-09-28T17:00:29.5Z")).map_err(text)?;
    assert_eq!(
        (
            current.instrument.as_str(),
            current.bid,
            current.ask,
            current.at,
            current.feed
        ),
        (
            "AAPL",
            price("255.1")?,
            price("255.2")?,
            at("2026-09-28T17:00:29.5Z")?,
            Feed::Iex
        )
    );
    assert!(
        read(feed("AAPL", "255.2", "255.2", "2026-09-28T17:00:20Z")).is_ok(),
        "a locked quote exactly at the bound is current"
    );
    let refused = Cause::Absent {
        what: "a current, uncrossed IEX quote at the wake",
    };
    for (name, quoted) in [
        (
            "crossed",
            feed("AAPL", "255.3", "255.2", "2026-09-28T17:00:29.5Z"),
        ),
        (
            "another symbol",
            feed("MSFT", "255.1", "255.2", "2026-09-28T17:00:29.5Z"),
        ),
        (
            "stale",
            feed("AAPL", "255.1", "255.2", "2026-09-28T17:00:19.999999999Z"),
        ),
        (
            "ahead",
            feed("AAPL", "255.1", "255.2", "2026-09-28T17:00:30.250000001Z"),
        ),
    ] {
        assert_eq!(
            read(quoted)
                .map(|quote| quote.bid)
                .map_err(|cause| cause.to_string()),
            Err(refused.to_string()),
            "{name}"
        );
    }
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
        quote: GateQuote::of(&facts()?.broker.quote),
        quote_max_age: loaded.quote_max_age,
        clock: Rc::new(TestClock(Rc::new(Cell::new(at(NOW)?)))),
        stop_limit_offset: None,
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

/// A Robinhood preflight of AAPL: cash 1,000 and buying power `buying_power`, and `positions`.
fn robinhood_preflight(
    buying_power: &str,
    positions: Vec<BrokerPosition>,
) -> Result<PreflightFacts, String> {
    Ok(PreflightFacts {
        account: RobinhoodSnapshot {
            cash: usd("1000")?,
            buying_power: usd(buying_power)?,
            positions,
            open_orders: Vec::new(),
        },
        quote: RobinhoodQuote {
            symbol: symbol("AAPL")?,
            bid: price("255.15")?,
            ask: price("255.25")?,
        },
    })
}

fn market() -> Result<MarketFacts, String> {
    let broker = facts()?.broker;
    Ok(MarketFacts {
        asset: broker.asset,
        quote: broker.quote,
        minute_bars: broker.minute_bars,
    })
}

/// DEC-933 items 1 and 2: the account is Robinhood's, read by the least-risk mapping (the lower of
/// cash and buying power as non-marginable buying power, the cash as equity, no prior-close
/// equity, no maintenance excess, no crypto, 1×), under Robinhood's own account rules, with its
/// positions and orders as read; the asset record, the IEX quote and the minute bars are Alpaca's.
#[test]
fn the_robinhood_facts_are_its_account_beside_alpacas_market_reads() -> Result<(), String> {
    let held = BrokerPosition {
        instrument: symbol("AAPL")?,
        qty: SignedQty::parse("0").map_err(text)?,
        avg_entry_price: price("250")?,
    };
    let robinhood = robinhood_preflight("900", vec![held.clone()])?;
    let facts = robinhood_facts(market()?, &robinhood).map_err(text)?;
    assert_eq!(
        facts.account,
        BrokerAccount {
            status: "ACTIVE".to_owned(),
            crypto_status: "INACTIVE".to_owned(),
            trading_blocked: false,
            account_blocked: false,
            trade_suspended_by_user: false,
            multiplier: 1,
            equity: usd("1000")?,
            cash: usd("1000")?,
            buying_power: usd("900")?,
            non_marginable_buying_power: usd("900")?,
            accrued_fees: Usd::ZERO,
            last_equity: Usd::ZERO,
            maintenance_margin: usd("1000")?,
        }
    );
    assert_eq!(
        facts.account_rules,
        AccountRules {
            account_type: AccountType::Cash,
            regime: DeclaredRegime::LegacyPdt,
        }
    );
    assert_eq!(facts.positions, vec![held]);
    assert!(facts.open_orders.is_empty());
    let alpaca = market()?;
    assert_eq!(facts.asset, alpaca.asset);
    assert_eq!(
        facts.quote, alpaca.quote,
        "the gate's quote stays the IEX one"
    );
    assert_eq!(facts.minute_bars, alpaca.minute_bars);

    let richer =
        robinhood_facts(market()?, &robinhood_preflight("1100", Vec::new())?).map_err(text)?;
    assert_eq!(
        (
            richer.account.buying_power,
            richer.account.non_marginable_buying_power
        ),
        (usd("1000")?, usd("1000")?),
        "a broker figure above the cash is clamped to the cash, in both buying power fields"
    );
    Ok(())
}

/// A flat Robinhood account passes the judge, and the gate reads it as a cash account under
/// `legacy_pdt` with no prior-close equity and 1× buying power (DEC-933 item 1).
#[test]
fn the_gate_reads_a_robinhood_account_as_cash_under_legacy_pdt() -> Result<(), String> {
    let loaded = artifacts()?;
    let robinhood =
        robinhood_facts(market()?, &robinhood_preflight("900", Vec::new())?).map_err(text)?;
    let paper = PaperFacts {
        broker: robinhood,
        liquidity: facts()?.liquidity,
    };
    load_contexts(&loaded, &paper, at(NOW)?, &agent(), Environment::Paper).map_err(text)?;
    let template =
        gate_template(&loaded, &paper, at(NOW)?, reviewed_gate_config()?).map_err(text)?;
    assert_eq!(template.account.account_type, AccountType::Cash);
    assert_eq!(template.account.regime, DayTradeRegime::LegacyPdt);
    assert_eq!(template.account.prior_close_equity, Usd::ZERO);
    assert_eq!(template.account.equity, usd("1000")?);
    assert_eq!(template.account.model_buying_power, usd("900")?);
    assert!(!template.account.crypto_active);
    Ok(())
}

/// A Robinhood quote of another instrument than the IEX quote is no quote of this run's.
#[test]
fn a_robinhood_quote_of_another_instrument_refuses_the_facts() -> Result<(), String> {
    let mut robinhood = robinhood_preflight("900", Vec::new())?;
    robinhood.quote.symbol = symbol("MSFT")?;
    let refused = robinhood_facts(market()?, &robinhood);
    assert!(
        matches!(refused, Err(Cause::Absent { what }) if what == "the instrument's Robinhood quote"),
        "{refused:?}"
    );
    Ok(())
}

/// The validation a cycle runs on the run context `connection` gives, for the reviewed mandate
/// set to `mandate`'s environment: whether V-001 fires.
fn v001_fires(
    loaded: &Artifacts,
    mandate: Environment,
    connection: Environment,
) -> Result<bool, String> {
    let contexts =
        load_contexts(loaded, &facts()?, at(NOW)?, &agent(), connection).map_err(text)?;
    let mut value = loaded.mandate.canonical().map_err(text)?;
    let Value::Object(members) = &mut value else {
        return Err("the reviewed mandate is an object".to_owned());
    };
    let named = match mandate {
        Environment::Paper => "paper",
        Environment::Live => "live",
    };
    members.insert(
        Key::new("environment").map_err(text)?,
        Value::Str(named.to_owned()),
    );
    let document = mandate_spec::Mandate::parse(&value).map_err(text)?;
    let report = validate(&document, &contexts.run.validation).map_err(text)?;
    Ok(report.violations.contains(&Violation::V001))
}

#[test]
fn the_run_context_names_the_chosen_connections_environment_never_a_constant() -> Result<(), String>
{
    let loaded = artifacts()?;
    for connection in [Environment::Paper, Environment::Live] {
        let contexts =
            load_contexts(&loaded, &facts()?, at(NOW)?, &agent(), connection).map_err(text)?;
        assert_eq!(
            contexts.run.validation.connection_environment,
            Some(connection),
            "V-001 compares the connection the run chose (DEC-965)"
        );
    }
    assert!(
        !v001_fires(&loaded, Environment::Live, Environment::Live)?,
        "a live mandate on the live connection the door chose passes V-001 (R13)"
    );
    assert!(
        !v001_fires(&loaded, Environment::Paper, Environment::Paper)?,
        "a paper mandate on a paper connection passes V-001 as before"
    );
    assert!(v001_fires(&loaded, Environment::Paper, Environment::Live)?);
    assert!(v001_fires(&loaded, Environment::Live, Environment::Paper)?);
    Ok(())
}

fn environment() -> impl Strategy<Value = Environment> {
    prop_oneof![Just(Environment::Paper), Just(Environment::Live)]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// V-001 on the run context, over any sequence of runs on one set of artifacts, each pairing
    /// a mandate environment with the connection the run chose: it fires exactly when the two
    /// differ, computed here from the pair alone, so no run's connection carries into the next.
    #[test]
    fn v001_on_the_run_context_fires_exactly_when_mandate_and_connection_differ(
        runs in proptest::collection::vec((environment(), environment()), 1..6),
    ) {
        let loaded = artifacts().map_err(TestCaseError::fail)?;
        for (mandate, connection) in runs {
            let fires = v001_fires(&loaded, mandate, connection).map_err(TestCaseError::fail)?;
            prop_assert_eq!(fires, mandate != connection, "{:?} on {:?}", mandate, connection);
        }
    }
}

/// The run's own validation reads Alpaca's stop-limit classes on a paper connection, the only
/// paper broker (LT-14), and none on a live one, the profile absent, so V-008 fails closed there
/// (DEC-1152 item 2): the reviewed equity mandate, with no offset, passes V-008 in the paper run's
/// validation and is refused it in a live run's.
#[test]
fn the_runs_validation_reads_alpacas_classes_on_paper_and_none_live() -> Result<(), String> {
    assert_eq!(
        run_stop_limit_classes(Environment::Paper),
        Some(BTreeSet::from([mandate_domain::AssetClass::Crypto]))
    );
    assert_eq!(run_stop_limit_classes(Environment::Live), None);
    let loaded = artifacts()?;
    for (connection, refused) in [(Environment::Paper, false), (Environment::Live, true)] {
        let contexts =
            load_contexts(&loaded, &facts()?, at(NOW)?, &agent(), connection).map_err(text)?;
        let report = validate(&loaded.mandate, &contexts.run.validation).map_err(text)?;
        assert_eq!(
            report.violations.contains(&Violation::V008),
            refused,
            "{connection:?}"
        );
    }
    Ok(())
}
