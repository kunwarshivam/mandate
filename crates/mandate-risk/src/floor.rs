//! Check 2's eligibility floor (trading spec §3.2, DEC-31), items 1 to 7 in list order, after the
//! working universe and before concentration. Only an opening or an increase reaches here: the
//! floor never applies to a risk-reducing order in a held instrument (§3.2's last paragraph, MI-1).

use mandate_num::{Price, Qty, Usd};
use mandate_time::UtcNanos;

use crate::gate::Stop;
use crate::{AssetClass, EtpClass, GateError, GateInput, ReasonCode, Verdict};

/// The first §3.2 item the instrument fails, or `None` when it passes all seven.
///
/// Item 1's universe half is the working-universe check just before this one; its `status` and
/// `tradable` half reports `not_in_working_universe`, the code the working universe already uses
/// (DEC-129 item 25 asks the founder to retire `not_in_universe`). Item 3's `ptp_no_exception` has
/// no code of its own and reports `ipo_not_tradable`, the one item-3 code the registry has. Items 4
/// to 6 are US-equity items and item 7 is crypto's: a price floor and a 20-day volume have no
/// meaning for a crypto pair, which has its own 30-day volume floor instead.
pub(crate) fn eligibility(input: &GateInput<'_>) -> Result<Option<Stop>, GateError> {
    let i = input.instrument;
    let config = input.config;
    let equity = i.asset_class == AssetClass::UsEquity;
    let failed = if !(i.status_active && i.tradable) {
        Some(ReasonCode::NotInWorkingUniverse)
    } else if equity && !i.exchange.is_some_and(|e| e.is_eligible()) {
        Some(ReasonCode::IneligibleExchange)
    } else if i.ipo || i.ptp_no_exception {
        Some(ReasonCode::IpoNotTradable)
    } else if equity {
        equity_items(input)?
    } else {
        below(i.median_dollar_volume_30d, config.crypto_liquidity_floor_usd)
            .then_some(ReasonCode::BelowLiquidityFloor)
    };
    Ok(failed.map(|code| (Verdict::Deny, code)))
}

/// Items 4 to 6 for a US equity: the price floor, the 20-day liquidity floor, then the ETP rule.
fn equity_items(input: &GateInput<'_>) -> Result<Option<ReasonCode>, GateError> {
    let i = input.instrument;
    let config = input.config;
    let prior_close = i.prior_close.map(per_share).transpose()?;
    if below(prior_close, config.price_floor) {
        return Ok(Some(ReasonCode::BelowPriceFloor));
    }
    if below(i.median_dollar_volume_20d, config.liquidity_floor_usd) {
        return Ok(Some(ReasonCode::BelowLiquidityFloor));
    }
    let enabled = input.mandate.leveraged_etps_enabled()
        && input.mandate.leveraged_etp_disclosure_accepted();
    let complex = i.etp != EtpClass::Plain;
    Ok((classification_stale(input)? || (complex && !enabled))
        .then_some(ReasonCode::LeveragedEtpNotEnabled))
}

/// §3.2 item 6's "older than the configured age", failing closed: a classification with no date
/// is as unknown as one past `etp_classification_max_age_s` (DEC-129 item 10). Exactly at the age
/// is not older, so it passes (DEC-129 item 15).
fn classification_stale(input: &GateInput<'_>) -> Result<bool, GateError> {
    let Some(at) = input.instrument.etp_classified_at else {
        return Ok(true);
    };
    let expires = at
        .secs()
        .checked_add(i64::from(input.config.etp_classification_max_age_s))
        .ok_or(GateError::ConfigOutOfRange)?;
    Ok(input.now > UtcNanos::from_parts(expires, at.nanos())?)
}

/// A floor item fails when the figure is missing or strictly below the floor: `≥ floor` passes.
fn below(figure: Option<Usd>, floor: Usd) -> bool {
    figure.is_none_or(|v| v < floor)
}

/// One share at `price`, as dollars, so a price compares against a dollar floor exactly.
fn per_share(price: Price) -> Result<Usd, GateError> {
    Ok(Qty::parse("1")?.notional(price)?)
}
