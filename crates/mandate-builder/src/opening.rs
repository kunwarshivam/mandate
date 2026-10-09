//! The opening policy intersected with the broker's capability profile (E7-23 B3; trading spec
//! §5.1 and §5.2; DEC-529 item 2, DEC-531 items 2 and 3, DEC-854).
//!
//! The platform's policy opens with a limit order, sized as a quantity, and a US equity only in
//! the regular session. The profile says which quantity forms and times in force a limit order may
//! take in each row. The builder sizes to what both allow. An empty intersection refuses the buy
//! before any intent, and never falls back to another order type (LT-3). It never touches an exit
//! (`AGENTS.md` rule 13). Shared code never names a broker (LT-2).

use std::collections::BTreeMap;

use mandate_domain::{AssetClass, CapabilityProfile, MarketSession, QuantityForm, TimeInForce};
use mandate_time::UtcNanos;

use crate::BuilderError;
use crate::builder::{AccountSnapshot, BuilderMandate, Market, ModelOutput, Proposal, RiskContext};

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
    let _ = (profile, asset_class, session, time_in_force);
    Err(BuilderError::Unimplemented)
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
    let _ = (venue, mandate, account, market, risk, outputs, now);
    Err(BuilderError::Unimplemented)
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
    let _ = (profile, openings);
    Err(BuilderError::Unimplemented)
}
