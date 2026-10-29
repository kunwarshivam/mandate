//! The order builder's sizing and classification over the runtime's ports (stream H): the
//! runtime's model outputs mapped to the builder's, the builder's proposal mapped back to the
//! runtime's, and the §6 classification with the §6.2 step 5c policy overlay.

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_builder::{
    AccountSnapshot, Action as BuilderAction, ActionContext, BuilderMandate, Clip as BuilderClip,
    Direction, Market, ModelOutput as BuilderModelOutput,
};
use mandate_canon::Value;
use mandate_domain::{AutonomyDecision, Purpose as BuilderPurpose};
use mandate_num::{Price, Qty, Usd, UsdExact};
use mandate_runtime::{
    Autonomy, Classified, DecisionClip, ExitOrigin, ModelDirection, OrderExecution, OwnerRequest,
    Proposal, Purpose, SignalInputs, TimeInForce,
};
use mandate_spec::ValidatedMandate;
use mandate_time::UtcNanos;

use crate::context::BuilderContext;
use crate::error::PortError;

fn builder_output(
    output: &mandate_runtime::ModelOutput,
    outer_model: &str,
    outer_instrument: &InstrumentId,
    mandate: &BuilderMandate,
    context: &BuilderContext,
) -> Result<BuilderModelOutput, PortError> {
    if output.model_id != outer_model || output.instrument_id != *outer_instrument {
        return Err(PortError::Absent {
            what: "a consistently keyed model output",
        });
    }
    let pinned = mandate
        .models
        .iter()
        .find(|model| {
            model.id.as_str() == output.model_id && model.version.as_str() == output.model_version
        })
        .ok_or(PortError::Absent {
            what: "a model output pinned by the validated mandate",
        })?;
    let content_hash = context
        .model_content_hashes
        .get(&(output.model_id.clone(), output.model_version.clone()))
        .copied()
        .ok_or(PortError::Absent {
            what: "the model output's trusted content hash",
        })?;
    if content_hash != output.content_hash {
        return Err(PortError::Absent {
            what: "the model output's matching trusted content hash",
        });
    }
    let direction = match &output.direction {
        ModelDirection::Long => Direction::Long,
        ModelDirection::Other(_) => {
            return Err(PortError::Absent {
                what: "a supported model output direction",
            });
        }
    };
    Ok(BuilderModelOutput {
        model_id: pinned.id.clone(),
        model_version: pinned.version.clone(),
        content_hash,
        instrument: mandate_domain::AssetId::parse(output.instrument_id.as_str())
            .map_err(mandate_builder::BuilderError::from)?,
        as_of: UtcNanos::from_parts(output.as_of.secs(), 0).map_err(|_| PortError::Absent {
            what: "the model output's timestamp",
        })?,
        expires_at: UtcNanos::from_parts(output.expires_at.secs(), 0).map_err(|_| {
            PortError::Absent {
                what: "the model output's expiry",
            }
        })?,
        direction,
        conviction: output.conviction,
        confidence: output.confidence,
    })
}

fn builder_outputs(
    inputs: &SignalInputs,
    mandate: &BuilderMandate,
    context: &BuilderContext,
) -> Result<Vec<BuilderModelOutput>, PortError> {
    inputs
        .outputs
        .iter()
        .flat_map(|(model, instruments)| {
            instruments
                .iter()
                .map(move |(instrument, output)| (model, instrument, output))
        })
        .map(|(model, instrument, output)| {
            builder_output(output, model, instrument, mandate, context)
        })
        .collect()
}

fn runtime_purpose(purpose: BuilderPurpose) -> Purpose {
    match purpose {
        BuilderPurpose::Open => Purpose::Open,
        BuilderPurpose::Increase => Purpose::Increase,
        BuilderPurpose::DiscretionaryExit => Purpose::DiscretionaryExit,
        BuilderPurpose::OwnerExit => Purpose::OwnerExit,
        BuilderPurpose::RiskExit => Purpose::RiskExit,
        BuilderPurpose::Protective => Purpose::Protective,
    }
}

