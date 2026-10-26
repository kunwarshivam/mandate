//! The mandate's risk limits and drawdown ladder as the gate reads them, moved here unchanged from
//! `mandate-gate-inputs` so the paper shell's binding gate and the cell's runtime's advisory pass
//! read one copy (DEC-2387 item 1).

use mandate_num::{Fraction, Usd};
use mandate_risk::ValidatedMandate as GateMandate;
use mandate_risk::spec_types::{GoalState, RiskLimits, Rung, RungAction, ScaleAction};

use crate::error::PortError;

fn usd(text: &str) -> Result<Usd, PortError> {
    Ok(Usd::parse(text)?)
}

/// The mandate's risk limits and drawdown ladder as the gate reads them, with the goal running.
///
/// # Errors
/// [`PortError`] for a limit or ladder rung the gate cannot read.
pub fn gate_mandate(document: &mandate_spec::Mandate) -> Result<GateMandate, PortError> {
    let risk = &document.risk;
    let scale_action = match risk.scale_action {
        mandate_spec::document::ScaleAction::LimitBuys => ScaleAction::LimitBuys,
        mandate_spec::document::ScaleAction::TrimToTarget => ScaleAction::TrimToTarget,
    };
    let drawdown_ladder = risk
        .drawdown_ladder
        .iter()
        .enumerate()
        .map(|(index, rung)| {
            Ok(Rung {
                index: u8::try_from(index).map_err(|_| PortError::Absent {
                    what: "the mandate's drawdown ladder",
                })?,
                at: Fraction::parse(rung.at.as_str())?,
                action: match rung.action {
                    mandate_spec::document::LadderAction::ScaleSizes => RungAction::ScaleSizes,
                    mandate_spec::document::LadderAction::ExitsOnly => RungAction::ExitsOnly,
                    mandate_spec::document::LadderAction::FlattenAndPause => {
                        RungAction::FlattenAndPause
                    }
                },
                factor: rung
                    .factor
                    .as_ref()
                    .map(|factor| Fraction::parse(factor.as_str()))
                    .transpose()?,
                scale_action: Some(scale_action),
            })
        })
        .collect::<Result<Vec<_>, PortError>>()?;
    Ok(GateMandate::from_validated_parts(
        RiskLimits {
            max_position_usd: usd(document.risk.max_position_usd.as_str())?,
            max_position_fraction: Fraction::parse(document.risk.max_position_fraction.as_str())?,
            max_order_usd: usd(document.risk.max_order_usd.as_str())?,
            max_gross_exposure_usd: usd(document.risk.max_gross_exposure_usd.as_str())?,
            max_orders_per_day: document.risk.max_orders_per_day,
            reentry_cooldown_s: document.risk.reentry_cooldown_s,
            rebalance_band: Fraction::parse(document.behavior.sizing.rebalance_band.as_str())?,
            breach_confirm_s: document.risk.breach_confirm_s,
            drawdown_ladder,
        },
        GoalState::Running,
        document.universe.leveraged_etps_enabled,
        document.universe.leveraged_etp_disclosure_version.is_some(),
    ))
}
