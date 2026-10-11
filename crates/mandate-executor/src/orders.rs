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
use crate::listing::{lookup_target, query_unknown, queryable};
use crate::payload::{int, text};
use crate::protection::{overdue, protection_cancelled};
use crate::state::{EVERY_AGENT, ExecutorState, restriction_for};
use crate::types::{
    AccountState, BrokerAccount, BrokerFill, BrokerOrder, BrokerReject, BrokerRequest, EventId,
    Mode, OrderState, Purpose, StatusMapping,
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
pub(crate) fn known(batch: &Batch<'_, '_>, raw: Option<&str>) -> Option<ClientOrderId> {
    let id = ClientOrderId::parse(raw?).ok()?;
    batch.view.orders.contains_key(&id).then_some(id)
}

/// The complete nullable and boolean evidence carried by `OrderStateChanged` (DEC-459).
#[derive(Default)]
pub(crate) struct StateEvidence {
    pub(crate) attempted: Option<OrderState>,
    pub(crate) broker_status: Option<String>,
    pub(crate) filled_qty: Option<Qty>,
    pub(crate) reject_code: Option<String>,
    pub(crate) replaces: Option<ClientOrderId>,
    pub(crate) replaced_by: Option<ClientOrderId>,
    pub(crate) replaced_by_broker_order_id: Option<String>,
    pub(crate) lookup_absent: bool,
    pub(crate) ignored: bool,
    pub(crate) cancel_requested: bool,
    pub(crate) cancel_confirmed: bool,
    pub(crate) cancel_overdue: bool,
    pub(crate) adopted: bool,
    pub(crate) ladder_step: bool,
}

fn nullable(value: Option<Value>) -> Value {
    value.unwrap_or(Value::Null)
}

/// The one constructor for every newly journaled `OrderStateChanged`; [`Batch::journal`] appends
/// its `risk_clock`.
fn state_change_fields(
    id: &ClientOrderId,
    state: OrderState,
    evidence: StateEvidence,
) -> Vec<(&'static str, Value)> {
    vec![
        ("client_order_id", text(id.as_str())),
        ("state", text(state_name(state))),
        (
            "attempted",
            nullable(evidence.attempted.map(|state| text(state_name(state)))),
        ),
        ("broker_status", nullable(evidence.broker_status.map(text))),
        (
            "filled_qty",
            nullable(evidence.filled_qty.map(|qty| text(qty.to_string()))),
        ),
        ("reject_code", nullable(evidence.reject_code.map(text))),
        (
            "replaces",
            nullable(evidence.replaces.map(|order| text(order.as_str()))),
        ),
        (
            "replaced_by",
            nullable(evidence.replaced_by.map(|order| text(order.as_str()))),
        ),
        (
            "replaced_by_broker_order_id",
            nullable(evidence.replaced_by_broker_order_id.map(text)),
        ),
        (
            "lookup",
            if evidence.lookup_absent {
                text("absent")
            } else {
                Value::Null
            },
        ),
        ("ignored", Value::Bool(evidence.ignored)),
        ("cancel_requested", Value::Bool(evidence.cancel_requested)),
        ("cancel_confirmed", Value::Bool(evidence.cancel_confirmed)),
        ("cancel_overdue", Value::Bool(evidence.cancel_overdue)),
        ("adopted", Value::Bool(evidence.adopted)),
        ("ladder_step", Value::Bool(evidence.ladder_step)),
    ]
}

pub(crate) fn record_state_change(
    batch: &mut Batch<'_, '_>,
    id: &ClientOrderId,
    state: OrderState,
    evidence: StateEvidence,
) -> Result<EventId, ExecutorError> {
    batch.journal(
        "OrderStateChanged",
        None,
        state_change_fields(id, state, evidence),
    )
}

/// Journals one transition, or — when §5.7 has no such edge — the attempt, marked ignored, with
/// the order left where it was (§5.7: "journaled and ignored").
pub(crate) fn transition(
    batch: &mut Batch<'_, '_>,
    id: &ClientOrderId,
    to: OrderState,
    mut evidence: StateEvidence,
) -> Result<EventId, ExecutorError> {
    let from = batch
        .view
        .orders
        .get(id)
        .map_or(OrderState::Unknown, |order| order.state);
    let state = if legal(from, to) {
        to
    } else {
        evidence.attempted = Some(to);
        evidence.ignored = true;
        from
    };
    record_state_change(batch, id, state, evidence)
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
    let status = StateEvidence {
        broker_status: Some(order.status.clone()),
        filled_qty: Some(order.filled_qty),
        ..StateEvidence::default()
    };
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
            transition(
                batch,
                &id,
                to,
                StateEvidence {
                    reject_code: order.reject_code.clone(),
                    ..status
                },
            )?;
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
    crate::reconcile::settle_doubted_placements(batch, &id, order)
}

/// §5.7's last row: a status outside the table pauses the agent and alerts the owner. The order
/// is left in its state, because a status that cannot be mapped is a state that cannot be derived.
fn unmapped(batch: &mut Batch<'_, '_>, id: &ClientOrderId) -> Result<(), ExecutorError> {
    let (agent, to) = match batch.view.orders.get(id).map(|order| order.agent.clone()) {
        Some(Some(agent)) => (agent.0, "paused"),
        Some(None) => (EVERY_AGENT.to_owned(), "exits_only"),
        None => (String::new(), "paused"),
    };
    let applied = batch.journal(
        "AgentModeApplied",
        None,
        vec![
            ("agent", text(agent)),
            ("to", text(to)),
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
        transition(batch, &id, OrderState::Unknown, StateEvidence::default())?;
        query_unknown(batch, id)?;
    }
    Ok(())
}

/// The broker refusing our own `client_order_id` means the order is already there: it is queried,
/// never failed (E7-2 step 6).
pub(crate) fn duplicate(batch: &mut Batch<'_, '_>, raw: &str) -> Result<(), ExecutorError> {
    let Some(id) = known(batch, Some(raw)) else {
        return Ok(());
    };
    transition(batch, &id, OrderState::Unknown, StateEvidence::default())?;
    query_unknown(batch, id)
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
    record_state_change(
        batch,
        &id,
        OrderState::Unknown,
        StateEvidence {
            lookup_absent: true,
            ..StateEvidence::default()
        },
    )?;
    let config = batch.ports.config;
    let confirmed = batch.view.orders.get(&id).is_some_and(|order| {
        order.absent_lookups >= config.unknown_absent_lookups
            && order.first_absence_at.is_some_and(|first| {
                batch.at().secs().saturating_sub(first.secs()) >= config.unknown_absent_window_s
            })
    });
    if confirmed {
        transition(batch, &id, OrderState::Intent, StateEvidence::default())?;
        resubmit(batch, &id)?;
    }
    Ok(())
}

/// A cancel the broker confirmed (§5.7): the order it names becomes `Canceled`, which releases its
/// reservation and clears the unconfirmed cancel that held its instrument (§5.4). An id this
/// executor did not derive, or does not carry, asks for a reconciliation rather than being
/// dropped. The exits the cancel held back are released after the step, as after any other.
pub(crate) fn cancelled(batch: &mut Batch<'_, '_>, raw: &str) -> Result<(), ExecutorError> {
    let Some(id) = known(batch, Some(raw)) else {
        batch.request_reconciliation();
        return Ok(());
    };
    transition(
        batch,
        &id,
        OrderState::Canceled,
        StateEvidence {
            cancel_confirmed: true,
            ..StateEvidence::default()
        },
    )?;
    match batch.view.orders.get(&id) {
        Some(order) if order.purpose == Purpose::Protective => {
            let instrument = order.instrument.clone();
            protection_cancelled(batch, &instrument, &id)
        }
        _ => Ok(()),
    }
}

/// An `Unknown` order is queried again once `unknown_absent_window_s ÷ (N − 1)` seconds (rounded
/// up) have passed since it went `Unknown` or since its last absence, so N lookups span the whole
/// window and no faster. A bracket placement's doubt is asked after by the entry its id names,
/// never by the handle no broker order carries ([`lookup_target`], DEC-878 item 1).
pub(crate) fn lookups_due(batch: &mut Batch<'_, '_>) {
    if !queryable(&batch.view) {
        return;
    }
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
        batch.broker(BrokerRequest::GetOrderByClientId(lookup_target(
            &batch.view,
            &id,
        )));
    }
}

/// `replaced`: the old order becomes `Replaced` and the new one, linked to it under an id derived
/// from the event that records the replacement, becomes `Accepted` holding the old reservation
/// (§5.7, interpretation 26).
fn replaced(
    batch: &mut Batch<'_, '_>,
    id: &ClientOrderId,
    order: &BrokerOrder,
    status: StateEvidence,
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
    transition(
        batch,
        id,
        OrderState::Replaced,
        StateEvidence {
            replaced_by: Some(linked.clone()),
            replaced_by_broker_order_id: order.replaced_by_broker_order_id.clone(),
            ..status
        },
    )?;
    record_state_change(
        batch,
        &linked,
        OrderState::Accepted,
        StateEvidence {
            replaces: Some(id.clone()),
            ..StateEvidence::default()
        },
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
        every_agent_alerted(
            batch,
            Mode::ExitsOnly,
            &restriction_for(EXTERNAL),
            ingested,
            "external_activity",
        )?;
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
            transition(batch, &id, to, StateEvidence::default())?;
        }
    }
    Ok(())
}

