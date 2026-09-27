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
use crate::orders::{EXTERNAL, every_agent_alerted, fill, status_mapping};
use crate::payload::{int, text};
use crate::ports::Ports;
use crate::state::{Adoption, EVERY_AGENT, ExecutorState, restriction_for};
use crate::types::{
    AgentId, BrokerRequest, BrokerSnapshot, Difference, DifferenceKind, EventId, Mode, OrderState,
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
    if cash_or_fees_to_compare(&batch.view, snapshot) {
        return incomplete(batch, differences);
    }
    let unexplained = differences.iter().any(|difference| !difference.adopted);
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
        let broker = snapshot
            .positions
            .iter()
            .find(|held| held.instrument == instrument)
            .map_or(SignedQty::ZERO, |held| held.qty);
        let mismatch = broker != model;
        let observed = batch.journal(
            "BrokerPositionObserved",
            None,
            vec![
                ("instrument", text(instrument.as_str())),
                ("broker_qty", text(broker.to_string())),
                ("model_qty", text(model.to_string())),
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

/// Whether a run has cash or fees to compare: once the broker has reported an account there is a
/// cash figure, and a fee posting always has fees (§11 steps 4 and 5). Before any account is
/// reported there is no base to compare the broker's cash with, as in the full comparison.
fn cash_or_fees_to_compare(state: &ExecutorState, snapshot: &BrokerSnapshot) -> bool {
    state.observed.is_some() || snapshot.reason == ReconcileReason::FeePosting
}

/// The subject and alert key of a run that could not complete.
const INCOMPLETE: &str = "incomplete";

/// Steps 4 and 5, cash and fees, are the cash slice's (DEC-140's slice-5 amendment). A run that
/// would compare either is not completed: no `ReconciliationRun` is published as covering a
/// comparison it skipped, so `reconciled_through` does not advance. Instead every agent goes
/// `exits_only` under `reconciliation:incomplete` and the owner is alerted by the restriction's
/// own event id, once, through `every_agent_alerted`: openings stop, and rule 13's exits stay open
/// (`AGENTS.md` rule 3). Until the cash slice lands, every account that has reported disables
/// openings after its first reconciliation; the cash slice's owner acknowledgment lifts it.
fn incomplete(
    batch: &mut Batch<'_, '_>,
    differences: Vec<Difference>,
) -> Result<(ReconciliationVerdict, Vec<Difference>), ExecutorError> {
    let restriction = restriction_for(INCOMPLETE);
    let every = (AgentId(EVERY_AGENT.to_owned()), restriction.clone());
    if !batch.view.restrictions.contains_key(&every) {
        let subject = batch.next_id();
        every_agent_alerted(
            batch,
            Mode::ExitsOnly,
            &restriction,
            subject,
            "reconciliation_incomplete",
        )?;
    }
    Ok((ReconciliationVerdict::Mismatch, differences))
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

    use super::{cash_or_fees_to_compare, reconcile};
    use crate::error::ExecutorError;
    use crate::ids::{ClientOrderId, IntentId};
    use crate::payload::object;
    use crate::ports::{IdGen, InstrumentSnapshot, MandateView, Ports};
    use crate::state::{ExecutorState, ObservedAccount, fold};
    use crate::step::handle;
    use crate::types::{
        AccountRef, AccountScope, AccountState, ActivityCursor, AgentId, BrokerAccount,
        BrokerPosition, BrokerRequest, BrokerSnapshot, BrokerUpdate, Effect, EventId,
        ExecutorConfig, ExitTier, FoldedEvent, Input, IntentBody, IntentHandoff, MandateVersion,
        Mode, Order, OrderState, Purpose, ReconcileReason, Seq, WorkspaceId, WriterEpoch,
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

        /// A scheduled snapshot in which the broker holds the ten `AAPL` the model holds.
        fn holding_ten(&self) -> Result<BrokerSnapshot, ExecutorError> {
            Ok(BrokerSnapshot {
                positions: vec![BrokerPosition {
                    instrument: aapl()?,
                    qty: SignedQty::parse("10")?,
                    avg_entry_price: Price::parse("150")?,
                }],
                ..self.snapshot(ReconcileReason::Scheduled)?
            })
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

    #[test]
    fn cash_or_fees_are_to_compare_once_an_account_is_reported_or_at_a_fee_posting()
    -> Result<(), ExecutorError> {
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        assert!(!cash_or_fees_to_compare(
            &state,
            &snapshot(ReconcileReason::Scheduled)?
        ));
        assert!(cash_or_fees_to_compare(
            &state,
            &snapshot(ReconcileReason::FeePosting)?
        ));
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
        assert!(cash_or_fees_to_compare(
            &state,
            &snapshot(ReconcileReason::Scheduled)?
        ));
        Ok(())
    }

    /// DEC-140's slice-5 amendment, rule 3: a run with cash to compare is not completed. It
    /// publishes no `ReconciliationRun`, puts every agent `exits_only` under
    /// `reconciliation:incomplete` and alerts the owner by the restriction's id, once. After it an
    /// opening is not submitted, an exit still is (rule 13), and a second agent is held too.
    #[test]
    fn an_incomplete_run_holds_openings_loudly_and_keeps_exits_open() -> Result<(), ExecutorError> {
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
        executor.run(
            Input::BrokerUpdate(BrokerUpdate::Account(account("100000")?)),
            &ports,
        )?;
        let refused = executor.run(Input::BrokerSnapshot(executor.holding_ten()?), &ports)?;
        assert_eq!(
            drafted(&refused),
            vec!["BrokerPositionObserved", "AgentModeApplied"],
            "the position agrees, and no `ReconciliationRun` is published as covering the cash it \
             did not compare"
        );
        let restricted = refused.iter().find_map(|effect| match effect {
            Effect::Journal(draft) if draft.event_type == "AgentModeApplied" => Some(draft),
            _ => None,
        });
        let field = |name: &str| {
            restricted
                .and_then(|draft| draft.payload.get(name))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        assert_eq!(
            (field("agent"), field("to"), field("restriction")),
            (
                Some("*".to_owned()),
                Some("exits_only".to_owned()),
                Some("reconciliation:incomplete".to_owned())
            )
        );
        let alerts: Vec<_> = refused
            .iter()
            .filter_map(|effect| match effect {
                Effect::Notify(reference) => {
                    Some((reference.subject_event.clone(), reference.message_key))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            alerts,
            vec![(
                restricted
                    .map(|draft| draft.event_id.clone())
                    .unwrap_or(EventId(String::new())),
                "reconciliation_incomplete"
            )],
            "the owner is alerted once, by the restriction's own event id"
        );
        assert_eq!(executor.state.reconciled_through(), None);
        assert_eq!(
            executor
                .state
                .effective_mode(&AgentId("agent-b".to_owned())),
            Mode::ExitsOnly,
            "every agent, including one that placed nothing"
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
        let exit = executor.run(
            intent(
                "01JABCDEFGHJKMNPQRSTVWXYZ1",
                "agent-a",
                Side::Sell,
                Purpose::RiskExit,
            )?,
            &ports,
        )?;
        assert_eq!(submitted(&exit), 1, "while an exit still is (rule 13)");

        let again = executor.run(Input::BrokerSnapshot(executor.holding_ten()?), &ports)?;
        assert!(
            !drafted(&again).contains(&"AgentModeApplied")
                && !drafted(&again).contains(&"ReconciliationRun"),
            "a later incomplete run restricts nothing new and is not published either: {:?}",
            drafted(&again)
        );
        assert!(
            !again
                .iter()
                .any(|effect| matches!(effect, Effect::Notify(_))),
            "and alerts no more"
        );
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
        let observed = run
            .iter()
            .find_map(|effect| match effect {
                Effect::Journal(draft) if draft.event_type == "BrokerPositionObserved" => {
                    Some(draft)
                }
                _ => None,
            })
            .ok_or(ExecutorError::Unimplemented { story: "E7-3" })?;
        let field = |name: &str| observed.payload.get(name).and_then(Value::as_str);
        assert_eq!(
            (field("broker_qty"), field("model_qty")),
            (Some("7"), Some("10")),
            "each quantity under its own key"
        );
        assert_eq!(observed.payload.get("mismatch"), Some(&Value::Bool(true)));
        Ok(())
    }
}
