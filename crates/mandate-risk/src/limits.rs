//! The mandate limits of mandate spec §5.3 as the gate's checks 2, 6 and 7 read them, and the
//! account's own 1× bound of trading spec §9.3. Only an opening or an increase reaches here: every
//! exit returns before these limits, which is MI-1.

use mandate_num::Usd;
use mandate_time::UtcNanos;

use crate::gate::Stop;
use crate::spec_types::RiskLimits;
use crate::{AssetId, Computed, GateError, GateInput, ReasonCode, RiskSnapshot, Verdict};

/// Check 2's mandate items after the working universe, in `ref.py`'s `gate` order:
/// concentration (position + working + proposed against min(usd, fraction × E)), order size,
/// then the re-entry cooldown.
pub(crate) fn position_order_and_cooldown(
    input: &GateInput<'_>,
    computed: &mut Computed,
) -> Result<Option<Stop>, GateError> {
    let instrument = &input.proposed.instrument;
    let limits = input.mandate.risk();
    let order = order_usd(input)?;
    let cap = position_cap(limits, input.risk)?;
    let held = input
        .agent
        .market_values
        .get(instrument)
        .copied()
        .unwrap_or(Usd::ZERO);
    let total = held
        .checked_add(working_cost(input, Some(instrument), false)?)?
        .checked_add(order)?;
    computed.instrument_total = Some(total);
    computed.cap = Some(cap);
    computed.order_usd = Some(order);
    if total > cap {
        return Ok(Some((Verdict::Deny, ReasonCode::ConcentrationLimit)));
    }
    if order > limits.max_order_usd {
        return Ok(Some((Verdict::Deny, ReasonCode::MaxOrderSize)));
    }
    reentry_cooldown(input, computed)
}

/// Mandate §5.3's per-instrument cap, `min(max_position_usd, max_position_fraction × E)`, exact:
/// the one cap check 2's concentration and §5.5's trim both read (DEC-399 item 3).
pub(crate) fn position_cap(limits: &RiskLimits, risk: &RiskSnapshot) -> Result<Usd, GateError> {
    Ok(limits.max_position_usd.min(
        risk.agent_equity
            .times_fraction(limits.max_position_fraction)?,
    ))
}

/// Mandate §5.3: no opening within `reentry_cooldown_s` of "the agent's last exit fill in any
/// instrument of the group"; an instrument with no group is a group of one. The latest in-group
/// exit binds, and it is the one `computed` reports.
fn reentry_cooldown(
    input: &GateInput<'_>,
    computed: &mut Computed,
) -> Result<Option<Stop>, GateError> {
    let instrument = &input.proposed.instrument;
    let groups = &input.agent.instrument_groups;
    let last_in_group = input
        .agent
        .last_exit_fill_at
        .iter()
        .filter(
            |(other, _)| match (groups.get(instrument), groups.get(*other)) {
                (Some(mine), Some(theirs)) => mine == theirs,
                (None, None) => *other == instrument,
                _ => false,
            },
        )
        .max_by_key(|(_, last)| **last);
    let Some((other, last)) = last_in_group else {
        return Ok(None);
    };
    let ends = UtcNanos::from_parts(
        last.secs()
            .checked_add(i64::from(input.mandate.risk().reentry_cooldown_s))
            .ok_or(GateError::ConfigOutOfRange)?,
        last.nanos(),
    )?;
    if input.now < ends {
        computed.last_exit_fill_at = Some(*last);
        computed.instrument = Some(other.clone());
        return Ok(Some((Verdict::Deny, ReasonCode::ReentryCooldown)));
    }
    Ok(None)
}

/// Check 6's mandate item: each client order id counts once, rejected ones included, so the
/// proposal is the `orders_today + 1`th and is denied once the count has reached the limit.
pub(crate) fn orders_per_day(input: &GateInput<'_>, computed: &mut Computed) -> Option<Stop> {
    let today = input.agent.orders_today;
    computed.orders_today = Some(today);
    (today >= input.mandate.risk().max_orders_per_day)
        .then_some((Verdict::Deny, ReasonCode::MaxOrdersPerDay))
}

/// Check 7's gross exposure (§9.3): the account at 1×, then the agent's mandate limit, each
/// Σ |MV| + working opening orders + the proposal against min(limit, equity).
///
/// An account-1× denial carries no figures: `Computed` has keys for the agent's `gross` and
/// `gross_limit` only, and #136's API has none for the account's, so the deny is told apart from
/// the agent-limit one only by `computed.gross` being unset (backlog, #160 review).
pub(crate) fn gross_exposure(
    input: &GateInput<'_>,
    computed: &mut Computed,
) -> Result<Option<Stop>, GateError> {
    let order = order_usd(input)?;
    let account_gross = sum_abs(input.account.market_values.values())?
        .checked_add(working_cost(input, None, true)?)?
        .checked_add(order)?;
    if account_gross > input.account.equity {
        return Ok(Some((Verdict::Deny, ReasonCode::GrossExposureLimit)));
    }
    let gross = sum_abs(input.agent.market_values.values())?
        .checked_add(working_cost(input, None, false)?)?
        .checked_add(order)?;
    let limit = input
        .mandate
        .risk()
        .max_gross_exposure_usd
        .min(input.risk.agent_equity);
    computed.gross = Some(gross);
    computed.gross_limit = Some(limit);
    Ok((gross > limit).then_some((Verdict::Deny, ReasonCode::GrossExposureLimit)))
}

fn order_usd(input: &GateInput<'_>) -> Result<Usd, GateError> {
    Ok(input.proposed.qty.notional(input.proposed.limit_price)?)
}

/// Σ `max_cost` of the working opening orders, the agent's own or (`whole_account`) every agent's,
/// in one instrument or in all of them. Protective legs reduce risk and are not exposure.
fn working_cost(
    input: &GateInput<'_>,
    instrument: Option<&AssetId>,
    whole_account: bool,
) -> Result<Usd, GateError> {
    let mut sum = Usd::ZERO;
    for order in input.account.working_orders.values() {
        let counted = order.opening
            && !order.protective
            && (whole_account || order.agent == input.agent.agent)
            && instrument.is_none_or(|i| *i == order.instrument);
        if counted {
            sum = sum.checked_add(order.max_cost)?;
        }
    }
    Ok(sum)
}

fn sum_abs<'v>(values: impl Iterator<Item = &'v Usd>) -> Result<Usd, GateError> {
    let mut sum = Usd::ZERO;
    for value in values {
        sum = sum.checked_add(value.abs())?;
    }
    Ok(sum)
}
