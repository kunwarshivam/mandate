//! DEC-2380 items 2 and 5, X1b-3 G2b: the executor's ports read the deployment from the journal
//! and the artifact store. The control stream's confirmed version and registrations, read through
//! `mandate-gate-inputs`, answer the mandate, instrument and protection ports and name the
//! instrument the executor marks; the binding gate's input is assembled from that deployment, the
//! journal's account, mark and clock, the quote, and the facts the journal does not hold. A
//! deployment the store cannot read answers nothing and holds every opening; an exit never reads
//! the stored bars or the broker's facts (`AGENTS.md` rule 13, DEC-2274 item 6).

mod support;

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use mandate_accounting::{AccountType, AssetClass, InstrumentId, Side};
use mandate_canon::{Digest, Value, to_canonical};
use mandate_executor::{
    AccountRef, AccountScope, AgentId, BindingGateInput, BindingGateRequest, BindingGateSource,
    InstrumentSnapshot, MandateView, MarketObservation, Purpose, RiskClock, TimeInForce,
    WorkspaceId,
};
use mandate_executord::Protections;
use mandate_executord::deployment::{BrokerFacts, Deployment, LiquidityRead, Sources};
use mandate_executord::gate::{AccountRules, AssetRecord, Liquidity};
use mandate_executord::marks::Marked;
use mandate_executord::quotes::QuoteFeed;
use mandate_journal::{ArtifactError, ArtifactRef, ArtifactSource, StreamId};
use mandate_marketdata::dataset::Store as Datasets;
use mandate_marketdata::model::{AssetClass as Class, Bar, DatasetId, Feed, Kind, Records, Symbol};
use mandate_num::{Price, Qty, Usd};
use mandate_risk::{
    DayTradeRegime, GateInput, GatePass, Origin, ProposedKind, ProposedOrder, ReasonCode,
    TimeInForce as GateTimeInForce, Verdict, evaluate,
};
use mandate_time::{Date, ExchangeCalendar, UtcNanos, new_york_midnight};
use support::{
    ACCOUNT, ACCOUNT_STREAM, AGENT, CONNECTION, CONTROL_STREAM, Error, Journal, OTHER_AGENT,
    WORKSPACE, at, object, tick,
};

const MANDATE: &str = include_str!("../../mandate-shell/tests/fixtures/tracer/mandate.json");
const FEE: &str = include_str!("../../mandate-shell/tests/fixtures/tracer/config/fee-config.json");
const CALENDAR: &str =
    include_str!("../../mandate-shell/tests/fixtures/tracer/config/trading-calendar.json");
const RULES: &str = include_str!("../../mandate-shell/tests/fixtures/tracer/config/rule-set.json");
const E7_7_MODEL: &[u8] = b"{\"id\":\"quant.ma_crossover\",\"version\":\"1.0.0\"}\n";
const CONTENT: &str =
    r#"{"kind":"quant_model_content","model_id":"quant.ma_crossover","model_version":"1.0.0"}"#;
const AAPL_ID: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
const SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";
const SNAPSHOT: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-09-21T00:00:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b28f4066-5c6d-479b-a2af-85dc1a8f16fb","symbol":"SPY"}"#;
const ENVELOPE: &str = "autonomy behavior capital connection_id environment goal name notifications protection risk universe";
/// Monday 12:00 in New York, inside the regular session.
const MONDAY: &str = "2026-09-21T16:00:00Z";

type Artifacts = BTreeMap<Digest, Vec<u8>>;

fn json(text: &str) -> Result<Value, Error> {
    Ok(mandate_canon::parse(text.as_bytes())?)
}

fn reference(bytes: &[u8]) -> String {
    format!("sha256:{}", Digest::of(bytes).to_hex())
}

fn put(store: &mut Artifacts, object: &str) -> String {
    store.insert(Digest::of(object.as_bytes()), object.as_bytes().to_vec());
    reference(object.as_bytes())
}

/// What a test changes in the deployment: the mandate's crypto stop-limit offset, and a loss an
/// agent retired from the same connection added before this one was deployed.
#[derive(Default, Clone, Copy)]
struct Variant {
    offset: Option<&'static str>,
    retired_loss: Option<&'static str>,
}

/// The account's journal with the agent's SPY mandate registered, created, confirmed and deployed
/// on the account's connection, and the store holding every object those records name; the
/// deployed version's reference.
fn deployed() -> Result<(Journal, Artifacts, String), Error> {
    deployed_as(Variant::default())
}

