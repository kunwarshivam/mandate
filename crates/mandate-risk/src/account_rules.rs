//! E6-6's US account rules beside the session: §5.3 rules 2, 4 to 8 (check 4), buying power (§9.5,
//! check 7) and the `legacy_pdt` day-trade budget (§9.2, check 8).

use mandate_num::{Fraction, Qty, ShareIncrement, Usd};
use mandate_time::new_york_date_and_hour;

use crate::gate::Stop;
use crate::{
    AccountType, AssetClass, DayTradeRegime, GateError, GateInput, GatePass, ProposedKind,
    ReasonCode, Side, TimeInForce, Verdict, WorkingOrder,
};

/// The rules of §5.3 that deny an opening with no registered reason code: rule 2 (quantity at
/// least `min_order_size`, and whole shares in an instrument that is not fractionable) and rule 7
/// (a fractional equity order is TIF day, and §5.2 keeps it out of a bracket). A denial without a
/// code is unrepresentable in [`crate::CheckOutcome`] and minting one would break ES-09, so the
/// gate refuses to decide an opening that breaks one, which adds no risk (DEC-129 item 27). A
/// reduction is never refused for them: the broker judges its size, a sell of the whole position is
/// always valid, and `AGENTS.md` rule 3 forbids a safe default that blocks an exit.
pub(crate) fn unregistered_rules(input: &GateInput<'_>) -> Result<(), GateError> {
    let p = input.proposed;
    let i = input.instrument;
    let refuse = |rule| Err(GateError::Unimplemented(rule, "DEC-129 item 27"));
    if p.qty < i.min_order_size {
        return refuse("the reason code of §5.3 rule 2's minimum size");
    }
    let fractional = i.asset_class == AssetClass::UsEquity
        && p.qty.portion(Fraction::ONE, ShareIncrement::Whole)? != p.qty;
    if fractional && !i.fractionable {
        return refuse("the reason code of §5.3 rule 2's increment");
    }
    if fractional && (p.tif != TimeInForce::Day || matches!(p.kind, ProposedKind::Bracket { .. })) {
        return refuse("the reason code of §5.3 rule 7");
    }
    Ok(())
}

/// §5.3 rule 4, for a reduction: the sell quantity plus the agent's own open sells in the
/// instrument, protective legs included, is at most the agent's position. At the first pass the
/// agent's own protective orders are left out, because the executor cancels them first; the re-run
/// before submission counts them (§5.3's closing paragraph).
pub(crate) fn sell_available(input: &GateInput<'_>) -> Result<Option<Stop>, GateError> {
    let p = input.proposed;
    let first = input.pass == GatePass::First;
    let mut open_sells = Qty::ZERO;
    for (_, o) in input.account.working_orders.iter().filter(|(id, o)| {
        input.agent.working_orders.contains(id)
            && o.instrument == p.instrument
            && o.side == Side::Sell
            && !(first && o.protective)
    }) {
        open_sells = open_sells.checked_add(o.open_qty)?;
    }
    let held = input
        .agent
        .positions
        .get(&p.instrument)
        .copied()
        .unwrap_or(Qty::ZERO);
    Ok((p.qty.checked_add(open_sells)? > held)
        .then_some((Verdict::Deny, ReasonCode::SellExceedsAvailable)))
}

/// §5.3 rules 5, 6 and 8, for an opening. Rules 5 and 6 are both `working_order_limit`, and one
/// working non-protective order per instrument on the account (rule 6) is already one side at a
/// time (rule 5). They bind openings only: §9.6 lists them as market-conduct controls, which "deny
/// opening and increasing orders", and `AGENTS.md` rule 13 exempts every exit from conduct controls
/// (DEC-150 item 1). Rule 8: a plain equity opening in an instrument with a resting protective order
/// is `add_blocked_by_protective_order` (§5.4), because a risk-increasing order never cancels
/// protection; a bracket is a new tranche with its own, and crypto follows DEC-36's sequence.
pub(crate) fn one_working_order(input: &GateInput<'_>) -> Option<Stop> {
    let p = input.proposed;
    let here = |o: &&WorkingOrder| o.instrument == p.instrument;
    let resting = input.account.working_orders.values().filter(here);
    if resting.clone().any(|o| !o.protective) {
        return Some((Verdict::Deny, ReasonCode::WorkingOrderLimit));
    }
    let plain_equity = input.instrument.asset_class == AssetClass::UsEquity
        && !matches!(p.kind, ProposedKind::Bracket { .. });
    (plain_equity && resting.clone().any(|o| o.protective))
        .then_some((Verdict::Deny, ReasonCode::AddBlockedByProtectiveOrder))
}

