//! The run's artifacts from the control stream, not from files (E7-19 slice 2 remainder, the
//! brief's slice Q1, X-8; E19-11, DEC-505): `Artifacts::from_registered` binds the confirmed
//! version and the effective registrations, and the liquidity facts read the instrument those
//! artifacts bind. The stream deploys the E7-7 paper mandate re-pinned to SPY, with the reviewed
//! fee schedule, calendar, rule set and model content registered beside SPY's DEC-523 snapshot.
//!
//! The gate's account rules come from the connector and the broker, not the shell (slice Q2, X-9,
//! DEC-840): the account type and day-trading regime are the connector's declaration, and the
//! maintenance excess and prior-close equity are the broker's account answer (A1, DEC-524).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use mandate_accounting::AssetClass as TradedClass;
use mandate_accounting::InstrumentId;
use mandate_alpaca::{
    AccountRules, Asset, AssetSnapshot, DeclaredRegime, Exchange as BrokerExchange,
    Feed as QuoteFeed, LatestQuote, MinuteBar, MinuteBars, alpaca_account_rules, wire,
};
use mandate_canon::{DecStr, Digest, Value, to_canonical};
use mandate_executor::BindingGateConfigRefs;
use mandate_marketdata::dataset::Store;
use mandate_marketdata::model::{AssetClass, Bar, DatasetId, Feed, Kind, Records, Symbol};
use mandate_num::{Price, Qty, Usd};
use mandate_risk::{AccountSnapshot, AccountType, DayTradeRegime, Exchange as GateExchange};
use mandate_shell::Cause;
use mandate_shell::control::{
    Configuration, ConfirmedVersion, ControlRecord, Pinned, RunFacts, configuration,
    confirmed_version,
};
use mandate_shell::paper::{
    Artifacts, BrokerFacts, LiquidityFacts, PaperFacts, liquidity_facts, load_contexts,
};
use mandate_spec::context::{AgentId, Membership};
use mandate_time::{Date, UtcNanos};

const MANDATE: &str = include_str!("fixtures/tracer/mandate.json");
const FEE: &str = include_str!("fixtures/tracer/config/fee-config.json");
const CALENDAR: &str = include_str!("fixtures/tracer/config/trading-calendar.json");
const RULES: &str = include_str!("fixtures/tracer/config/rule-set.json");
const E7_7_MODEL: &[u8] = include_bytes!("fixtures/tracer/config/model-artifact.json");
const AAPL: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
const SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";
const SNAPSHOT: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-09-21T00:00:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b28f4066-5c6d-479b-a2af-85dc1a8f16fb","symbol":"SPY"}"#;
const ENVELOPE: &str = "autonomy behavior capital connection_id environment goal name notifications protection risk universe";
const NOW: &str = "2026-09-28T17:00:00Z";

fn json(text: &str) -> Value {
    mandate_canon::parse(text.as_bytes()).unwrap()
}

fn reference(bytes: &[u8]) -> String {
    format!("sha256:{}", Digest::of(bytes))
}

/// The content object `mandate model register` stores for the pinned model (DEC-504 item 1), as
/// production registers it.
fn model() -> String {
    let content = mandate_modelhost::content("quant.ma_crossover", "1.0.0").unwrap();
    String::from_utf8(content.canonical).unwrap()
}

