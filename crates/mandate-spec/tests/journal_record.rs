//! `JournaledFact::from_record` (E7-10, journal spec v0.7 §9.2's mapping table): the fact each
//! journal record maps to, so `ValidationContext::from_journal` has a production source (DEC-168,
//! DEC-169).
//!
//! The records are written out here from §9.2's member tables, not read from the test vectors, so
//! this oracle is independent of `mandate-refcases`' run over the vectors' `journaled_facts`. Every
//! test maps at least one record to a fact it must construct, so a stub that maps nothing, or
//! refuses everything, fails each of them.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{base, obj, s, with};
use mandate_canon::{Digest, Value, to_canonical};
use mandate_domain::{AssetId, Environment};
use mandate_num::Usd;
use mandate_spec::context::{AgentId, JournaledFact};
use mandate_spec::document::{ConnectionId, ModelId, Pointer, Source};
use mandate_spec::validate::RegisteredModel;
use mandate_spec::{Mandate, MandateVersion, SpecError};
use mandate_time::Date;

const CONNECTION: &str = "conn_alpaca_paper_01";
const MODEL_HASH: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const DISCLOSURE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const RECORD: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const ASSET_A: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
const ASSET_B: &str = "7b4a1c2e-3333-4a2b-9c3d-000000000003";

fn sha(hex: &str) -> Value {
    s(&format!("sha256:{hex}"))
}

fn digest(hex: &str) -> Result<Digest, SpecError> {
    Digest::from_hex(hex).ok_or(SpecError::InvalidInput { what: "hex" })
}

fn connection() -> Result<ConnectionId, SpecError> {
    Ok(ConnectionId::parse(CONNECTION)?)
}

fn texts(items: &[&str]) -> Value {
    Value::Array(items.iter().map(|t| s(t)).collect())
}

fn nothing_stored(_: &Digest) -> Option<Value> {
    None
}

/// The record's fact, with no document stored and no account connection: enough for every
/// control-stream record but a deployment.
fn mapped(event_type: &str, payload: &Value) -> Result<Option<JournaledFact>, SpecError> {
    JournaledFact::from_record(event_type, payload, &nothing_stored, None)
}

/// The test base mandate, parsed, and its version as a record names it.
fn document() -> Result<(Value, Mandate, MandateVersion, Value), SpecError> {
    let value = base();
    let mandate = Mandate::parse(&value)?;
    let version = mandate.version()?;
    let named = s(&format!("sha256:{}", version.digest().to_hex()));
    Ok((value, mandate, version, named))
}

#[test]
#[ignore = "pending E7-10"]
fn each_connection_and_disclosure_record_maps_to_its_fact() -> Result<(), SpecError> {
    let established = obj(vec![
        ("connection_id", s(CONNECTION)),
        ("broker", s("alpaca")),
        ("environment", s("live")),
        ("scopes", texts(&["account:write", "trading"])),
    ]);
    assert_eq!(
        mapped("ConnectionEstablished", &established)?,
        Some(JournaledFact::ConnectionEstablished {
            connection_id: connection()?,
            environment: Environment::Live,
        }),
        "the environment is the record's, `live` here so no default can pass"
    );
    assert_eq!(
        mapped(
            "ConnectionRevoked",
            &obj(vec![("connection_id", s(CONNECTION))])
        )?,
        Some(JournaledFact::ConnectionRevoked {
            connection_id: connection()?,
        })
    );
    let accepted = obj(vec![
        ("document", s("leveraged_etp")),
        ("version", sha(DISCLOSURE)),
        ("user", s("user_owner_01")),
        (
            "step_up",
            obj(vec![
                ("assertion_id", s("assert_1")),
                ("authenticated_at", s("2026-09-20T13:06:50.000000000Z")),
                ("method", s("webauthn")),
            ]),
        ),
    ]);
    assert_eq!(
        mapped("DisclosureAccepted", &accepted)?,
        Some(JournaledFact::DisclosureAccepted {
            version: digest(DISCLOSURE)?,
        })
    );
    Ok(())
}

