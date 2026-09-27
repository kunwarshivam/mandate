//! Reconciliation: one pure function over a snapshot, the folded state, and the account ledger
//! (E7-3, trading-domain spec §11).

use std::collections::BTreeSet;

use mandate_accounting::InstrumentId;
use mandate_canon::Value;
use mandate_num::SignedQty;

use crate::batch::Batch;
use crate::codec::{side_name, state_name};
use crate::error::ExecutorError;
use crate::ids::ClientOrderId;
use crate::orders::{EXTERNAL, fill, status_mapping};
use crate::payload::{int, text};
use crate::ports::Ports;
use crate::state::{Adoption, EVERY_AGENT, ExecutorState, restriction_for};
use crate::types::{
    BrokerRequest, BrokerSnapshot, Difference, DifferenceKind, EventId, Mode, OrderState,
    ReconcileReason, Reconciliation, ReconciliationVerdict, StatusMapping,
};

/// The order of the steps is part of the algorithm: **orders, then fills, then positions, then
/// cash, then fees, then `ReconciliationRun`** (task brief interpretation 12).
///
/// 1. **Open orders, by client order id.** The broker wins: every difference is adopted with
///    `OrderStateChanged` plus a `CompensatingEvent` naming the difference and the corrected event
///    ids. An order the broker has and we do not is external activity (§7.1): every agent on the
///    account goes `exits_only` until the owner acknowledges. A reject naming a `client_order_id`
///    we do not know counts only toward the 403 threshold and is not external activity unless it
///    carries a fill.
/// 2. **Fills since the checkpoint, by fill id.** Every missing one is ingested, as `FillApplied`
///    or, where its order is already terminal, `LateFillApplied`, which itself schedules another
///    reconciliation. Fills are always applied to accounting (§5.7).
/// 3. **Positions**, compared only *after* step 2 — a difference a missing fill explains is not a
///    mismatch, and pausing on it would pause a healthy agent at every restart.
/// 4. **Cash**, within 0.01 × fills since the last broker cash snapshot plus accrued unposted
///    fees. Paper's simulated fees and dividends are excluded from this comparison while staying
///    in P&L and buying power (§10, interpretation 25).
/// 5. **Fees**, exact once posted: a difference alerts and is never silently adjusted.
/// 6. **`ReconciliationRun` last**, appended with `expected_head` equal to
///    [`BrokerSnapshot::taken_at_head`], so an `OrderSubmitted` that landed in between makes the
///    append answer `HeadMismatch` and the run is recomputed against a fresh snapshot rather than
///    published as covering something it never saw (interpretation 15). A submission journaled
///    after the snapshot's head is not compared at all, because the snapshot could not have seen
///    it.
///
/// **Adoption is scoped to the order set**, which is the only row §11's on-mismatch column
/// adopts. A position, cash, or fee difference is never written away: writing one away would make
/// the ledger agree with the broker while destroying the evidence that they disagreed
/// (interpretation 13). A mismatch pauses the agents holding the instrument and alerts, and
/// nothing in this crate ever lifts that (interpretation 14).
pub fn reconcile(
    state: &ExecutorState,
    snapshot: &BrokerSnapshot,
    ports: &Ports<'_>,
) -> Result<Reconciliation, ExecutorError> {
    let mut batch = Batch::new(state, ports)?;
    let (verdict, differences) = run(&mut batch, snapshot)?;
    Ok(Reconciliation {
        effects: batch.effects,
        verdict,
        differences,
        expected_head: snapshot.taken_at_head,
    })
}