fn deployed_as(variant: Variant) -> Result<(Journal, Artifacts, String), Error> {
    let journal = Journal::default();
    journal.write(
        CONTROL_STREAM,
        "c1",
        "ConnectionEstablished",
        2,
        None,
        json(&format!(
            r#"{{"account_ref":"{ACCOUNT}","broker":"alpaca","connection_id":"{CONNECTION}","environment":"paper","scopes":["trading"],"user":"user_founder"}}"#
        ))?,
    )?;
    if let Some(loss) = variant.retired_loss {
        journal.write(
            CONTROL_STREAM,
            "r1",
            "AgentStopped",
            1,
            None,
            json(&format!(
                r#"{{"agent_id":"agent_old","connection_id":"{CONNECTION}","loss_added":"{loss}","retired_on":"2026-09-20"}}"#
            ))?,
        )?;
    }
    let mut store = Artifacts::new();
    let mandate = MANDATE
        .replace(AAPL_ID, SPY)
        .replace(r#""AAPL""#, r#""SPY""#)
        .replace(
            &Digest::of(E7_7_MODEL).to_hex(),
            &Digest::of(CONTENT.as_bytes()).to_hex(),
        )
        .replace("conn_alpaca_paper_01", CONNECTION)
        .replace(
            r#""crypto_stop_limit_offset": null"#,
            &variant
                .offset
                .map_or("\"crypto_stop_limit_offset\": null".to_owned(), |offset| {
                    format!(r#""crypto_stop_limit_offset": "{offset}""#)
                }),
        );
    let document = to_canonical(&json(&mandate)?);
    let version = reference(&document);
    store.insert(Digest::of(&document), document);
    let paths: Vec<String> = ENVELOPE.split(' ').map(|p| format!("/{p}")).collect();
    let provenance: Vec<String> = paths
        .iter()
        .map(|path| format!(r#"{{"path":"{path}","source":"user_entered"}}"#))
        .collect();
    let confirmed: Vec<String> = paths.iter().map(|path| format!(r#""{path}""#)).collect();
    let mut payloads = vec![(
        "ConfigSnapshotRegistered",
        format!(
            r#"{{"admits_instruments":false,"content_hash":"{}","kind":"model_version","model_id":"quant.ma_crossover","model_version":"1.0.0","params":["fast_periods","slow_periods"]}}"#,
            put(&mut store, CONTENT)
        ),
    )];
    for (kind, object) in [
        ("fee_config", FEE.trim()),
        ("trading_calendar", CALENDAR.trim()),
        ("rule_set", RULES.trim()),
        ("instrument_snapshot", SNAPSHOT),
    ] {
        let hash = put(&mut store, object);
        payloads.push((
            "ConfigSnapshotRegistered",
            format!(
                r#"{{"admits_instruments":null,"content_hash":"{hash}","kind":"{kind}","model_id":null,"model_version":null,"params":[]}}"#
            ),
        ));
    }
    payloads.push((
        "MandateVersionCreated",
        format!(
            r#"{{"mandate_version":"{version}","provenance":[{}],"record_ref":"sha256:{}"}}"#,
            provenance.join(","),
            "7".repeat(64)
        ),
    ));
    payloads.push((
        "MandateConfirmed",
        format!(
            r#"{{"confirmed_paths":[{}],"mandate_version":"{version}"}}"#,
            confirmed.join(",")
        ),
    ));
    payloads.push((
        "AgentDeployed",
        format!(
            r#"{{"agent_id":"{AGENT}","connection_id":"{CONNECTION}","mandate_version":"{version}","record_ref":"sha256:{}"}}"#,
            "6".repeat(64)
        ),
    ));
    for (index, (event_type, payload)) in payloads.into_iter().enumerate() {
        journal.write(
            CONTROL_STREAM,
            &format!("d{index}"),
            event_type,
            if event_type == "AgentDeployed" { 2 } else { 1 },
            None,
            json(&payload)?,
        )?;
    }
    tick(&journal, "t1", MONDAY)?;
    Ok((journal, store, version))
}

/// The account's snapshot and SPY's sane mark at `MONDAY`, as the executor journals them.
fn journaled(journal: &Journal) -> Result<(), Error> {
    let text = |value: &str| Value::Str(value.to_owned());
    journal.write(
        ACCOUNT_STREAM,
        "a1",
        "StreamOpened",
        1,
        None,
        object(vec![("stream_id", text(ACCOUNT_STREAM))])?,
    )?;
    journaled_at(journal, "a", "2026-09-21T16:00:00.000000000Z")
}

/// The account's snapshot and SPY's sane mark at `clock`, with ids prefixed `prefix`.
fn journaled_at(journal: &Journal, prefix: &str, clock: &str) -> Result<(), Error> {
    let text = |value: &str| Value::Str(value.to_owned());
    journal.write(
        ACCOUNT_STREAM,
        &format!("{prefix}2"),
        "AccountSnapshotRecorded",
        1,
        None,
        object(vec![
            ("account_blocked", Value::Bool(false)),
            ("buying_power", text("200000")),
            ("cash", text("100000")),
            ("crypto_status", text("INACTIVE")),
            ("equity", text("100000")),
            (
                "multiplier",
                Value::Int(mandate_canon::Int::new(2).ok_or("an int")?),
            ),
            ("non_marginable_buying_power", text("90000")),
            ("risk_clock", text(clock)),
            ("status", text("ACTIVE")),
            ("trade_suspended_by_user", Value::Bool(false)),
            ("trading_blocked", Value::Bool(false)),
        ])?,
    )?;
    journal.write(
        ACCOUNT_STREAM,
        &format!("{prefix}3"),
        "MarkUpdated",
        2,
        None,
        object(vec![
            ("bid", text("99.95")),
            ("feed", text("fixture")),
            ("instrument_id", text(SPY)),
            ("price", text("99.95")),
            ("risk_clock", text(clock)),
            ("sane", Value::Bool(true)),
            ("session", text("regular")),
            ("source", text("quote")),
        ])?,
    )
}

/// A feed with one sane quote for every instrument, 99.95 by 100.05, observed at the instant
/// asked.
struct Quoting;

impl QuoteFeed for Quoting {
    fn observe(
        &self,
        instrument: &InstrumentId,
        now: UtcNanos,
    ) -> Result<MarketObservation, &'static str> {
        Ok(MarketObservation {
            instrument: instrument.clone(),
            bid: Price::parse("99.95").ok(),
            bid_size: Qty::parse("2").ok(),
            ask: Price::parse("100.05").ok(),
            last_trade: None,
            mark: None,
            sane: true,
            observed_at: RiskClock::from_secs(now.secs()),
        })
    }

    fn feed(&self) -> &'static str {
        "fixture"
    }
}

/// The broker's facts for SPY, counting each read.
struct Read(Rc<Cell<u32>>);

impl BrokerFacts for Read {
    fn asset(&self, _symbol: &str) -> Option<AssetRecord> {
        self.0.set(self.0.get() + 1);
        Some(AssetRecord {
            active: true,
            tradable: true,
            fractionable: true,
            ipo: false,
            ptp_no_exception: false,
        })
    }

    fn rules(&self) -> Option<AccountRules> {
        self.0.set(self.0.get() + 1);
        Some(AccountRules {
            account_type: AccountType::Margin,
            regime: DayTradeRegime::LegacyPdt,
            prior_close_equity: Usd::parse("100000").ok()?,
        })
    }

    fn halted(&self, _symbol: &str) -> Option<bool> {
        self.0.set(self.0.get() + 1);
        Some(false)
    }
}

/// SPY's stored liquidity, counting each read.
struct Bars(Rc<Cell<u32>>);

impl LiquidityRead for Bars {
    fn liquidity(&self, _symbol: &str, _now: UtcNanos) -> Option<Liquidity> {
        self.0.set(self.0.get() + 1);
        Some(Liquidity {
            prior_close: Price::parse("100").ok()?,
            median_dollar_volume_20d: Usd::parse("5000000000").ok()?,
            adv_20d: Qty::parse("50000000").ok()?,
            trailing_5m_volume: Qty::parse("1000000").ok()?,
        })
    }
}

fn scope() -> AccountScope {
    AccountScope {
        account: AccountRef(ACCOUNT.to_owned()),
        workspace: WorkspaceId(WORKSPACE.to_owned()),
    }
}

/// The deployment over `journal` and `store`, with every fact the journal does not hold read, and
/// the count of broker and bars reads.
fn deployment(journal: &Journal, store: Artifacts) -> (Deployment, Rc<Cell<u32>>) {
    let reads = Rc::new(Cell::new(0));
    let rows = journal.clone();
    let sources = Sources {
        rows: Box::new(move |stream: &StreamId| mandate_store::Store::rows(&rows, stream)),
        artifacts: Box::new(store),
        quotes: Box::new(Quoting),
        liquidity: Box::new(Bars(Rc::clone(&reads))),
        broker: Box::new(Read(Rc::clone(&reads))),
    };
    (Deployment::new(&scope(), sources), reads)
}

fn agent() -> AgentId {
    AgentId(AGENT.to_owned())
}

fn spy() -> Result<InstrumentId, Error> {
    Ok(InstrumentId::new(SPY)?)
}

fn request<'a>(
    agent: &'a AgentId,
    instrument: &'a InstrumentId,
    purpose: Purpose,
    side: Side,
) -> Result<BindingGateRequest<'a>, Error> {
    Ok(BindingGateRequest {
        agent,
        instrument,
        side,
        qty: Qty::parse("2")?,
        limit: Price::parse("100.05")?,
        purpose,
        tif: TimeInForce::Day,
        protection: None,
    })
}

fn opening(deployment: &Deployment) -> Result<Option<BindingGateInput>, Error> {
    let (agent, spy) = (agent(), spy()?);
    Ok(deployment.input(&request(&agent, &spy, Purpose::Open, Side::Buy)?))
}

/// The gate's verdict on the opening, as the executor evaluates its snapshot.
fn verdict(snapshot: &BindingGateInput) -> Result<(Verdict, Option<ReasonCode>), Error> {
    let proposed = ProposedOrder {
        instrument: snapshot.asset.clone(),
        side: Side::Buy,
        qty: Qty::parse("2")?,
        limit_price: Price::parse("100.05")?,
        kind: ProposedKind::Plain,
        tif: GateTimeInForce::Day,
        extended_hours: false,
        origin: Origin::OrderBuilder,
        owner_confirmed_bid: None,
        client_order_id: snapshot.gate_client_order_id,
        fee_reservation: snapshot.fee_reservation,
    };
    let decision = evaluate(&GateInput {
        now: snapshot.now,
        pass: GatePass::First,
        config: &snapshot.config,
        mandate: &snapshot.mandate,
        risk: &snapshot.risk,
        account: &snapshot.account,
        agent: &snapshot.agent,
        instrument: &snapshot.instrument,
        market: &snapshot.market,
        conduct: &snapshot.conduct,
        universe: &snapshot.universe,
        proposed: &proposed,
    })?;
    Ok((decision.verdict, decision.reason))
}

/// The deployed agent's mandate, instrument and protection come from its confirmed version and
/// registrations: its version, its one covered instrument with its class, grid and fill
/// references, and protection for an opening on it; and that instrument is the one marked.
#[test]
fn a_deployed_agent_is_answered_from_its_reviewed_artifacts() -> Result<(), Error> {
    let (journal, store, version) = deployed()?;
    let (deployment, _) = deployment(&journal, store);
    let (agent, spy) = (agent(), spy()?);
    let other = AgentId(OTHER_AGENT.to_owned());
    assert_eq!(
        deployment.version(&agent).map(|v| v.0),
        Some(version.clone())
    );
    assert_eq!(deployment.version(&other), None);
    assert!(deployment.covers(&agent, &spy));
    assert!(!deployment.covers(&other, &spy));
    assert!(!deployment.covers(&agent, &InstrumentId::new(AAPL_ID)?));
    assert_eq!(deployment.asset_class(&spy), Some(AssetClass::UsEquity));
    assert!(deployment.increment(&spy).is_some());
    assert!(deployment.fill_refs(&spy).is_some());
    assert_eq!(deployment.asset_class(&InstrumentId::new(AAPL_ID)?), None);
    let entry = Price::parse("100")?;
    assert!(matches!(
        deployment.protection(&agent, &spy, entry),
        Ok(Some(_))
    ));
    assert_eq!(deployment.instruments(), BTreeSet::from([spy]));
    Ok(())
}

/// A deployment whose objects the store does not hold answers nothing: no version from the
/// wiring, no instrument, no protection to hand (the gate holds the opening, so none is needed),
/// nothing to mark, and no gate input.
#[test]
fn a_deployment_the_store_cannot_read_answers_nothing() -> Result<(), Error> {
    let (journal, _, _) = deployed()?;
    journaled(&journal)?;
    let (deployment, reads) = deployment(&journal, Artifacts::new());
    let (agent, spy) = (agent(), spy()?);
    assert_eq!(deployment.version(&agent), None);
    assert!(!deployment.covers(&agent, &spy));
    assert_eq!(deployment.asset_class(&spy), None);
    assert_eq!(
        deployment.protection(&agent, &spy, Price::parse("100")?),
        Ok(None)
    );
    assert!(deployment.instruments().is_empty());
    assert!(opening(&deployment)?.is_none());
    assert_eq!(reads.get(), 0, "nothing is read for a deployment unread");
    Ok(())
}

/// An untraded account's opening on every read fact is answered, judged on the agent's whole
/// allocation, which is its equity until it first trades; the gate allows it.
#[test]
fn an_opening_on_every_read_fact_is_allowed() -> Result<(), Error> {
    let (journal, store, version) = deployed()?;
    journaled(&journal)?;
    let (deployment, reads) = deployment(&journal, store);
    let snapshot = opening(&deployment)?.ok_or("an input")?;
    assert_eq!(snapshot.now, at(MONDAY)?);
    assert_eq!(snapshot.asset.as_str(), SPY);
    assert_eq!(snapshot.config_refs.mandate_version, Some(version));
    assert_eq!(snapshot.risk.agent_equity, snapshot.risk.capital_base);
    assert_eq!(snapshot.risk.agent_equity, snapshot.risk.high_water_mark);
    assert_eq!(snapshot.account.model_buying_power, Usd::parse("90000")?);
    assert_eq!(snapshot.feed, "fixture");
    assert!(reads.get() > 0, "the broker's facts and the bars were read");
    assert_eq!(verdict(&snapshot)?, (Verdict::Allow, None));
    Ok(())
}

/// An exit is never answered and never reads the stored bars or the broker's facts, whatever the
/// deployment, so nothing it waits on can hold it (`AGENTS.md` rule 13, DEC-2274 item 6).
#[test]
fn an_exit_never_reads_the_bars_or_the_broker() -> Result<(), Error> {
    let (journal, store, _) = deployed()?;
    journaled(&journal)?;
    let (deployment, reads) = deployment(&journal, store);
    let (agent, spy) = (agent(), spy()?);
    for purpose in [
        Purpose::DiscretionaryExit,
        Purpose::RiskExit,
        Purpose::Protective,
        Purpose::OwnerExit,
        Purpose::Flatten,
    ] {
        assert!(
            deployment
                .input(&request(&agent, &spy, purpose, Side::Sell)?)
                .is_none(),
            "{purpose:?}"
        );
    }
    assert_eq!(reads.get(), 0);
    Ok(())
}

/// Once the account holds a fill, the allocation is no longer the agent's equity, so its opening
/// is held until the account risk view reads the agent's risk.
#[test]
fn a_traded_account_holds_the_opening() -> Result<(), Error> {
    let (journal, store, _) = deployed()?;
    journaled(&journal)?;
    let text = |value: &str| Value::Str(value.to_owned());
    journal.write(
        ACCOUNT_STREAM,
        "a4",
        "FillApplied",
        1,
        None,
        object(vec![
            ("client_order_id", text("md-other")),
            ("fill_id", text("f1")),
            ("instrument_id", text(SPY)),
            ("qty_gross", text("1")),
            ("risk_clock", text("2026-09-21T16:00:00.000000000Z")),
            ("side", text("buy")),
        ])?,
    )?;
    let (deployment, _) = deployment(&journal, store);
    assert!(opening(&deployment)?.is_none());
    Ok(())
}

/// The opening is held while the clock has not ticked, the account has no snapshot, or the
/// facts the journal does not hold are unread.
#[test]
fn an_unread_fact_holds_the_opening() -> Result<(), Error> {
    let (journal, store, _) = deployed()?;
    let (deployment, _) = deployment(&journal, store.clone());
    assert!(opening(&deployment)?.is_none(), "no snapshot or mark");
    journaled(&journal)?;
    let rows = journal.clone();
    let unread = Deployment::new(
        &scope(),
        Sources {
            rows: Box::new(move |stream: &StreamId| mandate_store::Store::rows(&rows, stream)),
            artifacts: Box::new(store),
            quotes: Box::new(Quoting),
            liquidity: Box::new(mandate_executord::deployment::Unread),
            broker: Box::new(mandate_executord::deployment::Unread),
        },
    );
    assert!(opening(&unread)?.is_none(), "no broker facts or bars");
    Ok(())
}

/// An opening the gate holds anyway, another agent's or on another instrument, is handed no
/// protection and never stops the process, whose exits go on (`AGENTS.md` rule 13).
#[test]
fn an_opening_the_deployment_does_not_cover_is_handed_no_protection() -> Result<(), Error> {
    let (journal, store, _) = deployed()?;
    let (deployment, _) = deployment(&journal, store);
    let other = AgentId(OTHER_AGENT.to_owned());
    let entry = Price::parse("100")?;
    assert_eq!(deployment.protection(&other, &spy()?, entry), Ok(None));
    assert_eq!(
        deployment.protection(&agent(), &InstrumentId::new(AAPL_ID)?, entry),
        Ok(None)
    );
    Ok(())
}

/// A run given the production deployment reads its ports from it in place of the fixture's, and
/// keeps its broker's quote feed: the fixture's ports cover nothing and mark nothing, the
/// deployed run's cover and mark the deployed instrument.
#[test]
fn a_deployed_run_reads_its_ports_from_the_deployment() -> Result<(), Error> {
    let (journal, store, _) = deployed()?;
    let broker = || -> Result<_, Error> {
        Ok(mandate_executord::alpaca::AlpacaBroker::new(
            mandate_executord::paper::FixturePaper::new(at(MONDAY)?),
        )?)
    };
    let fixture = mandate_executord::run::fixture(broker()?, &scope())?;
    let (agent, spy) = (agent(), spy()?);
    assert!(!fixture.wiring().mandates.covers(&agent, &spy));
    assert!(fixture.wiring().marked.instruments().is_empty());
    let rows = journal.clone();
    let run = mandate_executord::run::fixture(broker()?, &scope())?.deployed(
        &scope(),
        mandate_executord::deployment::Production {
            rows: Box::new(move |stream: &StreamId| mandate_store::Store::rows(&rows, stream)),
            artifacts: Box::new(store),
            liquidity: Box::new(mandate_executord::deployment::Unread),
        },
    );
    let wiring = run.wiring();
    assert!(wiring.mandates.covers(&agent, &spy));
    assert_eq!(
        wiring.instruments.asset_class(&spy),
        Some(AssetClass::UsEquity)
    );
    assert!(matches!(
        wiring
            .protections
            .protection(&agent, &spy, Price::parse("100")?),
        Ok(Some(_))
    ));
    assert_eq!(wiring.marked.instruments(), BTreeSet::from([spy.clone()]));
    assert_eq!(wiring.quotes.feed(), "none", "the fixture broker's feed");
    assert!(
        wiring
            .gate
            .input(&request(&agent, &spy, Purpose::Open, Side::Buy)?)
            .is_none(),
        "the broker's facts are unread, so the opening is held"
    );
    Ok(())
}

/// An artifact store that counts its reads and can be taken down.
struct Counted {
    store: Artifacts,
    reads: Rc<Cell<u32>>,
    down: Rc<Cell<bool>>,
}

impl ArtifactSource for Counted {
    fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        self.reads.set(self.reads.get() + 1);
        if self.down.get() {
            return Err(ArtifactError::Unavailable);
        }
        self.store.read_artifact(reference)
    }
}

/// The deployment over `journal` and the counted `store`, with every other fact read, and the
/// store's read count and switch.
fn counted(journal: &Journal, store: Artifacts) -> (Deployment, Rc<Cell<u32>>, Rc<Cell<bool>>) {
    let (reads, down) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(false)));
    let rows = journal.clone();
    let deployment = Deployment::new(
        &scope(),
        Sources {
            rows: Box::new(move |stream: &StreamId| mandate_store::Store::rows(&rows, stream)),
            artifacts: Box::new(Counted {
                store,
                reads: Rc::clone(&reads),
                down: Rc::clone(&down),
            }),
            quotes: Box::new(Quoting),
            liquidity: Box::new(Bars(Rc::new(Cell::new(0)))),
            broker: Box::new(Read(Rc::new(Cell::new(0)))),
        },
    );
    (deployment, reads, down)
}

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

