//! The opening policy intersected with the broker's capability profile (E7-23 B3; trading spec
//! §5.1 and §5.2; DEC-529 item 2, DEC-531 items 2 and 3, DEC-854).
//!
//! The platform's policy opens with a limit order, sized as a quantity, and a US equity only in
//! the regular session. The profile says which quantity forms and times in force a limit order may
//! take in each row. The builder sizes to what both allow. An empty intersection refuses the buy
//! before any intent, and never falls back to another order type (LT-3). It never touches an exit
//! (`AGENTS.md` rule 13). Shared code never names a broker (LT-2).

use std::collections::BTreeMap;

use mandate_domain::{
    AssetClass, CapabilityProfile, MarketSession, OrderType, QuantityForm, TimeInForce,
};
use mandate_num::Qty;
use mandate_time::UtcNanos;

use crate::BuilderError;
use crate::builder::{
    AccountSnapshot, Action, BuilderMandate, Market, ModelOutput, Proposal, RiskContext, propose,
};

/// The quantity forms an opening may take, in the order they are preferred: fractional first, so a
/// profile that offers it sizes on the market's own grid as [`propose`] does (LT-14), then whole
/// shares. Never notional: the builder sizes a quantity (DEC-854 item 1).
const OPENING_FORMS: [QuantityForm; 2] = [QuantityForm::Fractional, QuantityForm::Whole];

/// Where an opening goes: the connection's profile, and the time in force the caller sends the
/// opening with (the builder does not choose it; DEC-854 item 2).
#[derive(Debug, Clone, Copy)]
pub struct Venue<'a> {
    pub profile: &'a CapabilityProfile,
    pub time_in_force: TimeInForce,
}

/// The quantity form an opening limit order takes in this asset class and session under this
/// time in force: fractional when the profile's `limit` cell for it lists the time in force,
/// otherwise whole when that cell does, never notional (DEC-854 items 1 to 3). A US equity outside
/// the regular session has none, whatever the profile declares.
///
/// # Errors
/// [`BuilderError::NoOpeningForm`] when the intersection is empty.
pub fn opening_form(
    profile: &CapabilityProfile,
    asset_class: AssetClass,
    session: MarketSession,
    time_in_force: TimeInForce,
) -> Result<QuantityForm, BuilderError> {
    let outside_the_policy_session =
        asset_class == AssetClass::UsEquity && session != MarketSession::Regular;
    if outside_the_policy_session {
        return Err(BuilderError::NoOpeningForm);
    }
    OPENING_FORMS
        .into_iter()
        .find(|&form| {
            profile
                .cell(asset_class, session, OrderType::Limit, form)
                .is_ok_and(|cell| cell.times_in_force.contains(&time_in_force))
        })
        .ok_or(BuilderError::NoOpeningForm)
}

/// [`crate::propose`] at the venue: a buy is sized in the quantity form [`opening_form`] gives,
/// whole shares truncating §8.3 step 3's budget so that one share must pass every limit or
/// nothing is proposed (DEC-529 item 2). An empty intersection refuses a buy
/// [`BuilderError::NoOpeningForm`]; an exit or a hold is exactly what `propose` gives (DEC-854
/// item 4).
pub fn propose_on(
    venue: &Venue<'_>,
    mandate: &BuilderMandate,
    account: &AccountSnapshot,
    market: &Market,
    risk: &RiskContext,
    outputs: &[ModelOutput],
    now: UtcNanos,
) -> Result<Proposal, BuilderError> {
    let form = match opening_form(
        venue.profile,
        market.asset_class,
        market.session,
        venue.time_in_force,
    ) {
        Ok(form) => form,
        Err(BuilderError::NoOpeningForm) => {
            let proposal = propose(mandate, account, market, risk, outputs, now)?;
            return match proposal.action {
                Action::Buy { .. } => Err(BuilderError::NoOpeningForm),
                Action::Hold { .. } | Action::Sell { .. } => Ok(proposal),
            };
        }
        Err(other) => return Err(other),
    };
    let mut sized = market.clone();
    if form == QuantityForm::Whole {
        sized.increment = sized.increment.max(Qty::parse("1")?);
    }
    propose(mandate, account, &sized, risk, outputs, now)
}

/// Deployment's check that the profile can meet the opening policy: every asset class the mandate
/// allows, with the time in force its openings carry, has an opening form in its opening session
/// (US equities: regular; crypto: crypto). An empty map is refused (DEC-854 item 6).
///
/// # Errors
/// [`BuilderError::NoOpeningForm`] for the first class the profile cannot meet.
pub fn deployable(
    profile: &CapabilityProfile,
    openings: &BTreeMap<AssetClass, TimeInForce>,
) -> Result<(), BuilderError> {
    if openings.is_empty() {
        return Err(BuilderError::NoOpeningForm);
    }
    for (&asset_class, &time_in_force) in openings {
        let opening_session = match asset_class {
            AssetClass::UsEquity => MarketSession::Regular,
            AssetClass::Crypto => MarketSession::Crypto,
        };
        opening_form(profile, asset_class, opening_session, time_in_force)?;
    }
    Ok(())
}
