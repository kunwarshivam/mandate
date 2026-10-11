//! Reconciliation: one pure function over a snapshot, the folded state, and the account ledger
//! (E7-3, trading-domain spec §11).

use std::collections::BTreeSet;

use mandate_accounting::{InstrumentId, Side};
use mandate_canon::Value;
use mandate_num::{Fraction, Qty, SignedQty};

use crate::batch::Batch;
use crate::codec::{side_name, state_name};
use crate::error::ExecutorError;
use crate::ids::ClientOrderId;
use crate::listing::query_unknown;
use crate::orders::{
    EXTERNAL, StateEvidence, account_fields, account_restriction, every_agent_alerted, fill,
    record_state_change, status_mapping, transition,
};
use crate::payload::{int, text};
use crate::ports::Ports;
use crate::state::{Adoption, EVERY_AGENT, ExecutorState, restriction_for};
use crate::types::{
    Adopted, BrokerAccount, BrokerOrder, BrokerSnapshot, Difference, EventId, Mode, OrderState,
    ReconcileReason, Reconciliation, ReconciliationVerdict, StatusMapping, Unexplained,
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
            differences.push(Difference::adopting(
                Adopted::MissingFill,
                missing.fill_id.0.clone(),
            ));
        }
    }
    positions(batch, snapshot, &mut differences)?;
    let recorded = cash(batch, snapshot, &mut differences)?;
    if snapshot.reason == ReconcileReason::FeePosting {
        fees(batch, snapshot, recorded, &mut differences)?;
    }
    account_restriction(batch, &snapshot.account)?;
    let unexplained = differences.iter().any(|difference| !difference.adopted());
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
    let checkpoint = if snapshot.cursor.0.is_empty() {
        Value::Null
    } else {
        text(snapshot.cursor.0.clone())
    };
    batch.journal(
        "ReconciliationRun",
        None,
        vec![
            ("result", text(result)),
            ("checkpoint", checkpoint),
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
            None if bracket_rests(&batch.view, &id, snapshot) => ours,
            None => OrderState::Unknown,
        };
        if adopted != ours {
            adopt(batch, &id, ours, adopted, differences)?;
            if adopted == OrderState::Unknown {
                query_unknown(batch, id)?;
            }
        }
    }
    let mut external = None;
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
        let ingested = batch.journal("ExternalActivityIngested", None, pairs)?;
        differences.push(Difference::unexplained(
            Unexplained::ExternalActivity,
            open.broker_order_id.clone(),
        ));
        external = external.or(Some(ingested));
    }
    if let Some(first) = external {
        every_agent_alerted(
            batch,
            Mode::ExitsOnly,
            &restriction_for(EXTERNAL),
            first,
            "external_activity",
        )?;
    }
    Ok(())
}

/// Whether `id`, a live protective order no broker order is named for, is a filled bracket's
/// placement whose whole bracket rests as recorded under its entry (DEC-878 item 2), so step 1
/// keeps it rather than adopting it `Unknown`.
///
/// The placement is unsent (no `OrderSubmitted` of its own: a sent OCO is found by its own id or
/// not at all), its id names an entry that was sent as a bracket, and the snapshot lists that
/// entry, by the entry's own `client_order_id`, in status `filled` with exactly two legs nested
/// under it: one resting sell stop at the recorded stop and one resting sell limit with no stop
/// price at the recorded take-profit, each for the placement's quantity. A leg rests when §5.7
/// maps its status to `Accepted` and none of it has filled. The legs' own `client_order_id`s are
/// the broker's and are never compared (items 1 and 3); a missing stop never reads as covering
/// (item 9).
fn bracket_rests(view: &ExecutorState, id: &ClientOrderId, snapshot: &BrokerSnapshot) -> bool {
    id.protected_entry().is_some_and(|entry| {
        snapshot
            .open_orders
            .iter()
            .find(|open| open.client_order_id.as_deref() == Some(entry.as_str()))
            .is_some_and(|parent| bracket_rests_under(view, id, parent))
    })
}

/// [`bracket_rests`]'s rule over one broker's listing of the entry itself, the shape both the
/// open-orders snapshot and the read-back of the entry's own id answer with (`nested=true`): the
/// whole bracket rests as recorded — the entry `filled`, exactly two resting sell legs under it,
/// one stop at the recorded stop and one sell limit with no stop price at the recorded
/// take-profit, each for the placement's quantity — or it does not, and nothing is kept, settled
/// or confirmed on a bracket this answer cannot vouch for.
pub(crate) fn bracket_rests_under(
    view: &ExecutorState,
    id: &ClientOrderId,
    parent: &BrokerOrder,
) -> bool {
    let unsent = view
        .details
        .get(id)
        .is_none_or(|detail| detail.request.is_none());
    let (Some(entry), Some(placement)) = (id.protected_entry(), view.orders.get(id)) else {
        return false;
    };
    let Some(recorded) = view
        .details
        .get(&entry)
        .and_then(|detail| detail.request.as_ref())
        .and_then(|request| request.bracket.clone())
    else {
        return false;
    };
    let resting = |leg: &BrokerOrder| {
        leg.side == Side::Sell
            && leg.qty == placement.qty
            && leg.filled_qty == Qty::ZERO
            && matches!(
                status_mapping(&leg.status),
                Ok(StatusMapping::Becomes(OrderState::Accepted))
            )
    };
    let stops = parent
        .legs
        .iter()
        .filter(|leg| resting(leg) && leg.stop_price == Some(recorded.stop))
        .count();
    let take_profits = parent
        .legs
        .iter()
        .filter(|leg| {
            resting(leg)
                && leg.stop_price.is_none()
                && leg.limit_price == Some(recorded.take_profit)
        })
        .count();
    unsent && parent.status == "filled" && parent.legs.len() == 2 && stops == 1 && take_profits == 1
}

/// The answer to the in-doubt lookup of a filled bracket's placement (DEC-878 item 1, the
/// "Not decided here" this slice took; #1292's contract item 3): the broker's description of an
/// order settles the doubt of every unsent placement naming it as its entry — and present only
/// when the whole bracket rests as recorded under it ([`bracket_rests_under`]), the same rule
/// step 1 keeps the placement by. Settling journals the placement's own `Unknown → Accepted:
/// found` (trading-domain spec §5.7, whose every transition is journaled before it takes
/// effect); anything else the answer shows leaves the placement in doubt: `Unknown`, never
/// confirmed absent, never resubmitted, and always free to hold exits as rule 13 allows on a
/// real doubt. The described entry is folded by [`super::orders::described`] before this runs,
/// so the entry's own state is already what the broker says.
pub(crate) fn settle_doubted_placements(
    batch: &mut Batch<'_, '_>,
    described: &ClientOrderId,
    parent: &BrokerOrder,
) -> Result<(), ExecutorError> {
    let doubted: Vec<ClientOrderId> = batch
        .view
        .orders
        .keys()
        .filter(|id| {
            batch
                .view
                .orders
                .get(id)
                .is_some_and(|order| order.state == OrderState::Unknown)
                && id.protected_entry().as_ref() == Some(described)
        })
        .cloned()
        .collect();
    for id in doubted {
        if bracket_rests_under(&batch.view, &id, parent) {
            transition(
                batch,
                &id,
                OrderState::Accepted,
                StateEvidence {
                    broker_status: Some(parent.status.clone()),
                    ..StateEvidence::default()
                },
            )?;
        }
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
        differences.push(Difference::adopting(
            Adopted::OrderState,
            adoption.subject.as_str(),
        ));
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
    let corrected = record_state_change(
        batch,
        id,
        to,
        StateEvidence {
            adopted: true,
            ..StateEvidence::default()
        },
    )?;
    compensate(batch, corrected, id, from, to)?;
    differences.push(Difference::adopting(Adopted::OrderState, id.as_str()));
    Ok(())
}

/// Step 3: the model's net position, after the missing fills, against the broker's, exactly
/// (§11), except that a crypto asset fee the broker has not yet posted is still in the broker's
/// quantity (§6.3, RC-07). A difference is a mismatch: the agents holding the instrument are
/// paused and the owner is alerted, and nothing is written away. A pending corporate action, §11's
/// other explanation, comes with the slice that folds corporate actions; until then it shows as a
/// mismatch, which pauses rather than trades.
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
        let unposted = batch
            .view
            .asset_fees
            .get(&instrument)
            .copied()
            .unwrap_or(Qty::ZERO);
        let expected = model.checked_add(SignedQty::from(unposted))?;
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
            differences.push(Difference::unexplained(
                Unexplained::Position,
                instrument.as_str(),
            ));
        }
    }
    Ok(())
}

/// Step 4: the broker's cash against the model's, within 0.01 × the fills since the last broker
/// cash snapshot plus the accrued unposted fees (§11). The model's cash is the last broker cash
/// plus the fills' cash since: an accrual is not a cash movement (§8.3), so the unposted fees
/// widen the band and never move the model. Paper's simulated fees are not in the model's cash at
/// all (§10, interpretation 25). The snapshot is journaled as `AccountSnapshotRecorded`, whole,
/// with the model's cash, the band, and whether the broker's cash fell inside it; it becomes the
/// base the next comparison is measured from. Before any account was reported there is no base to
/// compare with, and nothing is recorded.
///
/// §11's cash row is two tiers, "alert above threshold; pause if persistent": a run out of the
/// band alerts the owner and pauses nobody, and the [`CASH_PERSISTENT_RUNS`]th consecutive one
/// pauses every agent until the owner acknowledges `cash`. The count is folded from the recorded
/// snapshots, so it survives the re-anchor each snapshot makes and a restart (DEC-146).
fn cash(
    batch: &mut Batch<'_, '_>,
    snapshot: &BrokerSnapshot,
    differences: &mut Vec<Difference>,
) -> Result<Option<EventId>, ExecutorError> {
    let Some(base) = batch.view.observed.clone() else {
        return Ok(None);
    };
    let model = base.cash.checked_add(batch.view.cash_flow)?;
    let band = batch
        .view
        .fill_notional
        .times_fraction(Fraction::parse(CASH_BAND)?)?
        .checked_add(batch.view.unposted_fees.abs())?;
    let drift = snapshot.account.cash.checked_sub(model)?.abs();
    let inside = drift <= band;
    let mut fields = account_fields(&snapshot.account)?;
    fields.push(("model_cash", text(model.to_string())));
    fields.push(("cash_band", text(band.to_string())));
    fields.push(("cash_in_band", Value::Bool(inside)));
    let recorded = batch.journal("AccountSnapshotRecorded", None, fields)?;
    if !inside {
        differences.push(Difference::unexplained(Unexplained::Cash, "cash"));
        if batch.view.cash_out_of_band >= CASH_PERSISTENT_RUNS {
            every_agent_alerted(
                batch,
                Mode::Paused,
                &restriction_for("cash"),
                recorded.clone(),
                "reconciliation_cash",
            )?;
        } else {
            batch.notify(recorded.clone(), "reconciliation_cash_drift");
        }
    }
    Ok(Some(recorded))
}

/// The cash tolerance per unit of fill notional (§11).
const CASH_BAND: &str = "0.01";

/// How many consecutive runs out of the cash band make a difference persistent, which pauses
/// every agent; fewer only alert (§11, DEC-146).
pub(crate) const CASH_PERSISTENT_RUNS: u32 = 2;

/// Step 5, at a fee posting: fees are exact once posted. A difference pauses every agent and alerts
/// the owner, naming the recorded snapshot (recorded here if step 4 had no base to record it
/// against), until the owner acknowledges `fees`; it is never silently adjusted (§11). A snapshot
/// this step cannot record is left out of the batch, the working copy restored to before it, and
/// the pause and the alert run all the same ([`fee_step_pause_and_alert`], DEC-305 item 3).
fn fees(
    batch: &mut Batch<'_, '_>,
    snapshot: &BrokerSnapshot,
    recorded: Option<EventId>,
    differences: &mut Vec<Difference>,
) -> Result<(), ExecutorError> {
    if snapshot.account.accrued_fees == batch.view.unposted_fees {
        return Ok(());
    }
    let recorded = match recorded {
        Some(recorded) => Some(recorded),
        None => {
            let before = batch.view.clone();
            let own = fee_step_snapshot_fields(&snapshot.account)
                .and_then(|fields| batch.journal("AccountSnapshotRecorded", None, fields));
            match own {
                Ok(recorded) => Some(recorded),
                Err(_) => {
                    batch.view = before;
                    None
                }
            }
        }
    };
    fee_step_pause_and_alert(batch, recorded)?;
    differences.push(Difference::unexplained(Unexplained::Fee, "fees"));
    Ok(())
}

