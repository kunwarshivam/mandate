//! One deployment's reviewed artifacts, read from its control stream and the store
//! (DEC-2380 item 5, DEC-505).

use std::collections::BTreeMap;

use mandate_canon::{Digest, Value, to_canonical};
use mandate_domain::Environment;
use mandate_gate_inputs::control::{
    Configuration, ConfirmedVersion, ControlRecord, LiveAdmission, Origin, Pinned, RunFacts,
    SnapshotExchange, configuration, confirmed_version,
};
use mandate_gate_inputs::{InputError, Reviewed};
use mandate_num::ShareIncrement;
use mandate_risk::{EtpClass, Exchange};
use mandate_spec::context::{AgentId, Membership};
use mandate_time::{Date, UtcNanos};

const MANDATE: &str = include_str!("../../mandate-shell/tests/fixtures/tracer/mandate.json");
const FEE: &str = include_str!("../../mandate-shell/tests/fixtures/tracer/config/fee-config.json");
const CALENDAR: &str =
    include_str!("../../mandate-shell/tests/fixtures/tracer/config/trading-calendar.json");
const RULES: &str = include_str!("../../mandate-shell/tests/fixtures/tracer/config/rule-set.json");
const E7_7_MODEL: &[u8] = b"{\"id\":\"quant.ma_crossover\",\"version\":\"1.0.0\"}\n";
const CONTENT: &str =
    r#"{"kind":"quant_model_content","model_id":"quant.ma_crossover","model_version":"1.0.0"}"#;
const AAPL: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
const SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";
const SNAPSHOT: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-09-21T00:00:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b28f4066-5c6d-479b-a2af-85dc1a8f16fb","symbol":"SPY"}"#;
const ENVELOPE: &str = "autonomy behavior capital connection_id environment goal name notifications protection risk universe";

type Store = BTreeMap<Digest, Vec<u8>>;

fn text<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

fn json(text_: &str) -> Result<Value, String> {
    mandate_canon::parse(text_.as_bytes()).map_err(text)
}

fn reference(bytes: &[u8]) -> String {
    format!("sha256:{}", Digest::of(bytes).to_hex())
}

fn origin() -> Origin {
    Origin {
        stream_id: "ctl:ws_spy".to_owned(),
        environment: Environment::Paper,
    }
}

fn put(store: &mut Store, object: &str) -> String {
    store.insert(Digest::of(object.as_bytes()), object.as_bytes().to_vec());
    reference(object.as_bytes())
}

/// The control stream of a deployed SPY version whose snapshot is `snapshot`, and its store:
/// registrations of the model, fee, calendar, rule set and snapshot, then the version created,
/// confirmed and deployed.
fn deployed(snapshot: &str) -> Result<(Vec<ControlRecord>, Store), String> {
    let mut store = Store::new();
    let mandate = MANDATE
        .replace(AAPL, SPY)
        .replace(r#""AAPL""#, r#""SPY""#)
        .replace(
            &Digest::of(E7_7_MODEL).to_hex(),
            &Digest::of(CONTENT.as_bytes()).to_hex(),
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
        ("instrument_snapshot", snapshot),
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
            r#"{{"agent_id":"agent_spy","mandate_version":"{version}","record_ref":"sha256:{}"}}"#,
            "6".repeat(64)
        ),
    ));
    let mut records = Vec::new();
    for (seq, (event_type, payload)) in (1_u64..).zip(payloads) {
        records.push(ControlRecord {
            seq,
            event_type: event_type.to_owned(),
            payload: json(&payload)?,
            origin: origin(),
        });
    }
    Ok((records, store))
}

fn inputs(snapshot: &str) -> Result<(ConfirmedVersion, Configuration), String> {
    let (records, store) = deployed(snapshot)?;
    let day = Date::parse("2026-09-28").map_err(text)?;
    let run = RunFacts {
        agent: AgentId::new("agent_spy"),
        validation_date: day,
        membership: Membership {
            workspace_users: 1,
            approver_users: 1,
        },
        live: LiveAdmission::refusing(),
    };
    let confirmed = confirmed_version(&records, &store, &run).map_err(|e| format!("{e:?}"))?;
    let model = confirmed
        .mandate()
        .behavior
        .signal_models
        .first()
        .ok_or("a model")?;
    let pinned = Pinned {
        asset_id: SPY.to_owned(),
        symbol: "SPY".to_owned(),
        model_id: model.id.as_str().to_owned(),
        model_version: model.version.clone(),
        content_hash: model.content_hash,
    };
    let config = configuration(&records, &store, &pinned, day).map_err(|e| format!("{e:?}"))?;
    Ok((confirmed, config))
}

fn refusal(result: Result<Reviewed, InputError>) -> Option<&'static str> {
    match result {
        Err(InputError::Missing { what }) => Some(what),
        Err(_) | Ok(_) => None,
    }
}