/// Check 7's buying power (§9.5, DEC-34, DEC-129 items 8 and 16): quantity × limit + the fee
/// reservation against the lower of the model's and the broker's figure, both already net of
/// existing reservations, so nothing is subtracted again. Crypto compares with the broker's
/// non-marginable figure. A negative fee reservation could only loosen the bound, so it counts as
/// zero. A cash account's shortfall is settled cash, a margin account's is buying power. Only an
/// opening reaches here: buying power never denies an exit (`AGENTS.md` rule 13).
pub(crate) fn buying_power(input: &GateInput<'_>) -> Result<Option<Stop>, GateError> {
    let p = input.proposed;
    let a = input.account;
    let cost = p
        .qty
        .notional(p.limit_price)?
        .checked_add(p.fee_reservation.max(Usd::ZERO))?;
    let broker = if input.instrument.asset_class == AssetClass::Crypto {
        a.broker_non_marginable_buying_power
    } else {
        a.broker_buying_power
    };
    if cost <= a.model_buying_power.min(broker) {
        return Ok(None);
    }
    let code = match a.account_type {
        AccountType::Cash => ReasonCode::InsufficientSettledBuyingPower,
        AccountType::Margin => ReasonCode::InsufficientBuyingPower,
    };
    Ok(Some((Verdict::Deny, code)))
}

/// Check 8, §9.2's `legacy_pdt` budget for a US-equity opening in a margin account below the
/// equity threshold: allowed only if `remaining ≥ required`. `remaining` is `3 − count`, or 0 once
/// the account is flagged; at or above the threshold it is unlimited, so nothing is checked.
/// `required` is 1, plus 1 if the same security was sold earlier today, plus the open same-day
/// positions, plus the account's working same-day opening orders in other instruments. Crypto never
/// counts, `intraday_margin` denies nothing per order (DEC-129 item 7), and only an opening reaches
/// here, since exits are never denied for the day-trade count (`AGENTS.md` rule 13).
///
/// The budget is the account's, so `DayTradeLedger` must be too: `AgentSnapshot::day_trades` is
/// read as the account-wide ledger, which E6-6's second slice folds from every agent's fills on the
/// account (DEC-129 item 6, DEC-150 item 6).
pub(crate) fn day_trade_budget(input: &GateInput<'_>) -> Result<Option<Stop>, GateError> {
    let a = input.account;
    let applies = a.regime == DayTradeRegime::LegacyPdt
        && a.account_type == AccountType::Margin
        && input.instrument.asset_class == AssetClass::UsEquity
        && a.prior_close_equity < input.config.legacy_pdt_equity_threshold;
    if !applies {
        return Ok(None);
    }
    let ledger = &input.agent.day_trades;
    let remaining = if ledger.flagged_pattern_day_trader {
        0
    } else {
        3_u32.saturating_sub(ledger.window_count)
    };
    let instrument = &input.proposed.instrument;
    let (today, _) = new_york_date_and_hour(input.now)?;
    let working_elsewhere = a
        .working_orders
        .values()
        .filter(|o| {
            o.opening && !o.protective && o.submitted_on == today && o.instrument != *instrument
        })
        .count();
    let required = 1_usize
        .saturating_add(usize::from(ledger.sold_earlier_today.contains(instrument)))
        .saturating_add(ledger.open_same_day_positions.len())
        .saturating_add(working_elsewhere);
    let remaining = usize::try_from(remaining).map_err(|_| GateError::ConfigOutOfRange)?;
    Ok((required > remaining).then_some((Verdict::Deny, ReasonCode::LegacyPdtDayTradeBudget)))
}