fn runtime_proposal(
    proposal: mandate_builder::Proposal,
    context: &BuilderContext,
    mandate: &BuilderMandate,
    validated: &ValidatedMandate,
) -> Result<Option<Proposal>, PortError> {
    let mut clips_applied = Vec::new();
    if proposal.clipped_by.contains(&BuilderClip::Limits) {
        let delta = proposal.sizes.delta.ok_or(PortError::Absent {
            what: "the builder delta behind its limit clip",
        })?;
        let max_order = UsdExact::of(mandate.limits.max_order_usd);
        let position_room = proposal
            .sizes
            .cap
            .checked_sub(proposal.sizes.current_mv)?
            .checked_sub(UsdExact::of(context.account.working_opening_cost))?;
        let gross_room = UsdExact::of(mandate.limits.max_gross_exposure_usd)
            .min(UsdExact::of(context.account.agent_equity))?
            .checked_sub(UsdExact::of(context.account.gross_usd))?;
        if max_order.is_below(delta)? {
            clips_applied.push(DecisionClip::MaxOrderUsd);
        }
        if position_room.is_below(delta)? {
            clips_applied.push(DecisionClip::PositionCap);
        }
        if gross_room.is_below(delta)? {
            clips_applied.push(DecisionClip::GrossExposureCap);
        }
        if clips_applied.is_empty() {
            return Err(PortError::Absent {
                what: "the exact journal clip applied by the builder",
            });
        }
    }
    if proposal.clipped_by.contains(&BuilderClip::Goal) {
        return Err(PortError::Absent {
            what: "the exact journal goal clip applied by the builder",
        });
    }
    let (side, purpose, qty, limit) = match proposal.action {
        BuilderAction::Hold { .. } => return Ok(None),
        BuilderAction::Buy {
            purpose,
            qty,
            limit_price,
            ..
        } => (Side::Buy, runtime_purpose(purpose), qty, limit_price),
        BuilderAction::Sell {
            purpose,
            qty,
            limit_price,
            ..
        } => (Side::Sell, runtime_purpose(purpose), qty, limit_price),
    };
    let instrument =
        InstrumentId::new(context.market.instrument.as_str()).map_err(|_| PortError::Absent {
            what: "the builder market's runtime instrument id",
        })?;
    validate_order_execution(side, purpose, context.execution)?;
    let mandate_requires_protection = validated.mandate().protection.enabled;
    let execution_matches_mandate = match side {
        Side::Buy => context.execution.protection_required == mandate_requires_protection,
        Side::Sell => !context.execution.protection_required,
    };
    if context.execution.asset_class != context.market.asset_class || !execution_matches_mandate {
        return Err(PortError::Absent {
            what: "the mandate-derived execution policy",
        });
    }
    let outputs_used = proposal
        .combined
        .outputs_used
        .iter()
        .map(|model| model.as_str().to_owned())
        .collect();
    let model_weights = validated
        .mandate()
        .behavior
        .signal_models
        .iter()
        .map(|model| {
            (
                model.id.as_str().to_owned(),
                Value::Str(model.weight.to_string()),
            )
        })
        .collect();
    Ok(Some(Proposal {
        instrument,
        asset_class: context.market.asset_class,
        side,
        qty,
        limit,
        purpose,
        exit_origin: (purpose == Purpose::DiscretionaryExit).then_some(ExitOrigin::Signal),
        exit_conviction: Some(Value::Str(proposal.combined.exit_conviction.to_string())),
        buy_conviction: Some(Value::Str(proposal.combined.buy_conviction.to_string())),
        combined_score: Value::Str(proposal.combined.score.to_string()),
        outputs_used,
        model_weights,
        clips_applied,
        execution: Some(context.execution),
    }))
}

/// Checks that `side` cannot open a short for `purpose`, that `execution` carries the protective
/// prices exactly when it requires them, and that its time in force is one its asset class allows.
///
/// # Errors
/// [`PortError::Absent`] naming the first check that fails.
pub fn validate_order_execution(
    side: Side,
    purpose: Purpose,
    execution: OrderExecution,
) -> Result<(), PortError> {
    let direction_is_valid = match purpose {
        Purpose::Open | Purpose::Increase => side == Side::Buy,
        Purpose::RiskExit
        | Purpose::OwnerExit
        | Purpose::DiscretionaryExit
        | Purpose::Protective
        | Purpose::Flatten => side == Side::Sell,
    };
    if !direction_is_valid {
        return Err(PortError::Absent {
            what: "an order direction that cannot create a short position",
        });
    }
    if execution.protection_required != execution.protection.is_some() {
        return Err(PortError::Absent {
            what: "the mandate-required protective prices",
        });
    }
    match (execution.asset_class, execution.tif) {
        (AssetClass::Crypto, TimeInForce::Day) | (AssetClass::UsEquity, TimeInForce::Ioc) => {
            Err(PortError::Absent {
                what: "a time in force allowed by the mandate's asset class",
            })
        }
        (AssetClass::UsEquity, TimeInForce::Day | TimeInForce::Gtc)
        | (AssetClass::Crypto, TimeInForce::Gtc | TimeInForce::Ioc) => Ok(()),
    }
}

fn action_order_matches(
    action_order_usd: Usd,
    proposal_qty: Qty,
    proposal_limit: Price,
) -> Result<bool, PortError> {
    Ok(action_order_usd == proposal_qty.notional(proposal_limit)?)
}