/// The fee step's own `AccountSnapshotRecorded` payload, the one step 5 records when step 4 had no
/// base to record one against: every member journal spec v0.7 §9.2 closes, with `model_cash`,
/// `cash_band`, and `cash_in_band` present as `null` (§4.2), because a fee posting compares no
/// cash and rule 24 makes all three `null` together. Each member has §9.2's type: the statuses
/// text, the three flags booleans, `multiplier` an integer, and the five amounts decimal text. The
/// vectors' `snapshot_fees` draft is that payload, and §9.1's absent-member rule refuses it with
/// a member missing, so the reduced form (`orders::account_fields` alone) is refused at `append`
/// (DEC-261 item 7, DEC-303). The batch stamps `risk_clock` beside these members (DEC-306). `fees`
/// journals this form since the change that registered the schema (DEC-389 item 2, DEC-402).
///
/// # Errors
/// [`ExecutorError::NonCanonicalPayload`] only for a member the canonical form cannot carry, as
/// [`crate::orders::account_fields`].
pub(crate) fn fee_step_snapshot_fields(
    account: &BrokerAccount,
) -> Result<Vec<(&'static str, Value)>, ExecutorError> {
    let mut fields = account_fields(account)?;
    fields.push(("model_cash", Value::Null));
    fields.push(("cash_band", Value::Null));
    fields.push(("cash_in_band", Value::Null));
    Ok(fields)
}