/// Another agent's order on the account, never filled, means the account is not flat and its
/// buying power is spoken for, so the opening is held until the account risk view reads them.
#[test]
fn another_agents_order_holds_the_opening() -> Result<(), Error> {
    let (journal, store, _) = deployed()?;
    journaled(&journal)?;
    journal.write(
        ACCOUNT_STREAM,
        "a4",
        "OrderRequestRecorded",
        1,
        None,
        object(vec![
            ("agent_id", text(OTHER_AGENT)),
            ("risk_clock", text("2026-09-21T16:00:00.000000000Z")),
        ])?,
    )?;
    let (deployment, _) = deployment(&journal, store);
    assert!(opening(&deployment)?.is_none());
    Ok(())
}

/// The gate judges only the version the journal has in force: a version applied on the account
/// stream that is not the one the control stream deploys holds the opening, and a rejected one
/// changes nothing.
#[test]
fn a_version_the_journal_does_not_have_in_force_holds_the_opening() -> Result<(), Error> {
    for (result, held) in [("applied", true), ("rejected", false)] {
        let (journal, store, _) = deployed()?;
        journaled(&journal)?;
        journal.write(
            ACCOUNT_STREAM,
            "a4",
            "MandateVersionApplied",
            1,
            None,
            object(vec![
                ("agent_id", text(AGENT)),
                ("new_version", text(&format!("sha256:{}", "9".repeat(64)))),
                ("result", text(result)),
                ("risk_clock", text("2026-09-21T16:00:00.000000000Z")),
            ])?,
        )?;
        let (deployment, _) = deployment(&journal, store);
        assert_eq!(opening(&deployment)?.is_none(), held, "{result}");
    }
    Ok(())
}