/// `mandate-builder`'s `conviction_linear` sizing of `inputs` under `validated`, as the runtime's
/// proposal: `None` for a hold. An instrument whose thesis ended (`inputs.ended_theses`, mandate
/// spec §8.6) takes §8.3 step 2's first branch instead: the whole position sold, with
/// `exit_origin` `removed_instrument`, reading no model output, so an output the mandate no longer
/// pins cannot hold the exit (MI-34, DEC-2391). The runtime's working exits in the instrument come
/// off the position first, so an exit already working is never stacked; when they cover it, the
/// subtraction refuses to go below zero and the branch holds. A pinned universe takes no such branch: the
/// owner's pin is the universe, and the research agent is off (V-037).
///
/// # Errors
/// [`PortError::Absent`] for an output the mandate does not pin or the registry does not hash, a
/// clip the journal cannot name, or an execution policy the mandate does not derive;
/// [`PortError::Builder`] and [`PortError::Num`] as the builder and its amounts refuse.
pub fn size(
    validated: &ValidatedMandate,
    builder: &BuilderContext,
    inputs: &SignalInputs,
) -> Result<Option<Proposal>, PortError> {
    let mandate = BuilderMandate::try_from(validated)?;
    let now = UtcNanos::from_parts(inputs.now.secs(), 0).map_err(|_| PortError::Absent {
        what: "the decision timestamp",
    })?;
    if !validated.mandate().universe.pinned
        && inputs
            .ended_theses
            .iter()
            .any(|ended| ended.as_str() == builder.market.instrument.as_str())
    {
        return thesis_exit(
            false,
            &builder.account,
            &builder.market,
            builder.execution,
            inputs,
        );
    }
    let outputs = builder_outputs(inputs, &mandate, builder)?;
    let proposal = mandate_builder::propose(
        &mandate,
        &builder.account,
        &builder.market,
        &builder.risk,
        &outputs,
        now,
    )?;
    runtime_proposal(proposal, builder, &mandate, validated)
}

/// An owner's or a client's request for a buy (journal spec §2 and §9.9, DEC-2858) as the
/// runtime's proposal: the order builder's [`mandate_builder::requested_buy`] over `builder`'s
/// facts, clipped and never sized, at the ask, with no conviction, score, model output or weight
/// (journal spec §9.1 rule 8) and every limit that cut it in `clips_applied`. `Ok(None)` when the
/// builder proposes nothing for it.
///
/// # Errors
/// [`PortError::Absent`] for a request in another instrument than `builder`'s market, or an
/// execution policy the mandate does not derive; [`PortError::Builder`] as the builder refuses.
pub fn requested(
    _validated: &ValidatedMandate,
    _builder: &BuilderContext,
    _request: &OwnerRequest,
) -> Result<Option<Proposal>, PortError> {
    Err(mandate_builder::BuilderError::Unimplemented.into())
}

/// Mandate spec v0.10 §8.3 step 2's first branch on its own (MI-34, DEC-2391): when `market`'s
/// instrument's thesis ended (`inputs.ended_theses`) and the universe is unpinned, the whole
/// `account` position less the runtime's working exits in it, at the bid, with `exit_origin`
/// `removed_instrument`; `Ok(None)` for any other instrument, a pinned universe (V-037: the owner's
/// list is the universe), or nothing left to sell. It reads no model output, no validated mandate,
/// and no opening fact, so nothing an opening needs (the account's equity against the allocation,
/// a protective stop, a model registry) can hold the exit (`AGENTS.md` rule 13). Its
/// `DecisionMade` carries no c, b, or s and no outputs, weights, or clips (journal spec §9.1 rule
/// 8). When the working exits cover the position, the subtraction refuses to go below zero and the
/// branch holds, so an exit already working is never stacked.
///
/// # Errors
/// [`PortError::Absent`] for an execution that is not a plain sell of the market's asset class;
/// [`PortError::Builder`] as the exit's notional refuses.
pub fn thesis_exit(
    pinned: bool,
    account: &AccountSnapshot,
    market: &Market,
    execution: OrderExecution,
    inputs: &SignalInputs,
) -> Result<Option<Proposal>, PortError> {
    let instrument = market.instrument.as_str();
    if pinned
        || !inputs
            .ended_theses
            .iter()
            .any(|ended| ended.as_str() == instrument)
    {
        return Ok(None);
    }
    let working = inputs
        .working_exits
        .iter()
        .find(|(held, _)| held.as_str() == instrument)
        .map_or(Qty::ZERO, |(_, qty)| *qty);
    let unsold = AccountSnapshot {
        position_qty: account
            .position_qty
            .checked_sub(working)
            .unwrap_or(Qty::ZERO),
        ..account.clone()
    };
    let BuilderAction::Sell {
        purpose,
        qty,
        limit_price,
        ..
    } = mandate_builder::propose_thesis_exit(&unsold, market)?.action
    else {
        return Ok(None);
    };
    validate_order_execution(Side::Sell, runtime_purpose(purpose), execution)?;
    if execution.asset_class != market.asset_class || execution.protection_required {
        return Err(PortError::Absent {
            what: "a plain sell's execution in the market's asset class",
        });
    }
    Ok(Some(Proposal {
        instrument: InstrumentId::new(instrument).map_err(|_| PortError::Absent {
            what: "the builder market's runtime instrument id",
        })?,
        asset_class: market.asset_class,
        side: Side::Sell,
        qty,
        limit: limit_price,
        purpose: runtime_purpose(purpose),
        exit_origin: Some(ExitOrigin::RemovedInstrument),
        exit_conviction: None,
        buy_conviction: None,
        combined_score: Value::Null,
        outputs_used: BTreeSet::new(),
        model_weights: BTreeMap::new(),
        clips_applied: Vec::new(),
        execution: Some(execution),
    }))
}

