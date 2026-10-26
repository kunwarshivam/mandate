//! SPY deployed on the control stream as production registers it, and the paper broker's facts for
//! its run (E19-11, DEC-505; the first paper trade brief's slices Q1 and H3), shared by
//! `registered.rs` and `observed.rs`.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use mandate_accounting::AssetClass as TradedClass;
use mandate_accounting::InstrumentId;
use mandate_alpaca::{
    Asset, AssetSnapshot, Exchange as BrokerExchange, Feed as QuoteFeed, LatestQuote, MinuteBar,
    MinuteBars, alpaca_account_rules, wire,
};
use mandate_canon::{Digest, Value, to_canonical};
use mandate_domain::Environment;
use mandate_num::{Price, Qty, Usd};
use mandate_shell::Cause;
use mandate_shell::control::{
    Configuration, ConfirmedVersion, ControlRecord, LiveAdmission, Origin, Pinned, RunFacts,
    configuration, confirmed_version,
};
use mandate_shell::paper::{Artifacts, BrokerFacts, LiquidityFacts, PaperFacts};
use mandate_spec::context::{AgentId, Membership};
use mandate_time::{Date, UtcNanos};
use std::collections::BTreeMap;

pub const MANDATE: &str = include_str!("../fixtures/tracer/mandate.json");
pub const FEE: &str = include_str!("../fixtures/tracer/config/fee-config.json");
/// The live Robinhood fee schedule the R0 rehearsal registers (DEC-958).
pub const LIVE_FEE: &str = include_str!("../../../../config/fees/robinhood-live-2026-10-16.json");
pub const CALENDAR: &str = include_str!("../fixtures/tracer/config/trading-calendar.json");
pub const RULES: &str = include_str!("../fixtures/tracer/config/rule-set.json");
/// The E7-7 model artifact's bytes, inlined from the deleted fixture: their digest is the hash the
/// E7-7 mandate pins and `spy_mandate` replaces, and their `{"id","version"}` shape is what a
/// DEC-504 content object is not (DEC-475 item 6, X-4).
pub const E7_7_MODEL: &[u8] = b"{\"id\":\"quant.ma_crossover\",\"version\":\"1.0.0\"}\n";
pub const AAPL: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
pub const SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";
pub const SNAPSHOT: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-09-21T00:00:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b28f4066-5c6d-479b-a2af-85dc1a8f16fb","symbol":"SPY"}"#;
pub const ENVELOPE: &str = "autonomy behavior capital connection_id environment goal name notifications protection risk universe";

pub fn json(text: &str) -> Value {
    mandate_canon::parse(text.as_bytes()).unwrap()
}

pub fn reference(bytes: &[u8]) -> String {
    format!("sha256:{}", Digest::of(bytes))
}

/// The content object `mandate model register` stores for the pinned model (DEC-504 item 1), as
/// production registers it.
pub fn model() -> String {
    let content = mandate_modelhost::content("quant.ma_crossover", "1.0.0").unwrap();
    String::from_utf8(content.canonical).unwrap()
}