#[test]
fn the_deployment_binds_its_registered_instrument_and_objects() -> Result<(), String> {
    let (confirmed, config) = inputs(SNAPSHOT)?;
    let reviewed = Reviewed::from_registered(&confirmed, &config).map_err(text)?;
    let instrument = &reviewed.instrument;
    assert_eq!(instrument.asset_id.as_str(), SPY);
    assert_eq!(instrument.symbol.as_str(), "SPY");
    assert_eq!(instrument.exchange, SnapshotExchange::Arca);
    assert_eq!(instrument.gate_exchange, Exchange::Arca);
    assert_eq!(instrument.increment, ShareIncrement::Whole);
    assert_eq!(instrument.increment_qty.to_string(), "1");
    assert_eq!(instrument.etp, EtpClass::Plain);
    assert_eq!(
        instrument.etp_classified_at,
        UtcNanos::parse_rfc3339("2026-09-21T00:00:00Z").map_err(text)?
    );
    assert_eq!(reviewed.model_id, "quant.ma_crossover");
    assert_eq!(reviewed.model_version, "1.0.0");
    assert_eq!(reviewed.model_hash, Digest::of(CONTENT.as_bytes()));
    assert_eq!(reviewed.quote_max_age.as_secs(), 10);
    assert_eq!(reviewed.gate_config.close_window_minutes, 10);
    assert_eq!(reviewed.executor_config.exit_step_s, 5);
    let refs = &reviewed.config_refs;
    assert_eq!(refs.fee_config, Some(reference(FEE.trim().as_bytes())));
    assert_eq!(
        refs.trading_calendar,
        Some(reference(CALENDAR.trim().as_bytes()))
    );
    assert_eq!(
        refs.instrument_snapshot,
        Some(reference(SNAPSHOT.as_bytes()))
    );
    assert_eq!(refs.rule_set, Some(reference(RULES.trim().as_bytes())));
    let canonical = reviewed.mandate.canonical_bytes().map_err(text)?;
    assert_eq!(refs.mandate_version, Some(reference(&canonical)));
    Ok(())
}

#[test]
fn a_nasdaq_listing_reaches_the_gate_as_nasdaq() -> Result<(), String> {
    let (confirmed, config) = inputs(&SNAPSHOT.replace("\"arca\"", "\"nasdaq\""))?;
    let reviewed = Reviewed::from_registered(&confirmed, &config).map_err(text)?;
    assert_eq!(reviewed.instrument.exchange, SnapshotExchange::Nasdaq);
    assert_eq!(reviewed.instrument.gate_exchange, Exchange::Nasdaq);
    Ok(())
}

#[test]
fn a_binding_from_another_stream_is_refused() -> Result<(), String> {
    let other = Origin {
        stream_id: "ctl:ws_other".to_owned(),
        environment: Environment::Paper,
    };
    let (confirmed, config) = inputs(SNAPSHOT)?;
    let mut each: Vec<Configuration> = Vec::new();
    for index in 0..5 {
        let mut changed = config.clone();
        let registered = match index {
            0 => &mut changed.model_version,
            1 => &mut changed.fee_config,
            2 => &mut changed.trading_calendar,
            3 => &mut changed.rule_set,
            _ => &mut changed.instrument_snapshot,
        };
        registered.origin = other.clone();
        each.push(changed);
    }
    for changed in each {
        assert_eq!(
            refusal(Reviewed::from_registered(&confirmed, &changed)),
            Some("every binding journaled on the deployment's own control stream")
        );
    }
    Ok(())
}

#[test]
fn another_model_is_refused() -> Result<(), String> {
    let (confirmed, config) = inputs(SNAPSHOT)?;
    for content in [
        CONTENT.replace("quant_model_content", "other_content"),
        CONTENT.replace("\"quant.ma_crossover\"", "\"quant.other\""),
        CONTENT.replace("\"1.0.0\"", "\"1.0.1\""),
    ] {
        let mut changed = config.clone();
        changed.model_version.bytes = content.into_bytes();
        assert_eq!(
            refusal(Reviewed::from_registered(&confirmed, &changed)),
            Some("the registered model")
        );
    }
    let mut rehashed = config.clone();
    rehashed.model_version.content_hash = Digest::of(b"another");
    assert_eq!(
        refusal(Reviewed::from_registered(&confirmed, &rehashed)),
        Some("the registered model")
    );
    let mut unreadable = config;
    unreadable.model_version.bytes = b"not json".to_vec();
    assert_eq!(
        refusal(Reviewed::from_registered(&confirmed, &unreadable)),
        Some("the registered model content")
    );
    Ok(())
}

#[test]
fn another_instrument_is_refused() -> Result<(), String> {
    let (confirmed, config) = inputs(SNAPSHOT)?;
    let mut symbol = config.clone();
    symbol.instrument.symbol = "QQQ".to_owned();
    let mut asset = config;
    asset.instrument.instrument_id = AAPL.to_owned();
    for changed in [symbol, asset] {
        assert_eq!(
            refusal(Reviewed::from_registered(&confirmed, &changed)),
            Some("the registered instrument")
        );
    }
    Ok(())
}

#[test]
fn the_fee_schedule_must_name_the_mandates_environment() -> Result<(), String> {
    let (confirmed, config) = inputs(SNAPSHOT)?;
    let live = FEE.trim().replace("\"paper\"", "\"live\"");
    let mut changed = config.clone();
    changed.fee_config.bytes = live.into_bytes();
    assert!(Reviewed::from_registered(&confirmed, &changed).is_err());
    let mut rules = config;
    rules.rule_set.bytes = RULES.trim().replace("\"E7-7\"", "\"E7-8\"").into_bytes();
    assert_eq!(
        refusal(Reviewed::from_registered(&confirmed, &rules)),
        Some("story")
    );
    Ok(())
}