/// The six steps into one batch, answering the verdict and every difference found.
pub(crate) fn run(
    batch: &mut Batch<'_, '_>,
    snapshot: &BrokerSnapshot,
) -> Result<(ReconciliationVerdict, Vec<Difference>), ExecutorError> {
    let mut differences = Vec::new();
    owed(batch, &mut differences)?;
    let before = batch.view.clone();
    orders(batch, snapshot, &mut differences)?;
    for missing in &snapshot.fills {
        if !batch.view.fills.contains(&missing.fill_id) {
            fill(batch, missing, Some(&before))?;
            differences.push(Difference {
                kind: DifferenceKind::MissingFill,
                subject: missing.fill_id.0.clone(),
                adopted: true,
            });
        }
    }
    positions(batch, snapshot, &mut differences)?;
    cash_and_fees(&batch.view, snapshot)?;
    let unexplained = differences
        .iter()
        .any(|difference| !difference.adopted && difference.kind != DifferenceKind::MissingFill);
    let verdict = if unexplained {
        ReconciliationVerdict::Mismatch
    } else if differences.is_empty() {
        ReconciliationVerdict::Clean
    } else {
        ReconciliationVerdict::Adopted
    };
    let result = match verdict {
        ReconciliationVerdict::Clean => "clean",
        ReconciliationVerdict::Adopted => "adopted",
        ReconciliationVerdict::Mismatch => "mismatch",
    };
    batch.journal(
        "ReconciliationRun",
        None,
        vec![
            ("result", text(result)),
            ("checkpoint", text(snapshot.cursor.0.clone())),
            ("snapshot_head", int(snapshot.taken_at_head.0)?),
            (
                "differences",
                int(u64::try_from(differences.len()).unwrap_or(u64::MAX))?,
            ),
        ],
    )?;
    Ok((verdict, differences))
}

/// Step 1: our live orders against the broker's open orders, and the broker's against ours.
fn orders(
    batch: &mut Batch<'_, '_>,
    snapshot: &BrokerSnapshot,
    differences: &mut Vec<Difference>,
) -> Result<(), ExecutorError> {
    let covered: Vec<(ClientOrderId, OrderState)> = batch
        .view
        .orders
        .values()
        .filter(|order| !order.state.is_terminal() && order.state != OrderState::Intent)
        .filter(|order| {
            batch
                .view
                .details
                .get(&order.client_order_id)
                .and_then(|detail| detail.submitted_seq)
                .is_none_or(|seq| seq <= snapshot.taken_at_head)
        })
        .map(|order| (order.client_order_id.clone(), order.state))
        .collect();
    for (id, ours) in covered {
        let theirs = snapshot
            .open_orders
            .iter()
            .find(|open| open.client_order_id.as_deref() == Some(id.as_str()));
        let adopted = match theirs {
            Some(open) => match status_mapping(&open.status) {
                Ok(StatusMapping::Becomes(state)) => state,
                Ok(StatusMapping::AcceptedFlaggedRestricted) => OrderState::Accepted,
                Ok(StatusMapping::ReplacedPair) => OrderState::Replaced,
                Ok(StatusMapping::Unchanged) | Err(_) => ours,
            },
            None => OrderState::Unknown,
        };
        if adopted != ours {
            adopt(batch, &id, ours, adopted, differences)?;
            if adopted == OrderState::Unknown {
                batch.broker(BrokerRequest::GetOrderByClientId(id));
            }
        }
    }
    let mut external = false;
    for open in &snapshot.open_orders {
        let ours = open
            .client_order_id
            .as_deref()
            .and_then(|raw| ClientOrderId::parse(raw).ok())
            .is_some_and(|id| batch.view.orders.contains_key(&id));
        if ours {
            continue;
        }
        let mut pairs = vec![
            ("broker_order_id", text(open.broker_order_id.clone())),
            ("instrument", text(open.instrument.as_str())),
            ("side", text(side_name(open.side))),
            ("qty", text(open.qty.to_string())),
        ];
        if let Some(raw) = &open.client_order_id {
            pairs.push(("client_order_id", text(raw.clone())));
        }
        batch.journal("ExternalActivityIngested", None, pairs)?;
        differences.push(Difference {
            kind: DifferenceKind::ExternalActivity,
            subject: open.broker_order_id.clone(),
            adopted: false,
        });
        external = true;
    }
    if external {
        let agents = agents(&batch.view, None);
        let first = restrict(batch, &agents, Mode::ExitsOnly, EXTERNAL)?;
        batch.notify(first, "external_activity");
    }
    Ok(())
}

/// Journals the `CompensatingEvent` an adoption already on the journal is still owed — the run a
/// crash cut between the two (§11) — before anything else is compared.
fn owed(batch: &mut Batch<'_, '_>, differences: &mut Vec<Difference>) -> Result<(), ExecutorError> {
    let owed: Vec<(EventId, Adoption)> = batch
        .view
        .uncompensated
        .iter()
        .map(|(id, adoption)| (id.clone(), adoption.clone()))
        .collect();
    for (corrected, adoption) in owed {
        compensate(
            batch,
            corrected,
            &adoption.subject,
            adoption.from,
            adoption.to,
        )?;
        differences.push(Difference {
            kind: DifferenceKind::OrderState,
            subject: adoption.subject.as_str().to_owned(),
            adopted: true,
        });
    }
    Ok(())
}