/// The E7-7 paper mandate re-pinned to SPY and to `content`'s hash.
pub fn spy_mandate(content: &str) -> String {
    let pinned = Digest::of(content.as_bytes()).to_hex();
    let mandate = MANDATE.replace(AAPL, SPY).replace(r#""AAPL""#, r#""SPY""#);
    mandate.replace(&Digest::of(E7_7_MODEL).to_hex(), &pinned)
}

/// `mandate` made `live`, with the stop-limit offset V-008 asks for on the Robinhood connection a
/// live stream records (DEC-539 item 2, DEC-1152 item 1).
pub fn live_mandate(mandate: &str) -> String {
    mandate
        .replace(r#""environment": "paper""#, r#""environment": "live""#)
        .replace(
            r#""crypto_stop_limit_offset": null"#,
            r#""crypto_stop_limit_offset": "0.01""#,
        )
}

/// The control stream of a deployed SPY version, and the store of every object it names, each
/// stored as written so its reference is the hash of those bytes.
pub struct Stream {
    pub records: Vec<ControlRecord>,
    pub store: BTreeMap<Digest, Vec<u8>>,
    /// The stream every record is journaled on: the paper workspace `ws_spy` unless
    /// [`Stream::live`] built it.
    pub origin: Origin,
}

/// The paper workspace's control stream every paper fixture is journaled on.
pub fn paper_origin() -> Origin {
    Origin {
        stream_id: "ctl:ws_spy".to_owned(),
        environment: Environment::Paper,
    }
}

/// The live workspace's own control stream (DEC-952 item 1).
pub fn live_origin() -> Origin {
    Origin {
        stream_id: "ctl:ws_live".to_owned(),
        environment: Environment::Live,
    }
}

impl Stream {
    pub fn deployed(snapshot: &str, rules: &str, fee: &str) -> Self {
        Self::with_model(snapshot, rules, fee, &model())
    }

    /// The deployment with `content` registered as the pinned model `quant.ma_crossover` 1.0.0, and
    /// the mandate re-pinned to its hash.
    pub fn with_model(snapshot: &str, rules: &str, fee: &str, content: &str) -> Self {
        Self::of(&spy_mandate(content), snapshot, rules, fee, content)
    }

    /// The deployment with `calendar` registered as its trading calendar, for the refusals that
    /// mutate it.
    pub fn with_calendar(snapshot: &str, rules: &str, fee: &str, calendar: &str) -> Self {
        let content = model();
        Self::of_objects(
            &spy_mandate(&content),
            snapshot,
            rules,
            fee,
            calendar,
            &content,
        )
    }

    /// The deployment of `mandate`, with `content` registered as its pinned model.
    pub fn of(mandate: &str, snapshot: &str, rules: &str, fee: &str, content: &str) -> Self {
        Self::of_objects(mandate, snapshot, rules, fee, CALENDAR, content)
    }

    /// The SPY deployment as a live workspace journals it (DEC-952 item 1): the mandate `live`, its
    /// connection established as a live Robinhood one so V-001 agrees, the fee schedule naming
    /// `fee_environment`, and every record on the live workspace's own control stream.
    pub fn live(fee_environment: &str) -> Self {
        Self::live_priced(&FEE.replace(
            r#""environment":"paper""#,
            &format!(r#""environment":"{fee_environment}""#),
        ))
    }

    /// The same live deployment with `fee` registered as its fee schedule.
    pub fn live_priced(fee: &str) -> Self {
        let content = model();
        let mandate = live_mandate(&spy_mandate(&content));
        let mut stream = Self {
            records: Vec::new(),
            store: BTreeMap::new(),
            origin: live_origin(),
        };
        stream.fill(&mandate, SNAPSHOT, RULES, fee, CALENDAR, &content);
        stream.record(
            "ConnectionEstablished",
            r#"{"broker":"robinhood","connection_id":"conn_alpaca_paper_01","environment":"live","scopes":["trading"]}"#,
        );
        stream
    }

    /// [`Stream::of`] with the registered trading calendar named too.
    pub fn of_objects(
        mandate: &str,
        snapshot: &str,
        rules: &str,
        fee: &str,
        calendar: &str,
        content: &str,
    ) -> Self {
        let mut stream = Self {
            records: Vec::new(),
            store: BTreeMap::new(),
            origin: paper_origin(),
        };
        stream.fill(mandate, snapshot, rules, fee, calendar, content);
        stream
    }

    /// Registers the model and the four configuration objects, then creates, confirms and deploys
    /// `mandate`, each record on [`Stream::origin`]: the five registrations are records 0 to 4,
    /// in the order model, fee, calendar, rule set, snapshot, and the deployment is record 7.
    fn fill(
        &mut self,
        mandate: &str,
        snapshot: &str,
        rules: &str,
        fee: &str,
        calendar: &str,
        content: &str,
    ) {
        let document = to_canonical(&json(mandate));
        let version = reference(&document);
        let paths: Vec<String> = ENVELOPE.split(' ').map(|p| format!("/{p}")).collect();
        let provenance: Vec<String> = paths
            .iter()
            .map(|path| format!(r#"{{"path":"{path}","source":"user_entered"}}"#))
            .collect();
        let confirmed: Vec<String> = paths.iter().map(|path| format!(r#""{path}""#)).collect();
        let stream = self;
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
            ("trading_calendar", calendar),
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
    }

    /// Stores `object`'s bytes as written and returns their reference.
    pub fn put(&mut self, object: &str) -> String {
        self.store
            .insert(Digest::of(object.as_bytes()), object.as_bytes().to_vec());
        reference(object.as_bytes())
    }

    /// Appends a record at the next `seq`, committed a second after the one before it, from
    /// 2026-09-20 (DEC-2384: the membership fold reads each record's `event_time`).
    pub fn record(&mut self, event_type: &str, payload: &str) {
        let seq = u64::try_from(self.records.len()).unwrap() + 1;
        let start = UtcNanos::parse_rfc3339("2026-09-20T00:00:00Z").unwrap();
        self.records.push(ControlRecord {
            seq,
            event_time: Some(
                UtcNanos::from_parts(start.secs() + i64::try_from(seq).unwrap(), 0).unwrap(),
            ),
            event_type: event_type.to_owned(),
            payload: json(payload),
            origin: self.origin.clone(),
        });
    }

    pub fn artifacts(&self) -> Result<Artifacts, Cause> {
        self.artifacts_on(Date::parse("2026-09-28").unwrap())
    }

    /// The run's artifacts for a run on `day`.
    pub fn artifacts_on(&self, day: Date) -> Result<Artifacts, Cause> {
        let (confirmed, config) = self.inputs_on(day);
        Artifacts::from_registered(&confirmed, &config)
    }

    /// The phase-1 confirmed version and the effective configuration this stream deploys.
    pub fn inputs(&self) -> (ConfirmedVersion, Configuration) {
        self.inputs_on(Date::parse("2026-09-28").unwrap())
    }

    /// [`Stream::inputs`] for a run on `day`.
    pub fn inputs_on(&self, day: Date) -> (ConfirmedVersion, Configuration) {
        self.inputs_admitting(day, LiveAdmission::refusing())
    }

    /// [`Stream::inputs_on`] for a run that supplies `live` as its live admission.
    pub fn inputs_admitting(
        &self,
        day: Date,
        live: LiveAdmission,
    ) -> (ConfirmedVersion, Configuration) {
        let run = RunFacts {
            agent: AgentId::new("agent_spy"),
            validation_date: day,
            membership: Membership {
                workspace_users: 1,
                approver_users: 1,
            },
            live,
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
        let config = configuration(&self.records, &self.store, &pinned, day).unwrap();
        (confirmed, config)
    }
}

pub fn minute_bars(symbol: &str, now: UtcNanos) -> MinuteBars {
    let bars = (1..=5)
        .rev()
        .map(|minutes: i64| MinuteBar {
            start: UtcNanos::from_parts(now.secs() - 60 * minutes, 0).unwrap(),
            volume: Qty::parse("100").unwrap(),
        })
        .collect();
    MinuteBars {
        instrument: InstrumentId::new(symbol).unwrap(),
        bars,
    }
}

/// The recorded empty, active paper account, a fresh SPY quote and SPY's minute bars, beside the
/// broker's asset record for `asset_id` and `symbol` listed on `exchange`, all read at `now`.
pub fn spy_facts(
    asset_id: &str,
    symbol: &str,
    exchange: BrokerExchange,
    now: UtcNanos,
) -> PaperFacts {
    let (price, qty) = (
        |text| Price::parse(text).unwrap(),
        |text| Qty::parse(text).unwrap(),
    );
    let account = include_bytes!("../fixtures/tracer/alpaca/account.json");
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
        at: UtcNanos::from_parts(now.secs() - 1, 500_000_000).unwrap(),
        bid: price("655.1"),
        bid_size: qty("2"),
        ask: price("655.2"),
        ask_size: qty("1"),
        feed: QuoteFeed::Iex,
        clock_skew: None,
    };
    let broker = BrokerFacts {
        account: wire::account(account).unwrap(),
        account_rules: alpaca_account_rules(),
        positions: Vec::new(),
        open_orders: Vec::new(),
        asset: AssetSnapshot {
            asset,
            loaded_at: UtcNanos::from_parts(now.secs() - 1, 0).unwrap(),
        },
        quote,
        minute_bars: minute_bars("SPY", now),
    };
    let liquidity = LiquidityFacts {
        prior_close: price("655.2"),
        median_dollar_volume_20d: Usd::parse("45864000000").unwrap(),
        adv_20d: qty("70000000"),
        trailing_5m_volume: qty("500"),
    };
    PaperFacts { broker, liquidity }
}