/// Every agent of the account to `mode` under `restriction`, and the owner alerted about
/// `subject` under `key`, in one call: the `*` restriction reaches an agent the executor has not
/// yet seen (DEC-133 item 39), and the alert cannot be split from it (§7.1, §7.3). Originated here
/// rather than copied. The alert carries the subject event's id and a message key only
/// (`AGENTS.md` rule 6).
pub(crate) fn every_agent_alerted(
    batch: &mut Batch<'_, '_>,
    mode: Mode,
    restriction: &str,
    subject: EventId,
    key: &'static str,
) -> Result<(), ExecutorError> {
    batch.journal(
        "AgentModeApplied",
        None,
        vec![
            ("agent", text(EVERY_AGENT)),
            ("to", text(crate::codec::mode_name(mode))),
            ("restriction", text(restriction)),
            ("originated", Value::Bool(true)),
        ],
    )?;
    batch.notify(subject, key);
    Ok(())
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
    batch.journal("AccountStateObserved", None, account_fields(account)?)?;
    account_restriction(batch, account)
}

/// §7.3's first row, read on any account the broker reports, pushed or read by a reconciliation:
/// a status other than `ACTIVE`, or any blocking flag, restricts the account as blocked and pauses
/// every agent, once.
pub(crate) fn account_restriction(
    batch: &mut Batch<'_, '_>,
    account: &BrokerAccount,
) -> Result<(), ExecutorError> {
    let blocked = account.status != "ACTIVE"
        || account.trading_blocked
        || account.account_blocked
        || account.trade_suspended_by_user;
    if blocked {
        restrict(batch, Restriction::Blocked, "account_trading_blocked")?;
    }
    Ok(())
}