/// The E7-7 paper mandate re-pinned to SPY and to `content`'s hash.
fn spy_mandate(content: &str) -> String {
    let pinned = Digest::of(content.as_bytes()).to_hex();
    let mandate = MANDATE.replace(AAPL, SPY).replace(r#""AAPL""#, r#""SPY""#);
    mandate.replace(&Digest::of(E7_7_MODEL).to_hex(), &pinned)
}

fn now() -> UtcNanos {
    UtcNanos::parse_rfc3339(NOW).unwrap()
}

/// The control stream of a deployed SPY version, and the store of every object it names, each
/// stored as written so its reference is the hash of those bytes.
#[derive(Default)]
struct Stream {
    records: Vec<ControlRecord>,
    store: BTreeMap<Digest, Vec<u8>>,
}

impl Stream {
    fn deployed(snapshot: &str, rules: &str, fee: &str) -> Self {
        Self::with_model(snapshot, rules, fee, &model())
    }

    /// The deployment with `content` registered as the pinned model `quant.ma_crossover` 1.0.0, and
    /// the mandate re-pinned to its hash.
    fn with_model(snapshot: &str, rules: &str, fee: &str, content: &str) -> Self {
        let mandate = spy_mandate(content);
        let document = to_canonical(&json(&mandate));
        let version = reference(&document);
        let paths: Vec<String> = ENVELOPE.split(' ').map(|p| format!("/{p}")).collect();
        let provenance: Vec<String> = paths
            .iter()
            .map(|path| format!(r#"{{"path":"{path}","source":"user_entered"}}"#))
            .collect();
        let confirmed: Vec<String> = paths.iter().map(|path| format!(r#""{path}""#)).collect();
        let mut stream = Self::default();
        stream.store.insert(Digest::of(&document), document);
        let model = format!(
            r#"{{"admits_instruments":false,"content_hash":"{}","kind":"model_version","model_id":"quant.ma_crossover","model_version":"1.0.0","params":["fast_periods","slow_periods"]}}"#,
            stream.put(content)
        );
        let created = format!(
            r#"{{"mandate_version":"{version}","provenance":[{}],"record_ref":"sha256:{}"}}"#,
            provenance.join(","),
            "7".repeat(64)
        );
        let confirmation = format!(
            r#"{{"confirmed_paths":[{}],"mandate_version":"{version}"}}"#,
            confirmed.join(",")
        );
        let deployed = format!(
            r#"{{"agent_id":"agent_spy","mandate_version":"{version}","record_ref":"sha256:{}"}}"#,
            "6".repeat(64)
        );
        stream.record("ConfigSnapshotRegistered", &model);
        for (kind, object) in [
            ("fee_config", fee),
            ("trading_calendar", CALENDAR),
            ("rule_set", rules),
            ("instrument_snapshot", snapshot),
        ] {
            let hash = stream.put(object);
            let payload = format!(
                r#"{{"admits_instruments":null,"content_hash":"{hash}","kind":"{kind}","model_id":null,"model_version":null,"params":[]}}"#
            );
            stream.record("ConfigSnapshotRegistered", &payload);
        }
        stream.record("MandateVersionCreated", &created);
        stream.record("MandateConfirmed", &confirmation);
        stream.record("AgentDeployed", &deployed);
        stream
    }

    /// Stores `object`'s bytes as written and returns their reference.
    fn put(&mut self, object: &str) -> String {
        self.store
            .insert(Digest::of(object.as_bytes()), object.as_bytes().to_vec());
        reference(object.as_bytes())
    }

    fn record(&mut self, event_type: &str, payload: &str) {
        self.records.push(ControlRecord {
            seq: u64::try_from(self.records.len()).unwrap() + 1,
            event_type: event_type.to_owned(),
            payload: json(payload),
        });
    }

    fn artifacts(&self) -> Result<Artifacts, Cause> {
        let (confirmed, config) = self.inputs();
        Artifacts::from_registered(&confirmed, &config)
    }

    /// The phase-1 confirmed version and the effective configuration this stream deploys.
    fn inputs(&self) -> (ConfirmedVersion, Configuration) {
        let run = RunFacts {
            agent: AgentId::new("agent_spy"),
            validation_date: Date::parse("2026-09-28").unwrap(),
            membership: Membership {
                workspace_users: 1,
                approver_users: 1,
            },
        };
        let confirmed = confirmed_version(&self.records, &self.store, &run).unwrap();
        let mandate = confirmed.mandate();
        let model = &mandate.behavior.signal_models[0];
        let pinned = Pinned {
            asset_id: SPY.to_owned(),
            symbol: "SPY".to_owned(),
            model_id: model.id.as_str().to_owned(),
            model_version: model.version.clone(),
            content_hash: model.content_hash,
        };
        let trade_date = Date::parse("2026-09-28").unwrap();
        let config = configuration(&self.records, &self.store, &pinned, trade_date).unwrap();
        (confirmed, config)
    }
}

/// The artifacts bind the registered SPY instrument on the exchange its snapshot names, and the
/// deployed model, and their config references are the hashes of the registered objects and the
/// confirmed version.
#[test]
fn the_artifacts_bind_the_registered_instrument_and_objects() {
    let nasdaq = SNAPSHOT.replace("arca", "nasdaq");
    for (snapshot, venues) in [
        (
            nasdaq.as_str(),
            (BrokerExchange::Nasdaq, GateExchange::Nasdaq),
        ),
        (SNAPSHOT, (BrokerExchange::Arca, GateExchange::Arca)),
    ] {
        let identity = Stream::deployed(snapshot, RULES, FEE).artifacts().unwrap();
        let identity = identity.production_identity();
        assert_eq!((identity.broker_exchange, identity.gate_exchange), venues);
    }
    let stream = Stream::deployed(SNAPSHOT, RULES, FEE);
    let artifacts = stream.artifacts().unwrap();
    let identity = artifacts.production_identity();
    assert_eq!(identity.asset_id.as_str(), SPY);
    assert_eq!(identity.symbol.as_str(), "SPY");
    let triple = (identity.model_id, identity.model_version);
    assert_eq!(triple, ("quant.ma_crossover", "1.0.0"));
    assert_eq!(identity.model_hash, Digest::of(model().as_bytes()));
    let mandate = spy_mandate(&model());
    let refs = BindingGateConfigRefs::complete(
        reference(FEE.as_bytes()),
        reference(CALENDAR.as_bytes()),
        reference(SNAPSHOT.as_bytes()),
        reference(RULES.as_bytes()),
        reference(&to_canonical(&json(&mandate))),
    );
    assert_eq!(artifacts.config_refs(), &refs);
}

/// A registered object is judged as its file is: a fee schedule or a rule set the run cannot use
/// is refused, never replaced by a reviewed default. A configuration whose instrument or model is
/// not the confirmed mandate's is refused too, and so is registered content whose own
/// `model_version` or `model_id` is not the pin's although its hash is the pinned one, content
/// that names the pin under another `kind`, and the E7-7 file's `{"id","version"}` shape, which is
/// no DEC-504 content object and has no fallback (DEC-504).
#[test]
fn a_registered_object_the_run_cannot_use_is_refused() {
    let aggressive = FEE.replace("conservative_v1", "aggressive_v1");
    let slow_quotes = RULES.replace(r#""iex_quote_max_age_s":"#, r#""iex_quote_max_age_s":6"#);
    assert_ne!(slow_quotes, RULES, "the edit applied");
    for stream in [
        Stream::deployed(SNAPSHOT, RULES, &aggressive),
        Stream::deployed(SNAPSHOT, &slow_quotes, FEE),
    ] {
        assert_absent(stream.artifacts());
    }
    let pinned = r#""model_version":"1.0.0""#;
    let renamed = model().replace(pinned, r#""model_version":"1.0.1""#);
    let other_id = model().replace("quant.ma_crossover", "quant.other");
    let other_kind = model().replace(
        r#""kind":"quant_model_content""#,
        r#""kind":"llm_model_content""#,
    );
    let e7_7_shape = String::from_utf8(E7_7_MODEL.to_vec()).unwrap();
    for content in [renamed, other_id, other_kind, e7_7_shape] {
        assert_ne!(content, model(), "the edit applied");
        assert_absent(Stream::with_model(SNAPSHOT, RULES, FEE, &content).artifacts());
    }
    let (confirmed, config) = Stream::deployed(SNAPSHOT, RULES, FEE).inputs();
    let mut other_symbol = config.clone();
    other_symbol.instrument.symbol = "QQQ".to_owned();
    let mut other_asset = config.clone();
    other_asset.instrument.instrument_id = AAPL.to_owned();
    let mut other_model = config;
    other_model.model_version.content_hash = Digest::of(b"another model");
    for config in [other_symbol, other_asset, other_model] {
        assert_absent(Artifacts::from_registered(&confirmed, &config));
    }
}

fn assert_absent(refused: Result<Artifacts, Cause>) {
    let code = refused.as_ref().err().map(Cause::code);
    assert_eq!(code, Some("absent"), "{:?}", refused.err());
}

/// Twenty-five daily SPY bars ending on Friday 2026-09-25, the last session completed at [`NOW`].
fn daily(root: &Path, symbol: &str) -> PathBuf {
    let id = DatasetId::new(
        AssetClass::UsEquity,
        Feed::Iex,
        Kind::Bars("1Day".parse().unwrap()),
        Symbol::parse(symbol).unwrap(),
    )
    .unwrap();
    let store = Store::new(root);
    let mut day = Date::parse("2026-08-24").unwrap();
    for close in 631..656 {
        while day.is_weekend() {
            day = day.next().unwrap();
        }
        let start = UtcNanos::parse_rfc3339(&format!("{day}T04:00:00Z")).unwrap();
        let price = DecStr::parse(&format!("{close}.20")).unwrap();
        let bar = Bar {
            start,
            open: price.clone(),
            high: price.clone(),
            low: price.clone(),
            close: price.clone(),
            volume: DecStr::parse("70000000").unwrap(),
            vwap: price,
            trade_count: 400_000,
        };
        store.put_day(&id, day, &Records::Bars(vec![bar])).unwrap();
        day = day.next().unwrap();
    }
    store.dataset_dir(&id)
}

fn minute_bars(symbol: &str) -> MinuteBars {
    let bars = (55..60)
        .map(|minute| MinuteBar {
            start: UtcNanos::parse_rfc3339(&format!("2026-09-28T16:{minute}:00Z")).unwrap(),
            volume: Qty::parse("100").unwrap(),
        })
        .collect();
    MinuteBars {
        instrument: InstrumentId::new(symbol).unwrap(),
        bars,
    }
}

/// The liquidity facts read the instrument the registered artifacts bind: SPY's daily and minute
/// bars give SPY's figures, and AAPL's minute bars are refused for it.
#[test]
fn the_liquidity_facts_read_the_registered_instrument() {
    let artifacts = Stream::deployed(SNAPSHOT, RULES, FEE).artifacts().unwrap();
    let symbol = artifacts.production_identity().symbol;
    let root = std::env::temp_dir().join(format!("mandate-shell-q1-{}", std::process::id()));
    let aapl = daily(&root.join("aapl"), "AAPL");
    let daily = daily(&root, "SPY");
    let wrong_daily = liquidity_facts(symbol, &aapl, &minute_bars("SPY"), now());
    assert!(wrong_daily.is_err(), "AAPL's bars: {wrong_daily:?}");
    let facts = liquidity_facts(symbol, &daily, &minute_bars("SPY"), now()).unwrap();
    let figures = (
        facts.prior_close.to_string(),
        facts.trailing_5m_volume.to_string(),
    );
    assert_eq!(figures, ("655.2".to_owned(), "500".to_owned()));
    let other = liquidity_facts(symbol, &daily, &minute_bars("AAPL"), now());
    assert_eq!(other.err().map(|cause| cause.code()), Some("absent"));
    fs::remove_dir_all(&root).unwrap();
}

/// The recorded empty, active paper account, a fresh SPY quote and SPY's minute bars, beside the
/// broker's asset record for `asset_id` and `symbol` listed on `exchange`.
fn spy_facts(asset_id: &str, symbol: &str, exchange: BrokerExchange) -> PaperFacts {
    let at = |text| UtcNanos::parse_rfc3339(text).unwrap();
    let (price, qty) = (
        |text| Price::parse(text).unwrap(),
        |text| Qty::parse(text).unwrap(),
    );
    let account = include_bytes!("fixtures/tracer/alpaca/account.json");
    let asset = Asset {
        asset_id: asset_id.to_owned(),
        instrument: InstrumentId::new(symbol).unwrap(),
        class: TradedClass::UsEquity,
        exchange,
        active: true,
        tradable: true,
        fractionable: true,
        ipo: false,
        ptp_no_exception: false,
        min_order_size: None,
        min_trade_increment: None,
        price_increment: None,
    };
    let quote = LatestQuote {
        instrument: InstrumentId::new("SPY").unwrap(),
        at: at("2026-09-28T16:59:59.5Z"),
        bid: price("655.1"),
        bid_size: qty("2"),
        ask: price("655.2"),
        ask_size: qty("1"),
        feed: QuoteFeed::Iex,
    };
    let broker = BrokerFacts {
        account: wire::account(account).unwrap(),
        account_rules: alpaca_account_rules(),
        positions: Vec::new(),
        open_orders: Vec::new(),
        asset: AssetSnapshot {
            asset,
            loaded_at: at("2026-09-28T16:59:59Z"),
        },
        quote,
        minute_bars: minute_bars("SPY"),
    };
    let liquidity = LiquidityFacts {
        prior_close: price("655.2"),
        median_dollar_volume_20d: Usd::parse("45864000000").unwrap(),
        adv_20d: qty("70000000"),
        trailing_5m_volume: qty("500"),
    };
    PaperFacts { broker, liquidity }
}

/// The preflight judges the broker's asset record against the instrument the registered artifacts
/// bind (X-8): SPY's id, listed on arca as its snapshot says, holds; AAPL's id, SPY on nasdaq, or
/// another symbol is refused at the asset record, and a registered snapshot whose ETP
/// classification is dated after the run is refused at the classification.
#[test]
fn the_preflight_judges_the_registered_instruments_asset_record() {
    let artifacts = Stream::deployed(SNAPSHOT, RULES, FEE).artifacts().unwrap();
    let agent = mandate_runtime::AgentId("agent_spy".to_owned());
    let judged = |facts: PaperFacts| load_contexts(&artifacts, &facts, now(), &agent).map(|_| ());
    let spy = judged(spy_facts(SPY, "SPY", BrokerExchange::Arca));
    assert!(spy.is_ok(), "{spy:?}");
    let others = [
        (AAPL, "SPY", BrokerExchange::Arca),
        (SPY, "SPY", BrokerExchange::Nasdaq),
        (SPY, "QQQ", BrokerExchange::Arca),
    ];
    for (asset_id, symbol, exchange) in others {
        let refused = format!("{:?}", judged(spy_facts(asset_id, symbol, exchange)));
        let at_the_record = refused.contains("Absent") && refused.contains("asset record");
        assert!(at_the_record, "{asset_id} {symbol} {exchange:?}: {refused}");
    }
    let classified_later = SNAPSHOT.replace("2026-09-21T00:00:00Z", "2026-09-29T00:00:00Z");
    assert_ne!(classified_later, SNAPSHOT, "the edit applied");
    let later = Stream::deployed(&classified_later, RULES, FEE)
        .artifacts()
        .unwrap();
    let facts = spy_facts(SPY, "SPY", BrokerExchange::Arca);
    let refused = format!(
        "{:?}",
        load_contexts(&later, &facts, now(), &agent).map(|_| ())
    );
    let at_the_classification =
        refused.contains("Absent") && refused.contains("ETP classification");
    assert!(at_the_classification, "{refused}");
}

/// The gate's account snapshot for SPY's registered run, with the broker's account answer edited
/// to `equity`, `maintenance_margin` and `last_equity` and the connector's declaration replaced by
/// `rules`.
fn gate_account(rules: AccountRules, figures: [&str; 3]) -> Result<AccountSnapshot, Cause> {
    let artifacts = Stream::deployed(SNAPSHOT, RULES, FEE).artifacts().unwrap();
    let mut facts = spy_facts(SPY, "SPY", BrokerExchange::Arca);
    let [equity, maintenance_margin, last_equity] = figures.map(|text| Usd::parse(text).unwrap());
    facts.broker.account.equity = equity;
    facts.broker.account.maintenance_margin = maintenance_margin;
    facts.broker.account.last_equity = last_equity;
    facts.broker.account_rules = rules;
    let agent = mandate_runtime::AgentId("agent_spy".to_owned());
    let contexts = load_contexts(&artifacts, &facts, now(), &agent)?;
    let decision = contexts.run.decision.expect("a decision context");
    Ok(decision.gate.expect("an advisory gate context").account)
}

/// Alpaca's declared rules (§7.2: always margin, `intraday_margin`) reach the gate with the
/// broker's own figures: the maintenance excess is equity less `maintenance_margin`, and the
/// prior-close equity is `last_equity` (§9.2, DEC-524 items 2 and 4). Each expected excess is
/// worked by hand, and the accounts differ in every figure, so no constant passes.
#[test]
#[ignore = "pending E7-19"]
fn the_gate_takes_the_brokers_maintenance_excess_and_prior_close_equity() {
    let cases = [
        (["1000000", "0", "1000000"], "1000000"),
        (["250000.5", "40000.25", "248000"], "210000.25"),
        (["30000", "29999.99", "31000"], "0.01"),
        (["25000", "25000", "24000.01"], "0"),
    ];
    for (figures, excess) in cases {
        let account = gate_account(alpaca_account_rules(), figures).unwrap();
        assert_eq!(account.account_type, AccountType::Margin, "{figures:?}");
        let regime = DayTradeRegime::IntradayMargin {
            maintenance_excess: Usd::parse(excess).unwrap(),
        };
        assert_eq!(account.regime, regime, "{figures:?}");
        assert_eq!(account.equity, Usd::parse(figures[0]).unwrap());
        let prior_close = Usd::parse(figures[2]).unwrap();
        assert_eq!(account.prior_close_equity, prior_close, "{figures:?}");
    }
}

/// Another declaration flows through as it was declared: the gate's account type and regime are
/// the connector's, never the shell's, whichever of §7.2's types and §9.2's regimes it names.
#[test]
#[ignore = "pending E7-19"]
fn another_declared_account_type_or_regime_reaches_the_gate() {
    let figures = ["250000.5", "40000.25", "248000"];
    let intraday = DayTradeRegime::IntradayMargin {
        maintenance_excess: Usd::parse("210000.25").unwrap(),
    };
    let cases = [
        (
            AccountType::Margin,
            DeclaredRegime::LegacyPdt,
            DayTradeRegime::LegacyPdt,
        ),
        (AccountType::Cash, DeclaredRegime::IntradayMargin, intraday),
        (
            AccountType::Cash,
            DeclaredRegime::LegacyPdt,
            DayTradeRegime::LegacyPdt,
        ),
    ];
    for (account_type, regime, expected) in cases {
        let rules = AccountRules {
            account_type,
            regime,
        };
        let account = gate_account(rules, figures).unwrap();
        assert_eq!(account.account_type, account_type, "{rules:?}");
        assert_eq!(account.regime, expected, "{rules:?}");
        assert_eq!(account.prior_close_equity, Usd::parse("248000").unwrap());
    }
}

/// Under `intraday_margin`, a maintenance figure no broker states (a negative requirement,
/// DEC-524 item 2) refuses the run rather than reading as no requirement, and a reported deficit
/// refuses its opening, since nothing yet turns it into §9.2's `exits_only` (DEC-840; `AGENTS.md`
/// rule 3). Each has its own reason.
#[test]
#[ignore = "pending E7-19"]
fn an_unreadable_maintenance_figure_or_a_deficit_refuses_the_run() {
    let cases = [
        (
            ["1000000", "-1", "1000000"],
            "the broker's maintenance excess",
        ),
        (
            ["30000", "30000.01", "31000"],
            "an account with no maintenance deficit",
        ),
    ];
    for (figures, reason) in cases {
        let refused = gate_account(alpaca_account_rules(), figures);
        let what = match refused {
            Err(Cause::Absent { what }) => Some(what),
            _ => None,
        };
        assert_eq!(what, Some(reason), "{figures:?}: {refused:?}");
    }
}
