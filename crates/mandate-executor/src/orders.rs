//! The §5.7 order state machine over what the broker says: acknowledgments, query answers,
//! status updates, fills, and silence.

use mandate_accounting::{
    Account, AccountType, AssetClass, Execution, FeeKind, Input as AccountingInput, Record,
};
use mandate_canon::Value;
use mandate_num::{Qty, Usd};
use mandate_time::{NewYorkTime, new_york_instant};

use crate::batch::Batch;
use crate::codec::{side_name, state_name};
use crate::error::ExecutorError;
use crate::ids::ClientOrderId;
use crate::intent::resubmit;
use crate::payload::{int, text};
use crate::state::{EVERY_AGENT, ExecutorState, restriction_for};
use crate::types::{
    AccountState, BrokerAccount, BrokerFill, BrokerOrder, BrokerReject, BrokerRequest, EventId,
    Mode, OrderState, StatusMapping,
};

/// Trading-domain spec §5.7's broker status table. It is **total**: every value the table names
/// maps, and any other value is [`ExecutorError::UnmappedBrokerStatus`], which the step turns into
/// a pause and an alert — the table's own last row — never a silent no-op.
pub(crate) fn status_mapping(raw: &str) -> Result<StatusMapping, ExecutorError> {
    Ok(match raw {
        "new" | "accepted" | "pending_new" | "accepted_for_bidding" | "held" => {
            StatusMapping::Becomes(OrderState::Accepted)
        }
        "partially_filled" => StatusMapping::Becomes(OrderState::PartiallyFilled),
        "filled" => StatusMapping::Becomes(OrderState::Filled),
        "done_for_day" | "stopped" | "calculated" => StatusMapping::Unchanged,
        "pending_cancel" => StatusMapping::Becomes(OrderState::PendingCancel),
        "canceled" => StatusMapping::Becomes(OrderState::Canceled),
        "expired" => StatusMapping::Becomes(OrderState::Expired),
        "rejected" => StatusMapping::Becomes(OrderState::Rejected),
        "suspended" => StatusMapping::AcceptedFlaggedRestricted,
        "pending_replace" => StatusMapping::Becomes(OrderState::PendingReplace),
        "replaced" => StatusMapping::ReplacedPair,
        other => {
            return Err(ExecutorError::UnmappedBrokerStatus {
                status: other.to_owned(),
            });
        }
    })
}

/// Whether §5.7's diagram has an edge from `from` to `to`. Terminal states have none. An update
/// can be missed, so a state the broker reports is reachable from any earlier live state that
/// leads to it: an acknowledgment may already report a fill or a cancel, and a `replaced` may
/// arrive without the `pending_replace` before it.
pub(crate) fn legal(from: OrderState, to: OrderState) -> bool {
    use OrderState::{
        Abandoned, Accepted, Canceled, Expired, Filled, Intent, PartiallyFilled, PendingCancel,
        PendingReplace, Rejected, Replaced, Submitting, Unknown,
    };
    match from {
        Intent => matches!(to, Submitting | Abandoned),
        Submitting => matches!(
            to,
            Accepted | PartiallyFilled | Filled | Canceled | Expired | Rejected | Unknown
        ),
        Unknown => matches!(
            to,
            Accepted | PartiallyFilled | Filled | Canceled | Expired | Rejected | Intent
        ),
        Accepted => matches!(
            to,
            PartiallyFilled
                | Filled
                | Rejected
                | PendingCancel
                | PendingReplace
                | Replaced
                | Canceled
                | Expired
        ),
        PartiallyFilled => matches!(
            to,
            PartiallyFilled
                | Filled
                | PendingCancel
                | PendingReplace
                | Replaced
                | Canceled
                | Expired
        ),
        PendingCancel => matches!(to, Canceled | Filled | Accepted | PartiallyFilled),
        PendingReplace => matches!(
            to,
            Replaced | Filled | Canceled | Expired | Accepted | PartiallyFilled
        ),
        Filled | Canceled | Rejected | Expired | Replaced | Abandoned => false,
    }
}

