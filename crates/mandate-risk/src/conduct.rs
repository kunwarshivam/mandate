//! Check 5 (mark freshness and the price collar, trading-domain spec §8.2 and §9.6), check 6's
//! market-conduct controls (§9.6), the pacing they apply to an allowed exit, and §9.6's minimum
//! resting time on a cancel.
//!
//! Every control compares against `GateInput::now`, the risk clock's tick, and an instant or a
//! count the executor folded into [`crate::ConductState`], [`crate::MarketSnapshot`] or
//! [`crate::AccountSnapshot`]; nothing here reads a clock (ES-21).
//!
//! **Only an opening or an increase is ever denied here.** A risk exit, a protective order and a
//! kill switch's own order pass every control untouched. An owner exit is paced by the
//! participation caps only. A discretionary exit is paced by all of them: the collar prices it, the
//! participation caps slice it, and in the close window it is sent as a marketable limit, but it is
//! never denied (`AGENTS.md` rule 13, MI-1, DEC-129 item 11).

use std::collections::BTreeSet;

use mandate_num::{Adverse, Fraction, NumError, Price, Qty, ShareIncrement, TickRule};
use mandate_time::UtcNanos;

use crate::gate::Stop;
use crate::{
    AssetClass, CancelInput, Check, CheckOutcome, Computed, Decision, GateError, GateInput,
    InstrumentRestriction, Pacing, PacingControl, ProposedKind, Purpose, ReasonCode, RestingSide,
    SaneQuote, SessionAt, Side, Verdict, limits,
};

/// The quote a risk mark and the collar may use at `now` (§8.2). `None` when there is none or when
/// it is stamped after `now`, which no mark taken at `now` can be; `QuoteUnsane` when it is crossed
/// (a locked quote, `bid = ask`, is sane).
///
/// Its age is not judged here. §4.2 leaves the staleness threshold to the data profile's
/// configuration and DEC-168 item 5 makes the bound the reader's, so a quote older than the
/// profile's threshold never reaches the gate: the fold supplies `None` instead (DEC-163 item 1).
pub(crate) fn usable_quote(input: &GateInput<'_>) -> Result<Option<SaneQuote>, GateError> {
    let Some(quote) = input.market.quote.filter(|q| q.at <= input.now) else {
        return Ok(None);
    };
    if quote.bid > quote.ask {
        return Err(GateError::QuoteUnsane);
    }
    Ok(Some(quote))
}

/// Check 5 for an opening or an increase: a fresh quote-based risk mark, then the collar.
///
/// No usable quote, or a `stale_mark` restriction on the instrument (DEC-129 item 23), is
/// `stale_mark`: §8.2 says "opening orders require a fresh quote-based risk mark". A price outside
/// `[floor, ceiling]` of [`collar`] is `price_outside_collar`. A reducing purpose never reaches
/// here, since §8.2 lets it take any mark source and the collar only prices it.
pub(crate) fn mark_and_collar(input: &GateInput<'_>) -> Result<Option<Stop>, GateError> {
    let restricted = input
        .agent
        .instrument_restrictions
        .get(&input.proposed.instrument)
        .is_some_and(|r| r.contains(&InstrumentRestriction::StaleMark));
    let quote = usable_quote(input)?;
    let Some(quote) = quote.filter(|_| !restricted) else {
        return Ok(Some((Verdict::Deny, ReasonCode::StaleMark)));
    };
    let (floor, ceiling) = collar(input, quote)?;
    let limit = input.proposed.limit_price;
    Ok((limit < floor || limit > ceiling)
        .then_some((Verdict::Deny, ReasonCode::PriceOutsideCollar)))
}

/// §9.6's collar as the closed interval a limit may sit in, each end rounded so the interval gets
/// narrower ([`Price::collar_bound`]). The aggressive end is `ask × (1 + x)` for a buy and
/// `bid × (1 − x)` for a sell; the passive end is the 20% band measured from the same side of the
/// quote, `ask × (1 − band)` for a buy and `bid × (1 + band)` for a sell (DEC-163 item 2).
fn collar(input: &GateInput<'_>, quote: SaneQuote) -> Result<(Price, Price), GateError> {
    let x = collar_x(input);
    let band = input.config.collar_passive_band;
    Ok(match input.proposed.side {
        Side::Buy => (
            quote.ask.collar_bound(band, Adverse::Down)?,
            quote.ask.collar_bound(x, Adverse::Up)?,
        ),
        Side::Sell => (
            quote.bid.collar_bound(x, Adverse::Down)?,
            quote.bid.collar_bound(band, Adverse::Up)?,
        ),
    })
}