/// A stopped agent's opening is held: the journal has no version in force for it.
#[test]
fn a_stopped_agents_opening_is_held() -> Result<(), Error> {
    let (journal, store, _) = deployed()?;
    journaled(&journal)?;
    journal.write(
        CONTROL_STREAM,
        "s1",
        "AgentStopped",
        1,
        None,
        json(&format!(
            r#"{{"agent_id":"{AGENT}","connection_id":"{CONNECTION}","loss_added":"0","retired_on":"2026-09-21"}}"#
        ))?,
    )?;
    let (deployment, _) = deployment(&journal, store);
    assert!(opening(&deployment)?.is_none());
    Ok(())
}

/// A quote that is not sane, or lacks a side, holds the opening.
#[test]
fn a_quote_not_sane_holds_the_opening() -> Result<(), Error> {
    struct Insane(bool);
    impl QuoteFeed for Insane {
        fn observe(
            &self,
            instrument: &InstrumentId,
            now: UtcNanos,
        ) -> Result<MarketObservation, &'static str> {
            Ok(MarketObservation {
                instrument: instrument.clone(),
                bid: Price::parse("99.95").ok(),
                bid_size: None,
                ask: if self.0 {
                    None
                } else {
                    Price::parse("100.05").ok()
                },
                last_trade: None,
                mark: None,
                sane: self.0,
                observed_at: RiskClock::from_secs(now.secs()),
            })
        }

        fn feed(&self) -> &'static str {
            "fixture"
        }
    }
    for one_sided in [false, true] {
        let (journal, store, _) = deployed()?;
        journaled(&journal)?;
        let rows = journal.clone();
        let deployment = Deployment::new(
            &scope(),
            Sources {
                rows: Box::new(move |stream: &StreamId| mandate_store::Store::rows(&rows, stream)),
                artifacts: Box::new(store),
                quotes: Box::new(Insane(one_sided)),
                liquidity: Box::new(Bars(Rc::new(Cell::new(0)))),
                broker: Box::new(Read(Rc::new(Cell::new(0)))),
            },
        );
        assert!(opening(&deployment)?.is_none(), "one sided: {one_sided}");
    }
    Ok(())
}