/// The `CompensatingEvent` for one adoption: the difference and the event it corrected.
fn compensate(
    batch: &mut Batch<'_, '_>,
    corrected: EventId,
    id: &ClientOrderId,
    from: OrderState,
    to: OrderState,
) -> Result<(), ExecutorError> {
    batch.journal(
        "CompensatingEvent",
        Some(corrected.clone()),
        vec![
            ("subject", text(id.as_str())),
            ("difference", text("order_state")),
            ("from", text(state_name(from))),
            ("to", text(state_name(to))),
            ("corrected_event_ids", Value::Array(vec![text(corrected.0)])),
        ],
    )?;
    Ok(())
}

/// Adopts the broker's state for one of our orders: `OrderStateChanged` to the broker's state and
/// a `CompensatingEvent` naming the difference and the event it corrected (§11). The journal
/// keeps both, so a replay reproduces the adoption rather than the assumption.
fn adopt(
    batch: &mut Batch<'_, '_>,
    id: &ClientOrderId,
    from: OrderState,
    to: OrderState,
    differences: &mut Vec<Difference>,
) -> Result<(), ExecutorError> {
    let corrected = batch.journal(
        "OrderStateChanged",
        None,
        vec![
            ("client_order_id", text(id.as_str())),
            ("state", text(state_name(to))),
            ("adopted", Value::Bool(true)),
        ],
    )?;
    compensate(batch, corrected, id, from, to)?;
    differences.push(Difference {
        kind: DifferenceKind::OrderState,
        subject: id.as_str().to_owned(),
        adopted: true,
    });
    Ok(())
}

/// Step 3: the model's net position, after the missing fills, against the broker's, exactly
/// (§11). A difference is a mismatch: the agents holding the instrument are paused and the owner is
/// alerted, and nothing is written away. The two explanations §11 allows, a pending corporate
/// action and a crypto asset fee the broker has not yet posted, come with the slices that fold
/// them; until then either shows as a mismatch, which pauses rather than trades.
fn positions(
    batch: &mut Batch<'_, '_>,
    snapshot: &BrokerSnapshot,
    differences: &mut Vec<Difference>,
) -> Result<(), ExecutorError> {
    let instruments: BTreeSet<InstrumentId> = batch
        .view
        .positions
        .keys()
        .cloned()
        .chain(
            snapshot
                .positions
                .iter()
                .map(|held| held.instrument.clone()),
        )
        .collect();
    for instrument in instruments {
        let model = batch
            .view
            .positions
            .get(&instrument)
            .copied()
            .unwrap_or(SignedQty::ZERO);
        let expected = model;
        let broker = snapshot
            .positions
            .iter()
            .find(|held| held.instrument == instrument)
            .map_or(SignedQty::ZERO, |held| held.qty);
        let mismatch = broker != expected;
        let observed = batch.journal(
            "BrokerPositionObserved",
            None,
            vec![
                ("instrument", text(instrument.as_str())),
                ("broker_qty", text(broker.to_string())),
                ("model_qty", text(expected.to_string())),
                ("mismatch", Value::Bool(mismatch)),
            ],
        )?;
        if mismatch {
            let holders = agents(&batch.view, Some(&instrument));
            restrict(batch, &holders, Mode::Paused, instrument.as_str())?;
            batch.notify(observed, "reconciliation_mismatch");
            differences.push(Difference {
                kind: DifferenceKind::Position,
                subject: instrument.as_str().to_owned(),
                adopted: false,
            });
        }
    }
    Ok(())
}

/// Steps 4 and 5, cash and fees, are the cash slice's (trading-domain spec §11). They fail closed:
/// once the broker has reported an account there is a cash figure to compare, and a fee posting
/// always has fees to compare, so either answers the later slice's stub and the executor stops
/// rather than publishing a run that skipped a comparison. Before any account is reported there is
/// no base to compare the broker's cash with, as in the full comparison.
fn cash_and_fees(state: &ExecutorState, snapshot: &BrokerSnapshot) -> Result<(), ExecutorError> {
    if state.observed.is_some() || snapshot.reason == ReconcileReason::FeePosting {
        return Err(ExecutorError::Unimplemented { story: "E7-3" });
    }
    Ok(())
}