/// The `decided_by` label of mandate spec §6.2 step 5c, the policy overlay (DEC-536 item 2;
/// journal spec §9.1).
pub const POLICY_OVERLAY: &str = "policy_overlay";

/// Mandate spec §6 classification of `proposal` against `action`, its trusted facts, with §6.2
/// step 5c (DEC-534 item 2, DEC-536): an opening or an increase while the confirmed mandate is
/// `policy_nonconforming` is denied, applied last so it only tightens. The overlay names itself
/// (`policy_overlay`) only when it changed the decision, so a `deny` the rules already decided
/// keeps their label. An exit never reads the overlay, so nothing here denies it (`AGENTS.md`
/// rule 13).
///
/// # Errors
/// [`PortError::Absent`] when `action` does not describe `proposal`; [`PortError::Builder`] as
/// the classification refuses.
pub fn classify(
    validated: &ValidatedMandate,
    action: &ActionContext,
    proposal: &Proposal,
    policy_nonconforming: bool,
) -> Result<Classified, PortError> {
    let Some(score) = action.combined_score else {
        return Err(mandate_builder::BuilderError::Unimplemented.into());
    };
    let expected_score = Value::Str(score.to_string());
    if action.instrument.as_str() != proposal.instrument.as_str()
        || action.asset_class != proposal.asset_class
        || runtime_purpose(action.purpose) != proposal.purpose
        || expected_score != proposal.combined_score
        || !action_order_matches(action.order_usd, proposal.qty, proposal.limit)?
    {
        return Err(PortError::Absent {
            what: "classification facts for the proposed action",
        });
    }
    let overlay_denies = proposal.purpose.adds_risk() && policy_nonconforming;
    let classification =
        mandate_builder::classify(mandate_builder::autonomy_policy(validated), action)?;
    let autonomy = match classification.decision {
        AutonomyDecision::Auto => Autonomy::Auto,
        AutonomyDecision::Ask => Autonomy::Ask,
        AutonomyDecision::Deny => Autonomy::Deny,
    };
    if overlay_denies && autonomy != Autonomy::Deny {
        return Ok(Classified {
            autonomy: Autonomy::Deny,
            decided_by: Some(POLICY_OVERLAY.to_owned()),
        });
    }
    Ok(Classified {
        autonomy,
        decided_by: Some(classification.by.label()),
    })
}

/// Mandate spec §6.2 step 5c when the effective policy's `auto_allowed` is false: an opening or
/// an increase `classify` answered `auto` becomes `ask`, naming the overlay (`policy_overlay`)
/// since it changed the decision. An `ask` or a `deny` keeps its own label, and an exit is never
/// touched (`AGENTS.md` rule 13), so this only tightens.
#[must_use]
pub fn auto_not_allowed(classified: Classified, proposal: &Proposal) -> Classified {
    if classified.autonomy == Autonomy::Auto && proposal.purpose.adds_risk() {
        Classified {
            autonomy: Autonomy::Ask,
            decided_by: Some(POLICY_OVERLAY.to_owned()),
        }
    } else {
        classified
    }
}

#[cfg(test)]
mod tests {
    use mandate_num::{Price, Qty, Usd};

    use super::action_order_matches;

    #[test]
    fn classification_order_facts_must_match_the_sized_proposal() -> Result<(), String> {
        let one_share = Usd::parse("255.2").map_err(|e| e.to_string())?;
        let limit = Price::parse("255.2").map_err(|e| e.to_string())?;
        assert!(
            action_order_matches(
                one_share,
                Qty::parse("1").map_err(|e| e.to_string())?,
                limit
            )
            .map_err(|e| e.to_string())?
        );
        assert!(
            !action_order_matches(
                one_share,
                Qty::parse("2").map_err(|e| e.to_string())?,
                limit
            )
            .map_err(|e| e.to_string())?,
            "one-share autonomy facts cannot classify a two-share proposal"
        );
        Ok(())
    }
}