#[test]
#[ignore = "pending E7-10"]
fn a_model_registration_maps_and_any_other_snapshot_does_not() -> Result<(), SpecError> {
    let registration = |kind: &str, model: bool| {
        let model_member = |value: Value| if model { value } else { Value::Null };
        obj(vec![
            ("kind", s(kind)),
            ("content_hash", sha(MODEL_HASH)),
            ("model_id", model_member(s("quant.momentum"))),
            ("model_version", model_member(s("1.2.0"))),
            (
                "params",
                if model {
                    texts(&["fast_bars", "lookback_bars"])
                } else {
                    texts(&[])
                },
            ),
            ("admits_instruments", model_member(Value::Bool(true))),
        ])
    };
    assert_eq!(
        mapped(
            "ConfigSnapshotRegistered",
            &registration("model_version", true)
        )?,
        Some(JournaledFact::ModelRegistered {
            id: ModelId::parse("quant.momentum")?,
            model: RegisteredModel {
                version: "1.2.0".to_owned(),
                content_hash: digest(MODEL_HASH)?,
                params: BTreeSet::from(["fast_bars".to_owned(), "lookback_bars".to_owned()]),
                admits_instruments: true,
            },
        })
    );
    for kind in [
        "fee_config",
        "trading_calendar",
        "settlement_calendar",
        "instrument_snapshot",
        "rule_set",
        "mandate_version",
    ] {
        assert_eq!(
            mapped("ConfigSnapshotRegistered", &registration(kind, false))?,
            None,
            "a {kind} snapshot registers no model"
        );
    }
    Ok(())
}

#[test]
#[ignore = "pending E7-10"]
fn the_mandate_records_map_their_version_provenance_and_confirmation() -> Result<(), SpecError> {
    let (_, _, version, named) = document()?;
    let created = obj(vec![
        ("mandate_version", named.clone()),
        (
            "provenance",
            Value::Array(vec![
                obj(vec![
                    ("path", s("/autonomy")),
                    ("source", s("user_entered")),
                ]),
                obj(vec![("path", s("/goal")), ("source", s("user_stated"))]),
                obj(vec![
                    ("path", s("/risk/max_order_usd")),
                    ("source", s("platform_proposed")),
                ]),
            ]),
        ),
        ("record_ref", sha(RECORD)),
    ]);
    assert_eq!(
        mapped("MandateVersionCreated", &created)?,
        Some(JournaledFact::MandateVersionCreated {
            version,
            sources: BTreeMap::from([
                (Pointer::new("/autonomy"), Source::UserEntered),
                (Pointer::new("/goal"), Source::UserStated),
                (
                    Pointer::new("/risk/max_order_usd"),
                    Source::PlatformProposed
                ),
            ]),
        })
    );
    let confirmed = obj(vec![
        ("mandate_version", named),
        ("confirmed_paths", texts(&["/autonomy", "/goal"])),
        ("record_ref", sha(RECORD)),
    ]);
    assert_eq!(
        mapped("MandateConfirmed", &confirmed)?,
        Some(JournaledFact::MandateConfirmed {
            version,
            confirmed_paths: BTreeSet::from([Pointer::new("/autonomy"), Pointer::new("/goal")]),
        })
    );
    Ok(())
}

/// The deployment's connection, environment, allocation, and pinned instruments are the stored
/// document's (§9.2): the record names only the agent and the version.
#[test]
#[ignore = "pending E7-10"]
fn a_deployment_is_read_from_its_stored_document() -> Result<(), SpecError> {
    let (value, _, _, named) = document()?;
    let stored = value.clone();
    let documents = move |wanted: &Digest| {
        (Digest::of(&to_canonical(&stored)) == *wanted).then(|| stored.clone())
    };
    let deployed = obj(vec![
        ("agent_id", s("agent_a")),
        ("mandate_version", named),
        ("record_ref", sha(RECORD)),
    ]);
    assert_eq!(
        JournaledFact::from_record("AgentDeployed", &deployed, &documents, None)?,
        Some(JournaledFact::AgentVersionActive {
            agent: AgentId::new("agent_a"),
            connection_id: connection()?,
            environment: Environment::Paper,
            allocation_usd: Usd::parse("10000")?,
            pinned: BTreeSet::from([AssetId::parse(ASSET_A)?, AssetId::parse(ASSET_B)?]),
        })
    );
    assert_eq!(
        JournaledFact::from_record("AgentDeployed", &deployed, &nothing_stored, None)
            .map_err(|e| e.code()),
        Err("invalid_input"),
        "a deployment whose document is not stored is refused, never skipped"
    );
    let other = with("/capital/allocation_usd", Some(s("9000")));
    let impostor = move |_: &Digest| Some(other.clone());
    assert_eq!(
        JournaledFact::from_record("AgentDeployed", &deployed, &impostor, None)
            .map_err(|e| e.code()),
        Err("invalid_input"),
        "a document that does not hash to the version it is stored under is refused"
    );
    Ok(())
}