/// §9.6's tier: `collar_crypto_x` for crypto; for an equity `collar_liquid_x` when the 20-day
/// median dollar volume is at least the threshold (`≥`, the spec's own sign), `collar_other_x` when
/// it is known and below it, and the narrower `collar_liquid_x` when it is unknown, so the
/// ambiguous case admits fewer prices, not more (DEC-163 item 2).
fn collar_x(input: &GateInput<'_>) -> Fraction {
    let config = input.config;
    match input.instrument.asset_class {
        AssetClass::Crypto => config.collar_crypto_x,
        AssetClass::UsEquity => match input.instrument.median_dollar_volume_20d {
            Some(volume) if volume < config.collar_liquid_threshold_usd => config.collar_other_x,
            Some(_) | None => config.collar_liquid_x,
        },
    }
}

/// Check 6 for an opening or an increase, in §9.6's table order, then the mandate's orders per day
/// (DEC-163 item 3): the two participation caps, the order-to-fill ratio, the opposite-fill
/// interval, the close window, and self-trade prevention across related accounts.
pub(crate) fn conduct_controls(
    input: &GateInput<'_>,
    at: &SessionAt,
    computed: &mut Computed,
) -> Result<Option<Stop>, GateError> {
    let breached = Some((Verdict::Deny, ReasonCode::ConductLimitBreached));
    if !participation_holds(input)? || order_to_fill_breached(input) {
        return Ok(breached);
    }
    if inside_opposite_fill_interval(input)? {
        return Ok(Some((Verdict::Deny, ReasonCode::OppositeFillInterval)));
    }
    if at.close_window {
        return Ok(Some((Verdict::Deny, ReasonCode::CloseWindow)));
    }
    if self_trade(input) {
        return Ok(breached);
    }
    Ok(limits::orders_per_day(input, computed))
}

/// Both participation caps for an opening, compared exactly: the order against
/// `order_size_participation × trailing_5m_volume`, and the day's participation with it against
/// `daily_participation × adv_20d`. A volume the executor could not supply fails closed, as the
/// eligibility floor does for an unknown median (DEC-163 item 4).
///
/// Each cap is truncated at the 9 places a quantity holds, which changes no comparison: a
/// quantity on that grid exceeds the exact product exactly when it exceeds the truncated one.
fn participation_holds(input: &GateInput<'_>) -> Result<bool, GateError> {
    let (Some(trailing), Some(adv)) = (input.market.trailing_5m_volume, input.market.adv_20d)
    else {
        return Ok(false);
    };
    let qty = input.proposed.qty;
    let config = input.config;
    let order_cap =
        trailing.portion(config.order_size_participation, ShareIncrement::Fractional)?;
    let daily_cap = adv.portion(config.daily_participation, ShareIncrement::Fractional)?;
    let after = participation_today(input).checked_add(qty)?;
    Ok(qty <= order_cap && after <= daily_cap)
}

fn participation_today(input: &GateInput<'_>) -> Qty {
    input
        .conduct
        .participation_today
        .get(&input.proposed.instrument)
        .copied()
        .unwrap_or(Qty::ZERO)
}

/// §9.6's order-to-fill ratio, `orders ÷ max(fills, 1) ≤ order_to_fill_max`, evaluated once the
/// agent has sent `order_to_fill_min_orders` orders in the instrument today, and compared without
/// dividing: `orders > max × max(fills, 1)`. The proposal is not one of them: it is the order being
/// decided. The fold already leaves out exit-sequence and kill-switch cancels.
fn order_to_fill_breached(input: &GateInput<'_>) -> bool {
    let instrument = &input.proposed.instrument;
    let conduct = input.conduct;
    let config = input.config;
    let orders = conduct
        .orders_today_per_instrument
        .get(instrument)
        .copied()
        .unwrap_or(0);
    let fills = conduct
        .filled_today
        .get(instrument)
        .copied()
        .unwrap_or(0)
        .max(1);
    orders >= config.order_to_fill_min_orders
        && u64::from(orders) > u64::from(config.order_to_fill_max).saturating_mul(u64::from(fills))
}