/// The deployments are read once for a control-stream length and trade date; a longer control
/// stream or an opening on a new date reads them again.
#[test]
fn the_deployment_is_read_again_only_when_the_control_stream_or_the_date_moves() -> Result<(), Error>
{
    let (journal, store, _) = deployed()?;
    journaled(&journal)?;
    let (deployment, reads, _) = counted(&journal, store);
    let (agent, spy) = (agent(), spy()?);
    assert!(deployment.covers(&agent, &spy));
    let first = reads.get();
    assert!(first > 0);
    assert!(deployment.covers(&agent, &spy));
    assert!(opening(&deployment)?.is_some());
    assert_eq!(
        reads.get(),
        first,
        "the same control stream and date read nothing again"
    );
    journal.write(
        CONTROL_STREAM,
        "x1",
        "ConnectionEstablished",
        2,
        None,
        json(r#"{"account_ref":"acct9","broker":"alpaca","connection_id":"conn9","environment":"paper","scopes":["trading"],"user":"user_other"}"#)?,
    )?;
    assert!(deployment.covers(&agent, &spy));
    let second = reads.get();
    assert!(second > first, "a longer control stream reads again");
    tick(&journal, "t2", "2026-09-22T16:00:00Z")?;
    journaled_at(&journal, "b", "2026-09-22T16:00:00.000000000Z")?;
    assert!(opening(&deployment)?.is_some());
    assert!(reads.get() > second, "an opening on a new date reads again");
    Ok(())
}

/// A store outage after the deployment read never takes from the agent what its exits, fills and
/// marks read, while its openings are held; a failed read is never kept, so the deployment reads
/// again once the store answers.
#[test]
fn a_store_outage_holds_openings_and_keeps_what_exits_read() -> Result<(), Error> {
    let (journal, store, version) = deployed()?;
    journaled(&journal)?;
    let (deployment, _, down) = counted(&journal, store);
    let (agent, spy) = (agent(), spy()?);
    assert!(deployment.covers(&agent, &spy));
    down.set(true);
    journal.write(
        CONTROL_STREAM,
        "x1",
        "ConnectionEstablished",
        2,
        None,
        json(r#"{"account_ref":"acct9","broker":"alpaca","connection_id":"conn9","environment":"paper","scopes":["trading"],"user":"user_other"}"#)?,
    )?;
    assert!(
        !deployment.covers(&agent, &spy),
        "an opening reads only what reads now"
    );
    assert_eq!(
        deployment.protection(&agent, &spy, Price::parse("100")?),
        Ok(None)
    );
    assert!(opening(&deployment)?.is_none());
    assert_eq!(deployment.version(&agent).map(|v| v.0), Some(version));
    assert!(
        deployment.fill_refs(&spy).is_some(),
        "a fill is still accounted"
    );
    assert_eq!(deployment.asset_class(&spy), Some(AssetClass::UsEquity));
    assert!(deployment.increment(&spy).is_some());
    assert_eq!(deployment.instruments(), BTreeSet::from([spy.clone()]));
    down.set(false);
    assert!(
        deployment.covers(&agent, &spy),
        "read again once the store answers"
    );
    assert!(opening(&deployment)?.is_some());
    Ok(())
}

/// The untraded agent's inherited loss is the connection's loss carry its deployment was
/// validated with (mandate spec §5.7, MI-14), never zero over a retired agent's loss.
#[test]
fn the_untraded_agent_inherits_the_connections_loss_carry() -> Result<(), Error> {
    let (journal, store, _) = deployed_as(Variant {
        retired_loss: Some("25"),
        ..Variant::default()
    })?;
    journaled(&journal)?;
    let (deployment, _) = deployment(&journal, store);
    let snapshot = opening(&deployment)?.ok_or("an input")?;
    assert_eq!(snapshot.risk.inherited_loss, Usd::parse("25")?);
    Ok(())
}

/// The mandate's crypto stop-limit offset reaches the mandate port for its agent only.
#[test]
fn the_crypto_stop_limit_offset_is_the_mandates() -> Result<(), Error> {
    let (journal, store, _) = deployed_as(Variant {
        offset: Some("0.02"),
        ..Variant::default()
    })?;
    let (deployment, _) = deployment(&journal, store);
    assert_eq!(
        deployment.crypto_stop_limit_offset(&agent()),
        Some(mandate_num::Fraction::parse("0.02")?)
    );
    assert_eq!(
        deployment.crypto_stop_limit_offset(&AgentId(OTHER_AGENT.to_owned())),
        None
    );
    assert_eq!(deployment.exit_tier(&spy()?), None);
    Ok(())
}

/// Nothing read answers nothing.
#[test]
fn unread_answers_nothing() -> Result<(), Error> {
    let unread = mandate_executord::deployment::Unread;
    assert_eq!(BrokerFacts::asset(&unread, "SPY"), None);
    assert_eq!(BrokerFacts::rules(&unread), None);
    assert_eq!(BrokerFacts::halted(&unread, "SPY"), None);
    assert_eq!(LiquidityRead::liquidity(&unread, "SPY", at(MONDAY)?), None);
    Ok(())
}

/// The wiring the deployment replaces answers the binding gate from it: with every fact read,
/// the untraded agent's opening reaches the gate.
#[test]
fn the_wired_gate_reads_the_deployment() -> Result<(), Error> {
    let (journal, store, _) = deployed()?;
    journaled(&journal)?;
    let rows = journal.clone();
    let mut fixture = mandate_executord::fixture::wiring(&scope())?;
    fixture.quotes = Box::new(Quoting);
    let wiring = Deployment::wired(
        fixture,
        &scope(),
        Box::new(move |stream: &StreamId| mandate_store::Store::rows(&rows, stream)),
        Box::new(store),
        (
            Box::new(Bars(Rc::new(Cell::new(0)))),
            Box::new(Read(Rc::new(Cell::new(0)))),
        ),
    );
    let (agent, spy) = (agent(), spy()?);
    assert!(
        wiring
            .gate
            .input(&request(&agent, &spy, Purpose::Open, Side::Buy)?)
            .is_some()
    );
    Ok(())
}

/// A pod reads the deployment only when it mounts the artifact store: none without it, a refusal
/// for one named and missing, and the store with the bars when both are named.
#[test]
fn production_reads_the_mounted_store() -> Result<(), Error> {
    let none = || -> Result<mandate_executord::deployment::Rows, &'static str> {
        Ok(Box::new(|_: &StreamId| None))
    };
    let unset = mandate_executord::deployment::production(|_| None, none)?;
    assert!(unset.is_none());
    let dir = std::env::temp_dir().join(format!("executord-production-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let missing = dir.to_string_lossy().into_owned();
    assert!(
        mandate_executord::deployment::production(
            |name| (name == "MANDATE_ARTIFACTS").then(|| missing.clone()),
            none
        )
        .is_err()
    );
    mandate_artifacts_fs::FsArtifactStore::open(&dir)?;
    let mounted = mandate_executord::deployment::production(
        |name| (name == "MANDATE_ARTIFACTS").then(|| missing.clone()),
        none,
    )?;
    assert!(mounted.is_some());
    std::fs::remove_dir_all(&dir).ok();
    Ok(())
}

/// The stored bars are read by `mandate-bars` under the pod's root (DEC-2274): the figures it
/// gives for the symbol at the clock, and none for a symbol with no datasets.
#[test]
fn stored_bars_read_the_datasets_under_the_root() -> Result<(), Error> {
    let root = std::env::temp_dir().join(format!("executord-bars-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    let bar = |start: UtcNanos, close: &str, volume: &str| -> Result<Bar, Error> {
        let price = mandate_canon::DecStr::parse(close)?;
        Ok(Bar {
            start,
            open: price.clone(),
            high: price.clone(),
            low: price.clone(),
            close: price.clone(),
            volume: mandate_canon::DecStr::parse(volume)?,
            vwap: price,
            trade_count: 10,
        })
    };
    let dataset = |timeframe: &str| -> Result<DatasetId, Error> {
        Ok(DatasetId::new(
            Class::UsEquity,
            Feed::Iex,
            Kind::Bars(timeframe.parse()?),
            Symbol::parse("SPY")?,
        )?)
    };
    let calendar = ExchangeCalendar::us_equities()?;
    let mut day = Date::new(2026, 8, 1)?;
    let mut days = Vec::new();
    while day <= Date::new(2026, 9, 18)? {
        if calendar.is_trading_day(day)? {
            days.push(day);
        }
        day = day.next()?;
    }
    let days = days.split_off(days.len() - 30);
    let store = Datasets::new(&root);
    for (n, day) in days.into_iter().enumerate() {
        let bars = vec![bar(
            new_york_midnight(day)?,
            &format!("{}.5", 100 + n % 7),
            &format!("{}", 1_000_000 + n * 10_000),
        )?];
        store.put_day(&dataset("1Day")?, day, &Records::Bars(bars))?;
    }
    let open = at("2026-09-21T13:30:00Z")?.secs();
    let minutes = (0..390_i64)
        .map(|n| {
            bar(
                UtcNanos::from_parts(open + 60 * n, 0)?,
                "110",
                &(100 + n).to_string(),
            )
        })
        .collect::<Result<Vec<Bar>, Error>>()?;
    store.put_day(
        &dataset("1Min")?,
        Date::parse("2026-09-21")?,
        &Records::Bars(minutes),
    )?;
    let now = at(MONDAY)?;
    let expected = mandate_bars::liquidity(&root, "SPY", now)?;
    let read = mandate_executord::deployment::StoredBars { root: root.clone() };
    assert_eq!(
        LiquidityRead::liquidity(&read, "SPY", now),
        Some(Liquidity {
            prior_close: expected.prior_close,
            median_dollar_volume_20d: expected.median_dollar_volume_20d,
            adv_20d: expected.adv_20d,
            trailing_5m_volume: expected.trailing_5m_volume,
        })
    );
    assert_eq!(LiquidityRead::liquidity(&read, "QQQ", now), None);
    std::fs::remove_dir_all(&root).ok();
    Ok(())
}
