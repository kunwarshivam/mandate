//! The run's artifacts from the control stream, not from files (E7-19 slice 2 remainder, the
//! brief's slice Q1, X-8; E19-11, DEC-505): `Artifacts::from_registered` binds the confirmed
//! version and the effective registrations, and the liquidity facts read the instrument those
//! artifacts bind. The stream deploys the E7-7 paper mandate re-pinned to SPY, with the reviewed
//! fee schedule, calendar, rule set and model content registered beside SPY's DEC-523 snapshot.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use mandate_accounting::InstrumentId;
use mandate_alpaca::{Exchange as BrokerExchange, MinuteBar, MinuteBars};
use mandate_canon::{DecStr, Digest, Value, to_canonical};
use mandate_executor::BindingGateConfigRefs;
use mandate_marketdata::dataset::Store;
use mandate_marketdata::model::{AssetClass, Bar, DatasetId, Feed, Kind, Records, Symbol};
use mandate_num::Qty;
use mandate_risk::Exchange as GateExchange;
use mandate_shell::Cause;
use mandate_shell::control::{
    Configuration, ConfirmedVersion, ControlRecord, Pinned, RunFacts, configuration,
    confirmed_version,
};
use mandate_shell::paper::{Artifacts, liquidity_facts};
use mandate_spec::context::{AgentId, Membership};
use mandate_time::{Date, UtcNanos};

const MANDATE: &str = include_str!("fixtures/tracer/mandate.json");
const FEE: &str = include_str!("fixtures/tracer/config/fee-config.json");
const CALENDAR: &str = include_str!("fixtures/tracer/config/trading-calendar.json");
const RULES: &str = include_str!("fixtures/tracer/config/rule-set.json");
const MODEL: &str = include_str!("fixtures/tracer/config/model-artifact.json");
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
        let mandate = MANDATE.replace(AAPL, SPY).replace(r#""AAPL""#, r#""SPY""#);
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
            stream.put(MODEL)
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
#[ignore = "pending E7-19"]
fn the_artifacts_bind_the_registered_instrument_and_objects() {
    let nasdaq = SNAPSHOT.replace("arca", "nasdaq");
    let exchanges = [
        (
            nasdaq.as_str(),
            BrokerExchange::Nasdaq,
            GateExchange::Nasdaq,
        ),
        (SNAPSHOT, BrokerExchange::Arca, GateExchange::Arca),
    ];
    for (snapshot, broker, gate) in exchanges {
        let artifacts = Stream::deployed(snapshot, RULES, FEE).artifacts().unwrap();
        let identity = artifacts.production_identity();
        assert_eq!(
            (identity.broker_exchange, identity.gate_exchange),
            (broker, gate)
        );
    }
    let stream = Stream::deployed(SNAPSHOT, RULES, FEE);
    let artifacts = stream.artifacts().unwrap();
    let identity = artifacts.production_identity();
    assert_eq!(identity.asset_id.as_str(), SPY);
    assert_eq!(identity.symbol.as_str(), "SPY");
    let model = (
        identity.model_id,
        identity.model_version,
        identity.model_hash,
    );
    let hash = Digest::of(MODEL.as_bytes());
    assert_eq!(model, ("quant.ma_crossover", "1.0.0", hash));
    let mandate = MANDATE.replace(AAPL, SPY).replace(r#""AAPL""#, r#""SPY""#);
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
/// not the confirmed mandate's is refused too.
#[test]
#[ignore = "pending E7-19"]
fn a_registered_object_the_run_cannot_use_is_refused() {
    let aggressive = FEE.replace("conservative_v1", "aggressive_v1");
    let slow_quotes = RULES.replace(r#""iex_quote_max_age_s":"#, r#""iex_quote_max_age_s":6"#);
    assert_ne!(slow_quotes, RULES, "the edit applied");
    for stream in [
        Stream::deployed(SNAPSHOT, RULES, &aggressive),
        Stream::deployed(SNAPSHOT, &slow_quotes, FEE),
    ] {
        let refused = stream.artifacts().err();
        assert_eq!(
            refused.as_ref().map(Cause::code),
            Some("absent"),
            "{refused:?}"
        );
    }
    let (confirmed, config) = Stream::deployed(SNAPSHOT, RULES, FEE).inputs();
    let mut other_symbol = config.clone();
    other_symbol.instrument.symbol = "QQQ".to_owned();
    let mut other_asset = config.clone();
    other_asset.instrument.instrument_id = AAPL.to_owned();
    let mut other_model = config;
    other_model.model_version.content_hash = Digest::of(b"another model");
    for config in [other_symbol, other_asset, other_model] {
        let refused = Artifacts::from_registered(&confirmed, &config).err();
        assert_eq!(
            refused.as_ref().map(Cause::code),
            Some("absent"),
            "{refused:?}"
        );
    }
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
#[ignore = "pending E7-19"]
fn the_liquidity_facts_read_the_registered_instrument() {
    let artifacts = Stream::deployed(SNAPSHOT, RULES, FEE).artifacts().unwrap();
    let symbol = artifacts.production_identity().symbol;
    let root = std::env::temp_dir().join(format!("mandate-shell-q1-{}", std::process::id()));
    let daily = daily(&root, "SPY");
    let facts = liquidity_facts(symbol, &daily, &minute_bars("SPY"), now());
    let figures = facts.map(|facts| {
        (
            facts.prior_close.to_string(),
            facts.trailing_5m_volume.to_string(),
        )
    });
    assert_eq!(
        figures.as_ref().map_err(Cause::code),
        Ok(&("655.2".to_owned(), "500".to_owned()))
    );
    let other = liquidity_facts(symbol, &daily, &minute_bars("AAPL"), now());
    assert_eq!(other.err().map(|cause| cause.code()), Some("absent"));
    fs::remove_dir_all(&root).unwrap();
}