#[test]
#[ignore = "pending E7-10"]
fn a_retirement_maps_its_connection_date_and_loss() -> Result<(), SpecError> {
    let stopped = obj(vec![
        ("agent_id", s("agent_a")),
        ("connection_id", s(CONNECTION)),
        ("reason", s("owner_stop")),
        ("retired_on", s("2026-09-25")),
        ("loss_added", s("125.5")),
    ]);
    assert_eq!(
        mapped("AgentStopped", &stopped)?,
        Some(JournaledFact::AgentStopped {
            agent: AgentId::new("agent_a"),
            connection_id: connection()?,
            retired_on: Date::parse("2026-09-25")?,
            loss_added_usd: Usd::parse("125.5")?,
        })
    );
    Ok(())
}

/// No record names its account stream's connection yet (DEC-261 item 10), so the snapshot's fact
/// takes it from the caller, and without one the record is refused.
#[test]
#[ignore = "pending E7-10"]
fn a_snapshot_takes_its_connection_from_the_caller() -> Result<(), SpecError> {
    let snapshot = obj(vec![
        ("status", s("ACTIVE")),
        ("crypto_status", s("ACTIVE")),
        ("trading_blocked", Value::Bool(false)),
        ("account_blocked", Value::Bool(false)),
        ("trade_suspended_by_user", Value::Bool(false)),
        ("multiplier", common::i(1)),
        ("equity", s("25000")),
        ("cash", s("10000")),
        ("buying_power", s("20000")),
        ("non_marginable_buying_power", s("10000")),
        ("accrued_fees", s("0")),
        ("model_cash", Value::Null),
        ("cash_band", Value::Null),
        ("cash_in_band", Value::Null),
        ("risk_clock", s("2026-09-21T21:00:00.000000000Z")),
    ]);
    let ours = connection()?;
    assert_eq!(
        JournaledFact::from_record(
            "AccountSnapshotRecorded",
            &snapshot,
            &nothing_stored,
            Some(&ours)
        )?,
        Some(JournaledFact::AccountSnapshot {
            connection_id: ours.clone(),
            equity_usd: Usd::parse("25000")?,
        })
    );
    assert_eq!(
        mapped("AccountSnapshotRecorded", &snapshot).map_err(|e| e.code()),
        Err("invalid_input"),
        "a snapshot with no connection given is refused"
    );
    Ok(())
}

/// A record the table does not name maps to none; one it names but that does not hold a member the
/// fact needs, in §9.2's form, is refused rather than read as a default.
#[test]
#[ignore = "pending E7-10"]
fn an_unnamed_record_maps_to_none_and_a_malformed_one_is_refused() -> Result<(), SpecError> {
    let revoked = obj(vec![("connection_id", s(CONNECTION))]);
    assert_eq!(
        mapped("ConnectionRevoked", &revoked)?,
        Some(JournaledFact::ConnectionRevoked {
            connection_id: connection()?,
        }),
        "the control: a well-formed record maps"
    );
    for event_type in ["OwnerAlertSent", "PolicyChanged", "MarkUpdated"] {
        assert_eq!(
            mapped(event_type, &revoked)?,
            None,
            "{event_type} maps to none"
        );
    }
    let without_environment = obj(vec![
        ("connection_id", s(CONNECTION)),
        ("broker", s("alpaca")),
        ("scopes", texts(&["trading"])),
    ]);
    let backtest = obj(vec![
        ("connection_id", s(CONNECTION)),
        ("broker", s("alpaca")),
        ("environment", s("backtest")),
        ("scopes", texts(&["trading"])),
    ]);
    let loss_not_a_decimal = obj(vec![
        ("agent_id", s("agent_a")),
        ("connection_id", s(CONNECTION)),
        ("reason", s("owner_stop")),
        ("retired_on", s("2026-09-25")),
        ("loss_added", s("x")),
    ]);
    for (event_type, payload) in [
        ("ConnectionEstablished", &without_environment),
        ("ConnectionEstablished", &backtest),
        ("AgentStopped", &loss_not_a_decimal),
    ] {
        assert_eq!(
            mapped(event_type, payload).map_err(|e| e.code()),
            Err("invalid_input"),
            "{event_type}: {payload:?}"
        );
    }
    Ok(())
}