/// The side an opening would trade against: a buy meets resting sells, and a sell resting buys.
fn opposite(side: Side) -> RestingSide {
    match side {
        Side::Buy => RestingSide::Sell,
        Side::Sell => RestingSide::Buy,
    }
}

/// §9.6: no opening "within" `opposite_fill_interval_s` after a fill on the other side in the same
/// instrument, the interval's last instant included: a fill exactly 60 s before `now` still blocks,
/// and one a nanosecond earlier does not (DEC-163 item 3).
fn inside_opposite_fill_interval(input: &GateInput<'_>) -> Result<bool, GateError> {
    let key = (
        input.proposed.instrument.clone(),
        opposite(input.proposed.side),
    );
    match input.conduct.last_opposite_fill_at.get(&key) {
        Some(last) => Ok(input.now <= after(*last, input.config.opposite_fill_interval_s)?),
        None => Ok(false),
    }
}

/// `at + secs`, or `ConfigOutOfRange` past the representable range.
fn after(at: UtcNanos, secs: u32) -> Result<UtcNanos, GateError> {
    let end = at
        .secs()
        .checked_add(i64::from(secs))
        .ok_or(GateError::ConfigOutOfRange)?;
    Ok(UtcNanos::from_parts(end, at.nanos())?)
}

/// §9.6's self-trade prevention: an opening is blocked while an order on the other side rests in
/// the same instrument in any account of the owner's related-accounts group, which the executor
/// supplies as [`crate::AccountSnapshot::related_account_resting`].
fn self_trade(input: &GateInput<'_>) -> bool {
    input
        .account
        .related_account_resting
        .get(&input.proposed.instrument)
        .is_some_and(|sides| sides.contains(&opposite(input.proposed.side)))
}

/// What the executor sends for an allowed order (DEC-129 item 11): the proposal as it stands, or a
/// [`Pacing`] when anything changed it. `None` for an opening or an increase, which is allowed as
/// proposed or not at all, and for every exit nothing touched.
///
/// - A market order where market orders are barred is sent as a marketable limit, whatever the
///   purpose (DEC-129 items 28 and 31, DEC-159): `market_barred`.
/// - A discretionary exit is priced by the collar and, in the close window, sent as a marketable
///   limit (§9.6).
/// - A discretionary or an owner exit is sliced by the participation caps (§9.6).
///
/// A risk exit and a protective order are paced by none of these (`AGENTS.md` rule 13).
pub(crate) fn pacing(
    input: &GateInput<'_>,
    purpose: Purpose,
    at: &SessionAt,
    market_barred: bool,
) -> Result<Option<Pacing>, GateError> {
    let proposed = input.proposed;
    let mut pacing = Pacing {
        qty: proposed.qty,
        limit_price: proposed.limit_price,
        marketable_limit_required: market_barred && proposed.kind == ProposedKind::Market,
        applied: BTreeSet::new(),
    };
    if purpose == Purpose::DiscretionaryExit {
        price_by_collar(input, &mut pacing)?;
        if at.close_window {
            pacing.marketable_limit_required = true;
            pacing.applied.insert(PacingControl::CloseWindow);
        }
    }
    if matches!(purpose, Purpose::DiscretionaryExit | Purpose::OwnerExit) {
        slice(input, &mut pacing)?;
    }
    Ok((pacing.marketable_limit_required || !pacing.applied.is_empty()).then_some(pacing))
}

/// The collar prices a discretionary exit rather than denying it: a limit below the aggressive
/// end is raised to it, one above the passive end is lowered to it, and a market order is sent as
/// a marketable limit at the aggressive end, since "the collar prices them" leaves no exit
/// unpriced (DEC-163 item 5). An equity price is then put on the Reg NMS grid inward, so it stays
/// inside the collar; a crypto price is left at the bound, because the instrument snapshot carries
/// no venue increment, and the executor's rounding against the order (§2.1) moves it inward too.
///
/// With no usable quote there is no collar to price by, and the exit goes as proposed: §8.2 lets a
/// reduction take any mark source, and a crossed quote refuses nothing that reduces risk.
fn price_by_collar(input: &GateInput<'_>, pacing: &mut Pacing) -> Result<(), GateError> {
    let Some(quote) = usable_quote(input).ok().flatten() else {
        return Ok(());
    };
    let (floor, ceiling) = collar(input, quote)?;
    let limit = input.proposed.limit_price;
    let market = input.proposed.kind == ProposedKind::Market;
    let priced = if market || limit < floor {
        on_grid(input, floor, Adverse::Down)?
    } else if limit > ceiling {
        on_grid(input, ceiling, Adverse::Up)?
    } else {
        return Ok(());
    };
    pacing.limit_price = priced;
    pacing.marketable_limit_required |= market;
    pacing.applied.insert(PacingControl::Collar);
    Ok(())
}