/// The agents a restriction reaches: those with an order in the instrument, or every agent the
/// executor knows of, and always the account-wide `*` when none is attributable, so a mismatch
/// never pauses nobody. The set holds `*` at most once: it is a mode key only once a restriction
/// for every agent is folded, and an account-wide call inserts it anyway.
fn agents(state: &ExecutorState, instrument: Option<&InstrumentId>) -> Vec<String> {
    let mut named: BTreeSet<String> = state
        .orders
        .values()
        .filter(|order| instrument.is_none_or(|wanted| &order.instrument == wanted))
        .map(|order| order.agent.0.clone())
        .chain(
            instrument
                .is_none()
                .then(|| state.modes.keys().map(|agent| agent.0.clone()))
                .into_iter()
                .flatten(),
        )
        .collect();
    if named.is_empty() || instrument.is_none() {
        named.insert(EVERY_AGENT.to_owned());
    }
    named.into_iter().collect()
}

/// Applies one reconciliation restriction to each agent named, answering the first draft's id.
fn restrict(
    batch: &mut Batch<'_, '_>,
    agents: &[String],
    mode: Mode,
    subject: &str,
) -> Result<EventId, ExecutorError> {
    let mut first = None;
    for agent in agents {
        let applied = batch.journal(
            "AgentModeApplied",
            None,
            vec![
                ("agent", text(agent.clone())),
                ("to", text(crate::codec::mode_name(mode))),
                ("restriction", text(restriction_for(subject))),
                ("originated", Value::Bool(true)),
            ],
        )?;
        first = first.or(Some(applied));
    }
    first.ok_or_else(|| ExecutorError::NotInterpreted {
        what: "a restriction on no agent".to_owned(),
        story: "E7-3",
    })
}

/// The cash and fee comparisons are the next slice's, so a run that would need either refuses
/// rather than skipping it (§11).
#[cfg(test)]
mod tests {
    use mandate_num::Usd;

    use super::cash_and_fees;
    use crate::error::ExecutorError;
    use crate::state::{ExecutorState, ObservedAccount};
    use crate::types::{
        AccountRef, AccountScope, AccountState, ActivityCursor, BrokerAccount, BrokerSnapshot,
        ReconcileReason, Seq, WorkspaceId,
    };

    fn snapshot(reason: ReconcileReason) -> BrokerSnapshot {
        BrokerSnapshot {
            open_orders: Vec::new(),
            positions: Vec::new(),
            account: BrokerAccount {
                status: "ACTIVE".to_owned(),
                crypto_status: "ACTIVE".to_owned(),
                trading_blocked: false,
                account_blocked: false,
                trade_suspended_by_user: false,
                multiplier: 1,
                equity: Usd::ZERO,
                cash: Usd::ZERO,
                buying_power: Usd::ZERO,
                non_marginable_buying_power: Usd::ZERO,
                accrued_fees: Usd::ZERO,
            },
            fills: Vec::new(),
            cursor: ActivityCursor("cursor-1".to_owned()),
            reason,
            taken_at_head: Seq(1),
        }
    }

    #[test]
    fn a_run_with_cash_or_fees_to_compare_refuses_until_the_cash_slice() {
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        let refused = Err(ExecutorError::Unimplemented { story: "E7-3" });
        assert_eq!(
            cash_and_fees(&state, &snapshot(ReconcileReason::Scheduled)),
            Ok(()),
            "with no account reported and no fee posting there is nothing to compare"
        );
        assert_eq!(
            cash_and_fees(&state, &snapshot(ReconcileReason::FeePosting)),
            refused,
            "a fee posting always has fees to compare"
        );
        state.observed = Some(ObservedAccount {
            state: AccountState::Active,
            multiplier: 1,
            equity: Usd::ZERO,
            cash: Usd::ZERO,
            buying_power: Usd::ZERO,
            non_marginable_buying_power: Usd::ZERO,
            accrued_fees: Usd::ZERO,
        });
        assert_eq!(
            cash_and_fees(&state, &snapshot(ReconcileReason::Scheduled)),
            refused,
            "once the broker has reported an account there is a cash figure to compare"
        );
    }
}
