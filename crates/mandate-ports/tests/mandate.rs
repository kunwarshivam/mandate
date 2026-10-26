//! The mandate's limits as the gate reads them (DEC-2388 item 1), moved with `gate_mandate` from
//! `mandate-gate-inputs`.

use mandate_canon::Digest;
use mandate_num::{Fraction, Usd};
use mandate_ports::gate_mandate;
use mandate_risk::spec_types::{GoalState, RiskLimits, Rung, RungAction, ScaleAction};

const MANDATE: &str = include_str!("../../mandate-shell/tests/fixtures/tracer/mandate.json");

fn text<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

fn document(json: &str) -> Result<mandate_spec::Mandate, String> {
    let value = mandate_canon::parse(json.trim().as_bytes()).map_err(text)?;
    mandate_spec::Mandate::parse(&value).map_err(text)
}

fn fraction(text_: &str) -> Result<Fraction, String> {
    Fraction::parse(text_).map_err(text)
}

fn usd(text_: &str) -> Result<Usd, String> {
    Usd::parse(text_).map_err(text)
}

#[test]
fn the_limits_and_ladder_are_the_mandates() -> Result<(), String> {
    let mandate = gate_mandate(&document(MANDATE)?).map_err(text)?;
    let scale = Some(ScaleAction::LimitBuys);
    assert_eq!(
        mandate.risk(),
        &RiskLimits {
            max_position_usd: usd("1000")?,
            max_position_fraction: fraction("1")?,
            max_order_usd: usd("300")?,
            max_gross_exposure_usd: usd("1000")?,
            max_orders_per_day: 50,
            reentry_cooldown_s: 3600,
            rebalance_band: fraction("0.05")?,
            breach_confirm_s: 60,
            drawdown_ladder: vec![
                Rung {
                    index: 0,
                    at: fraction("0.03")?,
                    action: RungAction::ScaleSizes,
                    factor: Some(fraction("0.5")?),
                    scale_action: scale,
                },
                Rung {
                    index: 1,
                    at: fraction("0.06")?,
                    action: RungAction::ExitsOnly,
                    factor: None,
                    scale_action: scale,
                },
                Rung {
                    index: 2,
                    at: fraction("0.08")?,
                    action: RungAction::FlattenAndPause,
                    factor: None,
                    scale_action: scale,
                },
            ],
        }
    );
    assert_eq!(mandate.goal_state(), GoalState::Running);
    assert!(!mandate.leveraged_etps_enabled());
    assert!(!mandate.leveraged_etp_disclosure_accepted());
    Ok(())
}

#[test]
fn the_trim_action_and_etp_choices_are_copied() -> Result<(), String> {
    let changed = MANDATE.replace("\"limit_buys\"", "\"trim_to_target\"");
    assert_ne!(changed, MANDATE, "the fixture names limit_buys");
    let mut parsed = document(&changed)?;
    let mandate = gate_mandate(&parsed).map_err(text)?;
    assert!(
        mandate
            .risk()
            .drawdown_ladder
            .iter()
            .all(|rung| rung.scale_action == Some(ScaleAction::TrimToTarget))
    );
    assert!(!mandate.leveraged_etps_enabled());
    assert!(!mandate.leveraged_etp_disclosure_accepted());
    parsed.universe.leveraged_etps_enabled = true;
    let enabled = gate_mandate(&parsed).map_err(text)?;
    assert!(enabled.leveraged_etps_enabled());
    assert!(!enabled.leveraged_etp_disclosure_accepted());
    parsed.universe.leveraged_etp_disclosure_version = Some(Digest::of(b"disclosure-v1"));
    let disclosed = gate_mandate(&parsed).map_err(text)?;
    assert!(disclosed.leveraged_etps_enabled());
    assert!(disclosed.leveraged_etp_disclosure_accepted());
    Ok(())
}
