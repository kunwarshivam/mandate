//! Reconciliation: one pure function over a snapshot, the folded state, and the account ledger
//! (E7-3, trading-domain spec §11).

use std::collections::BTreeSet;

use mandate_accounting::InstrumentId;
use mandate_canon::Value;
use mandate_num::{Fraction, Qty, SignedQty};

use crate::batch::Batch;
use crate::codec::{side_name, state_name};
use crate::error::ExecutorError;
use crate::ids::ClientOrderId;
use crate::orders::{EXTERNAL, account_fields, every_agent_alerted, fill, status_mapping};
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
    let recorded = cash(batch, snapshot, &mut differences)?;
    if snapshot.reason == ReconcileReason::FeePosting {
        fees(batch, snapshot, recorded, &mut differences)?;
    }
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
        differences.push(Difference {
            kind: DifferenceKind::ExternalActivity,
            subject: open.broker_order_id.clone(),
            adopted: false,
        });
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
            differences.push(Difference {
                kind: DifferenceKind::Position,
                subject: instrument.as_str().to_owned(),
                adopted: false,
            });
        }
    }
    Ok(())
}

/// Step 4: the broker's cash against the model's, within 0.01 × the fills since the last broker
/// cash snapshot plus the accrued unposted fees (§11). Paper's simulated fees are not in the
/// model's cash at all (§10, interpretation 25). Above the band, every agent is paused and the owner
/// alerted until the owner acknowledges `cash`. The snapshot is journaled as
/// `AccountSnapshotRecorded`, whole, and becomes the base the next comparison is measured from.
/// Before any account was reported there is no base to compare with, and nothing is recorded.
fn cash(
    batch: &mut Batch<'_, '_>,
    snapshot: &BrokerSnapshot,
    differences: &mut Vec<Difference>,
) -> Result<Option<EventId>, ExecutorError> {
    let Some(base) = batch.view.observed.clone() else {
        return Ok(None);
    };
    let model = base
        .cash
        .checked_add(batch.view.cash_flow)?
        .checked_sub(batch.view.unposted_fees)?;
    let band = batch
        .view
        .fill_notional
        .times_fraction(Fraction::parse(CASH_BAND)?)?
        .checked_add(batch.view.unposted_fees.abs())?;
    let drift = snapshot.account.cash.checked_sub(model)?.abs();
    let mut fields = account_fields(&snapshot.account)?;
    fields.push(("model_cash", text(model.to_string())));
    let recorded = batch.journal("AccountSnapshotRecorded", None, fields)?;
    if drift > band {
        every_agent_alerted(
            batch,
            Mode::Paused,
            &restriction_for("cash"),
            recorded.clone(),
            "reconciliation_cash",
        )?;
        differences.push(Difference {
            kind: DifferenceKind::Cash,
            subject: "cash".to_owned(),
            adopted: false,
        });
    }
    Ok(Some(recorded))
}

/// The cash tolerance per unit of fill notional (§11).
const CASH_BAND: &str = "0.01";

/// Step 5, at a fee posting: fees are exact once posted. A difference pauses every agent and alerts
/// the owner, naming the recorded snapshot (recorded here if step 4 had no base to record it
/// against), until the owner acknowledges `fees`; it is never silently adjusted (§11).
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
        Some(recorded) => recorded,
        None => batch.journal(
            "AccountSnapshotRecorded",
            None,
            account_fields(&snapshot.account)?,
        )?,
    };
    every_agent_alerted(
        batch,
        Mode::Paused,
        &restriction_for("fees"),
        recorded,
        "reconciliation_fees",
    )?;
    differences.push(Difference {
        kind: DifferenceKind::Fee,
        subject: "fees".to_owned(),
        adopted: false,
    });
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

#[cfg(test)]
mod tests {
    use mandate_accounting::{
        AssetClass, Config, CryptoFees, EquityFees, InstrumentId, Side, TafCapBasis,
    };
    use mandate_num::{Bps, FeeCap, FeePerShare, FeeRate, Fraction, Qty, ShareIncrement, Usd};
    use mandate_time::{Date, TradingCalendar};