/// The order a broker id names, if it is one this executor derived and the fold carries.
fn known(batch: &Batch<'_, '_>, raw: Option<&str>) -> Option<ClientOrderId> {
    let id = ClientOrderId::parse(raw?).ok()?;
    batch.view.orders.contains_key(&id).then_some(id)
}

/// Journals one transition, or — when §5.7 has no such edge — the attempt, marked ignored, with
/// the order left where it was (§5.7: "journaled and ignored").
pub(crate) fn transition(
    batch: &mut Batch<'_, '_>,
    id: &ClientOrderId,
    to: OrderState,
    mut extra: Vec<(&'static str, Value)>,
) -> Result<EventId, ExecutorError> {
    let from = batch
        .view
        .orders
        .get(id)
        .map_or(OrderState::Unknown, |order| order.state);
    let mut pairs = vec![("client_order_id", text(id.as_str()))];
    if legal(from, to) {
        pairs.push(("state", text(state_name(to))));
    } else {
        pairs.push(("state", text(state_name(from))));
        pairs.push(("attempted", text(state_name(to))));
        pairs.push(("ignored", Value::Bool(true)));
    }
    pairs.append(&mut extra);
    batch.journal("OrderStateChanged", None, pairs)
}

/// An order the broker described — an acknowledgment, a query answer, or a pushed update — folded
/// through the status table. The broker's word on the order is adopted; a status outside the table
/// pauses the agent and alerts.
pub(crate) fn described(
    batch: &mut Batch<'_, '_>,
    order: &BrokerOrder,
) -> Result<(), ExecutorError> {
    let Some(id) = known(batch, order.client_order_id.as_deref()) else {
        batch.request_reconciliation();
        return Ok(());
    };
    let status = vec![
        ("broker_status", text(order.status.clone())),
        ("filled_qty", text(order.filled_qty.to_string())),
    ];
    let current = batch
        .view
        .orders
        .get(&id)
        .map_or(OrderState::Unknown, |known| known.state);
    match status_mapping(&order.status) {
        Err(_) => unmapped(batch, &id)?,
        Ok(StatusMapping::Unchanged) => {
            transition(batch, &id, current, status)?;
        }
        Ok(StatusMapping::AcceptedFlaggedRestricted) => {
            transition(batch, &id, OrderState::Accepted, status)?;
            batch.request_reconciliation();
        }
        Ok(StatusMapping::ReplacedPair) => replaced(batch, &id, order, status)?,
        Ok(StatusMapping::Becomes(to)) => {
            let mut extra = status;
            if let Some(code) = &order.reject_code {
                extra.push(("reject_code", text(code.clone())));
            }
            transition(batch, &id, to, extra)?;
        }
    }
    let applied = batch
        .view
        .orders
        .get(&id)
        .map_or(Qty::ZERO, |known| known.filled_qty);
    if order.filled_qty > applied {
        batch.request_reconciliation();
    }
    Ok(())
}

/// §5.7's last row: a status outside the table pauses the agent and alerts the owner. The order
/// is left in its state, because a status that cannot be mapped is a state that cannot be derived.
fn unmapped(batch: &mut Batch<'_, '_>, id: &ClientOrderId) -> Result<(), ExecutorError> {
    let agent = batch
        .view
        .orders
        .get(id)
        .map(|order| order.agent.0.clone())
        .unwrap_or_default();
    let applied = batch.journal(
        "AgentModeApplied",
        None,
        vec![
            ("agent", text(agent)),
            ("to", text("paused")),
            ("restriction", text("broker_status_unmapped")),
            ("originated", Value::Bool(true)),
        ],
    )?;
    batch.notify(applied, "broker_status_unmapped");
    Ok(())
}

/// A transport failure or an ambiguous answer: every order whose request was in flight is
/// `Unknown`, its reservation held, and queried by client order id — never treated as rejected
/// and never resent (interpretation 10).
pub(crate) fn silence(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let in_flight: Vec<ClientOrderId> = batch
        .view
        .orders
        .values()
        .filter(|order| order.state == OrderState::Submitting)
        .map(|order| order.client_order_id.clone())
        .collect();
    for id in in_flight {
        transition(batch, &id, OrderState::Unknown, Vec::new())?;
        batch.broker(BrokerRequest::GetOrderByClientId(id));
    }
    Ok(())
}

/// The broker refusing our own `client_order_id` means the order is already there: it is queried,
/// never failed (E7-2 step 6).
pub(crate) fn duplicate(batch: &mut Batch<'_, '_>, raw: &str) -> Result<(), ExecutorError> {
    let Some(id) = known(batch, Some(raw)) else {
        return Ok(());
    };
    transition(batch, &id, OrderState::Unknown, Vec::new())?;
    batch.broker(BrokerRequest::GetOrderByClientId(id));
    Ok(())
}

/// One answer that the broker does not have the order. It is counted, never acted on alone: only
/// `unknown_absent_lookups` absences spanning `unknown_absent_window_s` confirm it, and only then
/// does the order return to `Intent` for the gate to re-run (§5.7, interpretation 9).
pub(crate) fn absent(batch: &mut Batch<'_, '_>, raw: &str) -> Result<(), ExecutorError> {
    let Some(id) = known(batch, Some(raw)) else {
        return Ok(());
    };
    if batch.view.orders.get(&id).map(|order| order.state) != Some(OrderState::Unknown) {
        return Ok(());
    }
    batch.journal(
        "OrderStateChanged",
        None,
        vec![
            ("client_order_id", text(id.as_str())),
            ("state", text(state_name(OrderState::Unknown))),
            ("lookup", text("absent")),
        ],
    )?;
    let config = batch.ports.config;
    let confirmed = batch.view.orders.get(&id).is_some_and(|order| {
        order.absent_lookups >= config.unknown_absent_lookups
            && order.first_absence_at.is_some_and(|first| {
                batch.at().secs().saturating_sub(first.secs()) >= config.unknown_absent_window_s
            })
    });
    if confirmed {
        transition(batch, &id, OrderState::Intent, Vec::new())?;
        resubmit(batch, &id)?;
    }
    Ok(())
}

/// A cancel the broker confirmed, which only the kill switch's and E7-4's cancels ask for: a later
/// slice of this stack.
pub(crate) fn cancelled() -> Result<(), ExecutorError> {
    Err(ExecutorError::Unimplemented { story: "E7-3" })
}

/// An `Unknown` order is queried again once `unknown_absent_window_s ÷ (N − 1)` seconds (rounded
/// up) have passed since it went `Unknown` or since its last absence, so N lookups span the whole
/// window and no faster.
pub(crate) fn lookups_due(batch: &mut Batch<'_, '_>) {
    let config = batch.ports.config;
    let gaps = i64::from(config.unknown_absent_lookups.saturating_sub(1).max(1));
    let spacing = config
        .unknown_absent_window_s
        .saturating_add(gaps.saturating_sub(1))
        .checked_div(gaps)
        .unwrap_or(config.unknown_absent_window_s);
    let now = batch.at().secs();
    let due: Vec<ClientOrderId> = batch
        .view
        .orders
        .values()
        .filter(|order| order.state == OrderState::Unknown)
        .filter(|order| {
            batch
                .view
                .details
                .get(&order.client_order_id)
                .and_then(|detail| detail.last_absence.or(detail.unknown_since))
                .is_some_and(|since| now.saturating_sub(since.secs()) >= spacing)
        })
        .map(|order| order.client_order_id.clone())
        .collect();
    for id in due {
        batch.broker(BrokerRequest::GetOrderByClientId(id));
    }
}

/// `replaced`: the old order becomes `Replaced` and the new one, linked to it under an id derived
/// from the event that records the replacement, becomes `Accepted` holding the old reservation
/// (§5.7, interpretation 26).
fn replaced(
    batch: &mut Batch<'_, '_>,
    id: &ClientOrderId,
    order: &BrokerOrder,
    mut status: Vec<(&'static str, Value)>,
) -> Result<(), ExecutorError> {
    let from = batch
        .view
        .orders
        .get(id)
        .map_or(OrderState::Unknown, |known| known.state);
    if !legal(from, OrderState::Replaced) {
        transition(batch, id, OrderState::Replaced, status)?;
        return Ok(());
    }
    let linked = ClientOrderId::for_replacement(&batch.next_id())?;
    status.push(("replaced_by", text(linked.as_str())));
    if let Some(broker) = &order.replaced_by_broker_order_id {
        status.push(("replaced_by_broker_order_id", text(broker.clone())));
    }
    transition(batch, id, OrderState::Replaced, status)?;
    batch.journal(
        "OrderStateChanged",
        None,
        vec![
            ("client_order_id", text(linked.as_str())),
            ("state", text(state_name(OrderState::Accepted))),
            ("replaces", text(id.as_str())),
        ],
    )?;
    Ok(())
}

/// One broker fill, applied by its fill id, once (§5.7). A fill for a terminal order is a
/// `LateFillApplied` that triggers a reconciliation. A fill that cannot be one of our orders' —
/// not our id, another instrument or side, or more than the order has left — is still applied to
/// accounting, unattributed, as external activity: every agent goes `exits_only` and the owner is
/// alerted (§7.1), so filled quantity never exceeds an order's quantity.
///
/// `before` is the state a reconciliation began from, when the fill is one it found missing: an
/// order the same reconciliation adopted as terminal was not terminal when the fill happened, so
/// the fill is an ordinary one, not a late one.
pub(crate) fn fill(
    batch: &mut Batch<'_, '_>,
    fill: &BrokerFill,
    before: Option<&ExecutorState>,
) -> Result<(), ExecutorError> {
    if batch.view.fills.contains(&fill.fill_id) {
        return Ok(());
    }
    let ours = known(batch, fill.client_order_id.as_deref()).filter(|id| {
        batch.view.orders.get(id).is_some_and(|order| {
            order.instrument == fill.instrument
                && order.side == fill.side
                && order
                    .qty
                    .checked_sub(order.filled_qty)
                    .is_ok_and(|left| fill.qty <= left)
        })
    });
    let terminal = ours
        .as_ref()
        .and_then(|id| before.unwrap_or(&batch.view).orders.get(id))
        .is_some_and(|order| order.state.is_terminal());
    let mut pairs = vec![
        ("fill_id", text(fill.fill_id.0.clone())),
        ("instrument", text(fill.instrument.as_str())),
        ("side", text(side_name(fill.side))),
        ("qty_gross", text(fill.qty.to_string())),
        ("price", text(fill.price.to_string())),
        ("fees", text(fill.fees.to_string())),
        ("trade_date", text(fill.trade_date.to_string())),
    ];
    let prior = ours
        .as_ref()
        .and_then(|id| batch.view.orders.get(id))
        .map_or(Qty::ZERO, |order| order.filled_qty);
    let Some(id) = ours else {
        let ingested = batch.journal(
            "ExternalActivityIngested",
            None,
            vec![("fill_id", text(fill.fill_id.0.clone()))],
        )?;
        batch.journal("FillApplied", None, pairs)?;
        every_agent(batch, Mode::ExitsOnly, &restriction_for(EXTERNAL))?;
        batch.notify(ingested, "external_activity");
        return Ok(());
    };
    pairs.push(("client_order_id", text(id.as_str())));
    let kind = if terminal {
        "LateFillApplied"
    } else {
        "FillApplied"
    };
    batch.journal(kind, None, pairs)?;
    simulated_fee(batch, fill, &id, prior)?;
    if terminal {
        batch.request_reconciliation();
        return Ok(());
    }
    if let Some(order) = batch.view.orders.get(&id) {
        let pending = matches!(
            order.state,
            OrderState::PendingCancel | OrderState::PendingReplace
        );
        let to = if order.filled_qty >= order.qty {
            OrderState::Filled
        } else if pending {
            order.state
        } else {
            OrderState::PartiallyFilled
        };
        if to != order.state {
            transition(batch, &id, to, Vec::new())?;
        }
    }
    Ok(())
}

/// A restriction on every agent of the account, originated here rather than copied.
pub(crate) fn every_agent(
    batch: &mut Batch<'_, '_>,
    mode: Mode,
    restriction: &str,
) -> Result<EventId, ExecutorError> {
    batch.journal(
        "AgentModeApplied",
        None,
        vec![
            ("agent", text(EVERY_AGENT)),
            ("to", text(crate::codec::mode_name(mode))),
            ("restriction", text(restriction)),
            ("originated", Value::Bool(true)),
        ],
    )
}

/// The subject external activity is recorded and acknowledged under (§7.1).
pub(crate) const EXTERNAL: &str = "external_activity";

/// Paper's regulatory fees (trading-domain spec §10, R-22). Alpaca paper charges no SEC, TAF, or
/// CAT fee, so on a paper stream the fee live would charge is computed with `mandate-accounting`'s
/// own rules from the effective fee configuration and booked `simulated = true`: in P&L and buying
/// power, and out of the cash comparison. Crypto fees are the broker's own on paper too, so only
/// equity fills are simulated.
///
/// Only a fill attributed to one of our orders books one. §10 does not say whether an
/// unattributed paper fill (external activity, §7.1) does; the backlog carries that question, and
/// until it is answered an unattributed fill books no simulated fee.
///
/// The fee is keyed as the journal keys every fee (journal spec §6): `family` `equities`, `day` the
/// fill's New York trade date as the account's calendar derives it, `accrued` at full precision,
/// and `charged` zero, because a simulated fee is never charged by the broker. The components
/// travel beside them.
///
/// DEC-87's per-order TAF cap is `max(0, cap − TAF already charged on the fill's client order)`.
/// The order's `prior` filled quantity is replayed first as one earlier execution under the same
/// client order id, so the account's own TAF rule sees what the order has already been charged:
/// under `per_order` its cumulative TAF is `min(cap, rate × quantity)` however it was split, and
/// under `per_execution` the earlier execution changes nothing.
fn simulated_fee(
    batch: &mut Batch<'_, '_>,
    fill: &BrokerFill,
    id: &ClientOrderId,
    prior: Qty,
) -> Result<(), ExecutorError> {
    let equity =
        batch.ports.instruments.asset_class(&fill.instrument) == Some(AssetClass::UsEquity);
    if batch.view.environment() != Some("paper") || !equity {
        return Ok(());
    }
    let executed_at = new_york_instant(fill.trade_date, NewYorkTime::new(12, 0)?)?;
    let execution = |fill_id: String, qty_gross: Qty| Execution {
        fill_id,
        client_order_id: Some(id.as_str().to_owned()),
        instrument: fill.instrument.clone(),
        asset_class: AssetClass::UsEquity,
        side: fill.side,
        qty_gross,
        price: fill.price,
        liquidity: None,
        executed_at,
    };
    let fees = batch.ports.fees;
    let mut account = Account::opening(AccountType::Margin, Usd::ZERO, Vec::new());
    if !prior.is_zero() {
        let earlier = execution(format!("{}:earlier", fill.fill_id.0), prior);
        account = account
            .apply(&AccountingInput::Fill(earlier), fees)?
            .account;
    }
    let applied = account.apply(
        &AccountingInput::Fill(execution(fill.fill_id.0.clone(), fill.qty)),
        fees,
    )?;
    let Record::Fill {
        trade_date: Some(day),
        fees: components,
        ..
    } = applied.record
    else {
        return Ok(());
    };
    let accrued = components
        .iter()
        .try_fold(Usd::ZERO, |total, fee| total.checked_add(fee.usd))?;
    let mut pairs = vec![
        ("family", text("equities")),
        ("day", text(day.to_string())),
        ("accrued", text(accrued.to_string())),
        ("charged", text("0")),
        ("simulated", Value::Bool(true)),
        ("client_order_id", text(id.as_str())),
    ];
    for fee in &components {
        pairs.push((fee_name(fee.kind), text(fee.usd.to_string())));
    }
    batch.journal("FeesCharged", None, pairs)?;
    Ok(())
}

/// An account snapshot the broker pushed: journaled without the account number or id, which the
/// type has nowhere to hold (journal spec §6.4), and read against §7.3's first row.
pub(crate) fn account(
    batch: &mut Batch<'_, '_>,
    account: &BrokerAccount,
) -> Result<(), ExecutorError> {
    batch.journal(
        "AccountStateObserved",
        None,
        vec![
            ("status", text(account.status.clone())),
            ("crypto_status", text(account.crypto_status.clone())),
            ("trading_blocked", Value::Bool(account.trading_blocked)),
            ("account_blocked", Value::Bool(account.account_blocked)),
            (
                "trade_suspended_by_user",
                Value::Bool(account.trade_suspended_by_user),
            ),
            ("multiplier", int(u64::from(account.multiplier))?),
            ("equity", text(account.equity.to_string())),
            ("cash", text(account.cash.to_string())),
            ("buying_power", text(account.buying_power.to_string())),
            (
                "non_marginable_buying_power",
                text(account.non_marginable_buying_power.to_string()),
            ),
            ("accrued_fees", text(account.accrued_fees.to_string())),
        ],
    )?;
    let blocked = account.status != "ACTIVE"
        || account.trading_blocked
        || account.account_blocked
        || account.trade_suspended_by_user;
    if blocked {
        restrict(
            batch,
            AccountState::Blocked,
            Mode::Paused,
            "account_trading_blocked",
        )?;
    }
    Ok(())
}

/// Stores a detected restriction as account state and applies its agent effect to every agent
/// (§7.3), alerting the owner. A restriction already in force is not journaled again.
fn restrict(
    batch: &mut Batch<'_, '_>,
    state: AccountState,
    mode: Mode,
    reason: &'static str,
) -> Result<(), ExecutorError> {
    if batch.view.account_state >= state {
        return Ok(());
    }
    let name = match state {
        AccountState::Blocked => "blocked",
        AccountState::ClosingOnly => "closing_only",
        AccountState::Active => "active",
    };
    let changed = batch.journal(
        "AccountRestrictionChanged",
        None,
        vec![("restriction", text(name)), ("reason_code", text(reason))],
    )?;
    every_agent(batch, mode, reason)?;
    batch.notify(changed, reason);
    Ok(())
}

/// A reject the broker answered with. Journaled with its code; an order of ours it names is
/// `Rejected`, which releases its reservation; a closing-only message, or the configured run of
/// 403s with no known order-level cause, restricts the account to closing only (§7.3). A reject
/// naming an id we do not know counts toward that threshold and is nothing more.
pub(crate) fn reject(
    batch: &mut Batch<'_, '_>,
    reject: &BrokerReject,
) -> Result<(), ExecutorError> {
    let mut pairs = vec![
        ("http_status", int(u64::from(reject.http_status))?),
        ("message", text(reject.message.clone())),
    ];
    if let Some(raw) = &reject.client_order_id {
        pairs.push(("client_order_id", text(raw.clone())));
    }
    if let Some(code) = &reject.code {
        pairs.push(("code", text(code.clone())));
    }
    batch.journal("RejectObserved", None, pairs)?;
    if let Some(id) = known(batch, reject.client_order_id.as_deref()) {
        let extra = reject
            .code
            .iter()
            .map(|code| ("reject_code", text(code.clone())))
            .collect();
        transition(batch, &id, OrderState::Rejected, extra)?;
    }
    let message = reject.message.to_ascii_lowercase();
    let closing_only = message.contains("closing") || message.contains("restricted");
    let threshold = batch.view.consecutive_403s >= batch.ports.config.restriction_403_threshold;
    if closing_only || threshold {
        restrict(
            batch,
            AccountState::ClosingOnly,
            Mode::ExitsOnly,
            "account_restricted",
        )?;
        if threshold {
            batch.broker(BrokerRequest::GetAccount);
        }
    }
    Ok(())
}

/// The payload name of one fee component.
fn fee_name(kind: FeeKind) -> &'static str {
    match kind {
        FeeKind::Sec => "sec",
        FeeKind::Taf => "taf",
        FeeKind::Cat => "cat",
        FeeKind::CryptoAsset => "crypto_asset",
        FeeKind::CryptoUsd => "crypto_usd",
    }
}