/// The fee step's answer to a fee difference: every agent paused under the fees restriction, and
/// the owner alerted once under the generic `reconciliation_fees` key. The alert names `recorded`
/// when the snapshot recorded; when it did not, it names the `AgentModeApplied` this pause
/// journals, so the alert always names an event already journaled and carries nothing but that
/// opaque id and the key (`AGENTS.md` rules 5 and 6, DEC-305 items 3 and 4). It runs whatever the
/// snapshot's own validation did: a snapshot that cannot be recorded is left out of the batch,
/// never a reason to skip the pause, because a pause is risk reduction no validation may deny
/// (`AGENTS.md` rules 3 and 13, DEC-261 item 7). The one exception is a batch that can journal
/// nothing at all, a risk clock outside §4.7's range: the pause's own draft fails the same way, so
/// the whole input fails and nothing is sent. The guarantee also needs the fee step's snapshot to
/// conform to journal spec §9.2: `Batch::journal` does not validate a draft and `append` refuses a
/// batch whole, so a snapshot refused there would take the pause with it, which is why `fees`
/// journals [`fee_step_snapshot_fields`] (DEC-402).
pub(crate) fn fee_step_pause_and_alert(
    batch: &mut Batch<'_, '_>,
    recorded: Option<EventId>,
) -> Result<(), ExecutorError> {
    let paused = batch.journal(
        "AgentModeApplied",
        None,
        vec![
            ("agent", text(EVERY_AGENT)),
            ("to", text(crate::codec::mode_name(Mode::Paused))),
            ("restriction", text(restriction_for("fees"))),
            ("originated", Value::Bool(true)),
        ],
    )?;
    batch.notify(recorded.unwrap_or(paused), "reconciliation_fees");
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
        .filter_map(|order| order.agent.as_ref().map(|agent| agent.0.clone()))
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

#[cfg(test)]
pub(crate) mod tests {
    use mandate_accounting::{
        AssetClass, Config, CryptoFees, EquityFees, InstrumentId, Side, TafCapBasis,
    };
    use mandate_canon::{Value, to_canonical};
    use mandate_journal::Draft;
    use mandate_num::{
        Bps, FeeCap, FeePerShare, FeeRate, Fraction, Price, Qty, ShareIncrement, SignedQty, Usd,
    };
    use mandate_time::{Date, TradingCalendar};
    use serde_json::{Map, Value as Json};

    use super::{Batch, agents, reconcile};
    use crate::error::ExecutorError;
    use crate::ids::{ClientOrderId, IntentId};
    use crate::payload::object;
    use crate::ports::{ALLOWING_BINDING_GATE, IdGen, InstrumentSnapshot, MandateView, Ports};
    use crate::state::{ExecutorState, ObservedAccount, fold};
    use crate::step::handle;
    use crate::types::{
        AccountRef, AccountScope, AccountState, ActivityCursor, AgentId, BrokerAccount, BrokerFill,
        BrokerOrder, BrokerOutcome, BrokerPosition, BrokerRequest, BrokerSnapshot, BrokerUpdate,
        DifferenceKind, Effect, EventDraft, EventId, ExecutorConfig, ExitTier, FillId, FoldedEvent,
        Input, IntentBody, IntentHandoff, MandateVersion, Mode, Order, OrderState,
        ProtectionPrices, Purpose, ReconcileReason, Seq, TimeInForce, WorkspaceId, WriterEpoch,
    };

    /// Ids derived from the epoch, the head and the ordinal, as a production id generator does,
    /// and alphanumeric like its, so a protective order can be named for one (§2.3).
    pub(crate) struct Ids;

    impl IdGen for Ids {
        fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId {
            EventId(format!("e{}h{}o{ordinal}", epoch.0, head.0))
        }
    }

    struct SchemaIds;

    impl IdGen for SchemaIds {
        fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId {
            let suffix = epoch
                .0
                .saturating_add(head.0)
                .saturating_add(u64::from(ordinal));
            EventId(format!("01J8Z3M4{suffix:018}"))
        }
    }

    fn journal_accepts(draft: &EventDraft) -> Result<(), ExecutorError> {
        let causation = draft
            .causation_id
            .as_ref()
            .map_or_else(|| "null".to_owned(), |id| format!("\"{}\"", id.0));
        let body = format!(
            r#"{{"envelope_version":1,"environment":"paper","event_id":"{}",
            "stream_id":"acct:ws1:acct-1","event_type":"{}","schema_version":{},
            "event_time":"2026-09-21T14:00:00.000000000Z","clock_source":"local",
            "causation_id":{causation},"correlation_id":null,
            "actor":{{"kind":"system","id":"executor","version":"0.1.0","build":"sha256:{}"}},
            "config_refs":{{}},"payload":{},"artifact_refs":[],"pii_refs":[]}}"#,
            draft.event_id.0,
            draft.event_type,
            draft.schema_version,
            "3".repeat(64),
            String::from_utf8_lossy(&to_canonical(&draft.payload))
        );
        Draft::parse(body.as_bytes()).map(|_| ()).map_err(|error| {
            ExecutorError::NonCanonicalPayload {
                field: error.to_string(),
            }
        })
    }

    /// A mandate covering everything, and whole-share equities.
    pub(crate) struct Everything;

    impl MandateView for Everything {
        fn version(&self, _agent: &AgentId) -> Option<MandateVersion> {
            Some(MandateVersion(format!("sha256:{}", "5".repeat(64))))
        }

        fn crypto_stop_limit_offset(&self, _agent: &AgentId) -> Option<Fraction> {
            None
        }

        fn covers(&self, _agent: &AgentId, _instrument: &InstrumentId) -> bool {
            true
        }
    }

    impl InstrumentSnapshot for Everything {
        fn asset_class(&self, _instrument: &InstrumentId) -> Option<AssetClass> {
            Some(AssetClass::UsEquity)
        }

        fn increment(&self, _instrument: &InstrumentId) -> Option<ShareIncrement> {
            Some(ShareIncrement::Whole)
        }

        fn exit_tier(&self, _instrument: &InstrumentId) -> Option<ExitTier> {
            None
        }
    }

    pub(crate) fn executor_config() -> ExecutorConfig {
        ExecutorConfig {
            max_intent_age_s: 120,
            unknown_absent_lookups: 3,
            unknown_absent_window_s: 15,
            protective_replace_buffer_trading_days: 5,
            restriction_403_threshold: 3,
            bracket_partial_fill_timeout_s: 60,
            max_unprotected_s: 30,
            stop_watchdog_s: 30,
            exit_step_s: 10,
            gtc_expiry_days: 90,
        }
    }

    pub(crate) fn fees() -> Result<Config, ExecutorError> {
        let date = |raw: &str| Date::parse(raw).map_err(ExecutorError::from);
        Ok(Config {
            equities: EquityFees {
                sec_rate: FeeRate::parse("0.00003")?,
                taf_per_share: FeePerShare::parse("0.0002")?,
                taf_cap: FeeCap::parse("9.79")?,
                taf_cap_basis: TafCapBasis::PerExecution,
                cat_per_share: FeePerShare::parse("0.00001")?,
            },
            crypto: CryptoFees {
                maker: Bps::parse("15")?,
                taker: Bps::parse("25")?,
            },
            calendar: TradingCalendar::new(date("2026-09-01")?, date("2026-12-31")?, [], [])?,
        })
    }

    pub(crate) fn aapl() -> Result<InstrumentId, ExecutorError> {
        Ok(InstrumentId::new("AAPL")?)
    }

    pub(crate) fn account(cash: &str) -> Result<BrokerAccount, ExecutorError> {
        Ok(BrokerAccount {
            status: "ACTIVE".to_owned(),
            crypto_status: "ACTIVE".to_owned(),
            trading_blocked: false,
            account_blocked: false,
            trade_suspended_by_user: false,
            multiplier: 1,
            equity: Usd::parse(cash)?,
            cash: Usd::parse(cash)?,
            buying_power: Usd::parse(cash)?,
            non_marginable_buying_power: Usd::parse(cash)?,
            accrued_fees: Usd::ZERO,
            last_equity: Usd::parse(cash)?,
            maintenance_margin: Usd::ZERO,
        })
    }

    pub(crate) fn snapshot(reason: ReconcileReason) -> Result<BrokerSnapshot, ExecutorError> {
        Ok(BrokerSnapshot {
            open_orders: Vec::new(),
            positions: Vec::new(),
            account: account("0")?,
            fills: Vec::new(),
            cursor: ActivityCursor("cursor-1".to_owned()),
            reason,
            taken_at_head: Seq(0),
        })
    }

    fn intent(id: &str, agent: &str, side: Side, purpose: Purpose) -> Result<Input, ExecutorError> {
        Ok(Input::Intent(IntentHandoff {
            intent_id: IntentId(EventId(id.to_owned())),
            agent: AgentId(agent.to_owned()),
            tif: Some(TimeInForce::Day),
            body: IntentBody::Order {
                instrument: aapl()?,
                side,
                qty: Qty::parse("5")?,
                limit: Price::parse("150")?,
                purpose,
                protection: None,
            },
        }))
    }

    /// One executor over the journal it writes: every draft `handle` answers is committed and
    /// folded back, in order, as the shell does (journal spec §5.2).
    pub(crate) struct Executor {
        pub(crate) state: ExecutorState,
        pub(crate) journal: Vec<FoldedEvent>,
        epoch: u64,
    }

    impl Executor {
        pub(crate) fn opened(ports: &Ports<'_>) -> Result<Self, ExecutorError> {
            let mut executor = Self {
                state: ExecutorState::new(AccountScope {
                    account: AccountRef("acct-1".to_owned()),
                    workspace: WorkspaceId("ws1".to_owned()),
                }),
                journal: Vec::new(),
                epoch: 1,
            };
            executor.commit_one(
                "StreamOpened",
                object(vec![("environment", Value::Str("paper".to_owned()))])?,
            )?;
            executor.run(Input::Started(WriterEpoch(1)), ports)?;
            Ok(executor)
        }

        pub(crate) fn commit_one(
            &mut self,
            event_type: &str,
            payload: Value,
        ) -> Result<(), ExecutorError> {
            let seq = self.state.account_head().0.saturating_add(1);
            let event = FoldedEvent {
                stream: self.state.account_stream(),
                seq: Seq(seq),
                event_id: EventId(format!("j-{seq}")),
                event_type: event_type.to_owned(),
                causation_id: None,
                payload,
            };
            fold(&mut self.state, &event)?;
            self.journal.push(event);
            Ok(())
        }

        /// Handles one input and commits at most `keep` of its drafts; `usize::MAX` commits all.
        fn run_keeping(
            &mut self,
            input: Input,
            ports: &Ports<'_>,
            keep: usize,
        ) -> Result<Vec<Effect>, ExecutorError> {
            let effects = handle(&mut self.state, input, ports, &ALLOWING_BINDING_GATE)?;
            let mut kept = 0;
            for effect in &effects {
                if let Effect::Journal(draft) = effect {
                    if kept == keep {
                        break;
                    }
                    kept = kept.saturating_add(1);
                    let seq = self.state.account_head().0.saturating_add(1);
                    let event = FoldedEvent {
                        stream: self.state.account_stream(),
                        seq: Seq(seq),
                        event_id: draft.event_id.clone(),
                        event_type: draft.event_type.clone(),
                        causation_id: draft.causation_id.clone(),
                        payload: draft.payload.clone(),
                    };
                    fold(&mut self.state, &event)?;
                    self.journal.push(event);
                }
            }
            Ok(effects)
        }

        pub(crate) fn run(
            &mut self,
            input: Input,
            ports: &Ports<'_>,
        ) -> Result<Vec<Effect>, ExecutorError> {
            self.run_keeping(input, ports, usize::MAX)
        }

        /// A crash and a restart: a new process folds the same journal and takes a new epoch.
        pub(crate) fn restarted(&self, ports: &Ports<'_>) -> Result<Self, ExecutorError> {
            let mut next = Self {
                state: ExecutorState::new(AccountScope {
                    account: AccountRef("acct-1".to_owned()),
                    workspace: WorkspaceId("ws1".to_owned()),
                }),
                journal: self.journal.clone(),
                epoch: self.epoch.saturating_add(1),
            };
            for event in &self.journal {
                fold(&mut next.state, event)?;
            }
            let epoch = WriterEpoch(next.epoch);
            next.run(Input::Started(epoch), ports)?;
            Ok(next)
        }

        pub(crate) fn snapshot(
            &self,
            reason: ReconcileReason,
        ) -> Result<BrokerSnapshot, ExecutorError> {
            Ok(BrokerSnapshot {
                taken_at_head: self.state.account_head(),
                ..snapshot(reason)?
            })
        }
    }

    /// What a test expected and did not find. Never `Unimplemented`, whose story `ci pending`
    /// reads as a stub (DEC-137).
    pub(crate) fn missing(what: &str) -> ExecutorError {
        ExecutorError::NonCanonicalPayload {
            field: format!("expected {what}"),
        }
    }

    pub(crate) fn drafted(effects: &[Effect]) -> Vec<&str> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) => Some(draft.event_type.as_str()),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn submitted(effects: &[Effect]) -> usize {
        effects
            .iter()
            .filter(|effect| matches!(effect, Effect::Broker(BrokerRequest::Submit(_))))
            .count()
    }

    fn queried(effects: &[Effect]) -> usize {
        effects
            .iter()
            .filter(|effect| matches!(effect, Effect::Broker(BrokerRequest::GetOrderByClientId(_))))
            .count()
    }

    fn order(id: &ClientOrderId, state: OrderState) -> Result<Order, ExecutorError> {
        Ok(Order {
            client_order_id: id.clone(),
            intent_id: None,
            agent: Some(AgentId("agent-a".to_owned())),
            instrument: aapl()?,
            side: Side::Buy,
            qty: Qty::parse("10")?,
            filled_qty: Qty::ZERO,
            state,
            attempt: 1,
            purpose: Purpose::Open,
            absent_lookups: 0,
            first_absence_at: None,
            cancel_unconfirmed: false,
            replaced_by: None,
            created_on: None,
        })
    }

    /// §11 step 1 compares only the orders the broker could still hold. A finished order, and one
    /// resting in `Intent` (a held exit after a confirmed absence, which the broker never had),
    /// stay as they are when the broker lists neither: nothing is adopted and nothing queried, so
    /// no `Unknown` is minted to hold a reservation and block the instrument. A live one is
    /// adopted.
    #[test]
    fn a_reconciliation_compares_only_live_orders() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        state.epoch = Some(WriterEpoch(1));
        state.started = true;
        let resting = ClientOrderId::parse("md-01JABCDEFGHJKMNPQRSTVWXYZ1")?;
        for (id, at) in [
            (
                ClientOrderId::parse("md-01JABCDEFGHJKMNPQRSTVWXYZ0")?,
                OrderState::Canceled,
            ),
            (resting.clone(), OrderState::Intent),
        ] {
            state.orders.insert(id.clone(), order(&id, at)?);
        }
        let run = reconcile(&state, &snapshot(ReconcileReason::Scheduled)?, &ports)?;
        assert_eq!(
            (drafted(&run.effects), queried(&run.effects)),
            (vec!["ReconciliationRun"], 0),
            "neither is compared, so nothing is adopted and nothing queried"
        );
        let mut after = state.clone();
        for effect in &run.effects {
            if let Effect::Journal(draft) = effect {
                let seq = after.account_head().0.saturating_add(1);
                let stream = after.account_stream();
                fold(
                    &mut after,
                    &FoldedEvent {
                        stream,
                        seq: Seq(seq),
                        event_id: draft.event_id.clone(),
                        event_type: draft.event_type.clone(),
                        causation_id: None,
                        payload: draft.payload.clone(),
                    },
                )?;
            }
        }
        assert_eq!(
            after.order(&resting).map(|resting| resting.state),
            Some(OrderState::Intent),
            "the resting order stays in `Intent`"
        );

        let live = ClientOrderId::parse("md-01JABCDEFGHJKMNPQRSTVWXYZ2")?;
        state
            .orders
            .insert(live.clone(), order(&live, OrderState::Accepted)?);
        let run = reconcile(&state, &snapshot(ReconcileReason::Scheduled)?, &ports)?;
        assert!(
            drafted(&run.effects).contains(&"CompensatingEvent"),
            "while a live order the broker does not list is adopted: {:?}",
            drafted(&run.effects)
        );
        Ok(())
    }

    /// §11 steps 4 and 5: once an account is reported, a run compares cash within its band and
    /// fees exactly at a fee posting, records the snapshot as the next base, and is published; a
    /// difference pauses every agent and alerts the owner, and neither is written away.
    #[test]
    fn a_run_compares_cash_and_fees_and_records_the_snapshot() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        state.epoch = Some(WriterEpoch(1));
        state.started = true;
        state.observed = Some(ObservedAccount {
            state: AccountState::Active,
            multiplier: 1,
            equity: Usd::ZERO,
            cash: Usd::ZERO,
            buying_power: Usd::ZERO,
            non_marginable_buying_power: Usd::ZERO,
            accrued_fees: Usd::ZERO,
            complete: true,
        });
        let agreeing = reconcile(&state, &snapshot(ReconcileReason::FeePosting)?, &ports)?;
        assert_eq!(
            drafted(&agreeing.effects),
            vec!["AccountSnapshotRecorded", "ReconciliationRun"],
            "the broker agrees on cash and fees: the snapshot is recorded and the run published"
        );

        let mut drifted = snapshot(ReconcileReason::FeePosting)?;
        drifted.account.cash = Usd::parse("1")?;
        drifted.account.accrued_fees = Usd::parse("3")?;
        let run = reconcile(&state, &drifted, &ports)?;
        let restrictions: Vec<_> = run
            .effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) if draft.event_type == "AgentModeApplied" => {
                    draft.payload.get("restriction").and_then(Value::as_str)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            restrictions,
            vec!["reconciliation:fees"],
            "a fee difference pauses every agent; a first cash drift beyond the band only alerts \
             (§11's two tiers, DEC-146)"
        );
        let alerts: Vec<_> = run
            .effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Notify(reference) => Some(reference.message_key),
                _ => None,
            })
            .collect();
        assert_eq!(
            alerts,
            vec!["reconciliation_cash_drift", "reconciliation_fees"]
        );
        Ok(())
    }

    /// §11's band includes the accrued unposted fees, and the model's cash does not move by them:
    /// an accrual is not a cash movement (§8.3). With 3 accrued and no fill, the model is 0 and the
    /// band 3 either way, so 3 above or below agrees and a cent beyond differs.
    #[test]
    fn the_cash_band_includes_the_unposted_fees() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        state.epoch = Some(WriterEpoch(1));
        state.started = true;
        state.observed = Some(ObservedAccount {
            state: AccountState::Active,
            multiplier: 1,
            equity: Usd::ZERO,
            cash: Usd::ZERO,
            buying_power: Usd::ZERO,
            non_marginable_buying_power: Usd::ZERO,
            accrued_fees: Usd::ZERO,
            complete: true,
        });
        state.unposted_fees = Usd::parse("3")?;
        for (cash, differs) in [("3", false), ("3.01", true), ("-3", false), ("-3.01", true)] {
            let mut taken = snapshot(ReconcileReason::Scheduled)?;
            taken.account.cash = Usd::parse(cash)?;
            let run = reconcile(&state, &taken, &ports)?;
            assert_eq!(
                run.differences
                    .iter()
                    .any(|difference| difference.kind == DifferenceKind::Cash),
                differs,
                "the model is 0 and the band 0.01 × 0 + 3: {cash}"
            );
        }
        Ok(())
    }

    /// DEC-140's slice-5 amendment: a stream slice 5 wrote may hold every agent `exits_only` under
    /// `reconciliation:incomplete`. The cash slice completes those runs, and the owner's
    /// acknowledgment of `incomplete`, with step-up evidence, lifts the hold (§11, interpretation
    /// 14): openings are submitted again. A run that now completes lifts nothing by itself, and an
    /// acknowledgment without step-up evidence lifts nothing either.
    #[test]
    fn an_acknowledgment_of_incomplete_lifts_the_slice_5_hold() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = Executor::opened(&ports)?;
        executor.run(
            Input::BrokerUpdate(BrokerUpdate::Account(account("100000")?)),
            &ports,
        )?;
        executor.commit_one(
            "AgentModeApplied",
            object(vec![
                ("agent", Value::Str("*".to_owned())),
                ("to", Value::Str("exits_only".to_owned())),
                (
                    "restriction",
                    Value::Str("reconciliation:incomplete".to_owned()),
                ),
                ("originated", Value::Bool(true)),
                (
                    "risk_clock",
                    crate::payload::clock(crate::types::RiskClock::from_secs(0))?,
                ),
            ])?,
        )?;
        let completed = executor.run(
            Input::BrokerSnapshot(BrokerSnapshot {
                account: account("100000")?,
                ..executor.snapshot(ReconcileReason::Scheduled)?
            }),
            &ports,
        )?;
        assert_eq!(
            drafted(&completed),
            vec!["AccountSnapshotRecorded", "ReconciliationRun"],
            "the cash agrees, so the run completes"
        );
        let held = executor.run(
            intent(
                "01JABCDEFGHJKMNPQRSTVWXYZ0",
                "agent-a",
                Side::Buy,
                Purpose::Open,
            )?,
            &ports,
        )?;
        assert_eq!(submitted(&held), 0, "the hold outlives a completed run");

        let acknowledged = |step_up: &str| -> Result<Value, ExecutorError> {
            object(vec![
                ("subject", Value::Str("incomplete".to_owned())),
                ("user", Value::Str("user-1".to_owned())),
                ("step_up", Value::Str(step_up.to_owned())),
                (
                    "risk_clock",
                    crate::payload::clock(crate::types::RiskClock::from_secs(0))?,
                ),
            ])
        };
        executor.commit_one("OwnerAcknowledged", acknowledged("")?)?;
        assert_eq!(
            executor
                .state
                .effective_mode(&AgentId("agent-a".to_owned())),
            Mode::ExitsOnly,
            "no step-up evidence, no lift"
        );
        executor.commit_one("OwnerAcknowledged", acknowledged("assertion-1")?)?;
        assert_eq!(
            executor
                .state
                .effective_mode(&AgentId("agent-a".to_owned())),
            Mode::Normal
        );
        let opening = executor.run(
            intent(
                "01JABCDEFGHJKMNPQRSTVWXYZ1",
                "agent-a",
                Side::Buy,
                Purpose::Open,
            )?,
            &ports,
        )?;
        assert_eq!(submitted(&opening), 1, "an opening is submitted again");
        Ok(())
    }

    /// `resume`'s own claim: a restart never submits on state it has not reconciled. The startup
    /// run comes first, so a position mismatch it finds holds the resumed opening.
    #[test]
    fn the_startup_run_precedes_the_resume() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = Executor::opened(&ports)?;
        executor.run_keeping(
            intent(
                "01JABCDEFGHJKMNPQRSTVWXYZ0",
                "agent-a",
                Side::Buy,
                Purpose::Open,
            )?,
            &ports,
            1,
        )?;
        let mut executor = executor.restarted(&ports)?;
        let mut taken = executor.snapshot(ReconcileReason::Startup)?;
        taken.positions = vec![BrokerPosition {
            instrument: aapl()?,
            qty: SignedQty::parse("7")?,
            avg_entry_price: Price::parse("150")?,
        }];
        let startup = executor.run(Input::BrokerSnapshot(taken), &ports)?;
        assert_eq!(
            submitted(&startup),
            0,
            "nothing is submitted on a mismatched position"
        );
        assert_eq!(
            drafted(&startup),
            vec![
                "BrokerPositionObserved",
                "AgentModeApplied",
                "ReconciliationRun",
                "GateDecided"
            ],
            "the run, its pause and its record first, then the resumed intent, held"
        );
        Ok(())
    }

    /// DEC-133 item 13: the evidence kept instead of adopting, and what the owner reads before the
    /// acknowledgment, names the broker's quantity and the model's by key.
    #[test]
    fn a_position_difference_is_recorded_with_each_quantity_by_name() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = Executor::opened(&ports)?;
        executor.commit_one(
            "FillApplied",
            object(vec![
                ("fill_id", Value::Str("f-0".to_owned())),
                ("instrument", Value::Str("AAPL".to_owned())),
                ("side", Value::Str("buy".to_owned())),
                ("qty_gross", Value::Str("10".to_owned())),
                ("price", Value::Str("150".to_owned())),
                (
                    "risk_clock",
                    crate::payload::clock(crate::types::RiskClock::from_secs(0))?,
                ),
            ])?,
        )?;
        let mut taken = executor.snapshot(ReconcileReason::Scheduled)?;
        taken.positions = vec![BrokerPosition {
            instrument: aapl()?,
            qty: SignedQty::parse("7")?,
            avg_entry_price: Price::parse("150")?,
        }];
        let run = executor.run(Input::BrokerSnapshot(taken), &ports)?;
        let observed = run.iter().find_map(|effect| match effect {
            Effect::Journal(draft) if draft.event_type == "BrokerPositionObserved" => Some(draft),
            _ => None,
        });
        let field = |name: &str| {
            observed
                .and_then(|draft| draft.payload.get(name))
                .and_then(Value::as_str)
        };
        assert_eq!(
            (field("broker_qty"), field("model_qty")),
            (Some("7"), Some("10")),
            "a `BrokerPositionObserved` is drafted, each quantity under its own key"
        );
        assert_eq!(
            observed.and_then(|draft| draft.payload.get("mismatch")),
            Some(&Value::Bool(true))
        );
        Ok(())
    }

    #[test]
    fn reconciliation_writers_emit_payloads_the_closed_journal_schemas_accept()
    -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &SchemaIds,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = Executor::opened(&ports)?;
        executor.commit_one(
            "FillApplied",
            object(vec![
                ("fill_id", Value::Str("f-0".to_owned())),
                ("instrument", Value::Str("AAPL".to_owned())),
                ("side", Value::Str("buy".to_owned())),
                ("qty_gross", Value::Str("10".to_owned())),
                ("price", Value::Str("150".to_owned())),
                (
                    "risk_clock",
                    crate::payload::clock(crate::types::RiskClock::from_secs(0))?,
                ),
            ])?,
        )?;
        let mut taken = executor.snapshot(ReconcileReason::Scheduled)?;
        taken.positions = vec![BrokerPosition {
            instrument: aapl()?,
            qty: SignedQty::parse("7")?,
            avg_entry_price: Price::parse("150")?,
        }];
        let mismatch = executor.run(Input::BrokerSnapshot(taken), &ports)?;
        for event_type in ["BrokerPositionObserved", "AgentModeApplied"] {
            let draft = mismatch
                .iter()
                .find_map(|effect| match effect {
                    Effect::Journal(draft) if draft.event_type == event_type => Some(draft),
                    _ => None,
                })
                .ok_or_else(|| missing(event_type))?;
            journal_accepts(draft)?;
        }

        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        state.epoch = Some(WriterEpoch(1));
        state.started = true;
        let live = ClientOrderId::parse("md-01JABCDEFGHJKMNPQRSTVWXYZ2")?;
        state
            .orders
            .insert(live.clone(), order(&live, OrderState::Accepted)?);
        let adopted = reconcile(&state, &snapshot(ReconcileReason::Scheduled)?, &ports)?;
        let compensation = adopted
            .effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Journal(draft) if draft.event_type == "CompensatingEvent" => Some(draft),
                _ => None,
            })
            .ok_or_else(|| missing("CompensatingEvent"))?;
        journal_accepts(compensation)?;
        Ok(())
    }

    fn gate_decision(effects: &[Effect]) -> Option<(String, String)> {
        effects.iter().find_map(|effect| match effect {
            Effect::Journal(draft) if draft.event_type == "GateDecided" => {
                let field = |name: &str| {
                    draft
                        .payload
                        .get(name)
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                };
                field("verdict").zip(field("reason_code"))
            }
            _ => None,
        })
    }

    fn filled_ten(executor: &mut Executor) -> Result<(), ExecutorError> {
        executor.commit_one(
            "FillApplied",
            object(vec![
                ("fill_id", Value::Str("f-0".to_owned())),
                ("instrument", Value::Str("AAPL".to_owned())),
                ("side", Value::Str("buy".to_owned())),
                ("qty_gross", Value::Str("10".to_owned())),
                ("price", Value::Str("150".to_owned())),
                (
                    "risk_clock",
                    crate::payload::clock(crate::types::RiskClock::from_secs(0))?,
                ),
            ])?,
        )
    }

    fn run_completed(executor: &mut Executor) -> Result<(), ExecutorError> {
        executor.commit_one(
            "ReconciliationRun",
            object(vec![(
                "risk_clock",
                crate::payload::clock(crate::types::RiskClock::from_secs(0))?,
            )])?,
        )
    }

    /// The coordinator's ruling on #174 (comment 5857742391), rule 3: on a stream that has
    /// journaled an account, an opening is held, never denied, until a reconciliation has run since
    /// the process started; an exit is not held (rule 13); and once a run has completed, the first
    /// tick releases the held opening.
    #[test]
    fn an_opening_is_held_until_a_reconciliation_has_run_since_the_start()
    -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = Executor::opened(&ports)?;
        filled_ten(&mut executor)?;
        executor.run(
            Input::BrokerUpdate(BrokerUpdate::Account(account("100000")?)),
            &ports,
        )?;
        let opening = executor.run(
            intent(
                "01JABCDEFGHJKMNPQRSTVWXYZ0",
                "agent-a",
                Side::Buy,
                Purpose::Open,
            )?,
            &ports,
        )?;
        assert_eq!(
            (submitted(&opening), gate_decision(&opening)),
            (
                0,
                Some((
                    "hold".to_owned(),
                    "startup_reconciliation_pending".to_owned()
                ))
            ),
            "the opening is held, not denied"
        );
        let exit = executor.run(
            intent(
                "01JABCDEFGHJKMNPQRSTVWXYZ1",
                "agent-a",
                Side::Sell,
                Purpose::RiskExit,
            )?,
            &ports,
        )?;
        assert_eq!(
            submitted(&exit),
            1,
            "an exit is never held for it (rule 13)"
        );
        let early = executor.run(Input::Tick(crate::types::RiskClock::from_secs(1)), &ports)?;
        assert_eq!(
            submitted(&early),
            0,
            "no run yet, so the tick releases nothing"
        );

        run_completed(&mut executor)?;
        let released = executor.run(Input::Tick(crate::types::RiskClock::from_secs(2)), &ports)?;
        assert_eq!(
            submitted(&released),
            1,
            "a run since the start releases the held opening at the next tick"
        );

        for purpose in [
            Purpose::Protective,
            Purpose::OwnerExit,
            Purpose::DiscretionaryExit,
            Purpose::Flatten,
            Purpose::RiskExit,
        ] {
            let mut unreported = Executor::opened(&ports)?;
            filled_ten(&mut unreported)?;
            let reducing = unreported.run(
                intent("01JABCDEFGHJKMNPQRSTVWXYZ2", "agent-a", Side::Sell, purpose)?,
                &ports,
            )?;
            assert_eq!(
                (
                    submitted(&reducing),
                    gate_decision(&reducing).map(|(verdict, _)| verdict)
                ),
                (1, Some("allow".to_owned())),
                "on a stream with no account and no run, a {purpose:?} still goes (rule 13)"
            );
        }
        Ok(())
    }

    /// A run an earlier process appended does not count: after a restart the opening waits for the
    /// new process's own run. A stream that has never journaled an account holds its opening until
    /// one is journaled and a run has completed, whatever order they come in.
    #[test]
    fn a_run_from_before_the_restart_does_not_release_an_opening() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut unreported = Executor::opened(&ports)?;
        let open = unreported.run(
            intent(
                "01JABCDEFGHJKMNPQRSTVWXYZ0",
                "agent-a",
                Side::Buy,
                Purpose::Open,
            )?,
            &ports,
        )?;
        assert_eq!(
            (submitted(&open), gate_decision(&open)),
            (
                0,
                Some((
                    "hold".to_owned(),
                    "startup_reconciliation_pending".to_owned()
                ))
            ),
            "no account journaled: held, not denied, whatever the shell does"
        );
        run_completed(&mut unreported)?;
        let run_only =
            unreported.run(Input::Tick(crate::types::RiskClock::from_secs(1)), &ports)?;
        assert_eq!(submitted(&run_only), 0, "a run alone does not release it");
        unreported.run(
            Input::BrokerUpdate(BrokerUpdate::Account(account("100000")?)),
            &ports,
        )?;
        let both = unreported.run(Input::Tick(crate::types::RiskClock::from_secs(2)), &ports)?;
        assert_eq!(
            submitted(&both),
            1,
            "an account and a run since the start release it"
        );

        let mut executor = Executor::opened(&ports)?;
        executor.run(
            Input::BrokerUpdate(BrokerUpdate::Account(account("100000")?)),
            &ports,
        )?;
        run_completed(&mut executor)?;
        let mut at_once = executor.restarted(&ports)?;
        let restarted_on_the_run = at_once.run(
            intent(
                "01JABCDEFGHJKMNPQRSTVWXYZ3",
                "agent-a",
                Side::Buy,
                Purpose::Open,
            )?,
            &ports,
        )?;
        assert_eq!(
            submitted(&restarted_on_the_run),
            0,
            "a restart right after the last process's run still waits for its own"
        );
        let before = executor.run(
            intent(
                "01JABCDEFGHJKMNPQRSTVWXYZ1",
                "agent-a",
                Side::Buy,
                Purpose::Open,
            )?,
            &ports,
        )?;
        assert_eq!(submitted(&before), 1, "this process's own run releases it");

        let mut executor = executor.restarted(&ports)?;
        let after = executor.run(
            intent(
                "01JABCDEFGHJKMNPQRSTVWXYZ2",
                "agent-a",
                Side::Buy,
                Purpose::Open,
            )?,
            &ports,
        )?;
        assert_eq!(
            (submitted(&after), gate_decision(&after)),
            (
                0,
                Some((
                    "hold".to_owned(),
                    "startup_reconciliation_pending".to_owned()
                ))
            ),
            "the earlier process's run does not count after the restart"
        );
        Ok(())
    }

    fn alerts(effects: &[Effect]) -> Vec<&'static str> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Notify(reference) => Some(reference.message_key),
                _ => None,
            })
            .collect()
    }

    /// An executor that has run its startup reconciliation and then heard the broker report
    /// 100000 of cash, so openings are not held and the next run compares cash.
    pub(crate) fn reporting(ports: &Ports<'_>) -> Result<Executor, ExecutorError> {
        let mut executor = Executor::opened(ports)?;
        executor.run(
            Input::BrokerSnapshot(executor.snapshot(ReconcileReason::Startup)?),
            ports,
        )?;
        executor.run(
            Input::BrokerUpdate(BrokerUpdate::Account(account("100000")?)),
            ports,
        )?;
        Ok(executor)
    }

    fn cash_run(
        executor: &mut Executor,
        cash: &str,
        ports: &Ports<'_>,
    ) -> Result<Vec<Effect>, ExecutorError> {
        let taken = BrokerSnapshot {
            account: account(cash)?,
            ..executor.snapshot(ReconcileReason::Scheduled)?
        };
        executor.run(Input::BrokerSnapshot(taken), ports)
    }

    /// §11's cash row is two tiers (DEC-146): the first run out of the band alerts and pauses
    /// nobody; the second consecutive one pauses every agent. The count is folded from the
    /// journal, so it survives each snapshot's re-anchor and a restart; a run inside the band
    /// starts it again.
    #[test]
    fn a_cash_drift_alerts_at_once_and_pauses_when_it_persists() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let agent = AgentId("agent-a".to_owned());
        let mut executor = reporting(&ports)?;
        let first = cash_run(&mut executor, "90000", &ports)?;
        assert_eq!(
            (
                alerts(&first),
                drafted(&first).contains(&"AgentModeApplied")
            ),
            (vec!["reconciliation_cash_drift"], false),
            "run 1 out of the band alerts and pauses nobody"
        );
        assert_eq!(executor.state.effective_mode(&agent), Mode::Normal);

        let mut executor = executor.restarted(&ports)?;
        let second = cash_run(&mut executor, "80000", &ports)?;
        assert_eq!(
            alerts(&second),
            vec!["reconciliation_cash"],
            "run 2 out of the band, after the re-anchor and a restart, is persistent"
        );
        assert_eq!(
            executor.state.effective_mode(&agent),
            Mode::Paused,
            "and pauses every agent"
        );

        let mut executor = reporting(&ports)?;
        cash_run(&mut executor, "90000", &ports)?;
        let inside = cash_run(&mut executor, "90000", &ports)?;
        assert!(
            alerts(&inside).is_empty(),
            "the same cash is inside the band"
        );
        let again = cash_run(&mut executor, "80000", &ports)?;
        assert_eq!(
            alerts(&again),
            vec!["reconciliation_cash_drift"],
            "a run inside the band starts the count again"
        );
        assert_eq!(executor.state.effective_mode(&agent), Mode::Normal);
        Ok(())
    }

    /// §7.3's first row on the account a reconciliation reads: a run whose account is
    /// `trading_blocked` restricts the account and pauses every agent, through the same function a
    /// pushed account goes through, and an opening is then not submitted (#205 review, round 1,
    /// finding 4).
    #[test]
    fn a_run_whose_account_read_is_blocked_pauses_openings() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = reporting(&ports)?;
        let taken = BrokerSnapshot {
            account: BrokerAccount {
                trading_blocked: true,
                ..account("100000")?
            },
            ..executor.snapshot(ReconcileReason::Scheduled)?
        };
        let run = executor.run(Input::BrokerSnapshot(taken), &ports)?;
        assert!(
            drafted(&run).contains(&"AccountRestrictionChanged"),
            "the block is journaled: {:?}",
            drafted(&run)
        );
        assert_eq!(
            executor
                .state
                .effective_mode(&AgentId("agent-a".to_owned())),
            Mode::Paused
        );
        let opening = executor.run(
            intent(
                "01JABCDEFGHJKMNPQRSTVWXYZ0",
                "agent-a",
                Side::Buy,
                Purpose::Open,
            )?,
            &ports,
        )?;
        assert_eq!(submitted(&opening), 0, "an opening is not submitted");
        Ok(())
    }

    #[test]
    fn a_pushed_blocked_account_records_observation_before_restriction() -> Result<(), ExecutorError>
    {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = Executor::opened(&ports)?;
        let blocked = BrokerAccount {
            trading_blocked: true,
            ..account("100000")?
        };
        let effects = executor.run(Input::BrokerUpdate(BrokerUpdate::Account(blocked)), &ports)?;
        assert_eq!(
            drafted(&effects),
            [
                "AccountStateObserved",
                "AccountRestrictionChanged",
                "AgentModeApplied"
            ],
            "the complete account fact precedes its restriction and fail-closed mode effect"
        );
        Ok(())
    }

    /// Journal spec §9: an acknowledgment names its owner. One with no `user` is refused by the
    /// fold and lifts nothing, whatever its step-up evidence (#205 review, round 1, finding 6).
    #[test]
    fn an_acknowledgment_without_its_user_lifts_nothing() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = Executor::opened(&ports)?;
        executor.commit_one(
            "AgentModeApplied",
            object(vec![
                ("agent", Value::Str("*".to_owned())),
                ("to", Value::Str("paused".to_owned())),
                ("restriction", Value::Str("reconciliation:cash".to_owned())),
                ("originated", Value::Bool(true)),
                (
                    "risk_clock",
                    crate::payload::clock(crate::types::RiskClock::from_secs(0))?,
                ),
            ])?,
        )?;
        let unnamed = object(vec![
            ("subject", Value::Str("cash".to_owned())),
            ("step_up", Value::Str("assertion-1".to_owned())),
            (
                "risk_clock",
                crate::payload::clock(crate::types::RiskClock::from_secs(0))?,
            ),
        ])?;
        assert!(
            executor.commit_one("OwnerAcknowledged", unnamed).is_err(),
            "an acknowledgment with no user is refused"
        );
        assert_eq!(
            executor
                .state
                .effective_mode(&AgentId("agent-a".to_owned())),
            Mode::Paused,
            "and lifts nothing"
        );
        Ok(())
    }

    fn broker_fee(accrued: &str, charged: &str) -> Result<Value, ExecutorError> {
        object(vec![
            ("family", Value::Str("equities".to_owned())),
            ("day", Value::Str("2026-09-22".to_owned())),
            ("accrued", Value::Str(accrued.to_owned())),
            ("charged", Value::Str(charged.to_owned())),
            ("simulated", Value::Bool(false)),
            (
                "risk_clock",
                crate::payload::clock(crate::types::RiskClock::from_secs(0))?,
            ),
        ])
    }

    fn posting_run(
        executor: &mut Executor,
        cash: &str,
        ports: &Ports<'_>,
    ) -> Result<Vec<Effect>, ExecutorError> {
        let taken = BrokerSnapshot {
            account: account(cash)?,
            ..executor.snapshot(ReconcileReason::FeePosting)?
        };
        executor.run(Input::BrokerSnapshot(taken), ports)
    }

    /// §11's cash row is exact after posting (§8.3): a broker whose cash fell by exactly the posted
    /// charge agrees at the posting run, once and twice in a row, so a correct broker never
    /// alerts or pauses (#205 review, round 2, blocking 1).
    #[test]
    fn a_correct_posting_is_no_cash_difference() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = reporting(&ports)?;
        executor.commit_one("FeesCharged", broker_fee("5", "0")?)?;
        executor.commit_one("FeesCharged", broker_fee("0", "5")?)?;
        let first = posting_run(&mut executor, "99995", &ports)?;
        assert_eq!(
            (
                alerts(&first),
                drafted(&first).contains(&"AgentModeApplied")
            ),
            (Vec::<&str>::new(), false),
            "one posting with the broker's cash down by the charge: {:?}",
            drafted(&first)
        );
        executor.commit_one("FeesCharged", broker_fee("3", "3")?)?;
        let second = posting_run(&mut executor, "99992", &ports)?;
        assert_eq!(
            (
                alerts(&second),
                drafted(&second).contains(&"AgentModeApplied")
            ),
            (Vec::<&str>::new(), false),
            "and a second in a row: {:?}",
            drafted(&second)
        );
        assert_eq!(
            executor
                .state
                .effective_mode(&AgentId("agent-a".to_owned())),
            Mode::Normal
        );
        Ok(())
    }

    /// The never-observed hold on its own, not through any test harness's startup: an opening is
    /// held while no account has been journaled, however many runs complete, and released once an
    /// account is journaled and a run has completed since the start, whichever comes first: by the
    /// startup run's resume when the account came first, at the next tick when the run did (#222
    /// review, minor 3).
    #[test]
    fn the_hold_lifts_on_an_account_and_a_run_in_either_order() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let opening = |id: &str| intent(id, "agent-a", Side::Buy, Purpose::Open);
        let tick = |at: i64| Input::Tick(crate::types::RiskClock::from_secs(at));
        let report = || -> Result<Input, ExecutorError> {
            Ok(Input::BrokerUpdate(BrokerUpdate::Account(account(
                "100000",
            )?)))
        };

        let mut never = Executor::opened(&ports)?;
        let held = never.run(opening("01JABCDEFGHJKMNPQRSTVWXYZ0")?, &ports)?;
        assert_eq!(submitted(&held), 0, "no account reported: held");
        for at in 1..=3 {
            let taken = never.snapshot(ReconcileReason::Scheduled)?;
            never.run(Input::BrokerSnapshot(taken), &ports)?;
            let still = never.run(tick(at), &ports)?;
            assert_eq!(
                submitted(&still),
                0,
                "runs complete, but with no account reported the opening stays held"
            );
        }

        let mut account_first = Executor::opened(&ports)?;
        account_first.run(opening("01JABCDEFGHJKMNPQRSTVWXYZ1")?, &ports)?;
        account_first.run(report()?, &ports)?;
        let reported_only = account_first.run(tick(1), &ports)?;
        assert_eq!(submitted(&reported_only), 0, "an account alone: held");
        let taken = BrokerSnapshot {
            account: account("100000")?,
            ..account_first.snapshot(ReconcileReason::Startup)?
        };
        let startup = account_first.run(Input::BrokerSnapshot(taken), &ports)?;
        assert_eq!(
            submitted(&startup),
            1,
            "account, then run: the startup run's resume releases it at once"
        );

        let mut run_first = Executor::opened(&ports)?;
        run_first.run(opening("01JABCDEFGHJKMNPQRSTVWXYZ2")?, &ports)?;
        let taken = run_first.snapshot(ReconcileReason::Startup)?;
        run_first.run(Input::BrokerSnapshot(taken), &ports)?;
        let run_only = run_first.run(tick(1), &ports)?;
        assert_eq!(submitted(&run_only), 0, "a run alone: held");
        run_first.run(report()?, &ports)?;
        let released = run_first.run(tick(2), &ports)?;
        assert_eq!(submitted(&released), 1, "run, then account: released");
        Ok(())
    }

    /// §5.7's confirmed cancel: the broker's `CancelAccepted` makes the order `Canceled` with
    /// `cancel_confirmed`, releasing its reservation and the unconfirmed cancel that held its
    /// instrument; an id this executor does not carry asks for a reconciliation instead.
    #[test]
    fn a_confirmed_cancel_ends_the_order_and_an_unknown_one_asks_for_a_run()
    -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = reporting(&ports)?;
        executor.run(
            intent(
                "01JABCDEFGHJKMNPQRSTVWXYZ3",
                "agent-a",
                Side::Buy,
                Purpose::Open,
            )?,
            &ports,
        )?;
        let id = executor
            .state
            .orders
            .keys()
            .next()
            .cloned()
            .ok_or_else(|| missing("the submitted order"))?;
        executor.commit_one(
            "OrderStateChanged",
            object(vec![
                ("client_order_id", Value::Str(id.as_str().to_owned())),
                ("state", Value::Str("accepted".to_owned())),
                ("cancel_requested", Value::Bool(true)),
                (
                    "risk_clock",
                    crate::payload::clock(crate::types::RiskClock::from_secs(0))?,
                ),
            ])?,
        )?;
        let confirmed = executor.run(
            Input::Broker(Ok(BrokerOutcome::CancelAccepted {
                client_order_id: id.as_str().to_owned(),
            })),
            &ports,
        )?;
        assert_eq!(drafted(&confirmed), vec!["OrderStateChanged"]);
        let order = executor
            .state
            .order(&id)
            .ok_or(missing("the cancelled order"))?;
        assert_eq!(
            (order.state, order.cancel_unconfirmed),
            (OrderState::Canceled, false)
        );
        assert!(
            !executor.state.reservations.contains_key(&id),
            "the reservation is released"
        );

        let stranger = executor.run(
            Input::Broker(Ok(BrokerOutcome::CancelAccepted {
                client_order_id: "md-01JABCDEFGHJKMNPQRSTVWXYZ7".to_owned(),
            })),
            &ports,
        )?;
        assert!(
            stranger
                .iter()
                .any(|effect| matches!(effect, Effect::Broker(BrokerRequest::ListOpenOrders))),
            "an id it does not carry asks for a reconciliation: {stranger:?}"
        );
        Ok(())
    }

    /// A followed stream's journaled fact answers E7-4 slice 5's stub rather than being dropped,
    /// through the real `handle`, and leaves the state as it was.
    #[test]
    fn a_journaled_fact_from_a_followed_stream_answers_the_copy_stub() -> Result<(), ExecutorError>
    {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = reporting(&ports)?;
        let before = executor.state.clone();
        let fact = FoldedEvent {
            stream: "clock:ws1".to_owned(),
            seq: Seq(1),
            event_id: EventId("clock:ws1-1".to_owned()),
            event_type: "TradingDayStarted".to_owned(),
            causation_id: None,
            payload: object(vec![("date", Value::Str("2026-09-22".to_owned()))])?,
        };
        assert_eq!(
            handle(
                &mut executor.state,
                Input::Journal(fact),
                &ports,
                &ALLOWING_BINDING_GATE,
            )
            .map(|_| ()),
            Err(ExecutorError::Unimplemented { story: "E7-4" })
        );
        assert_eq!(executor.state, before, "a refused step changes nothing");
        Ok(())
    }

    /// Reconciled since its start, `agent-a`'s 10 AAPL, protected by one resting OCO (`md-oco-1`) at 170 over 140 that its lots
    /// own.
    pub(crate) fn protected_by_an_oco(ports: &Ports<'_>) -> Result<Executor, ExecutorError> {
        let mut executor = reporting(ports)?;
        let clock = || crate::payload::clock(crate::types::RiskClock::from_secs(0));
        let text = |raw: &str| Value::Str(raw.to_owned());
        executor.commit_one(
            "OrderSubmitted",
            object(vec![
                ("client_order_id", text("md-buy-a")),
                ("agent", text("agent-a")),
                ("instrument", text("AAPL")),
                ("side", text("buy")),
                ("qty", text("10")),
                ("limit", text("150")),
                ("risk_clock", clock()?),
            ])?,
        )?;
        executor.commit_one(
            "FillApplied",
            object(vec![
                ("fill_id", text("f-a")),
                ("client_order_id", text("md-buy-a")),
                ("instrument", text("AAPL")),
                ("side", text("buy")),
                ("qty_gross", text("10")),
                ("price", text("150")),
                ("risk_clock", clock()?),
            ])?,
        )?;
        executor.commit_one(
            "ProtectionChanged",
            object(vec![
                ("instrument", text("AAPL")),
                ("action", text("placed")),
                ("orders", text("md-oco-1")),
                ("qty", text("10")),
                ("take_profit", text("170")),
                ("stop", text("140")),
                ("risk_clock", clock()?),
            ])?,
        )?;
        Ok(executor)
    }

    /// Two agents' buys of 5 AAPL each, and a broker-created OCO leg for the 10 that DEC-160's
    /// rule cannot give to either.
    fn two_holders_and_an_ownerless_leg(ports: &Ports<'_>) -> Result<Executor, ExecutorError> {
        let mut executor = Executor::opened(ports)?;
        let clock = || crate::payload::clock(crate::types::RiskClock::from_secs(0));
        for (agent, id) in [("agent-a", "md-buy-a"), ("agent-b", "md-buy-b")] {
            executor.commit_one(
                "OrderSubmitted",
                object(vec![
                    ("client_order_id", Value::Str(id.to_owned())),
                    ("agent", Value::Str(agent.to_owned())),
                    ("instrument", Value::Str("AAPL".to_owned())),
                    ("side", Value::Str("buy".to_owned())),
                    ("qty", Value::Str("5".to_owned())),
                    ("limit", Value::Str("150".to_owned())),
                    ("risk_clock", clock()?),
                ])?,
            )?;
            executor.commit_one(
                "FillApplied",
                object(vec![
                    ("fill_id", Value::Str(format!("f-{id}"))),
                    ("client_order_id", Value::Str(id.to_owned())),
                    ("instrument", Value::Str("AAPL".to_owned())),
                    ("side", Value::Str("buy".to_owned())),
                    ("qty_gross", Value::Str("5".to_owned())),
                    ("price", Value::Str("150".to_owned())),
                    ("risk_clock", clock()?),
                ])?,
            )?;
            executor.commit_one(
                "OrderStateChanged",
                object(vec![
                    ("client_order_id", Value::Str(id.to_owned())),
                    ("state", Value::Str("filled".to_owned())),
                    ("risk_clock", clock()?),
                ])?,
            )?;
        }
        executor.commit_one(
            "ProtectionChanged",
            object(vec![
                ("instrument", Value::Str("AAPL".to_owned())),
                ("action", Value::Str("placed".to_owned())),
                ("orders", Value::Str("md-oco-1".to_owned())),
                ("qty", Value::Str("10".to_owned())),
                ("risk_clock", clock()?),
            ])?,
        )?;
        let leg = ClientOrderId::parse("md-oco-1")?;
        assert_eq!(
            executor.state.order(&leg).map(|leg| leg.agent.clone()),
            Some(None),
            "the leg is in the order set, and no one's"
        );
        Ok(executor)
    }

    /// #258 round 1, the reviewer's probe (DEC-160 3(c)): a snapshot that lists an ownerless leg
    /// finds it **present**. It is not external activity, and nobody's mode changes; the leg stays
    /// no one's, since a reconciliation establishes presence and not ownership.
    #[test]
    fn a_snapshot_listing_an_ownerless_leg_finds_it_present_not_external()
    -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = two_holders_and_an_ownerless_leg(&ports)?;
        let mut taken = executor.snapshot(ReconcileReason::Scheduled)?;
        taken.positions = vec![BrokerPosition {
            instrument: aapl()?,
            qty: SignedQty::parse("10")?,
            avg_entry_price: Price::parse("150")?,
        }];
        taken.open_orders = vec![BrokerOrder {
            broker_order_id: "b-oco".to_owned(),
            client_order_id: Some("md-oco-1".to_owned()),
            instrument: aapl()?,
            side: Side::Sell,
            qty: Qty::parse("10")?,
            filled_qty: Qty::ZERO,
            limit_price: Some(Price::parse("170")?),
            stop_price: None,
            status: "new".to_owned(),
            reject_code: None,
            replaced_by_broker_order_id: None,
            legs: Vec::new(),
            created_on: None,
        }];
        let run = executor.run(Input::BrokerSnapshot(taken), &ports)?;
        let drafts = drafted(&run);
        assert!(
            !drafts.contains(&"ExternalActivityIngested") && !drafts.contains(&"AgentModeApplied"),
            "{drafts:?}"
        );
        assert!(drafts.contains(&"ReconciliationRun"), "{drafts:?}");
        assert_eq!(
            executor
                .state
                .order(&ClientOrderId::parse("md-oco-1")?)
                .map(|leg| leg.agent.clone()),
            Some(None),
            "presence, not ownership"
        );
        Ok(())
    }

    /// #258 round 1 (d), DEC-160 3(d): the broker's confirmation of a cancel sent by the ownerless
    /// leg's own id completes it — the leg is `Canceled`, confirmed — rather than asking for a
    /// reconciliation.
    #[test]
    fn a_confirmed_cancel_of_an_ownerless_leg_completes_it() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = two_holders_and_an_ownerless_leg(&ports)?;
        let confirmed = executor.run(
            Input::Broker(Ok(BrokerOutcome::CancelAccepted {
                client_order_id: "md-oco-1".to_owned(),
            })),
            &ports,
        )?;
        assert_eq!(
            drafted(&confirmed),
            vec![
                "OrderStateChanged",
                "ProtectionChanged",
                "ProtectionChanged"
            ],
            "and, a protective order, it leaves the instrument's protection (slice 3a); with no \
             prices to re-place it at, the shortfall is journaled and the owner alerted, never \
             silent (the founder's decision on #468, DEC-367 item 4)"
        );
        assert!(
            confirmed.iter().any(|effect| matches!(
                effect,
                Effect::Notify(note) if note.message_key == "protection_expiring"
            )),
            "{confirmed:?}"
        );
        assert!(
            confirmed.iter().any(|effect| matches!(
                effect,
                Effect::Journal(draft)
                    if draft.payload.get("cancel_confirmed") == Some(&Value::Bool(true))
            )),
            "the confirmation is journaled as one (#258 round 1, minor 3)"
        );
        assert!(
            !confirmed
                .iter()
                .any(|effect| matches!(effect, Effect::Broker(BrokerRequest::ListOpenOrders))),
            "no reconciliation is asked for"
        );
        let leg = executor
            .state
            .order(&ClientOrderId::parse("md-oco-1")?)
            .ok_or_else(|| missing("the leg"))?;
        assert_eq!((leg.state, leg.agent.clone()), (OrderState::Canceled, None));
        Ok(())
    }

    /// #258 round 2, minor 1: an ownerless leg names no agent to restrict — never an empty `""`
    /// one — so a mismatch in its instrument falls to every agent (`*`), and an owned order there
    /// still names its own.
    #[test]
    fn an_ownerless_leg_names_no_agent_to_restrict() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = two_holders_and_an_ownerless_leg(&ports)?;
        assert_eq!(
            agents(&executor.state, Some(&aapl()?)),
            vec!["agent-a".to_owned(), "agent-b".to_owned()]
        );
        executor
            .state
            .orders
            .retain(|_, order| order.agent.is_none());
        assert_eq!(
            agents(&executor.state, Some(&aapl()?)),
            vec!["*".to_owned()]
        );
        Ok(())
    }

    /// #258 round 2, minor 1: a status outside §5.7's table on an ownerless leg restricts every
    /// agent to `exits_only` — openings held, exits untouched — rather than pausing a named agent.
    #[test]
    fn an_unmapped_status_on_an_ownerless_leg_holds_every_agents_openings()
    -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = two_holders_and_an_ownerless_leg(&ports)?;
        let ran = executor.run(
            Input::BrokerUpdate(BrokerUpdate::Order(BrokerOrder {
                broker_order_id: "b-oco".to_owned(),
                client_order_id: Some("md-oco-1".to_owned()),
                instrument: aapl()?,
                side: Side::Sell,
                qty: Qty::parse("10")?,
                filled_qty: Qty::ZERO,
                limit_price: None,
                stop_price: None,
                status: "teleported".to_owned(),
                reject_code: None,
                replaced_by_broker_order_id: None,
                legs: Vec::new(),
                created_on: None,
            })),
            &ports,
        )?;
        let applied: Vec<(Option<&str>, Option<&str>)> = ran
            .iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) if draft.event_type == "AgentModeApplied" => Some((
                    draft.payload.get("agent").and_then(Value::as_str),
                    draft.payload.get("to").and_then(Value::as_str),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(applied, vec![(Some("*"), Some("exits_only"))]);
        Ok(())
    }

    /// The journal vectors' fee-step snapshot (`fixtures/refcases/journal.json`, generated from
    /// `docs/specs/reference-cases/journal.yaml`, at `control_stream.drafts.snapshot_fees`): a draft
    /// the reference validator accepts, whose payload is §9.2's closed schema member for member.
    fn vector_snapshot_fees() -> Result<Map<String, Json>, ExecutorError> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/refcases/journal.json"
        );
        let text = std::fs::read_to_string(path).map_err(|_| non_canonical())?;
        let fixture: Json = serde_json::from_str(&text).map_err(|_| non_canonical())?;
        fixture
            .pointer("/control_stream/drafts/snapshot_fees/payload")
            .and_then(Json::as_object)
            .cloned()
            .ok_or_else(non_canonical)
    }

    fn non_canonical() -> ExecutorError {
        ExecutorError::NonCanonicalPayload {
            field: "journal fixture".to_owned(),
        }
    }

    /// The broker account the vectors' snapshot reads, built from the vector's own values, so the
    /// writer is asked for exactly the payload the vector holds.
    fn vector_account(vector: &Map<String, Json>) -> Result<BrokerAccount, ExecutorError> {
        let text = |name: &str| {
            vector
                .get(name)
                .and_then(Json::as_str)
                .ok_or_else(non_canonical)
        };
        let flag = |name: &str| {
            vector
                .get(name)
                .and_then(Json::as_bool)
                .ok_or_else(non_canonical)
        };
        let multiplier = vector
            .get("multiplier")
            .and_then(Json::as_u64)
            .ok_or_else(non_canonical)?;
        Ok(BrokerAccount {
            status: text("status")?.to_owned(),
            crypto_status: text("crypto_status")?.to_owned(),
            trading_blocked: flag("trading_blocked")?,
            account_blocked: flag("account_blocked")?,
            trade_suspended_by_user: flag("trade_suspended_by_user")?,
            multiplier: u32::try_from(multiplier).map_err(|_| non_canonical())?,
            equity: Usd::parse(text("equity")?)?,
            cash: Usd::parse(text("cash")?)?,
            buying_power: Usd::parse(text("buying_power")?)?,
            non_marginable_buying_power: Usd::parse(text("non_marginable_buying_power")?)?,
            accrued_fees: Usd::parse(text("accrued_fees")?)?,
            last_equity: Usd::parse(text("equity")?)?,
            maintenance_margin: Usd::ZERO,
        })
    }

    /// The fee step's snapshot is the vectors' `snapshot_fees` payload for the account it reads,
    /// member for member, each once, in the vector's own type and value: text as text, flags as
    /// booleans, `multiplier` as an integer, money as canonical decimal text, and `model_cash`,
    /// `cash_band`, and `cash_in_band` present as `null`, never absent (§4.2, rule 24: a fee
    /// posting compares no cash, so all three are `null` together). A member typed wrong, a value
    /// carrying anything the broker did not report, or a member written twice is refused here as
    /// `append` would refuse it. `risk_clock` is the one member this payload does not carry: the
    /// batch stamps it beside these (DEC-305 item 1, DEC-306).
    #[test]
    fn the_fee_steps_snapshot_carries_the_members_the_vectors_close() -> Result<(), ExecutorError> {
        let vector = vector_snapshot_fees()?;
        for name in ["model_cash", "cash_band", "cash_in_band"] {
            assert_eq!(
                vector.get(name),
                Some(&Json::Null),
                "the vectors carry {name} as null on the fee step (§4.2)"
            );
        }
        let mut closed: Vec<&str> = vector
            .keys()
            .map(String::as_str)
            .filter(|name| *name != "risk_clock")
            .collect();
        closed.sort_unstable();
        assert_eq!(
            closed.len(),
            14,
            "§9.2 closes fifteen members, and the batch stamps risk_clock: {closed:?}"
        );
        let written = super::fee_step_snapshot_fields(&vector_account(&vector)?)?;
        let mut names: Vec<&str> = written.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        assert_eq!(
            names, closed,
            "every member §9.2 closes, each once, less the batch's risk_clock"
        );
        for (name, value) in &written {
            let carried: Json =
                serde_json::from_slice(&to_canonical(value)).map_err(|_| non_canonical())?;
            assert_eq!(
                Some(&carried),
                vector.get(*name),
                "{name} carries the vector's value in the vector's type"
            );
        }
        Ok(())
    }

    /// The fee step's snapshot reads the account it is given: a second account, every value of
    /// which differs from the vectors' `snapshot_fees`, comes back member for member in §9.2's
    /// types, so a writer that ignores its argument and answers the vector's values fails here
    /// (#441 round 2, m1). The expected payload is written out by hand from the account, not read
    /// back through the writer or `account_fields`. The account's `last_equity` and
    /// `maintenance_margin` are set and differ from its equity, and the closed schema carries
    /// neither, so a writer that journaled either fails here too (DEC-524 item 3).
    #[test]
    fn the_fee_steps_snapshot_reads_the_account_it_is_given() -> Result<(), ExecutorError> {
        let vector = vector_snapshot_fees()?;
        let other = BrokerAccount {
            status: "ACCOUNT_UPDATED".to_owned(),
            crypto_status: "INACTIVE".to_owned(),
            trading_blocked: true,
            account_blocked: true,
            trade_suspended_by_user: true,
            multiplier: 2,
            equity: Usd::parse("31337.5")?,
            cash: Usd::parse("4321.09")?,
            buying_power: Usd::parse("62675")?,
            non_marginable_buying_power: Usd::parse("4321.09")?,
            accrued_fees: Usd::parse("1.23")?,
            last_equity: Usd::parse("29999.99")?,
            maintenance_margin: Usd::parse("7500.25")?,
        };
        let expected: Map<String, Json> = [
            ("status", Json::from("ACCOUNT_UPDATED")),
            ("crypto_status", Json::from("INACTIVE")),
            ("trading_blocked", Json::Bool(true)),
            ("account_blocked", Json::Bool(true)),
            ("trade_suspended_by_user", Json::Bool(true)),
            ("multiplier", Json::from(2_u64)),
            ("equity", Json::from("31337.5")),
            ("cash", Json::from("4321.09")),
            ("buying_power", Json::from("62675")),
            ("non_marginable_buying_power", Json::from("4321.09")),
            ("accrued_fees", Json::from("1.23")),
            ("model_cash", Json::Null),
            ("cash_band", Json::Null),
            ("cash_in_band", Json::Null),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
        let differing: Vec<&str> = expected
            .iter()
            .filter(|(name, value)| !value.is_null() && vector.get(name.as_str()) == Some(value))
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(
            differing,
            Vec::<&str>::new(),
            "every reported member differs from the vector's, so the vector's values cannot pass"
        );
        let written = super::fee_step_snapshot_fields(&other)?;
        let mut names: Vec<&str> = written.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        let mut wanted: Vec<&str> = expected.keys().map(String::as_str).collect();
        wanted.sort_unstable();
        assert_eq!(names, wanted, "every member §9.2 closes, each once");
        for (name, value) in &written {
            let carried: Json =
                serde_json::from_slice(&to_canonical(value)).map_err(|_| non_canonical())?;
            assert_eq!(
                Some(&carried),
                expected.get(*name),
                "{name} carries the account's own value in §9.2's type"
            );
        }
        Ok(())
    }

    /// A batch at a started state, for the fee step's own answer.
    fn pause_batch<'a>(ports: &'a Ports<'a>) -> Result<Batch<'a, 'a>, ExecutorError> {
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        state.epoch = Some(WriterEpoch(1));
        state.started = true;
        Batch::new(&state, ports)
    }

    /// What the fee step's answer left in a batch: each draft's id and type in order, the
    /// `AgentModeApplied` pauses, and each alert with its position among the effects.
    struct Answer {
        drafted: Vec<(usize, EventId, String)>,
        paused: Vec<(Option<String>, Option<String>, Option<String>)>,
        alerts: Vec<(usize, EventId, &'static str)>,
    }

    fn answer_of(effects: &[Effect]) -> Answer {
        let mut answer = Answer {
            drafted: Vec::new(),
            paused: Vec::new(),
            alerts: Vec::new(),
        };
        for (at, effect) in effects.iter().enumerate() {
            match effect {
                Effect::Journal(draft) => {
                    answer
                        .drafted
                        .push((at, draft.event_id.clone(), draft.event_type.clone()));
                    if draft.event_type == "AgentModeApplied" {
                        let member = |name: &str| {
                            draft
                                .payload
                                .get(name)
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                        };
                        answer
                            .paused
                            .push((member("agent"), member("to"), member("restriction")));
                    }
                }
                Effect::Notify(alert) => {
                    answer
                        .alerts
                        .push((at, alert.subject_event.clone(), alert.message_key));
                }
                Effect::Broker(_) | Effect::Timer(_) => {}
            }
        }
        answer
    }

    /// The type of the draft `subject` names, if this batch journaled it before `before`: an alert
    /// names only an event already journaled (`AGENTS.md` rule 5).
    fn journaled_before(answer: &Answer, subject: &EventId, before: usize) -> Option<String> {
        answer
            .drafted
            .iter()
            .find(|(at, id, _)| id == subject && *at < before)
            .map(|(_, _, event_type)| event_type.clone())
    }

    /// The fee step's answer to a fee difference runs whether or not its snapshot recorded: every
    /// agent paused under the fees restriction, and the owner alerted once, under the generic
    /// `reconciliation_fees` key, about an event this batch already journaled (`AGENTS.md` rules 3,
    /// 5, 6 and 13; DEC-305 item 3). When the snapshot recorded, the alert names it. When the
    /// snapshot's validation refused it, nothing recorded it, so the alert names the pause's own
    /// `AgentModeApplied`, never an id no event carries. Today `fees` journals the snapshot behind a
    /// `?`, so a refusal there would stop the pause this pin holds.
    #[test]
    fn the_fee_steps_pause_and_alert_run_whether_or_not_the_snapshot_recorded()
    -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let fee_pause = vec![(
            Some("*".to_owned()),
            Some("paused".to_owned()),
            Some("reconciliation:fees".to_owned()),
        )];

        let mut recorded_batch = pause_batch(&ports)?;
        let recorded = recorded_batch.journal(
            "AccountSnapshotRecorded",
            None,
            crate::orders::account_fields(&account("0")?)?,
        )?;
        super::fee_step_pause_and_alert(&mut recorded_batch, Some(recorded.clone()))?;
        let answer = answer_of(&recorded_batch.effects);
        assert_eq!(
            answer.paused, fee_pause,
            "a fee difference pauses every agent under the fees restriction"
        );
        assert_eq!(
            answer
                .alerts
                .iter()
                .map(|(_, subject, key)| (subject.clone(), *key))
                .collect::<Vec<_>>(),
            vec![(recorded, "reconciliation_fees")],
            "and alerts the owner once, naming the snapshot that recorded"
        );

        let mut refused_batch = pause_batch(&ports)?;
        super::fee_step_pause_and_alert(&mut refused_batch, None)?;
        let answer = answer_of(&refused_batch.effects);
        assert_eq!(
            answer.paused, fee_pause,
            "a refused snapshot never blocks the pause"
        );
        let subjects: Vec<(Option<String>, &str)> = answer
            .alerts
            .iter()
            .map(|(at, subject, key)| (journaled_before(&answer, subject, *at), *key))
            .collect();
        assert_eq!(
            subjects,
            vec![(Some("AgentModeApplied".to_owned()), "reconciliation_fees")],
            "and the owner is still alerted once, about the pause it journaled: {:?}",
            answer.alerts
        );
        Ok(())
    }

    /// `fees` runs its pause and alert at the call site too, not only in the tail: a fee difference
    /// at a fee posting with no earlier account to compare cash against (step 4 recorded nothing)
    /// pauses every agent under the fees restriction and alerts the owner once, under the generic
    /// `reconciliation_fees` key, naming the fee step's own snapshot, journaled before the alert
    /// (`AGENTS.md` rules 3, 5, 6 and 13; DEC-305 items 3 and 4, DEC-389 item 1). A `fees` that
    /// runs the tail only when step 4 had a base, or never, fails here.
    #[test]
    fn a_fee_difference_with_no_cash_base_still_pauses_and_alerts() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        state.epoch = Some(WriterEpoch(1));
        state.started = true;
        assert_eq!(state.observed, None, "step 4 has no base to record against");
        let mut differing = snapshot(ReconcileReason::FeePosting)?;
        differing.account.accrued_fees = Usd::parse("3")?;
        let run = reconcile(&state, &differing, &ports)?;
        let answer = answer_of(&run.effects);
        assert_eq!(
            answer.paused,
            vec![(
                Some("*".to_owned()),
                Some("paused".to_owned()),
                Some("reconciliation:fees".to_owned()),
            )],
            "a fee difference pauses every agent under the fees restriction"
        );
        let fee_alerts: Vec<(Option<String>, &str)> = answer
            .alerts
            .iter()
            .filter(|(_, _, key)| key.starts_with("reconciliation_fee"))
            .map(|(at, subject, key)| (journaled_before(&answer, subject, *at), *key))
            .collect();
        assert_eq!(
            fee_alerts,
            vec![(
                Some("AccountSnapshotRecorded".to_owned()),
                "reconciliation_fees"
            )],
            "and alerts the owner once, about the fee step's own snapshot: {:?}",
            answer.alerts
        );
        Ok(())
    }

    /// The executor's own `AccountSnapshotRecorded` payloads, journaled as `mandate-journal` would
    /// be asked to: each is accepted whole. Journal spec §9.2 needs the fee step's three cash members
    /// present as `null`, so `fees` journals [`super::fee_step_snapshot_fields`], or the snapshot of
    /// the step that pauses every agent and alerts the owner would be refused (DEC-261 item 7,
    /// DEC-389 item 2, DEC-402, `AGENTS.md` rules 3 and 13). Each payload is parsed before its
    /// members are compared, so a reduced form the journal refuses fails here at its member.
    #[test]
    fn the_fee_steps_snapshot_is_never_refused_for_its_members() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        state.epoch = Some(WriterEpoch(1));
        state.started = true;
        let mut differing = snapshot(ReconcileReason::FeePosting)?;
        differing.account.accrued_fees = Usd::parse("3")?;
        let mut payloads = Vec::new();
        for observed in [
            None,
            Some(ObservedAccount {
                state: AccountState::Active,
                multiplier: 1,
                equity: Usd::ZERO,
                cash: Usd::ZERO,
                buying_power: Usd::ZERO,
                non_marginable_buying_power: Usd::ZERO,
                accrued_fees: Usd::ZERO,
                complete: true,
            }),
        ] {
            state.observed = observed;
            let run = reconcile(&state, &differing, &ports)?;
            payloads.extend(run.effects.into_iter().filter_map(|effect| match effect {
                Effect::Journal(draft) if draft.event_type == "AccountSnapshotRecorded" => {
                    Some(draft.payload)
                }
                _ => None,
            }));
        }
        assert_eq!(
            payloads.len(),
            2,
            "the fee step's own snapshot, then a cash comparison's"
        );
        for payload in &payloads {
            let draft = format!(
                r#"{{"envelope_version":1,"environment":"paper","event_id":"01J8Z3N0A000000000000000R1",
                "stream_id":"acct:ws1:acct-1","event_type":"AccountSnapshotRecorded","schema_version":1,
                "event_time":"2026-09-21T21:00:00.000000000Z","clock_source":"broker",
                "causation_id":null,"correlation_id":null,
                "actor":{{"kind":"system","id":"executor","version":"0.1.0","build":"sha256:{}"}},
                "config_refs":{{}},"payload":{},"artifact_refs":[],"pii_refs":[]}}"#,
                "3".repeat(64),
                String::from_utf8_lossy(&to_canonical(payload))
            );
            assert_eq!(
                Draft::parse(draft.as_bytes()).map(|_| ()),
                Ok(()),
                "the snapshot is accepted whole"
            );
        }
        let nulled: Vec<bool> = payloads
            .iter()
            .map(|payload| payload.get("model_cash") == Some(&Value::Null))
            .collect();
        assert_eq!(
            nulled,
            vec![true, false],
            "the fee step's own snapshot writes `model_cash` as null; a cash comparison's compares"
        );
        Ok(())
    }

    /// One sell leg as Alpaca nests it under a filled entry: `qty` of it, `status`, and the
    /// prices a stop or a take-profit carries.
    fn leg(
        broker_id: &str,
        qty: &str,
        status: &str,
        stop: Option<&str>,
        limit: Option<&str>,
    ) -> Result<BrokerOrder, ExecutorError> {
        Ok(BrokerOrder {
            broker_order_id: broker_id.to_owned(),
            client_order_id: Some(format!("{broker_id}-client")),
            instrument: aapl()?,
            side: Side::Sell,
            qty: Qty::parse(qty)?,
            filled_qty: Qty::ZERO,
            limit_price: limit.map(Price::parse).transpose()?,
            stop_price: stop.map(Price::parse).transpose()?,
            status: status.to_owned(),
            reject_code: None,
            replaced_by_broker_order_id: None,
            legs: Vec::new(),
            created_on: Some(Date::parse("2026-09-22")?),
        })
    }

    /// The entry as the in-doubt lookup's answer describes it: `filled`, by its own
    /// `client_order_id`, with `legs` nested under it.
    fn listed_entry(entry: &str, legs: Vec<BrokerOrder>) -> Result<BrokerOrder, ExecutorError> {
        Ok(BrokerOrder {
            legs,
            broker_order_id: "e0000000".to_owned(),
            client_order_id: Some(entry.to_owned()),
            instrument: aapl()?,
            side: Side::Buy,
            qty: Qty::parse("10")?,
            filled_qty: Qty::parse("10")?,
            limit_price: Some(Price::parse("150")?),
            stop_price: None,
            status: "filled".to_owned(),
            reject_code: None,
            replaced_by_broker_order_id: None,
            created_on: Some(Date::parse("2026-09-22")?),
        })
    }

    /// DEC-878 item 2(c), #1292's contract items 2 and 3: the in-doubt lookup of a filled
    /// bracket's placement settles its doubt only when the answer — the entry read back by its
    /// own id, with its nested legs — shows the whole bracket resting **as recorded**: exactly
    /// the placement's quantity and the entry's recorded stop and take-profit prices, the check
    /// that is beyond any connector's reach from the handle alone and that runs where the record
    /// lives. A mismatched price or quantity refuses, and the placement stays `Unknown`: in
    /// doubt, never settled present, never confirmed absent, never resubmitted. The recorded
    /// answer settles it by §5.7's own `Unknown --> Accepted: found`, journaled before it takes
    /// effect.
    #[test]
    fn a_doubted_placement_settles_only_on_legs_matching_the_recorded_quantity_and_prices()
    -> Result<(), ExecutorError> {
        let stop_at = |stop: &str| leg("b2222222", "10", "held", Some(stop), None);
        let take_profit_at = |limit: &str| leg("a1111111", "10", "new", None, Some(limit));
        let cases = [
            (
                "the recorded stop at 140 and take-profit at 170, resting",
                (take_profit_at("170")?, stop_at("140")?),
                true,
            ),
            (
                "the stop at 141, not the recorded 140",
                (take_profit_at("170")?, stop_at("141")?),
                false,
            ),
            (
                "the take-profit at 169, not the recorded 170",
                (take_profit_at("169")?, stop_at("140")?),
                false,
            ),
            (
                "the stop for 9 of the 10",
                (
                    take_profit_at("170")?,
                    leg("b2222222", "9", "held", Some("140"), None)?,
                ),
                false,
            ),
            (
                "the take-profit for 9 of the 10",
                (
                    leg("a1111111", "9", "new", None, Some("170"))?,
                    stop_at("140")?,
                ),
                false,
            ),
            (
                "the stop canceled, resting no longer",
                (
                    take_profit_at("170")?,
                    leg("b2222222", "10", "canceled", Some("140"), None)?,
                ),
                false,
            ),
        ];
        for (case, (take_profit, stop), settles) in cases {
            let (config, fees) = (executor_config(), fees()?);
            let ports = Ports {
                ids: &Ids,
                mandates: &Everything,
                instruments: &Everything,
                config: &config,
                fees: &fees,
            };
            let mut executor = Executor::opened(&ports)?;
            executor.run(
                Input::BrokerUpdate(BrokerUpdate::Account(account("20000")?)),
                &ports,
            )?;
            executor.run(
                Input::BrokerSnapshot(BrokerSnapshot {
                    account: account("20000")?,
                    ..executor.snapshot(ReconcileReason::Startup)?
                }),
                &ports,
            )?;
            let submitted = executor.run(
                Input::Intent(IntentHandoff {
                    intent_id: IntentId(EventId("01JABCDEFGHJKMNPQRSTVWXYZ0".to_owned())),
                    agent: AgentId("agent-a".to_owned()),
                    tif: Some(TimeInForce::Day),
                    body: IntentBody::Order {
                        instrument: aapl()?,
                        side: Side::Buy,
                        qty: Qty::parse("10")?,
                        limit: Price::parse("150")?,
                        purpose: Purpose::Open,
                        protection: Some(ProtectionPrices {
                            stop: Price::parse("140")?,
                            take_profit: Some(Price::parse("170")?),
                        }),
                    },
                }),
                &ports,
            )?;
            let entry = submitted
                .iter()
                .find_map(|effect| match effect {
                    Effect::Broker(BrokerRequest::Submit(order)) if order.bracket.is_some() => {
                        Some(order.client_order_id.as_str().to_owned())
                    }
                    _ => None,
                })
                .ok_or_else(|| missing("the entry goes as one bracket (§5.4)"))?;
            executor.run(
                Input::BrokerUpdate(BrokerUpdate::Fill(BrokerFill {
                    fill_id: FillId("f-1".to_owned()),
                    client_order_id: Some(entry.clone()),
                    instrument: aapl()?,
                    side: Side::Buy,
                    qty: Qty::parse("10")?,
                    price: Price::parse("150")?,
                    fees: Usd::ZERO,
                    trade_date: Date::parse("2026-09-22")?,
                })),
                &ports,
            )?;
            executor.run(
                Input::BrokerUpdate(BrokerUpdate::Order(BrokerOrder {
                    broker_order_id: "e0000000".to_owned(),
                    client_order_id: Some(entry.clone()),
                    instrument: aapl()?,
                    side: Side::Buy,
                    qty: Qty::parse("10")?,
                    filled_qty: Qty::parse("10")?,
                    limit_price: Some(Price::parse("150")?),
                    stop_price: None,
                    status: "filled".to_owned(),
                    reject_code: None,
                    replaced_by_broker_order_id: None,
                    legs: Vec::new(),
                    created_on: Some(Date::parse("2026-09-22")?),
                })),
                &ports,
            )?;
            let placement = executor
                .journal
                .iter()
                .filter(|event| event.event_type == "ProtectionChanged")
                .filter(|event| {
                    matches!(
                        event.payload.get("action"),
                        Some(Value::Str(action)) if action == "placed"
                    )
                })
                .flat_map(|event| {
                    event
                        .payload
                        .get("orders")
                        .and_then(Value::as_array)
                        .map(|ids| {
                            ids.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                })
                .next()
                .ok_or_else(|| missing("the complete fill records the legs placed"))?;

            let adopted = executor.run(
                Input::BrokerSnapshot(BrokerSnapshot {
                    open_orders: Vec::new(),
                    positions: vec![BrokerPosition {
                        instrument: aapl()?,
                        qty: SignedQty::parse("10")?,
                        avg_entry_price: Price::parse("150")?,
                    }],
                    account: account("18500")?,
                    ..executor.snapshot(ReconcileReason::Scheduled)?
                }),
                &ports,
            )?;
            let asked_by_entry = adopted.iter().any(|effect| {
                matches!(
                    effect,
                    Effect::Broker(BrokerRequest::GetOrderByClientId(asked))
                        if asked.as_str() == entry
                )
            });
            let asked_by_handle = adopted.iter().any(|effect| {
                matches!(
                    effect,
                    Effect::Broker(BrokerRequest::GetOrderByClientId(asked))
                        if asked.as_str() == placement
                )
            });
            assert!(
                asked_by_entry && !asked_by_handle,
                "{case}: the adopted placement is asked after by the entry's own id, never the \
                 handle (DEC-878 item 1): {adopted:?}"
            );
            assert_eq!(
                executor
                    .state
                    .orders()
                    .get(&ClientOrderId::parse(&placement)?)
                    .map(|order| order.state),
                Some(OrderState::Unknown),
                "{case}: the broker listing nothing adopts the placement in doubt (DEC-878 item 5)"
            );

            executor.run(
                Input::Broker(Ok(BrokerOutcome::Order(listed_entry(
                    &entry,
                    vec![take_profit, stop],
                )?))),
                &ports,
            )?;

            let state = executor
                .state
                .orders()
                .get(&ClientOrderId::parse(&placement)?)
                .map(|order| order.state);
            if settles {
                assert_eq!(
                    state,
                    Some(OrderState::Accepted),
                    "{case}: the whole bracket resting under the filled entry as recorded settles \
                     the doubt, §5.7's `Unknown --> Accepted: found`"
                );
            } else {
                assert_eq!(
                    state,
                    Some(OrderState::Unknown),
                    "{case}: legs that do not match the recorded quantity or price settle \
                     nothing: the placement stays in doubt (DEC-878 item 2(c), rule 3)"
                );
            }
            let resubmitted = executor
                .journal
                .iter()
                .any(|event| {
                    event.event_type == "OrderSubmitted"
                        && matches!(event.payload.get("client_order_id"), Some(Value::Str(id)) if id == &placement)
                });
            assert!(
                !resubmitted,
                "{case}: a doubted placement is never resubmitted, whatever the legs it rests on \
                 (DEC-878 item 1, §5.7)"
            );
        }
        Ok(())
    }

    /// #1292's contract item 3, bounded to the reads that are folded: the doubt of a placement
    /// whose entry carries a fill the broker has reported but the journal has not applied asks
    /// for nothing — not by the entry, whose read-back would fold into §11's missing-fill ingest
    /// and ask for another gathered reconciliation, and never by the handle, which no broker
    /// order carries (DEC-878 item 1). The next reconciliation's gathered snapshot is what
    /// ingests the fill and ends the wait, so the doubt is bounded by a gather rather than by a
    /// 404-producing name, and it still neither confirms the placement absent nor resubmits it.
    #[test]
    fn a_doubt_behind_an_unapplied_fill_waits_for_the_gather() -> Result<(), ExecutorError> {
        let (config, fees) = (executor_config(), fees()?);
        let ports = Ports {
            ids: &Ids,
            mandates: &Everything,
            instruments: &Everything,
            config: &config,
            fees: &fees,
        };
        let mut executor = Executor::opened(&ports)?;
        executor.run(
            Input::BrokerUpdate(BrokerUpdate::Account(account("20000")?)),
            &ports,
        )?;
        executor.run(
            Input::BrokerSnapshot(BrokerSnapshot {
                account: account("20000")?,
                ..executor.snapshot(ReconcileReason::Startup)?
            }),
            &ports,
        )?;
        let submitted = executor.run(
            Input::Intent(IntentHandoff {
                intent_id: IntentId(EventId("01JABCDEFGHJKMNPQRSTVWXYZ0".to_owned())),
                agent: AgentId("agent-a".to_owned()),
                tif: Some(TimeInForce::Day),
                body: IntentBody::Order {
                    instrument: aapl()?,
                    side: Side::Buy,
                    qty: Qty::parse("10")?,
                    limit: Price::parse("150")?,
                    purpose: Purpose::Open,
                    protection: Some(ProtectionPrices {
                        stop: Price::parse("140")?,
                        take_profit: Some(Price::parse("170")?),
                    }),
                },
            }),
            &ports,
        )?;
        let entry = submitted
            .iter()
            .find_map(|effect| match effect {
                Effect::Broker(BrokerRequest::Submit(order)) if order.bracket.is_some() => {
                    Some(order.client_order_id.as_str().to_owned())
                }
                _ => None,
            })
            .ok_or_else(|| missing("the entry goes as one bracket (§5.4)"))?;
        executor.run(
            Input::BrokerUpdate(BrokerUpdate::Order(BrokerOrder {
                broker_order_id: "e0000000".to_owned(),
                client_order_id: Some(entry.clone()),
                instrument: aapl()?,
                side: Side::Buy,
                qty: Qty::parse("10")?,
                filled_qty: Qty::parse("10")?,
                limit_price: Some(Price::parse("150")?),
                stop_price: None,
                status: "filled".to_owned(),
                reject_code: None,
                replaced_by_broker_order_id: None,
                legs: Vec::new(),
                created_on: Some(Date::parse("2026-09-22")?),
            })),
            &ports,
        )?;
        let placement = executor
            .journal
            .iter()
            .filter(|event| event.event_type == "ProtectionChanged")
            .filter(|event| {
                matches!(
                    event.payload.get("action"),
                    Some(Value::Str(action)) if action == "placed"
                )
            })
            .flat_map(|event| {
                event
                    .payload
                    .get("orders")
                    .and_then(Value::as_array)
                    .map(|ids| {
                        ids.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
            .next()
            .ok_or_else(|| missing("the reported complete fill records the legs placed"))?;

        let adopted = executor.run(
            Input::BrokerSnapshot(BrokerSnapshot {
                open_orders: Vec::new(),
                positions: vec![BrokerPosition {
                    instrument: aapl()?,
                    qty: SignedQty::parse("10")?,
                    avg_entry_price: Price::parse("150")?,
                }],
                account: account("20000")?,
                ..executor.snapshot(ReconcileReason::Scheduled)?
            }),
            &ports,
        )?;
        assert!(
            adopted
                .iter()
                .all(|effect| !matches!(effect, Effect::Broker(_))),
            "a doubt whose entry's fill the journal has not applied asks the broker for no name \
             at all: the next gathered reconciliation ingests the fill and ends the wait \
             (DEC-878 item 1, §11): {adopted:?}"
        );
        assert_eq!(
            executor
                .state
                .orders()
                .get(&ClientOrderId::parse(&placement)?)
                .map(|order| order.state),
            Some(OrderState::Unknown),
            "the placement is still adopted in doubt (DEC-878 item 5), waiting for the gather"
        );
        Ok(())
    }
}