fn on_grid(input: &GateInput<'_>, price: Price, adverse: Adverse) -> Result<Price, GateError> {
    Ok(match input.instrument.asset_class {
        AssetClass::UsEquity => price.on_tick(TickRule::RegNmsEquity, adverse)?,
        AssetClass::Crypto => price,
    })
}

/// The participation caps slice an exit to the smaller of `order_size_participation ×
/// trailing_5m_volume` and what is left of `daily_participation × adv_20d` today, truncated to the
/// instrument's increment; the rest is re-proposed in a later interval or on a later day (§9.6,
/// DEC-129 item 11). Each control whose cap binds is named in [`Pacing::applied`], so a slice says
/// every limit it met.
///
/// A slice is never an order the broker would refuse, which would deny the exit (`AGENTS.md` rule
/// 13, DEC-163 item 4): a cap that works out to zero, like a volume the executor could not supply,
/// slices nothing, and a cap above zero but below the instrument's `min_order_size` slices at
/// `min_order_size`. A cap binds only where that slice is below the proposed quantity, so an exit
/// smaller than `min_order_size` goes whole.
fn slice(input: &GateInput<'_>, pacing: &mut Pacing) -> Result<(), GateError> {
    let proposed = input.proposed.qty;
    let increment = if input.instrument.fractionable {
        ShareIncrement::Fractional
    } else {
        ShareIncrement::Whole
    };
    let config = input.config;
    let mut caps = Vec::with_capacity(2);
    if let Some(trailing) = input.market.trailing_5m_volume {
        let cap = trailing.portion(config.order_size_participation, increment)?;
        caps.push((PacingControl::OrderSizeParticipation, cap));
    }
    if let Some(adv) = input.market.adv_20d {
        let daily = adv.portion(config.daily_participation, ShareIncrement::Fractional)?;
        let left = match daily.checked_sub(participation_today(input)) {
            Ok(left) => left,
            Err(NumError::Negative) => Qty::ZERO,
            Err(other) => return Err(other.into()),
        };
        caps.push((
            PacingControl::DailyParticipation,
            left.portion(Fraction::ONE, increment)?,
        ));
    }
    let minimum = input.instrument.min_order_size;
    for (control, cap) in caps {
        let slice = cap.max(minimum);
        if cap > Qty::ZERO && slice < proposed {
            pacing.qty = pacing.qty.min(slice);
            pacing.applied.insert(control);
        }
    }
    Ok(())
}

/// §9.6's minimum resting time: canceling a non-marketable opening order is denied
/// `min_resting_time` strictly inside `min_resting_time_s` of its resting instant, unless the
/// cancel precedes a risk-reducing order, which §9.6 exempts. The exemption is how every exit
/// sequence and kill switch cancels (DEC-163 item 7). Any other cancel is allowed.
///
/// The decision's purpose is the canceled order's: `Open` for an opening order, `Protective` for a
/// protective one, and otherwise `DiscretionaryExit`, since a working order records no origin to
/// tell its exit type by. Only check 6 applies to a cancel, so it is the only check listed.
pub(crate) fn evaluate_cancel(input: &CancelInput<'_>) -> Result<Decision, GateError> {
    let order = input.order;
    let purpose = if order.opening {
        Purpose::Open
    } else if order.protective {
        Purpose::Protective
    } else {
        Purpose::DiscretionaryExit
    };
    let bound = order.opening && !input.marketable && !input.precedes_risk_reducing_order;
    let too_soon =
        bound && input.now < after(input.resting_since, input.config.min_resting_time_s)?;
    let (verdict, reason, outcome) = if too_soon {
        (
            Verdict::Deny,
            Some(ReasonCode::MinRestingTime),
            CheckOutcome::Failed(Check::ConductControls, ReasonCode::MinRestingTime),
        )
    } else {
        (
            Verdict::Allow,
            None,
            CheckOutcome::Passed(Check::ConductControls),
        )
    };
    Ok(Decision {
        verdict,
        reason,
        purpose,
        pacing: None,
        checks: vec![outcome],
        computed: Computed::default(),
    })
}
