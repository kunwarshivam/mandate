//! One deployment's agent and instrument as the executor's ports answer them (DEC-2380 items 2
//! and 5).

mod common;

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_executor::{
    AgentId, BindingGateConfigRefs, FillConfigRefs, InstrumentSnapshot, MandateVersion,
    MandateView, ProtectionPrices,
};
use mandate_gate_inputs::{
    DeployedView, InputError, Reviewed, fill_refs, protection_prices, stop_limit_offset,
};
use mandate_num::{Fraction, Price, ShareIncrement};
use mandate_spec::document::Protection;
use mandate_spec::{DecGrammar, SchemaDec};

use common::{AAPL, SNAPSHOT, SPY, inputs, text};

fn reviewed() -> Result<Reviewed, String> {
    let (confirmed, config) = inputs(SNAPSHOT)?;
    Reviewed::from_registered(&confirmed, &config).map_err(text)
}

fn fraction(text_: &str) -> Result<SchemaDec, String> {
    SchemaDec::parse(text_, DecGrammar::OpenFraction).map_err(|error| format!("{error:?}"))
}

fn price(text_: &str) -> Result<Price, String> {
    Price::parse(text_).map_err(text)
}

fn instrument(text_: &str) -> Result<InstrumentId, String> {
    InstrumentId::new(text_).map_err(text)
}

fn agent(name: &str) -> AgentId {
    AgentId(name.to_owned())
}

#[test]
fn fills_name_the_registered_fee_calendar_and_snapshot() {
    let refs = BindingGateConfigRefs::complete(
        "sha256:fee",
        "sha256:cal",
        "sha256:ins",
        "sha256:set",
        "sha256:man",
    );
    assert_eq!(
        fill_refs(&refs),
        Some(FillConfigRefs {
            fee_config: "sha256:fee".to_owned(),
            trading_calendar: "sha256:cal".to_owned(),
            settlement_calendar: "sha256:cal".to_owned(),
            instrument_snapshot: "sha256:ins".to_owned(),
        })
    );
    for missing in 0..3 {
        let mut short = refs.clone();
        match missing {
            0 => short.fee_config = None,
            1 => short.trading_calendar = None,
            _ => short.instrument_snapshot = None,
        }
        assert_eq!(fill_refs(&short), None, "{missing}");
    }
}

#[test]
fn the_stop_limit_offset_is_read_only_while_protection_is_on() -> Result<(), String> {
    let mut protection = Protection {
        enabled: true,
        stop_distance: None,
        take_profit_distance: None,
        crypto_stop_limit_offset: Some(fraction("0.01")?),
    };
    assert_eq!(
        stop_limit_offset(&protection).map_err(text)?,
        Some(Fraction::parse("0.01").map_err(text)?)
    );
    protection.enabled = false;
    assert_eq!(stop_limit_offset(&protection).map_err(text)?, None);
    protection.enabled = true;
    protection.crypto_stop_limit_offset = None;
    assert_eq!(stop_limit_offset(&protection).map_err(text)?, None);
    Ok(())
}

#[test]
fn protection_brackets_the_entry_or_is_refused_without_a_stop() -> Result<(), String> {
    let mut protection = Protection {
        enabled: true,
        stop_distance: Some(fraction("0.05")?),
        take_profit_distance: Some(fraction("0.1")?),
        crypto_stop_limit_offset: None,
    };
    let entry = price("100")?;
    assert_eq!(
        protection_prices(&protection, entry).map_err(text)?,
        Some(ProtectionPrices {
            stop: price("95")?,
            take_profit: Some(price("110")?),
        })
    );
    protection.take_profit_distance = None;
    assert_eq!(
        protection_prices(&protection, entry).map_err(text)?,
        Some(ProtectionPrices {
            stop: price("95")?,
            take_profit: None,
        })
    );
    protection.stop_distance = None;
    assert!(matches!(
        protection_prices(&protection, entry),
        Err(InputError::Missing {
            what: "the mandate's stop distance"
        })
    ));
    protection.enabled = false;
    assert_eq!(protection_prices(&protection, entry).map_err(text)?, None);
    Ok(())
}

#[test]
fn the_view_answers_only_the_deployed_agent_on_its_instrument() -> Result<(), String> {
    let reviewed = reviewed()?;
    let view = DeployedView::new("agent_spy", &reviewed).map_err(text)?;
    let ours = agent("agent_spy");
    let other = agent("agent_other");
    let spy = instrument(SPY)?;
    let aapl = instrument(AAPL)?;
    assert_eq!(
        view.version(&ours),
        reviewed
            .config_refs
            .mandate_version
            .clone()
            .map(MandateVersion)
    );
    assert!(view.version(&ours).is_some());
    assert_eq!(view.version(&other), None);
    assert!(view.covers(&ours, &spy));
    assert!(!view.covers(&other, &spy));
    assert!(!view.covers(&ours, &aapl));
    assert_eq!(view.crypto_stop_limit_offset(&ours), None);
    assert_eq!(view.asset_class(&spy), Some(AssetClass::UsEquity));
    assert_eq!(view.asset_class(&aapl), None);
    assert_eq!(view.increment(&spy), Some(ShareIncrement::Whole));
    assert_eq!(view.increment(&aapl), None);
    assert_eq!(view.exit_tier(&spy), None);
    assert_eq!(view.fill_refs(&spy), fill_refs(&reviewed.config_refs));
    assert!(view.fill_refs(&spy).is_some());
    assert_eq!(view.fill_refs(&aapl), None);
    Ok(())
}

#[test]
fn the_view_protects_only_its_agents_instrument() -> Result<(), String> {
    let mut reviewed = reviewed()?;
    reviewed.mandate.protection.crypto_stop_limit_offset = Some(fraction("0.02")?);
    let view = DeployedView::new("agent_spy", &reviewed).map_err(text)?;
    let ours = agent("agent_spy");
    let other = agent("agent_other");
    let spy = instrument(SPY)?;
    let entry = price("100")?;
    assert_eq!(
        view.crypto_stop_limit_offset(&ours),
        Some(Fraction::parse("0.02").map_err(text)?)
    );
    assert_eq!(view.crypto_stop_limit_offset(&other), None);
    assert_eq!(
        view.protection(&ours, &spy, entry).map_err(text)?,
        Some(ProtectionPrices {
            stop: price("95")?,
            take_profit: Some(price("110")?),
        })
    );
    assert!(view.protection(&other, &spy, entry).is_err());
    assert!(view.protection(&ours, &instrument(AAPL)?, entry).is_err());
    Ok(())
}