/// Every account field §7.2 and §7.3 name, as journaled: never the account number or id, which
/// [`BrokerAccount`] has nowhere to hold (journal spec §6.4).
pub(crate) fn account_fields(
    account: &BrokerAccount,
) -> Result<Vec<(&'static str, Value)>, ExecutorError> {
    Ok(vec![
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
    ])
}

/// The two restrictions §7.3 detects. There is no variant for an active account, so a restriction
/// can never be written as `active`, which the fold refuses.
#[derive(Debug, Clone, Copy)]
enum Restriction {
    ClosingOnly,
    Blocked,
}

impl Restriction {
    fn state(self) -> AccountState {
        match self {
            Self::ClosingOnly => AccountState::ClosingOnly,
            Self::Blocked => AccountState::Blocked,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::ClosingOnly => "closing_only",
            Self::Blocked => "blocked",
        }
    }

    /// §7.3's agent effect: closing only restricts every agent to exits, and a blocked account
    /// pauses them.
    fn mode(self) -> Mode {
        match self {
            Self::ClosingOnly => Mode::ExitsOnly,
            Self::Blocked => Mode::Paused,
        }
    }
}

/// Stores a detected restriction as account state and applies its agent effect to every agent
/// (§7.3), alerting the owner. A restriction already in force is not journaled again, and the
/// answer says whether this one was new.
fn restrict(
    batch: &mut Batch<'_, '_>,
    restriction: Restriction,
    reason: &'static str,
) -> Result<bool, ExecutorError> {
    if batch.view.account_state >= restriction.state() {
        return Ok(false);
    }
    let changed = batch.journal(
        "AccountRestrictionChanged",
        None,
        vec![
            ("restriction", text(restriction.name())),
            ("reason_code", text(reason)),
        ],
    )?;
    every_agent_alerted(batch, restriction.mode(), reason, changed, reason)?;
    Ok(true)
}

