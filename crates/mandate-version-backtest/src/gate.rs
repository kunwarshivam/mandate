//! The gate's view of the run: the confirmed mandate as `mandate_risk`'s narrowed mandate, and the
//! risk state's snapshot in the gate's vocabulary.
//!
//! The mapping from a mandate document to the gate's mandate is the one the paper runner makes
//! (`mandate-shell`'s `paper::gate::gate_mandate`), restated here because the shell sits above this
//! crate. One shared mapping for both is E4-4's slice 2 (DEC-1000, "Alternatives considered").

use std::collections::BTreeSet;

use mandate_domain::{AgentMode, AssetId, MarketSession, Side as DomainSide};
use mandate_num::{Fraction, Ratio, Usd};
use mandate_risk::spec_types::{GoalState, LimitKey, RiskLimits, Rung, RungAction, ScaleAction};
use mandate_risk::{AgentMode as GateMode, InstrumentRestriction as GateRestriction, Session};
use mandate_spec::ValidatedMandate;
use mandate_spec::document::{LadderAction, ScaleAction as DocumentScaleAction};
use mandate_spec::risk::{self, Snapshot};

use crate::VersionError;

/// The confirmed mandate as the gate reads it, with the goal running.
pub(super) fn mandate(
    validated: &ValidatedMandate,
) -> Result<mandate_risk::ValidatedMandate, VersionError> {
    with_goal(validated, GoalState::Running)
}

/// The same mandate with the goal complete and holding (mandate spec §3.1).
pub(super) fn mandate_holding(
    validated: &ValidatedMandate,
) -> Result<mandate_risk::ValidatedMandate, VersionError> {
    with_goal(validated, GoalState::Holding)
}

fn with_goal(
    validated: &ValidatedMandate,
    goal: GoalState,
) -> Result<mandate_risk::ValidatedMandate, VersionError> {
    let document = validated.mandate();
    let risk = &document.risk;
    let scale_action = match risk.scale_action {
        DocumentScaleAction::LimitBuys => ScaleAction::LimitBuys,
        DocumentScaleAction::TrimToTarget => ScaleAction::TrimToTarget,
    };
    let drawdown_ladder = risk
        .drawdown_ladder
        .iter()
        .enumerate()
        .map(|(index, rung)| {
            Ok(Rung {
                index: u8::try_from(index).map_err(|_| mandate_num::NumError::Overflow)?,
                at: Fraction::parse(rung.at.as_str())?,
                action: match rung.action {
                    LadderAction::ScaleSizes => RungAction::ScaleSizes,
                    LadderAction::ExitsOnly => RungAction::ExitsOnly,
                    LadderAction::FlattenAndPause => RungAction::FlattenAndPause,
                },
                factor: rung
                    .factor
                    .as_ref()
                    .map(|factor| Fraction::parse(factor.as_str()))
                    .transpose()?,
                scale_action: Some(scale_action),
            })
        })
        .collect::<Result<Vec<_>, VersionError>>()?;
    Ok(mandate_risk::ValidatedMandate::from_validated_parts(
        RiskLimits {
            max_position_usd: Usd::parse(risk.max_position_usd.as_str())?,
            max_position_fraction: Fraction::parse(risk.max_position_fraction.as_str())?,
            max_order_usd: Usd::parse(risk.max_order_usd.as_str())?,
            max_gross_exposure_usd: Usd::parse(risk.max_gross_exposure_usd.as_str())?,
            max_orders_per_day: risk.max_orders_per_day,
            reentry_cooldown_s: risk.reentry_cooldown_s,
            rebalance_band: Fraction::parse(document.behavior.sizing.rebalance_band.as_str())?,
            breach_confirm_s: risk.breach_confirm_s,
            drawdown_ladder,
        },
        goal,
        document.universe.leveraged_etps_enabled,
        document.universe.leveraged_etp_disclosure_version.is_some(),
    ))
}

/// The gate's id for the run's instrument: the asset id's own text. The gate refuses only an empty
/// id, which a parsed asset id never is, so a refusal is the id's text not being canonical.
pub(super) fn asset(asset: &AssetId) -> Result<mandate_risk::AssetId, VersionError> {
    mandate_risk::AssetId::new(asset.as_str())
        .map_err(|_| VersionError::Num(mandate_num::NumError::NotCanonical))
}

/// The risk state's snapshot narrowed to the fields the gate reads.
pub(super) fn risk_snapshot(
    snapshot: &Snapshot,
) -> Result<mandate_risk::RiskSnapshot, VersionError> {
    Ok(mandate_risk::RiskSnapshot {
        agent_equity: snapshot.agent_equity,
        high_water_mark: snapshot.high_water_mark,
        day_start_equity: snapshot.day_start_equity,
        capital_base: snapshot.capital_base,
        inherited_loss: snapshot.inherited_loss,
        latched: snapshot
            .latched
            .iter()
            .map(|key| limit_key(*key))
            .collect::<BTreeSet<_>>(),
        active_rungs: snapshot.active_rungs.clone(),
        size_factor: Ratio::parse(&snapshot.size_factor.to_string())?,
        agent_mode: agent_mode(snapshot.agent_mode),
    })
}

fn limit_key(key: risk::LimitKey) -> LimitKey {
    match key {
        risk::LimitKey::MaxDailyLoss => LimitKey::MaxDailyLoss,
        risk::LimitKey::DrawdownRung(index) => LimitKey::DrawdownRung(index),
        risk::LimitKey::LifetimeFloor => LimitKey::LifetimeFloor,
        risk::LimitKey::ProfitStop => LimitKey::ProfitStop,
    }
}

pub(super) fn agent_mode(mode: AgentMode) -> GateMode {
    match mode {
        AgentMode::Normal => GateMode::Normal,
        AgentMode::ExitsOnly => GateMode::ExitsOnly,
        AgentMode::Paused => GateMode::Paused,
        AgentMode::Stopped => GateMode::Stopped,
    }
}

pub(super) fn instrument_restriction(restriction: risk::InstrumentRestriction) -> GateRestriction {
    match restriction {
        risk::InstrumentRestriction::StaleMark => GateRestriction::StaleMark,
        risk::InstrumentRestriction::RemovedInstrument => GateRestriction::RemovedInstrument,
    }
}

/// The gate's derived session as the builder's market names it.
pub(super) fn market_session(session: Session) -> MarketSession {
    match session {
        Session::Overnight => MarketSession::Overnight,
        Session::PreMarket => MarketSession::PreMarket,
        Session::Regular => MarketSession::Regular,
        Session::AfterHours => MarketSession::AfterHours,
        Session::Continuous => MarketSession::Crypto,
    }
}

pub(super) fn domain_side(side: mandate_accounting::Side) -> DomainSide {
    match side {
        mandate_accounting::Side::Buy => DomainSide::Buy,
        mandate_accounting::Side::Sell => DomainSide::Sell,
    }
}