    use super::reconcile;
    use crate::error::ExecutorError;
    use crate::ids::ClientOrderId;
    use crate::ports::{IdGen, InstrumentSnapshot, MandateView, Ports};
    use crate::state::{ExecutorState, ObservedAccount};
    use crate::types::{
        AccountRef, AccountScope, AccountState, ActivityCursor, AgentId, BrokerAccount,
        BrokerSnapshot, Effect, EventId, ExecutorConfig, ExitTier, MandateVersion, Order,
        OrderState, Purpose, ReconcileReason, Seq, WorkspaceId, WriterEpoch,
    };

    /// Ids derived from the epoch, the head and the ordinal, as a production id generator does.
    struct Ids;

    impl IdGen for Ids {
        fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId {
            EventId(format!("e-{}-{}-{ordinal}", epoch.0, head.0))
        }
    }

    /// A mandate covering everything, and whole-share equities: reconciliation reads neither.
    struct Everything;

    impl MandateView for Everything {
        fn version(&self, _agent: &AgentId) -> Option<MandateVersion> {
            Some(MandateVersion("v1".to_owned()))
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

    fn executor_config() -> ExecutorConfig {
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

    fn fees() -> Result<Config, ExecutorError> {
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

    /// A started executor on a paper stream: the epoch is what a batch needs.
    fn started() -> ExecutorState {
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        state.epoch = Some(WriterEpoch(1));
        state.started = true;
        state
    }

    fn drafted(effects: &[Effect]) -> Vec<&str> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) => Some(draft.event_type.as_str()),
                _ => None,
            })
            .collect()
    }

    fn order(id: &ClientOrderId, state: OrderState) -> Result<Order, ExecutorError> {
        Ok(Order {
            client_order_id: id.clone(),
            intent_id: None,
            agent: AgentId("agent-a".to_owned()),
            instrument: InstrumentId::new("AAPL")?,
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

    /// §11 step 1 compares only the orders the broker could still hold: a finished order, and one
    /// waiting in `Intent` with nothing at the broker yet, are not adopted when the broker lists
    /// neither; a live one is.
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
        let mut state = started();
        for (raw, at) in [
            ("md-01JABCDEFGHJKMNPQRSTVWXYZ0", OrderState::Canceled),
            ("md-01JABCDEFGHJKMNPQRSTVWXYZ1", OrderState::Intent),
        ] {
            let id = ClientOrderId::parse(raw)?;
            state.orders.insert(id.clone(), order(&id, at)?);
        }
        let run = reconcile(&state, &snapshot(ReconcileReason::Scheduled), &ports)?;
        assert_eq!(
            drafted(&run.effects),
            vec!["ReconciliationRun"],
            "neither is compared, so nothing is adopted"
        );

        let live = ClientOrderId::parse("md-01JABCDEFGHJKMNPQRSTVWXYZ2")?;
        state
            .orders
            .insert(live.clone(), order(&live, OrderState::Accepted)?);
        let run = reconcile(&state, &snapshot(ReconcileReason::Scheduled), &ports)?;
        assert!(
            drafted(&run.effects).contains(&"CompensatingEvent"),
            "while a live order the broker does not list is adopted: {:?}",
            drafted(&run.effects)
        );
        Ok(())
    }

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
        let mut state = started();
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
        let agreeing = reconcile(&state, &snapshot(ReconcileReason::FeePosting), &ports)?;
        assert_eq!(
            drafted(&agreeing.effects),
            vec!["AccountSnapshotRecorded", "ReconciliationRun"],
            "the broker agrees on cash and fees: the snapshot is recorded and the run published"
        );

        let mut drifted = snapshot(ReconcileReason::FeePosting);
        drifted.account.cash = Usd::parse("1")?;
        drifted.account.accrued_fees = Usd::parse("3")?;
        let run = reconcile(&state, &drifted, &ports)?;
        let restrictions: Vec<_> = run
            .effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) if draft.event_type == "AgentModeApplied" => draft
                    .payload
                    .get("restriction")
                    .and_then(mandate_canon::Value::as_str),
                _ => None,
            })
            .collect();
        assert_eq!(
            restrictions,
            vec!["reconciliation:cash", "reconciliation:fees"],
            "a cash drift beyond the band and a fee difference each pause every agent"
        );
        let alerts: Vec<_> = run
            .effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Notify(reference) => Some(reference.message_key),
                _ => None,
            })
            .collect();
        assert_eq!(alerts, vec!["reconciliation_cash", "reconciliation_fees"]);
        Ok(())
    }
}