/// A reject the broker answered with. Journaled with its status, code and message; an order of
/// ours it names is `Rejected`, which releases its reservation — unless a cancel of the order is
/// outstanding, in any live state: then the broker refused the cancel, not the order, and the
/// order is queried as overdue ([`overdue`], DEC-160 (7), (13)). The account is restricted to
/// closing only (§7.3) by DEC-143's reading: a message that says `closing` or `restricted`, since
/// no connector reject table maps a `code` yet, or the configured run of consecutive 403s, every
/// 403 counting, a named order's included. A reject naming an id we do not know counts toward
/// that run and is nothing more. The account is asked for again once, when the run restricts it,
/// not at every 403 after.
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
    let cancelling = known(batch, reject.client_order_id.as_deref()).filter(|id| {
        batch
            .view
            .orders
            .get(id)
            .is_some_and(|order| order.cancel_unconfirmed)
    });
    if let Some(id) = cancelling {
        overdue(batch, &id)?;
    } else if let Some(id) = known(batch, reject.client_order_id.as_deref()) {
        transition(
            batch,
            &id,
            OrderState::Rejected,
            StateEvidence {
                reject_code: reject.code.clone(),
                ..StateEvidence::default()
            },
        )?;
    }
    let message = reject.message.to_ascii_lowercase();
    let closing_only = message.contains("closing") || message.contains("restricted");
    let threshold = batch.view.consecutive_403s >= batch.ports.config.restriction_403_threshold;
    if (closing_only || threshold)
        && restrict(batch, Restriction::ClosingOnly, "account_restricted")?
        && threshold
    {
        batch.broker(BrokerRequest::GetAccount);
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

#[cfg(test)]
mod state_change_tests {
    use mandate_canon::{Value, to_canonical};
    use mandate_journal::Draft;
    use mandate_num::Qty;

    use super::{StateEvidence, state_change_fields};
    use crate::batch::schema_version;
    use crate::error::ExecutorError;
    use crate::ids::ClientOrderId;
    use crate::payload::{object, text};
    use crate::types::OrderState;

    fn accepted(evidence: StateEvidence) -> Result<(), ExecutorError> {
        let id = ClientOrderId::parse("md-order-1")?;
        let mut fields = state_change_fields(&id, OrderState::Accepted, evidence);
        fields.push(("risk_clock", text("2026-09-21T14:00:00.000000000Z")));
        let payload = object(fields)?;
        let body = format!(
            r#"{{"envelope_version":1,"environment":"paper",
            "event_id":"01J8Z3M4000000000000000001","stream_id":"acct:ws_1:ACCT1",
            "event_type":"OrderStateChanged","schema_version":{},
            "event_time":"2026-09-21T14:00:00.000000000Z","clock_source":"local",
            "causation_id":null,"correlation_id":null,
            "actor":{{"kind":"system","id":"executor","version":"0.1.0",
            "build":"sha256:{}"}},"config_refs":{{}},"payload":{},
            "artifact_refs":[],"pii_refs":[]}}"#,
            schema_version("OrderStateChanged"),
            "3".repeat(64),
            String::from_utf8_lossy(&to_canonical(&payload))
        );
        assert_eq!(
            Draft::parse(body.as_bytes()).map(|_| ()),
            Ok(()),
            "{payload:?}"
        );
        Ok(())
    }

    #[test]
    fn every_reachable_writer_evidence_shape_passes_the_journal_schema() -> Result<(), ExecutorError>
    {
        let cases = [
            StateEvidence::default(),
            StateEvidence {
                attempted: Some(OrderState::Filled),
                ignored: true,
                ..StateEvidence::default()
            },
            StateEvidence {
                broker_status: Some("partially_filled".to_owned()),
                filled_qty: Some(Qty::parse("1.25")?),
                reject_code: Some("insufficient_buying_power".to_owned()),
                ..StateEvidence::default()
            },
            StateEvidence {
                lookup_absent: true,
                ..StateEvidence::default()
            },
            StateEvidence {
                cancel_requested: true,
                ..StateEvidence::default()
            },
            StateEvidence {
                cancel_confirmed: true,
                ..StateEvidence::default()
            },
            StateEvidence {
                cancel_overdue: true,
                ..StateEvidence::default()
            },
            StateEvidence {
                adopted: true,
                ..StateEvidence::default()
            },
            StateEvidence {
                ladder_step: true,
                cancel_requested: true,
                ..StateEvidence::default()
            },
            StateEvidence {
                replaces: Some(ClientOrderId::parse("md-order-0")?),
                ..StateEvidence::default()
            },
            StateEvidence {
                replaced_by: Some(ClientOrderId::parse("md-order-2")?),
                replaced_by_broker_order_id: Some("broker-order-2".to_owned()),
                ..StateEvidence::default()
            },
        ];
        for evidence in cases {
            accepted(evidence)?;
        }
        Ok(())
    }

    #[test]
    fn constructor_emits_every_boolean_even_when_false() -> Result<(), ExecutorError> {
        let id = ClientOrderId::parse("md-order-1")?;
        let payload = object(state_change_fields(
            &id,
            OrderState::Accepted,
            StateEvidence::default(),
        ))?;
        for field in [
            "ignored",
            "cancel_requested",
            "cancel_confirmed",
            "cancel_overdue",
            "adopted",
            "ladder_step",
        ] {
            assert_eq!(payload.get(field), Some(&Value::Bool(false)), "{field}");
        }
        Ok(())
    }
}
