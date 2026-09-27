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
use crate::orders::{
    EXTERNAL, account_fields, account_restriction, every_agent_alerted, fill, status_mapping,
};
use crate::payload::{int, text};
use crate::ports::Ports;
use crate::state::{Adoption, EVERY_AGENT, ExecutorState, restriction_for};
use crate::types::{
    Adopted, BrokerRequest, BrokerSnapshot, Difference, EventId, Mode, OrderState, ReconcileReason,
    Reconciliation, ReconciliationVerdict, StatusMapping, Unexplained,
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
    differences.push(Difference::unexplained(Unexplained::Fee, "fees"));
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
    use mandate_canon::Value;
    use mandate_num::{
        Bps, FeeCap, FeePerShare, FeeRate, Fraction, Price, Qty, ShareIncrement, SignedQty, Usd,
    };
    use mandate_time::{Date, TradingCalendar};

    use super::reconcile;
    use crate::error::ExecutorError;
    use crate::ids::{ClientOrderId, IntentId};
    use crate::payload::object;
    use crate::ports::{IdGen, InstrumentSnapshot, MandateView, Ports};
    use crate::state::{ExecutorState, ObservedAccount, fold};
    use crate::step::handle;
    use crate::types::{
        AccountRef, AccountScope, AccountState, ActivityCursor, AgentId, BrokerAccount,
        BrokerPosition, BrokerRequest, BrokerSnapshot, BrokerUpdate, DifferenceKind, Effect,
        EventId, ExecutorConfig, ExitTier, FoldedEvent, Input, IntentBody, IntentHandoff,
        MandateVersion, Mode, Order, OrderState, Purpose, ReconcileReason, Seq, WorkspaceId,
        WriterEpoch,
    };

    /// Ids derived from the epoch, the head and the ordinal, as a production id generator does.
    struct Ids;

    impl IdGen for Ids {
        fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId {
            EventId(format!("e-{}-{}-{ordinal}", epoch.0, head.0))
        }
    }

    /// A mandate covering everything, and whole-share equities.
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

    fn aapl() -> Result<InstrumentId, ExecutorError> {
        Ok(InstrumentId::new("AAPL")?)
    }

    fn account(cash: &str) -> Result<BrokerAccount, ExecutorError> {
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
        })
    }

    fn snapshot(reason: ReconcileReason) -> Result<BrokerSnapshot, ExecutorError> {
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
    struct Executor {
        state: ExecutorState,
        journal: Vec<FoldedEvent>,
        epoch: u64,
    }

    impl Executor {
        fn opened(ports: &Ports<'_>) -> Result<Self, ExecutorError> {
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

        fn commit_one(&mut self, event_type: &str, payload: Value) -> Result<(), ExecutorError> {
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
            let effects = handle(&mut self.state, input, ports)?;
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

        fn run(&mut self, input: Input, ports: &Ports<'_>) -> Result<Vec<Effect>, ExecutorError> {
            self.run_keeping(input, ports, usize::MAX)
        }

        /// A crash and a restart: a new process folds the same journal and takes a new epoch.
        fn restarted(&self, ports: &Ports<'_>) -> Result<Self, ExecutorError> {
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

        fn snapshot(&self, reason: ReconcileReason) -> Result<BrokerSnapshot, ExecutorError> {
            Ok(BrokerSnapshot {
                taken_at_head: self.state.account_head(),
                ..snapshot(reason)?
            })
        }
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

    fn submitted(effects: &[Effect]) -> usize {
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
            agent: AgentId("agent-a".to_owned()),
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
    fn reporting(ports: &Ports<'_>) -> Result<Executor, ExecutorError> {
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
    /// held while no account has been journaled, however many runs complete, and released at the
    /// next tick once an account is journaled and a run has completed since the start, whichever
    /// comes first (#222 review, minor 3).
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
}
