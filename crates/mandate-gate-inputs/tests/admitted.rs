//! DEC-2819 item 3: a research deployment's reviewed artifacts. An unpinned mandate names no
//! instrument; an instrument the admission admitted is read from its own snapshot registered on the
//! deployment's stream, its symbol taken from the snapshot, and the deployment's model is the one
//! that admits instruments. A pinned mandate is read only as before, and an unpinned one never
//! through the pinned reading.

mod common;

use common::{SNAPSHOT, SPY, Store, deployed, deployed_with, origin, text};
use mandate_canon::Digest;
use mandate_domain::Environment;
use mandate_gate_inputs::Reviewed;
use mandate_gate_inputs::control::{
    Admitted, ConfigRefusal, ConfirmedVersion, ControlRecord, LiveAdmission, Pinned, RunFacts,
    admitted_configuration, configuration, confirmed_version,
};
use mandate_spec::context::{AgentId, Membership};
use mandate_time::Date;

const RESEARCH: &str = include_str!("../../../deploy/cell/demo/research-mandate.json");
const LLM_HASH: &str = "40f446f7858575d2c66a223f747348b3a91ef83e844e13815e527f4d181e5283";
const QUANT_HASH: &str = "cf36ebf8746a95c79443a2a0b5b49762809e082bced8a0a5bfdda7a3dd6c469d";
const LLM: &str =
    r#"{"kind":"research_model_content","model_id":"llm.research_agent","model_version":"0.1.0"}"#;
const QUANT: &str =
    r#"{"kind":"quant_model_content","model_id":"quant.ma_crossover","model_version":"1.0.0"}"#;
const QQQ_SNAPSHOT: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-09-21T00:00:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"nasdaq","increment":"whole","instrument_id":"eb82ffb0-efd6-4f4e-9307-8fead28e04f6","symbol":"QQQ"}"#;

fn day() -> Result<Date, String> {
    Date::parse("2026-09-28").map_err(text)
}

/// The demo's research version deployed, each of its models registered under the tests' content,
/// with `snapshots` registered.
fn research(snapshots: &[&str]) -> Result<(Vec<ControlRecord>, Store), String> {
    let mandate = RESEARCH
        .replace(LLM_HASH, &Digest::of(LLM.as_bytes()).to_hex())
        .replace(QUANT_HASH, &Digest::of(QUANT.as_bytes()).to_hex());
    deployed_with(
        &mandate,
        snapshots,
        &[
            ("llm.research_agent", "0.1.0", LLM, true),
            ("quant.ma_crossover", "1.0.0", QUANT, false),
        ],
    )
}

fn confirmed(records: &[ControlRecord], store: &Store) -> Result<ConfirmedVersion, String> {
    let run = RunFacts {
        agent: AgentId::new("agent_spy"),
        validation_date: day()?,
        membership: Membership {
            workspace_users: 1,
            approver_users: 1,
        },
        live: LiveAdmission::refusing(),
    };
    confirmed_version(records, store, &run).map_err(|e| format!("{e:?}"))
}

fn admitted(asset_id: &str) -> Admitted {
    Admitted {
        asset_id: asset_id.to_owned(),
        model_id: "llm.research_agent".to_owned(),
        model_version: "0.1.0".to_owned(),
        content_hash: Digest::of(LLM.as_bytes()),
    }
}

/// The admitted instrument's configuration reads its own registered snapshot, symbol included, and
/// the admitting model's registration; a snapshot of another instrument is passed over.
#[test]
fn an_admitted_instruments_configuration_reads_its_own_snapshot() -> Result<(), String> {
    let (records, store) = research(&[QQQ_SNAPSHOT, SNAPSHOT])?;
    let configured = admitted_configuration(&records, &store, &admitted(SPY), day()?)
        .map_err(|e| e.code().to_owned())?;
    assert_eq!(configured.instrument.instrument_id, SPY);
    assert_eq!(configured.instrument.symbol, "SPY");
    assert_eq!(
        configured.instrument_snapshot.content_hash,
        Digest::of(SNAPSHOT.as_bytes())
    );
    assert_eq!(
        configured.model_version.content_hash,
        Digest::of(LLM.as_bytes())
    );
    assert_eq!(configured.instrument_snapshot.origin, origin());
    Ok(())
}

/// An admitted instrument with no snapshot of its own is unregistered, never another's snapshot;
/// a model the mandate does not admit through is no registration of the admitting model.
#[test]
fn an_admitted_instrument_without_its_snapshot_or_model_is_unregistered() -> Result<(), String> {
    let (records, store) = research(&[QQQ_SNAPSHOT])?;
    assert_eq!(
        admitted_configuration(&records, &store, &admitted(SPY), day()?),
        Err(ConfigRefusal::Unregistered {
            kind: "instrument_snapshot"
        })
    );
    let (records, store) = research(&[SNAPSHOT])?;
    let quant = Admitted {
        model_id: "llm.research_agent".to_owned(),
        model_version: "9.9.9".to_owned(),
        ..admitted(SPY)
    };
    assert_eq!(
        admitted_configuration(&records, &store, &quant, day()?),
        Err(ConfigRefusal::Unregistered {
            kind: "model_version"
        })
    );
    Ok(())
}

/// A research version is reviewed for its admitted instrument, with the admitting model as its
/// model, and never through the pinned reading; a pinned version is never reviewed as admitted.
#[test]
fn a_research_version_is_reviewed_for_its_admitted_instrument_only() -> Result<(), String> {
    let (records, store) = research(&[SNAPSHOT])?;
    let version = confirmed(&records, &store)?;
    let configured = admitted_configuration(&records, &store, &admitted(SPY), day()?)
        .map_err(|e| e.code().to_owned())?;
    let reviewed = Reviewed::from_admitted(&version, &configured).map_err(text)?;
    assert_eq!(reviewed.instrument.asset_id.as_str(), SPY);
    assert_eq!(reviewed.instrument.symbol.as_str(), "SPY");
    assert_eq!(reviewed.model_id, "llm.research_agent");
    assert_eq!(reviewed.model_version, "0.1.0");
    assert_eq!(reviewed.mandate.environment, Environment::Paper);
    assert!(
        Reviewed::from_registered(&version, &configured).is_err(),
        "an unpinned version has no pinned instrument to be reviewed for"
    );
    let mut other_model = configured.clone();
    other_model.model_version.content_hash = Digest::of(b"another model");
    assert!(
        Reviewed::from_admitted(&version, &other_model).is_err(),
        "the registration must be the admitting model's own"
    );
    let (records, store) = deployed(SNAPSHOT)?;
    let pinned_version = confirmed(&records, &store)?;
    let model = pinned_version
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
    let configured =
        configuration(&records, &store, &pinned, day()?).map_err(|e| e.code().to_owned())?;
    assert!(
        Reviewed::from_admitted(&pinned_version, &configured).is_err(),
        "a pinned version is read only as pinned"
    );
    Ok(())
}
