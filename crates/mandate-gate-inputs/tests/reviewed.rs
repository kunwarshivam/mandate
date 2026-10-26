//! One deployment's reviewed artifacts, read from its control stream and the store
//! (DEC-2380 item 5, DEC-505).

mod common;

use mandate_canon::Digest;
use mandate_domain::Environment;
use mandate_gate_inputs::control::{Configuration, Origin, SnapshotExchange};
use mandate_gate_inputs::{InputError, Reviewed};
use mandate_num::ShareIncrement;
use mandate_risk::{EtpClass, Exchange};
use mandate_time::UtcNanos;

use common::{AAPL, CALENDAR, CONTENT, FEE, RULES, SNAPSHOT, SPY, inputs, reference, text};

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
