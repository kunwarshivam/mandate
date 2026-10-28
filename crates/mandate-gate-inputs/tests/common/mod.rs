//! The deployed SPY version's control stream and store, shared by the deployment tests.
#![allow(
    dead_code,
    reason = "each test binary uses only part of the shared deployment"
)]

use std::collections::BTreeMap;

use mandate_canon::{Digest, Value, to_canonical};
use mandate_domain::Environment;
use mandate_gate_inputs::control::{
    Configuration, ConfirmedVersion, ControlRecord, LiveAdmission, Origin, Pinned, RunFacts,
    configuration, confirmed_version,
};
use mandate_spec::context::{AgentId, Membership};
use mandate_time::Date;

pub const MANDATE: &str = include_str!("../../../mandate-shell/tests/fixtures/tracer/mandate.json");
pub const FEE: &str =
    include_str!("../../../mandate-shell/tests/fixtures/tracer/config/fee-config.json");
pub const CALENDAR: &str =
    include_str!("../../../mandate-shell/tests/fixtures/tracer/config/trading-calendar.json");
pub const RULES: &str =
    include_str!("../../../mandate-shell/tests/fixtures/tracer/config/rule-set.json");
const E7_7_MODEL: &[u8] = b"{\"id\":\"quant.ma_crossover\",\"version\":\"1.0.0\"}\n";
pub const CONTENT: &str =
    r#"{"kind":"quant_model_content","model_id":"quant.ma_crossover","model_version":"1.0.0"}"#;
pub const AAPL: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
pub const SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";
pub const SNAPSHOT: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-09-21T00:00:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b28f4066-5c6d-479b-a2af-85dc1a8f16fb","symbol":"SPY"}"#;
const ENVELOPE: &str = "autonomy behavior capital connection_id environment goal name notifications protection risk universe";

pub type Store = BTreeMap<Digest, Vec<u8>>;

pub fn text<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

fn json(text_: &str) -> Result<Value, String> {
    mandate_canon::parse(text_.as_bytes()).map_err(text)
}

pub fn reference(bytes: &[u8]) -> String {
    format!("sha256:{}", Digest::of(bytes).to_hex())
}

pub fn origin() -> Origin {
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
pub fn deployed(snapshot: &str) -> Result<(Vec<ControlRecord>, Store), String> {
    let mandate = MANDATE
        .replace(AAPL, SPY)
        .replace(r#""AAPL""#, r#""SPY""#)
        .replace(
            &Digest::of(E7_7_MODEL).to_hex(),
            &Digest::of(CONTENT.as_bytes()).to_hex(),
        );
    deployed_with(
        &mandate,
        &[snapshot],
        &[("quant.ma_crossover", "1.0.0", CONTENT, false)],
    )
}

/// The control stream of `mandate` deployed, and its store: registrations of each of `models` as
/// `(id, version, content, admits_instruments)` under its content's hash, then the fee, calendar
/// and rule set, then each of `snapshots`, then the version created, confirmed and deployed.
pub fn deployed_with(
    mandate: &str,
    snapshots: &[&str],
    models: &[(&str, &str, &str, bool)],
) -> Result<(Vec<ControlRecord>, Store), String> {
    let mut store = Store::new();
    let document = to_canonical(&json(mandate)?);
    let version = reference(&document);
    store.insert(Digest::of(&document), document);
    let paths: Vec<String> = ENVELOPE.split(' ').map(|p| format!("/{p}")).collect();
    let provenance: Vec<String> = paths
        .iter()
        .map(|path| format!(r#"{{"path":"{path}","source":"user_entered"}}"#))
        .collect();
    let confirmed: Vec<String> = paths.iter().map(|path| format!(r#""{path}""#)).collect();
    let mut payloads = Vec::new();
    for (id, model_version, content, admits) in models {
        let params = if *admits {
            "[]"
        } else {
            r#"["fast_periods","slow_periods"]"#
        };
        payloads.push((
            "ConfigSnapshotRegistered",
            format!(
                r#"{{"admits_instruments":{admits},"content_hash":"{}","kind":"model_version","model_id":"{id}","model_version":"{model_version}","params":{params}}}"#,
                put(&mut store, content)
            ),
        ));
    }
    let configs = [
        ("fee_config", FEE.trim()),
        ("trading_calendar", CALENDAR.trim()),
        ("rule_set", RULES.trim()),
    ];
    let snapshots = snapshots
        .iter()
        .map(|snapshot| ("instrument_snapshot", *snapshot));
    for (kind, object) in configs.into_iter().chain(snapshots) {
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
            event_time: None,
            event_type: event_type.to_owned(),
            payload: json(&payload)?,
            origin: origin(),
        });
    }
    Ok((records, store))
}

pub fn inputs(snapshot: &str) -> Result<(ConfirmedVersion, Configuration), String> {
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
