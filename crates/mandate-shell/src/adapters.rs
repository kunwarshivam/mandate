//! The production adapters: one per [`Stage`](crate::Stage), each binding one crate's public API.
//!
//! The implementation lands one slice at a time (DEC-77 stage 3, DEC-166). **Live** in slice 1:
//! [`StoredBars`], [`MovingAverage`] and [`AlpacaConnector`], whose upstreams are fully implemented
//! and whose every input the shell holds; since DEC-449's flip also [`RiskExitPath`], which probes
//! and plans over the journal, stream, clock, and agent the run's bridge hands it; and
//! [`ExecutorProtection`], through the executor's public protection probe. The journal, sink,
//! executor, reconciliation, mandate validation, sizing, classification, and advisory-gate
//! boundaries are also live when their trusted inputs are injected. The E7-7 shipping binary
//! injects the one-run paper contexts [`crate::paper`] assembles from its GET-only broker preflight
//! and the stored datasets (DEC-466, DEC-470); callers that omit either context fail closed rather
//! than inventing effective-dated inputs.
//!
//! What each will bind, per the task brief's step table:
//!
//! | Adapter | Binds |
//! |---|---|
//! | [`RiskExitPath`] | `mandate_risk::agent_flatten` (E6-3) |
//! | [`ExecutorProtection`] | `mandate_executor::is_protected` (E7-4) |
//! | [`SpecMandate`] | `mandate_spec::validate`, then `ValidatedMandate::new` (stream F) |
//! | [`StoredBars`] | `mandate_marketdata::dataset::read_manifest` and `dataset::read` (E2-1, E2-2) |
//! | [`MovingAverage`] | `mandate_backtest::Strategy::MovingAverageCrossover` (E4-2) |
//! | [`BuilderPlan`] | `mandate-builder`'s sizing and classification (stream H) |
//! | [`RiskGate`] | `mandate_risk::evaluate` as a dry run (E6-3, E6-6 to E6-8) |
//! | [`StoreJournal`] | `mandate_journal::MemoryJournal` in CI, `mandate-journal-pg` for the manual run |
//! | [`ExecutorSink`] | the two `IntentHandoff` types, joined with the deployment's `AgentId` |
//! | [`CoreExecutor`] | `mandate_executor::handle` and `fold` (stream K) |
//! | [`AlpacaConnector`] | `mandate_alpaca::TradingClient` over a `TradingTransport` (stream K) |
//! | [`ExecutorReconciler`] | `mandate_executor::reconcile` on the connector's snapshot (E7-3) |

use std::cell::{Ref, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::mem;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_alpaca::{RetryPolicy, TokioPause, TradingClient, TradingTransport};
use mandate_backtest::{Signal, Strategy, StrategyConfig};
use mandate_builder::{
    AccountSnapshot as BuilderAccountSnapshot, Action as BuilderAction, ActionContext,
    BuilderMandate, Clip as BuilderClip, Direction, Market as BuilderMarket,
    ModelOutput as BuilderModelOutput, RiskContext as BuilderRiskContext,
};
use mandate_canon::{Digest, Key, Object, Value};
use mandate_domain::{
    AutonomyDecision, CapabilityProfile, ProfileError, Purpose as BuilderPurpose,
};
use mandate_executor::{
    AccountRef, AccountScope, AgentId as ExecutorAgentId, BindingGateSource, BrokerConnector,
    BrokerOutcome, BrokerRequest, ConnectorError, EventId, ExecutorConfig, ExecutorState,
    FoldedEvent, IdGen, InstrumentSnapshot, MandateVersion, MandateView as ExecutorMandateView,
    Order as ExecutorOrder, Ports, Seq, WorkspaceId, WriterEpoch, fold, is_protected,
    paper_only_fee_config,
};
use mandate_journal::{AppendOutcome, ArtifactSource, MemoryJournal, StoredEvent, StreamId};
use mandate_journal_pg::PgJournal;
use mandate_marketdata::dataset;
use mandate_marketdata::inspect::{self, ActionsReport, GapClass, Inspection};
use mandate_marketdata::model::{Bar, Kind, Records, TimeUnit, Timeframe};
use mandate_num::{Bps, Fraction, Price, Qty, ShareIncrement, SignedQty, Usd, UsdExact};
use mandate_risk::{
    AccountSnapshot as GateAccountSnapshot, AgentSnapshot as GateAgentSnapshot, ConductState,
    Decision, FlattenInitiator, FlattenInput, GateConfig, GateInput, GatePass,
    InstrumentSnapshot as GateInstrumentSnapshot, MarketSnapshot as GateMarketSnapshot, Origin,
    ProposedKind, ProposedOrder, RiskSnapshot, TimeInForce as GateTimeInForce,
    ValidatedMandate as GateMandate, WorkingUniverse, agent_flatten,
};
use mandate_runtime::{
    AgentId, ApprovalSettings, Autonomy, Classified, DecisionClip, ExitOrigin, FlattenLeg,
    FlattenPlan, FlattenRequest, Initiator, IntentBody, IntentHandoff, MandateView, ModelDirection,
    OrderExecution, Proposal, Purpose, RiskClock, SignalInputs, TimeInForce,
};
use mandate_spec::document::ParamValue;
use mandate_spec::policy::PolicyLevel;
use mandate_spec::{ValidatedMandate, ValidationContext};
use mandate_time::{Date, ExchangeCalendar, TradingCalendar, UtcNanos};

use crate::control::Governance;
use crate::envelope::account_stream;
use crate::error::Cause;
use crate::stages::{
    Admitted, Bars, Classifier, Connector, Executor, ExitPath, Gate, JournalWriter, MandateSource,
    ModelRef, Protection, Reconciler, SignalModel, Sink, Sizing, Stages,
};

/// `mandate_risk::agent_flatten`, probed once against a synthetic request before anything starts
/// and, mid-run, planned over a fold of the journal taken at request time (DEC-449): the
/// agent's sub-ledger — attributed from the account-stream records, each fill's client order
/// resolving through the fold's order records to the agent that submitted it — and its working
/// orders, with asset classes from the mandate's universe and the session derived from the
/// step's risk clock. The journal's read capability, the account stream's id, and the clock
/// arrive with each `plan` call from the run's bridge — the adapter holds no journal and no
/// clock of its own, so no plan can be taken over a fold the run has written past or a session
/// the clock has left — while the agent and the mandate's path are held from construction, as
/// `Sources` carries both: the agent is the deployment's, fixed for the run, and the mandate is
/// the probe's synthetic request and the universe's asset classes.
pub struct RiskExitPath {
    agent: AgentId,
    mandate: PathBuf,
}

/// The exit-offset tier the spec sets for an equity whose volume the plan call cannot see: the
/// wider of the two equity tiers, which is the reading that never holds a risk exit back, and
/// inert for a risk-initiated flatten, whose sells price by the session alone with no bid to
/// bound (trading-domain spec §5.6). The owner-bid path narrows the tier in a follow-up.
const EQUITY_EXIT_OFFSET: &str = "0.05";

fn mandate_document(path: &Path) -> Result<mandate_spec::document::Mandate, Cause> {
    let bytes = std::fs::read(path).map_err(|_| {
        Cause::Spec(mandate_spec::SpecError::InvalidInput {
            what: "a mandate document that cannot be read",
        })
    })?;
    let value = mandate_canon::parse(&bytes).map_err(|_| {
        Cause::Spec(mandate_spec::SpecError::InvalidInput {
            what: "a mandate document that is not canonical JSON",
        })
    })?;
    mandate_spec::document::Mandate::parse(&value).map_err(|_| {
        Cause::Spec(mandate_spec::SpecError::InvalidInput {
            what: "a mandate document that does not parse",
        })
    })
}

impl RiskExitPath {
    /// The adapter for one agent over one mandate document, the two `Sources` members the fold
    /// reads (DEC-449).
    pub fn new(agent: AgentId, mandate: PathBuf) -> RiskExitPath {
        RiskExitPath { agent, mandate }
    }

    /// The mandate document, parsed strictly: the probe's synthetic request and the universe's
    /// asset classes both come from it, so a document that cannot be read is a refusal, not a
    /// pass (DEC-449 item 5).
    fn document(&self) -> Result<mandate_spec::document::Mandate, Cause> {
        mandate_document(&self.mandate)
    }

    /// Each pinned instrument's asset class, keyed by the mandate's own `asset_id` — already
    /// the strict `8-4-4-4-12` uuid the document's parse enforces, never the vendor's symbol —
    /// which is the id the journal's records must carry. A plan over an instrument the map
    /// does not carry is a refusal, never a guess at its class.
    fn classes(
        document: &mandate_spec::document::Mandate,
    ) -> BTreeMap<String, mandate_risk::AssetClass> {
        document
            .universe
            .pinned_instruments
            .iter()
            .map(|pinned| (pinned.asset_id.as_str().to_owned(), pinned.asset_class))
            .collect()
    }
}

impl ExitPath for RiskExitPath {
    fn probe(&self) -> Result<(), Cause> {
        let document = self.document()?;
        let positions = document
            .universe
            .pinned_instruments
            .iter()
            .map(|pinned| {
                Ok(mandate_risk::AgentPosition {
                    agent: mandate_risk::AgentId(1),
                    instrument: mandate_risk::AssetId::new(pinned.asset_id.as_str()).map_err(
                        |_| {
                            Cause::Spec(mandate_spec::SpecError::InvalidInput {
                                what: "a universe instrument the risk crate cannot read",
                            })
                        },
                    )?,
                    asset_class: pinned.asset_class,
                    qty: Qty::parse("1")?,
                })
            })
            .collect::<Result<Vec<_>, Cause>>()?;
        let open_orders = BTreeMap::new();
        let input = FlattenInput {
            agent: mandate_risk::AgentId(1),
            open_orders: &open_orders,
            agent_positions: &positions,
            broker_positions: &BTreeMap::new(),
            session: mandate_risk::Session::Regular,
            initiator: FlattenInitiator::RiskLimit,
            owner_confirmed_bid: None,
            max_exit_offset: Fraction::parse(EQUITY_EXIT_OFFSET)?,
            owner_floor_price: None,
        };
        agent_flatten(&input)?;
        Ok(())
    }

    fn plan(
        &self,
        request: &FlattenRequest,
        journal: &dyn JournalWriter,
        stream: &str,
        clock: Option<RiskClock>,
    ) -> Result<FlattenPlan, Cause> {
        let Some(clock) = clock else {
            return Err(Cause::Absent {
                what: "a risk clock has not advanced",
            });
        };
        let document = self.document()?;
        let classes = Self::classes(&document);
        let stored = journal.read(stream)?;
        let state = fold_of(&scope_of(stream)?, &stored)?;
        let mine: Vec<&ExecutorOrder> = state
            .orders()
            .values()
            .filter(|order| {
                order
                    .agent
                    .as_ref()
                    .is_some_and(|owner| owner.0.as_str() == self.agent.0.as_str())
            })
            .collect();
        let session = session_of(clock, &mine, &classes)?;
        let positions = sub_ledger_of(&mine, &classes, request.instrument.as_ref())?;
        let (open_orders, order_ids) =
            working_orders_of(&mine, clock, request.instrument.as_ref())?;
        let closed: BTreeSet<&str> = positions
            .iter()
            .map(|position| position.instrument.as_str())
            .collect();
        let input = FlattenInput {
            agent: mandate_risk::AgentId(1),
            open_orders: &open_orders,
            agent_positions: &positions,
            broker_positions: &BTreeMap::new(),
            session,
            initiator: match request.initiator {
                Initiator::Owner => FlattenInitiator::Owner,
                Initiator::RiskLimit | Initiator::PlatformOperator => FlattenInitiator::RiskLimit,
            },
            owner_confirmed_bid: request.confirmation.as_ref().map(|confirmed| confirmed.bid),
            max_exit_offset: Fraction::parse(EQUITY_EXIT_OFFSET)?,
            owner_floor_price: request
                .confirmation
                .as_ref()
                .map(|confirmed| confirmed.floor),
        };
        let planned = agent_flatten(&input)?;
        let leg = |instrument: &mandate_risk::AssetId,
                   qty: Qty,
                   deferred: bool|
         -> Result<FlattenLeg, Cause> {
            Ok(FlattenLeg {
                instrument: InstrumentId::new(instrument.as_str()).map_err(|_| {
                    Cause::Spec(mandate_spec::SpecError::InvalidInput {
                        what: "an instrument the accounting crate cannot read",
                    })
                })?,
                asset_class: classes
                    .get(instrument.as_str())
                    .copied()
                    .ok_or(Cause::Absent {
                        what: "an instrument the mandate's universe does not carry",
                    })?,
                qty,
                deferred_to_regular_session: deferred,
            })
        };
        let mut sells = Vec::new();
        for sell in &planned.sells {
            sells.push(leg(&sell.instrument, sell.qty, false)?);
        }
        for sell in &planned.deferred_sells {
            sells.push(leg(&sell.instrument, sell.qty, true)?);
        }
        let purpose = match planned.purpose {
            mandate_risk::Purpose::OwnerExit => Purpose::OwnerExit,
            mandate_risk::Purpose::RiskExit => Purpose::RiskExit,
            mandate_risk::Purpose::Open
            | mandate_risk::Purpose::Increase
            | mandate_risk::Purpose::DiscretionaryExit
            | mandate_risk::Purpose::Protective => {
                return Err(Cause::Spec(mandate_spec::SpecError::InvalidInput {
                    what: "a flatten plan with a purpose no flatten can carry",
                }));
            }
        };
        let mut cancel_client_order_ids = planned
            .cancel_client_order_ids
            .iter()
            .map(|id| {
                order_ids.get(id).cloned().ok_or(Cause::Absent {
                    what: "a working order the plan named but the fold does not hold",
                })
            })
            .collect::<Result<Vec<String>, Cause>>()?;
        cancel_client_order_ids.extend(unattributed_legs_of(&state, &closed));
        cancel_client_order_ids.sort();
        Ok(FlattenPlan {
            cancel_client_order_ids,
            sells,
            purpose,
            confirmation: request.confirmation.clone(),
        })
    }
}

/// §5.5's Close column (§5.4's leg rule, DEC-160 item 3d; §2.3's ownerless watchdog exit): an
/// agent-scoped flatten cancels, in every instrument it closes, the resting legs no holder
/// explains — the broker-created protective legs that could not be attributed and the
/// ownerless watchdog exit — each by its own `client_order_id`, exactly as the executor's own
/// sequence path does. They belong to no agent, so the risk crate's own filter cannot carry
/// them: the adapter unions them into the cancels itself. A resting leg in an instrument the
/// plan does not close is another holder's protection, untouched.
fn unattributed_legs_of(
    state: &mandate_executor::ExecutorState,
    closed: &BTreeSet<&str>,
) -> Vec<String> {
    state
        .orders()
        .values()
        .filter(|order| is_unattributed_exit(order))
        .filter(|order| !order.state.is_terminal() && order.filled_qty < order.qty)
        .filter(|order| closed.contains(order.instrument.as_str()))
        .map(|order| order.client_order_id.as_str().to_owned())
        .collect()
}

fn is_unattributed_exit(order: &ExecutorOrder) -> bool {
    let ownerless = match &order.agent {
        None => true,
        Some(agent) => agent.0.as_str() == "*",
    };
    ownerless
        && matches!(
            order.purpose,
            mandate_executor::Purpose::Protective | mandate_executor::Purpose::RiskExit
        )
}

/// The account scope a stream id names, in the `acct:{workspace}:{account}` form every journal
/// account stream takes.
fn scope_of(stream: &str) -> Result<mandate_executor::AccountScope, Cause> {
    let rest = stream.strip_prefix("acct:").ok_or(Cause::Absent {
        what: "an account stream id",
    })?;
    let (workspace, account) = rest.split_once(':').ok_or(Cause::Absent {
        what: "an account stream id",
    })?;
    Ok(mandate_executor::AccountScope {
        account: mandate_executor::AccountRef(account.to_owned()),
        workspace: mandate_executor::WorkspaceId(workspace.to_owned()),
    })
}

/// The state the stream's records fold to, through the executor's public fold over the records
/// as the journal normalized them: each stored body's journal-assigned `seq`, `prev_hash`, and
/// `recorded_at` must equal the row's columns, exactly those three are removed, and the
/// remaining draft is validated — the fold sees the draft's normalized payload, never the
/// stored bytes' form (DEC-449 item 1, amended by DEC-451 item 1).
fn fold_of(
    scope: &mandate_executor::AccountScope,
    stored: &[StoredEvent],
) -> Result<mandate_executor::ExecutorState, Cause> {
    let mut state = mandate_executor::ExecutorState::new(scope.clone());
    for row in stored {
        let sealed = mandate_canon::parse(&row.body).map_err(|_| {
            Cause::Spec(mandate_spec::SpecError::InvalidInput {
                what: "a stored journal body that is not canonical JSON",
            })
        })?;
        let mandate_canon::Value::Object(mut body) = sealed else {
            return Err(Cause::Spec(mandate_spec::SpecError::InvalidInput {
                what: "a stored journal body that is not an object",
            }));
        };
        let assigned_match = body.remove("seq").and_then(|value| value.as_int()) == Some(row.seq)
            && body
                .remove("prev_hash")
                .and_then(|value| value.as_str().map(str::to_owned))
                .is_some_and(|value| value == row.prev_hash.to_hex())
            && body
                .remove("recorded_at")
                .and_then(|value| value.as_str().map(str::to_owned))
                .is_some_and(|value| value == row.recorded_at);
        if !assigned_match {
            return Err(Cause::Spec(mandate_spec::SpecError::InvalidInput {
                what: "a stored journal body whose assigned fields do not match its row",
            }));
        }
        let draft_bytes = mandate_canon::to_canonical(&mandate_canon::Value::Object(body));
        let draft = mandate_journal::Draft::parse(&draft_bytes).map_err(|_| {
            Cause::Spec(mandate_spec::SpecError::InvalidInput {
                what: "a journal record the draft validation refuses",
            })
        })?;
        let normalized = mandate_canon::parse(draft.canonical_bytes()).map_err(|_| {
            Cause::Spec(mandate_spec::SpecError::InvalidInput {
                what: "a journal record that is not canonical JSON",
            })
        })?;
        let event = mandate_executor::FoldedEvent {
            stream: row.stream_id.clone(),
            seq: mandate_executor::Seq(row.seq),
            event_id: mandate_executor::EventId(row.event_id.clone()),
            event_type: row.event_type.clone(),
            causation_id: normalized
                .get("causation_id")
                .and_then(mandate_canon::Value::as_str)
                .map(|id| mandate_executor::EventId(id.to_owned())),
            payload: normalized.get("payload").cloned().ok_or(Cause::Spec(
                mandate_spec::SpecError::InvalidInput {
                    what: "a journal record without a payload",
                },
            ))?,
        };
        mandate_executor::fold(&mut state, &event)?;
    }
    Ok(state)
}

/// The session the clock derives, over the committed US-equities calendar — never a label a
/// caller supplies (trading-domain spec §4.3): an equity instrument makes the calendar's
/// answer load-bearing, and an all-crypto set of orders takes the continuous session, which
/// never defers a sell. A closed market inside the calendar's range is the overnight session,
/// so an equity sell waits for the open rather than the run halting — risk reduction is never
/// denied (AGENTS.md rule 13), and the risk crate's own derivation reads the same instant the
/// same way. Only an instant the calendar cannot cover at all is a refusal: no session, no
/// plan, and the bridge poisons rather than guess.
fn session_of(
    clock: RiskClock,
    mine: &[&ExecutorOrder],
    classes: &BTreeMap<String, mandate_risk::AssetClass>,
) -> Result<mandate_risk::Session, Cause> {
    let any_equity = mine.iter().any(|order| {
        classes.get(order.instrument.as_str()) != Some(&mandate_risk::AssetClass::Crypto)
    });
    if !any_equity {
        return Ok(mandate_risk::Session::Continuous);
    }
    let calendar = mandate_time::ExchangeCalendar::us_equities().map_err(|_| Cause::Absent {
        what: "the committed US equities calendar",
    })?;
    let now = UtcNanos::from_parts(clock.secs(), 0).map_err(|_| Cause::Absent {
        what: "a clock the calendar reads",
    })?;
    let (today, _) = mandate_time::new_york_date_and_hour(now).map_err(|_| Cause::Absent {
        what: "a New York date",
    })?;
    let tomorrow = today.next().map_err(|_| Cause::Absent {
        what: "a next day the calendar reads",
    })?;
    for date in [today, tomorrow] {
        let sessions = calendar.sessions(date).map_err(|_| Cause::Absent {
            what: "the calendar's sessions",
        })?;
        if let Some(found) = sessions.into_iter().find(|span| span.contains(now)) {
            return Ok(match found.session() {
                mandate_time::Session::Overnight => mandate_risk::Session::Overnight,
                mandate_time::Session::PreMarket => mandate_risk::Session::PreMarket,
                mandate_time::Session::Regular => mandate_risk::Session::Regular,
                mandate_time::Session::AfterHours => mandate_risk::Session::AfterHours,
            });
        }
    }
    Ok(mandate_risk::Session::Overnight)
}

/// Whether the order is in the flatten's scope: an owner exit names the one instrument it
/// closes, so only its orders are in; a kill switch closes the whole agent, so every order is.
fn in_scope(order: &ExecutorOrder, scope: Option<&InstrumentId>) -> bool {
    match scope {
        Some(instrument) => order.instrument == *instrument,
        None => true,
    }
}

/// The New York date the clock reads, for the working orders' submission dates.
fn new_york_today(clock: RiskClock) -> Result<mandate_time::Date, Cause> {
    let now = UtcNanos::from_parts(clock.secs(), 0).map_err(|_| Cause::Absent {
        what: "a clock the calendar reads",
    })?;
    let (today, _) = mandate_time::new_york_date_and_hour(now).map_err(|_| Cause::Absent {
        what: "a New York date",
    })?;
    Ok(today)
}

/// The deciding agent's sub-ledger, attributed from the folded orders (DEC-449 item 1): each of
/// the agent's orders contributes its signed filled quantity per instrument, and the account's
/// other agents' fills count toward nothing. An owner exit names the one instrument it closes
/// (mandate spec §6.1, trading-domain spec §5.5, DEC-257): the scope filters to it, so an exit
/// of one instrument never sells another's sub-ledger. A net short is refused — v1 never holds
/// one — and so is an instrument the mandate's universe does not carry, whose class the plan
/// cannot know.
fn sub_ledger_of(
    mine: &[&ExecutorOrder],
    classes: &BTreeMap<String, mandate_risk::AssetClass>,
    scope: Option<&InstrumentId>,
) -> Result<Vec<mandate_risk::AgentPosition>, Cause> {
    let mut net: BTreeMap<&InstrumentId, SignedQty> = BTreeMap::new();
    for order in mine {
        if !in_scope(order, scope) {
            continue;
        }
        let filled = SignedQty::from(order.filled_qty);
        let signed = if order.side == Side::Buy {
            filled
        } else {
            filled.negated()
        };
        let entry = net.entry(&order.instrument).or_insert(SignedQty::ZERO);
        *entry = entry.checked_add(signed)?;
    }
    let mut positions = Vec::new();
    for (instrument, qty) in net {
        if qty == SignedQty::ZERO {
            continue;
        }
        if qty.is_negative() {
            return Err(Cause::Spec(mandate_spec::SpecError::InvalidInput {
                what: "a short sub-ledger, which v1 never holds",
            }));
        }
        positions.push(mandate_risk::AgentPosition {
            agent: mandate_risk::AgentId(1),
            instrument: mandate_risk::AssetId::new(instrument.as_str()).map_err(|_| {
                Cause::Spec(mandate_spec::SpecError::InvalidInput {
                    what: "an instrument the risk crate cannot read",
                })
            })?,
            asset_class: classes
                .get(instrument.as_str())
                .copied()
                .ok_or(Cause::Absent {
                    what: "an instrument the mandate's universe does not carry",
                })?,
            qty: qty.abs(),
        });
    }
    Ok(positions)
}

/// The risk crate's working orders keyed by the adapter's internal numeric ids, beside the
/// reverse map the plan's cancels resolve through — the numeric space is the adapter's own and
/// invisible to the runtime, which keys orders by strings end to end (DEC-449 item 3).
type WorkingOrders = (
    BTreeMap<mandate_risk::ClientOrderId, mandate_risk::WorkingOrder>,
    BTreeMap<mandate_risk::ClientOrderId, String>,
);

/// The agent's working orders in the risk crate's shape: an order is working while its state is
/// not terminal and part of its quantity rests unfilled — the executor's own rule that a filled
/// quantity at or past the order's is done, whatever state the last journal record left it in.
/// An owner exit names the one instrument it closes, so the scope filters the cancels to its
/// orders in it alone. `max_cost` and `submitted_on` are unread by the flatten; the fold's
/// order carries no price, so the cost is zero and the date is the plan's own, derived only
/// when a working order exists to carry it.
fn working_orders_of(
    mine: &[&ExecutorOrder],
    clock: RiskClock,
    scope: Option<&InstrumentId>,
) -> Result<WorkingOrders, Cause> {
    let working: Vec<&ExecutorOrder> = mine
        .iter()
        .copied()
        .filter(|order| !order.state.is_terminal() && order.filled_qty < order.qty)
        .filter(|order| in_scope(order, scope))
        .collect();
    if working.is_empty() {
        return Ok((BTreeMap::new(), BTreeMap::new()));
    }
    let today = new_york_today(clock)?;
    let mut open_orders = BTreeMap::new();
    let mut order_ids = BTreeMap::new();
    for (index, order) in working.iter().enumerate() {
        let numeric =
            mandate_risk::ClientOrderId(u64::try_from(index).map_err(|_| Cause::Absent {
                what: "a working-order id",
            })?);
        order_ids.insert(numeric, order.client_order_id.as_str().to_owned());
        open_orders.insert(numeric, working_order_of(order, today)?);
    }
    Ok((open_orders, order_ids))
}

/// One working order in the risk crate's shape, mapped from the fold's own order (DEC-449
/// item 3): the executor's protective purpose is the risk crate's protective flag, an opening
/// purpose is the opening flag, and the open quantity is what still rests unfilled. `max_cost`
/// and `submitted_on` are unread by the flatten; the fold's order carries no price, so the
/// cost is zero and the date is the plan's own.
fn working_order_of(
    order: &ExecutorOrder,
    today: mandate_time::Date,
) -> Result<mandate_risk::WorkingOrder, Cause> {
    Ok(mandate_risk::WorkingOrder {
        agent: mandate_risk::AgentId(1),
        instrument: mandate_risk::AssetId::new(order.instrument.as_str()).map_err(|_| {
            Cause::Spec(mandate_spec::SpecError::InvalidInput {
                what: "an instrument the risk crate cannot read",
            })
        })?,
        side: order.side,
        max_cost: Usd::parse("0")?,
        open_qty: order.qty.checked_sub(order.filled_qty)?,
        protective: order.purpose == mandate_executor::Purpose::Protective,
        opening: order.purpose.adds_risk(),
        submitted_on: today,
    })
}

/// The executor's public protection probe over an uncovered synthetic one-share position in the
/// mandate's first pinned US equity. The position forces the executor to read the instrument
/// snapshot and answer `false`; that answer proves the protection question is available before the
/// tracer arms anything, rather than taking `is_protected`'s flat-position shortcut.
pub struct ExecutorProtection {
    mandate: PathBuf,
    config: Option<ExecutorConfig>,
}

impl ExecutorProtection {
    /// A protection probe over the deployment's mandate document and effective executor config.
    pub fn new(mandate: PathBuf, config: Option<ExecutorConfig>) -> ExecutorProtection {
        ExecutorProtection { mandate, config }
    }
}

struct ProtectionProbePorts {
    instrument: InstrumentId,
}

impl IdGen for ProtectionProbePorts {
    fn event_id(&self, _epoch: WriterEpoch, _head: Seq, _ordinal: u32) -> EventId {
        EventId("protection-probe".to_owned())
    }
}

impl ExecutorMandateView for ProtectionProbePorts {
    fn version(&self, _agent: &ExecutorAgentId) -> Option<MandateVersion> {
        None
    }

    fn crypto_stop_limit_offset(&self, _agent: &ExecutorAgentId) -> Option<Fraction> {
        None
    }

    fn covers(&self, _agent: &ExecutorAgentId, instrument: &InstrumentId) -> bool {
        instrument == &self.instrument
    }
}

impl InstrumentSnapshot for ProtectionProbePorts {
    fn asset_class(&self, instrument: &InstrumentId) -> Option<AssetClass> {
        (instrument == &self.instrument).then_some(AssetClass::UsEquity)
    }

    fn increment(&self, instrument: &InstrumentId) -> Option<ShareIncrement> {
        (instrument == &self.instrument).then_some(ShareIncrement::Whole)
    }

    fn exit_tier(&self, _instrument: &InstrumentId) -> Option<mandate_executor::ExitTier> {
        None
    }
}

fn protection_probe_date(raw: &str) -> Result<Date, Cause> {
    Date::parse(raw).map_err(|_| Cause::Absent {
        what: "the protection probe's fee calendar",
    })
}

fn protection_probe_payload(fields: &[(&str, &str)]) -> Result<Value, Cause> {
    let mut payload = Object::new();
    for (name, value) in fields {
        let key = Key::new(name).map_err(|_| Cause::Absent {
            what: "a protection probe payload key",
        })?;
        payload.insert(key, Value::Str((*value).to_owned()));
    }
    Ok(Value::Object(payload))
}

fn fold_protection_probe_position(
    state: &mut ExecutorState,
    instrument: &InstrumentId,
) -> Result<(), Cause> {
    let stream = "acct:protection-probe:protection-probe";
    let clock = "1970-01-01T00:00:10.000000000Z";
    for (seq, event_type, fields) in [
        (1, "StreamOpened", vec![("environment", "paper")]),
        (
            2,
            "OrderSubmitted",
            vec![
                ("client_order_id", "md-protection-probe"),
                ("agent", "protection-probe"),
                ("instrument", instrument.as_str()),
                ("side", "buy"),
                ("qty", "1"),
                ("limit", "1"),
                ("risk_clock", clock),
            ],
        ),
        (
            3,
            "FillApplied",
            vec![
                ("fill_id", "fill-protection-probe"),
                ("client_order_id", "md-protection-probe"),
                ("instrument", instrument.as_str()),
                ("side", "buy"),
                ("qty_gross", "1"),
                ("price", "1"),
                ("risk_clock", clock),
            ],
        ),
    ] {
        fold(
            state,
            &FoldedEvent {
                stream: stream.to_owned(),
                seq: Seq(seq),
                event_id: EventId(format!("protection-probe-{seq}")),
                event_type: event_type.to_owned(),
                causation_id: None,
                payload: protection_probe_payload(&fields)?,
            },
        )?;
    }
    Ok(())
}

impl Protection for ExecutorProtection {
    fn probe(&self) -> Result<(), Cause> {
        let document = mandate_document(&self.mandate)?;
        let pinned = document
            .universe
            .pinned_instruments
            .first()
            .ok_or(Cause::Absent {
                what: "a pinned instrument to probe for protection",
            })?;
        if pinned.asset_class != AssetClass::UsEquity {
            return Err(Cause::Absent {
                what: "the tracer's pinned US equity",
            });
        }
        let first = protection_probe_date("2026-01-01")?;
        let last = protection_probe_date("2026-12-31")?;
        let calendar = TradingCalendar::new(first, last, [], []).map_err(|_| Cause::Absent {
            what: "the protection probe's fee calendar",
        })?;
        let fees = paper_only_fee_config("paper", calendar, "2026-01-01")?;
        let config = self.config.ok_or(Cause::Absent {
            what: "the effective executor configuration",
        })?;
        let instrument =
            InstrumentId::new(pinned.asset_id.as_str()).map_err(|_| Cause::Absent {
                what: "the protection probe's instrument",
            })?;
        let probe_ports = ProtectionProbePorts {
            instrument: instrument.clone(),
        };
        let ports = Ports {
            ids: &probe_ports,
            mandates: &probe_ports,
            instruments: &probe_ports,
            config: &config,
            fees: &fees,
        };
        if ports.instruments.asset_class(&instrument) != Some(AssetClass::UsEquity) {
            return Err(Cause::Absent {
                what: "the protection probe's instrument asset class",
            });
        }
        if ports.instruments.increment(&instrument) != Some(ShareIncrement::Whole) {
            return Err(Cause::Absent {
                what: "the protection probe's instrument increment",
            });
        }
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("protection-probe".to_owned()),
            workspace: WorkspaceId("protection-probe".to_owned()),
        });
        fold_protection_probe_position(&mut state, &instrument)?;
        if is_protected(&state, &instrument, &ports)? {
            Err(Cause::Absent {
                what: "the uncovered protection probe position",
            })
        } else {
            Ok(())
        }
    }
}

/// Trusted facts for one decision that neither the mandate document nor runtime proposal contains.
#[derive(Debug, Clone)]
pub struct BuilderContext {
    pub account: BuilderAccountSnapshot,
    pub market: BuilderMarket,
    pub risk: BuilderRiskContext,
    pub action: ActionContext,
    /// Effective model content hashes from the trusted model registry, keyed by `(id, version)`.
    /// Runtime outputs do not carry this §8.1 identity, so absence refuses instead of copying the
    /// mandate's expected hash into an observation that never stated it.
    pub model_content_hashes: BTreeMap<(String, String), Digest>,
    pub execution: OrderExecution,
}

/// Proposed-order fields the runtime proposal does not carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryOrderFacts {
    pub kind: ProposedKind,
    pub tif: GateTimeInForce,
    pub extended_hours: bool,
    pub origin: Origin,
    pub owner_confirmed_bid: Option<Price>,
    pub client_order_id: mandate_risk::ClientOrderId,
    pub fee_reservation: Usd,
}

/// Every advisory-gate fact other than the four order fields copied from the runtime proposal.
#[derive(Debug, Clone)]
pub struct AdvisoryGateContext {
    pub now: UtcNanos,
    pub pass: GatePass,
    pub config: GateConfig,
    pub mandate: GateMandate,
    pub risk: RiskSnapshot,
    pub account: GateAccountSnapshot,
    pub agent: GateAgentSnapshot,
    pub instrument: GateInstrumentSnapshot,
    pub market: GateMarketSnapshot,
    pub conduct: ConductState,
    pub universe: WorkingUniverse,
    pub order: AdvisoryOrderFacts,
}

/// The typed facts used after admission for one deterministic decision.
#[derive(Debug, Clone)]
pub struct DecisionContext {
    pub builder: Option<BuilderContext>,
    pub gate: Option<AdvisoryGateContext>,
}

/// Inputs outside the mandate document for validation, admission, and one decision run.
#[derive(Debug, Clone)]
pub struct RunContext {
    pub validation: ValidationContext,
    pub policies: Vec<PolicyLevel>,
    pub author: String,
    pub restricted_instruments: BTreeSet<InstrumentId>,
    pub decision: Option<DecisionContext>,
    /// The effective policy set and model registry (DEC-484, D4b). `Some` makes the run write
    /// `ModelOutputRecorded` and `DecisionMade` at schema version 2 with their `policy_set` and
    /// `model_registry` refs, appended through the store's artifact-aware append (journal spec
    /// v0.16). `None` keeps the version-1 records of a run nothing governs yet.
    pub governance: Option<Governance>,
}

fn required_context(context: &Option<Rc<RunContext>>) -> Result<&RunContext, Cause> {
    context.as_deref().ok_or(Cause::Absent {
        what: "the trusted run context",
    })
}

fn builder_context(context: &RunContext) -> Result<&BuilderContext, Cause> {
    context
        .decision
        .as_ref()
        .and_then(|decision| decision.builder.as_ref())
        .ok_or(Cause::Absent {
            what: "the trusted builder context",
        })
}

fn gate_context(context: &RunContext) -> Result<&AdvisoryGateContext, Cause> {
    context
        .decision
        .as_ref()
        .and_then(|decision| decision.gate.as_ref())
        .ok_or(Cause::Absent {
            what: "the trusted advisory gate context",
        })
}

fn validated_mandate(path: &Path, context: &RunContext) -> Result<ValidatedMandate, Cause> {
    let document = mandate_document(path)?;
    ValidatedMandate::new(document, &context.validation, &context.policies).map_err(|_| {
        Cause::Absent {
            what: "a mandate with no validation or policy violations",
        }
    })
}

fn runtime_view(validated: &ValidatedMandate, context: &RunContext) -> Result<MandateView, Cause> {
    let document = validated.mandate();
    let working_universe = document
        .universe
        .pinned_instruments
        .iter()
        .map(|instrument| {
            InstrumentId::new(instrument.asset_id.as_str()).map_err(|_| Cause::Absent {
                what: "a runtime instrument id",
            })
        })
        .collect::<Result<BTreeSet<_>, Cause>>()?;
    let environment = match document.environment {
        mandate_domain::Environment::Paper => mandate_journal::Environment::Paper,
        mandate_domain::Environment::Live => mandate_journal::Environment::Live,
    };
    let approvers = document
        .autonomy
        .approval
        .approvers
        .iter()
        .map(|approver| approver.as_str().to_owned())
        .collect();
    Ok(MandateView {
        version: format!(
            "sha256:{}",
            document
                .version()
                .map_err(|_| Cause::Absent {
                    what: "the validated mandate's canonical version",
                })?
                .digest()
        ),
        working_universe,
        restricted_instruments: context.restricted_instruments.clone(),
        approval: ApprovalSettings {
            approvers,
            author: context.author.clone(),
            timeout_s: i64::from(document.autonomy.approval.timeout_s),
            environment,
        },
    })
}

/// The mandate document at `path`, validated with no violation against the injected run context.
pub struct SpecMandate {
    pub path: PathBuf,
    pub context: Option<Rc<RunContext>>,
}

impl MandateSource for SpecMandate {
    fn admitted(&self) -> Result<Admitted, Cause> {
        let context = required_context(&self.context)?;
        if context.governance.is_some() {
            return Err(Cause::Unimplemented { story: "E7-19" });
        }
        let validated = validated_mandate(&self.path, context)?;
        let document = validated.mandate();
        let mut instruments = document.universe.pinned_instruments.iter();
        let instrument = match (instruments.next(), instruments.next()) {
            (Some(instrument), None) => instrument,
            (None, _) | (Some(_), Some(_)) => {
                return Err(Cause::Absent {
                    what: "the tracer's one pinned instrument",
                });
            }
        };
        let mut models = document.behavior.signal_models.iter();
        let model = match (models.next(), models.next()) {
            (Some(model), None) => model,
            (None, _) | (Some(_), Some(_)) => {
                return Err(Cause::Absent {
                    what: "the tracer's one signal model",
                });
            }
        };
        let params = model
            .params
            .iter()
            .map(|param| {
                let value = match &param.value {
                    ParamValue::Decimal(value) => value.as_str().to_owned(),
                    ParamValue::Bool(value) => value.to_string(),
                    ParamValue::Text(value) => value.clone(),
                };
                (param.key.clone(), value)
            })
            .collect();
        let view = runtime_view(&validated, context)?;
        Ok(Admitted {
            environment: view.approval.environment,
            view,
            model: ModelRef {
                id: model.id.as_str().to_owned(),
                version: model.version.clone(),
                content_hash: model.content_hash,
                max_output_age_s: i64::from(model.max_output_age_s),
                params,
            },
            symbol: instrument.symbol.clone(),
            governed: None,
        })
    }
}

/// The stored daily bars in the dataset directory `dir`, read through `mandate-marketdata`'s own
/// `inspect` and `dataset::read`, one close per listed day in the manifest's date order. A day
/// holding anything but exactly one bar is refused, so the windows always span the days the
/// envelope names. The closes are returned exactly as stored: the shell adjusts, filters and
/// repairs nothing, so coverage it cannot trust is a refusal, never a guess (PB-15).
pub struct StoredBars {
    pub dir: PathBuf,
}

impl Bars for StoredBars {
    fn closes(&self, symbol: &str, now: UtcNanos) -> Result<Vec<Price>, Cause> {
        let inspection = inspect::inspect(&self.dir)?;
        trusted(&inspection, symbol, now)?;
        self.listed_closes(inspection.dataset.kind())
    }
}

impl StoredBars {
    /// One close per day the manifest lists with a file, in the manifest's date order, each read
    /// from that day's own partition. The loop walks the manifest, never the directory, so a
    /// partition the manifest does not list is not opened even if `trusted` were to let it pass
    /// (#248 review, minor 1).
    fn listed_closes(&self, kind: Kind) -> Result<Vec<Price>, Cause> {
        listed_bars(&self.dir, kind)?
            .iter()
            .map(|bar| Ok(Price::parse(bar.close.as_str())?))
            .collect()
    }
}

/// The daily bars [`StoredBars`] decides on, exactly as stored and only once `trusted` admits the
/// dataset at `now`, so the paper assembly's liquidity facts read the same span as the signal.
pub(crate) fn trusted_daily_bars(
    dir: &Path,
    symbol: &str,
    now: UtcNanos,
) -> Result<Vec<Bar>, Cause> {
    let inspection = inspect::inspect(dir)?;
    trusted(&inspection, symbol, now)?;
    listed_bars(dir, inspection.dataset.kind())
}

fn listed_bars(dir: &Path, kind: Kind) -> Result<Vec<Bar>, Cause> {
    let (_, days) = dataset::read_manifest(dir)?;
    let mut listed_bars = Vec::new();
    for listed in days.iter().filter(|listed| listed.file.is_some()) {
        let path = dir.join(dataset::partition_name(listed.day));
        match dataset::read(&path, kind)? {
            Records::Bars(bars) => match <[Bar; 1]>::try_from(bars) {
                Ok([bar]) => listed_bars.push(bar),
                Err(_) => return Err(untrusted("a listed day does not hold exactly one bar")),
            },
            Records::Trades(_) | Records::Quotes(_) => {
                return Err(untrusted("the dataset holds no bars"));
            }
        }
    }
    Ok(listed_bars)
}

/// Whether `inspect` found the dataset fit to decide on: the pinned symbol's daily bars through
/// the last completed equity session, every partition intact, no trading day without its bar, and
/// no split inside the span, whose prices the shell may not adjust (DEC-138 item 3). `inspect`'s
/// quality warnings are the vendor's records as stored (DEC-89) and are not refusals.
fn trusted(inspection: &Inspection, symbol: &str, now: UtcNanos) -> Result<(), Cause> {
    let daily = Kind::Bars(Timeframe::new(1, TimeUnit::Day)?);
    let latest = latest_completed_equity_day(now)?;
    let refusal = if inspection.dataset.symbol().as_str() != symbol {
        Some("the dataset is another instrument's")
    } else if inspection.dataset.kind() != daily {
        Some("the dataset is not daily bars")
    } else if !inspection.problems.is_empty() {
        Some("a partition cannot be trusted")
    } else if !inspection.duplicates.is_empty() {
        Some("two bars share a start")
    } else if !inspection.coverage.empty.is_empty() {
        Some("a trading day has no bar")
    } else if inspection.gaps.iter().any(|gap| {
        gap.stretches
            .iter()
            .any(|stretch| matches!(stretch.class, GapClass::TrueGap | GapClass::Unclassified))
    }) {
        Some("a trading day was never fetched")
    } else if inspection.coverage.span.map(|span| span.last()) != Some(latest) {
        Some("the dataset does not end on the last completed equity session")
    } else if split_inside(inspection) {
        Some("a split lies inside the span, or the corporate actions do not cover it")
    } else {
        None
    };
    match refusal {
        Some(what) => Err(untrusted(what)),
        None => Ok(()),
    }
}

/// The newest US-equity regular session whose end is not after the injected run clock. Calendar
/// data, session hours, holidays, and early closes all come from `mandate-time`; the shell invents
/// none of them, and the session is `mandate-time`'s
/// [`ExchangeCalendar::last_completed_regular_session`], the one the model host names too. A clock
/// before the calendar's first completed session, or past its range (DEC-517), is untrusted.
fn latest_completed_equity_day(now: UtcNanos) -> Result<Date, Cause> {
    let cannot_name = || untrusted("the equity calendar cannot name the last completed session");
    ExchangeCalendar::us_equities()
        .map_err(|_| cannot_name())?
        .last_completed_regular_session(now)
        .map_err(|_| cannot_name())?
        .ok_or_else(cannot_name)
}

/// Whether a split could have moved a close inside the span. Actions recorded for only part of the
/// span say nothing about the rest, so they count as one.
fn split_inside(inspection: &Inspection) -> bool {
    match &inspection.corporate_actions {
        ActionsReport::NotApplicable | ActionsReport::NotRecorded => false,
        ActionsReport::Incomplete(_) => true,
        ActionsReport::Applied { recorded, .. } => match inspection.coverage.span {
            Some(span) => recorded
                .actions
                .splits
                .iter()
                .any(|split| split.ex_date > span.first() && split.ex_date <= span.last()),
            None => false,
        },
    }
}

pub(crate) fn untrusted(what: &'static str) -> Cause {
    Cause::Untrusted { what }
}

/// E4-2's moving-average baseline, the one model this adapter implements, with the windows the
/// mandate's envelope gives it (`AGENTS.md` rule 11): a model the envelope names differently, or
/// a window it does not state, is a refusal rather than a default.
pub struct MovingAverage;

/// The id the envelope gives E4-2's baseline (DEC-157 item 4).
pub const MOVING_AVERAGE: &str = "quant.ma_crossover";

impl SignalModel for MovingAverage {
    fn signal(&self, model: &ModelRef, closes: &[Price]) -> Result<Signal, Cause> {
        if model.id != MOVING_AVERAGE {
            return Err(Cause::Absent {
                what: "the adapter implements only quant.ma_crossover",
            });
        }
        let strategy = Strategy::MovingAverageCrossover(StrategyConfig {
            fast_periods: window(model, "fast_periods")?,
            slow_periods: window(model, "slow_periods")?,
            collar: Bps::ZERO,
            target_notional: Usd::ZERO,
        });
        Ok(strategy.signal(closes)?)
    }
}

/// One window, as the envelope states it. `Strategy::signal` reads only the two windows; the
/// collar and the target notional above price and size a backtest's orders, which the tracer
/// takes from the order builder instead.
fn window(model: &ModelRef, key: &'static str) -> Result<u32, Cause> {
    match model.params.get(key).map(|value| value.parse::<u32>()) {
        Some(Ok(periods)) => Ok(periods),
        Some(Err(_)) | None => Err(Cause::Absent {
            what: "the model's params do not state its windows",
        }),
    }
}

/// `mandate-builder`'s `conviction_linear` sizing and §6 classification.
pub struct BuilderPlan {
    pub mandate: PathBuf,
    pub context: Option<Rc<RunContext>>,
}

fn builder_output(
    output: &mandate_runtime::ModelOutput,
    outer_model: &str,
    outer_instrument: &InstrumentId,
    mandate: &BuilderMandate,
    context: &BuilderContext,
) -> Result<BuilderModelOutput, Cause> {
    if output.model_id != outer_model || output.instrument_id != *outer_instrument {
        return Err(Cause::Absent {
            what: "a consistently keyed model output",
        });
    }
    let pinned = mandate
        .models
        .iter()
        .find(|model| {
            model.id.as_str() == output.model_id && model.version.as_str() == output.model_version
        })
        .ok_or(Cause::Absent {
            what: "a model output pinned by the validated mandate",
        })?;
    let content_hash = context
        .model_content_hashes
        .get(&(output.model_id.clone(), output.model_version.clone()))
        .copied()
        .ok_or(Cause::Absent {
            what: "the model output's trusted content hash",
        })?;
    if content_hash != output.content_hash {
        return Err(Cause::Absent {
            what: "the model output's matching trusted content hash",
        });
    }
    let direction = match &output.direction {
        ModelDirection::Long => Direction::Long,
        ModelDirection::Other(_) => {
            return Err(Cause::Absent {
                what: "a supported model output direction",
            });
        }
    };
    Ok(BuilderModelOutput {
        model_id: pinned.id.clone(),
        model_version: pinned.version.clone(),
        content_hash,
        instrument: mandate_domain::AssetId::parse(output.instrument_id.as_str())
            .map_err(mandate_builder::BuilderError::from)?,
        as_of: UtcNanos::from_parts(output.as_of.secs(), 0).map_err(|_| Cause::Absent {
            what: "the model output's timestamp",
        })?,
        expires_at: UtcNanos::from_parts(output.expires_at.secs(), 0).map_err(|_| {
            Cause::Absent {
                what: "the model output's expiry",
            }
        })?,
        direction,
        conviction: output.conviction,
        confidence: output.confidence,
    })
}

fn builder_outputs(
    inputs: &SignalInputs,
    mandate: &BuilderMandate,
    context: &BuilderContext,
) -> Result<Vec<BuilderModelOutput>, Cause> {
    inputs
        .outputs
        .iter()
        .flat_map(|(model, instruments)| {
            instruments
                .iter()
                .map(move |(instrument, output)| (model, instrument, output))
        })
        .map(|(model, instrument, output)| {
            builder_output(output, model, instrument, mandate, context)
        })
        .collect()
}

fn runtime_purpose(purpose: BuilderPurpose) -> Purpose {
    match purpose {
        BuilderPurpose::Open => Purpose::Open,
        BuilderPurpose::Increase => Purpose::Increase,
        BuilderPurpose::DiscretionaryExit => Purpose::DiscretionaryExit,
        BuilderPurpose::OwnerExit => Purpose::OwnerExit,
        BuilderPurpose::RiskExit => Purpose::RiskExit,
        BuilderPurpose::Protective => Purpose::Protective,
    }
}

fn runtime_proposal(
    proposal: mandate_builder::Proposal,
    context: &BuilderContext,
    mandate: &BuilderMandate,
    validated: &ValidatedMandate,
) -> Result<Option<Proposal>, Cause> {
    let mut clips_applied = Vec::new();
    if proposal.clipped_by.contains(&BuilderClip::Limits) {
        let delta = proposal.sizes.delta.ok_or(Cause::Absent {
            what: "the builder delta behind its limit clip",
        })?;
        let max_order = UsdExact::of(mandate.limits.max_order_usd);
        let position_room = proposal
            .sizes
            .cap
            .checked_sub(proposal.sizes.current_mv)?
            .checked_sub(UsdExact::of(context.account.working_opening_cost))?;
        let gross_room = UsdExact::of(mandate.limits.max_gross_exposure_usd)
            .min(UsdExact::of(context.account.agent_equity))?
            .checked_sub(UsdExact::of(context.account.gross_usd))?;
        if max_order.is_below(delta)? {
            clips_applied.push(DecisionClip::MaxOrderUsd);
        }
        if position_room.is_below(delta)? {
            clips_applied.push(DecisionClip::PositionCap);
        }
        if gross_room.is_below(delta)? {
            clips_applied.push(DecisionClip::GrossExposureCap);
        }
        if clips_applied.is_empty() {
            return Err(Cause::Absent {
                what: "the exact journal clip applied by the builder",
            });
        }
    }
    if proposal.clipped_by.contains(&BuilderClip::Goal) {
        return Err(Cause::Absent {
            what: "the exact journal goal clip applied by the builder",
        });
    }
    let (side, purpose, qty, limit) = match proposal.action {
        BuilderAction::Hold { .. } => return Ok(None),
        BuilderAction::Buy {
            purpose,
            qty,
            limit_price,
            ..
        } => (Side::Buy, runtime_purpose(purpose), qty, limit_price),
        BuilderAction::Sell {
            purpose,
            qty,
            limit_price,
            ..
        } => (Side::Sell, runtime_purpose(purpose), qty, limit_price),
    };
    let instrument =
        InstrumentId::new(context.market.instrument.as_str()).map_err(|_| Cause::Absent {
            what: "the builder market's runtime instrument id",
        })?;
    validate_order_execution(side, purpose, context.execution)?;
    let mandate_requires_protection = validated.mandate().protection.enabled;
    let execution_matches_mandate = match side {
        Side::Buy => context.execution.protection_required == mandate_requires_protection,
        Side::Sell => !context.execution.protection_required,
    };
    if context.execution.asset_class != context.market.asset_class || !execution_matches_mandate {
        return Err(Cause::Absent {
            what: "the mandate-derived execution policy",
        });
    }
    let outputs_used = proposal
        .combined
        .outputs_used
        .iter()
        .map(|model| model.as_str().to_owned())
        .collect();
    let model_weights = validated
        .mandate()
        .behavior
        .signal_models
        .iter()
        .map(|model| {
            (
                model.id.as_str().to_owned(),
                Value::Str(model.weight.to_string()),
            )
        })
        .collect();
    Ok(Some(Proposal {
        instrument,
        asset_class: context.market.asset_class,
        side,
        qty,
        limit,
        purpose,
        exit_origin: (purpose == Purpose::DiscretionaryExit).then_some(ExitOrigin::Signal),
        exit_conviction: Some(Value::Str(proposal.combined.exit_conviction.to_string())),
        buy_conviction: Some(Value::Str(proposal.combined.buy_conviction.to_string())),
        combined_score: Value::Str(proposal.combined.score.to_string()),
        outputs_used,
        model_weights,
        clips_applied,
        execution: Some(context.execution),
    }))
}

impl Sizing for BuilderPlan {
    fn size(&self, view: &MandateView, inputs: &SignalInputs) -> Result<Option<Proposal>, Cause> {
        let context = required_context(&self.context)?;
        let validated = validated_mandate(&self.mandate, context)?;
        if runtime_view(&validated, context)? != *view {
            return Err(Cause::Absent {
                what: "the admitted mandate view used for sizing",
            });
        }
        let mandate = BuilderMandate::try_from(&validated)?;
        let builder = builder_context(context)?;
        let outputs = builder_outputs(inputs, &mandate, builder)?;
        let now = UtcNanos::from_parts(inputs.now.secs(), 0).map_err(|_| Cause::Absent {
            what: "the decision timestamp",
        })?;
        let proposal = mandate_builder::propose(
            &mandate,
            &builder.account,
            &builder.market,
            &builder.risk,
            &outputs,
            now,
        )?;
        runtime_proposal(proposal, builder, &mandate, &validated)
    }
}

fn action_order_matches(
    action_order_usd: Usd,
    proposal_qty: Qty,
    proposal_limit: Price,
) -> Result<bool, Cause> {
    Ok(action_order_usd == proposal_qty.notional(proposal_limit)?)
}

impl Classifier for BuilderPlan {
    fn classify(&self, view: &MandateView, proposal: &Proposal) -> Result<Classified, Cause> {
        let context = required_context(&self.context)?;
        let validated = validated_mandate(&self.mandate, context)?;
        if runtime_view(&validated, context)? != *view {
            return Err(Cause::Absent {
                what: "the admitted mandate view used for classification",
            });
        }
        let action = &builder_context(context)?.action;
        let expected_score = Value::Str(action.combined_score.to_string());
        if action.instrument.as_str() != proposal.instrument.as_str()
            || action.asset_class != proposal.asset_class
            || runtime_purpose(action.purpose) != proposal.purpose
            || expected_score != proposal.combined_score
            || !action_order_matches(action.order_usd, proposal.qty, proposal.limit)?
        {
            return Err(Cause::Absent {
                what: "classification facts for the proposed action",
            });
        }
        let classification =
            mandate_builder::classify(mandate_builder::autonomy_policy(&validated), action)?;
        let autonomy = match classification.decision {
            AutonomyDecision::Auto => Autonomy::Auto,
            AutonomyDecision::Ask => Autonomy::Ask,
            AutonomyDecision::Deny => Autonomy::Deny,
        };
        Ok(Classified {
            autonomy,
            decided_by: Some(classification.by.label()),
        })
    }
}

/// `mandate_risk::evaluate`, as the runtime's advisory pass. The binding call stays inside
/// `mandate-executor` (`AGENTS.md` rules 1 and 12).
pub struct RiskGate {
    pub context: Option<Rc<RunContext>>,
}

fn proposed_order(proposal: &Proposal, facts: &AdvisoryOrderFacts) -> Result<ProposedOrder, Cause> {
    Ok(ProposedOrder {
        instrument: mandate_risk::AssetId::new(proposal.instrument.as_str()).map_err(|_| {
            Cause::Absent {
                what: "the gate's proposed instrument id",
            }
        })?,
        side: proposal.side,
        qty: proposal.qty,
        limit_price: proposal.limit,
        kind: facts.kind.clone(),
        tif: facts.tif,
        extended_hours: facts.extended_hours,
        origin: facts.origin,
        owner_confirmed_bid: facts.owner_confirmed_bid,
        client_order_id: facts.client_order_id,
        fee_reservation: facts.fee_reservation,
    })
}

impl Gate for RiskGate {
    fn evaluate(&self, proposal: &Proposal) -> Result<Decision, Cause> {
        let context = required_context(&self.context)?;
        let gate = gate_context(context)?;
        let proposed = proposed_order(proposal, &gate.order)?;
        Ok(mandate_risk::evaluate(&GateInput {
            now: gate.now,
            pass: gate.pass,
            config: &gate.config,
            mandate: &gate.mandate,
            risk: &gate.risk,
            account: &gate.account,
            agent: &gate.agent,
            instrument: &gate.instrument,
            market: &gate.market,
            conduct: &gate.conduct,
            universe: &gate.universe,
            proposed: &proposed,
        })?)
    }
}

/// The journal store: `mandate_journal::MemoryJournal` for CI and a scratch run,
/// `mandate-journal-pg` at the DSN for the manual paper run.
pub struct StoreJournal {
    backend: StoreBackend,
    recorded_at: UtcNanos,
}

enum StoreBackend {
    Memory(MemoryJournal),
    Postgres {
        journal: PgJournal,
        runtime: tokio::runtime::Runtime,
    },
    Unavailable,
}

impl StoreJournal {
    /// A scratch journal sealed with the injected clock value.
    pub fn memory(recorded_at: UtcNanos) -> Self {
        Self {
            backend: StoreBackend::Memory(MemoryJournal::new()),
            recorded_at,
        }
    }

    /// A DSN selects Postgres and never falls back to volatile memory.
    pub fn from_dsn(dsn: Option<String>, recorded_at: UtcNanos) -> Self {
        let backend = match dsn {
            None => StoreBackend::Memory(MemoryJournal::new()),
            Some(dsn) => match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => {
                    let journal = {
                        let _entered = runtime.enter();
                        PgJournal::from_dsn(&dsn)
                    };
                    match journal {
                        Ok(journal) => StoreBackend::Postgres { journal, runtime },
                        Err(_) => StoreBackend::Unavailable,
                    }
                }
                Err(_) => StoreBackend::Unavailable,
            },
        };
        Self {
            backend,
            recorded_at,
        }
    }

    fn stream(stream: &str) -> Result<StreamId, Cause> {
        StreamId::parse(stream).ok_or(Cause::Absent {
            what: "a valid journal stream id",
        })
    }
}

impl JournalWriter for StoreJournal {
    fn take_ownership(&mut self, stream: &str) -> Result<u64, Cause> {
        let stream = Self::stream(stream)?;
        match &mut self.backend {
            StoreBackend::Memory(journal) => Ok(journal.take_ownership(&stream)),
            StoreBackend::Postgres { journal, runtime } => runtime
                .block_on(journal.take_ownership(&stream))
                .map_err(|_| Cause::Absent {
                    what: "the durable journal",
                }),
            StoreBackend::Unavailable => Err(Cause::Absent {
                what: "the durable journal",
            }),
        }
    }

    fn read(&self, stream: &str) -> Result<Vec<StoredEvent>, Cause> {
        let stream = Self::stream(stream)?;
        match &self.backend {
            StoreBackend::Memory(journal) => Ok(journal.rows(&stream).to_vec()),
            StoreBackend::Postgres { journal, runtime } => runtime
                .block_on(journal.rows(&stream))
                .map_err(|_| Cause::Absent {
                    what: "the durable journal",
                }),
            StoreBackend::Unavailable => Err(Cause::Absent {
                what: "the durable journal",
            }),
        }
    }

    fn append(
        &mut self,
        stream: &str,
        expected_head: u64,
        writer_epoch: u64,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        let Ok(stream) = Self::stream(stream) else {
            return AppendOutcome::Unavailable;
        };
        let drafts: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
        match &mut self.backend {
            StoreBackend::Memory(journal) => journal.append(
                &stream,
                expected_head,
                writer_epoch,
                self.recorded_at,
                &drafts,
            ),
            StoreBackend::Postgres { journal, runtime } => {
                match runtime.block_on(journal.append(
                    &stream,
                    expected_head,
                    writer_epoch,
                    self.recorded_at,
                    &drafts,
                )) {
                    Ok(outcome) => outcome,
                    Err(_) => AppendOutcome::Unavailable,
                }
            }
            StoreBackend::Unavailable => AppendOutcome::Unavailable,
        }
    }
}

/// The runtime's `IntentSink`, handing to the executor with this deployment's `AgentId`.
pub struct ExecutorSink {
    pub agent: AgentId,
}

impl Sink for ExecutorSink {
    fn hand(&mut self, handoff: &IntentHandoff) -> Result<mandate_executor::IntentHandoff, Cause> {
        let body = match &handoff.body {
            IntentBody::Order {
                instrument,
                side,
                qty,
                limit,
                purpose,
            } => {
                let execution = handoff.execution.ok_or(Cause::Absent {
                    what: "the mandate-derived order execution policy",
                })?;
                validate_order_execution(*side, *purpose, execution)?;
                mandate_executor::IntentBody::Order {
                    instrument: instrument.clone(),
                    side: *side,
                    qty: *qty,
                    limit: *limit,
                    purpose: executor_purpose(*purpose),
                    protection: execution.protection.map(|prices| {
                        mandate_executor::ProtectionPrices {
                            stop: prices.stop,
                            take_profit: prices.take_profit,
                        }
                    }),
                }
            }
            IntentBody::Flatten(plan) => {
                if handoff.execution.is_some() {
                    return Err(Cause::Absent {
                        what: "a flatten without opening-order policy",
                    });
                }
                mandate_executor::IntentBody::Flatten(mandate_executor::FlattenPlan {
                    cancel_client_order_ids: plan.cancel_client_order_ids.clone(),
                    sells: plan
                        .sells
                        .iter()
                        .map(|leg| mandate_executor::FlattenLeg {
                            instrument: leg.instrument.clone(),
                            asset_class: leg.asset_class,
                            qty: leg.qty,
                            deferred_to_regular_session: leg.deferred_to_regular_session,
                        })
                        .collect(),
                    purpose: executor_purpose(plan.purpose),
                    confirmation: plan.confirmation.as_ref().map(|confirmation| {
                        mandate_executor::OwnerConfirmation {
                            bid: confirmation.bid,
                            bid_size: confirmation.bid_size,
                            floor: confirmation.floor,
                            user: confirmation.user.clone(),
                            step_up: confirmation.step_up.clone(),
                        }
                    }),
                })
            }
        };
        Ok(mandate_executor::IntentHandoff {
            intent_id: mandate_executor::IntentId(mandate_executor::EventId(
                handoff.intent_id.0.clone(),
            )),
            agent: ExecutorAgentId(self.agent.0.clone()),
            tif: executor_tif(handoff.execution),
            body,
        })
    }
}

fn validate_order_execution(
    side: Side,
    purpose: Purpose,
    execution: OrderExecution,
) -> Result<(), Cause> {
    let direction_is_valid = match purpose {
        Purpose::Open | Purpose::Increase => side == Side::Buy,
        Purpose::RiskExit
        | Purpose::OwnerExit
        | Purpose::DiscretionaryExit
        | Purpose::Protective
        | Purpose::Flatten => side == Side::Sell,
    };
    if !direction_is_valid {
        return Err(Cause::Absent {
            what: "an order direction that cannot create a short position",
        });
    }
    if execution.protection_required != execution.protection.is_some() {
        return Err(Cause::Absent {
            what: "the mandate-required protective prices",
        });
    }
    match (execution.asset_class, execution.tif) {
        (AssetClass::Crypto, TimeInForce::Day) | (AssetClass::UsEquity, TimeInForce::Ioc) => {
            Err(Cause::Absent {
                what: "a time in force allowed by the mandate's asset class",
            })
        }
        (AssetClass::UsEquity, TimeInForce::Day | TimeInForce::Gtc)
        | (AssetClass::Crypto, TimeInForce::Gtc | TimeInForce::Ioc) => Ok(()),
    }
}

fn executor_tif(execution: Option<OrderExecution>) -> Option<mandate_executor::TimeInForce> {
    execution.map(|execution| match (execution.asset_class, execution.tif) {
        (AssetClass::UsEquity, TimeInForce::Day) | (AssetClass::Crypto, TimeInForce::Day) => {
            mandate_executor::TimeInForce::Day
        }
        (AssetClass::UsEquity, TimeInForce::Gtc) | (AssetClass::Crypto, TimeInForce::Gtc) => {
            mandate_executor::TimeInForce::Gtc
        }
        (AssetClass::UsEquity, TimeInForce::Ioc) | (AssetClass::Crypto, TimeInForce::Ioc) => {
            mandate_executor::TimeInForce::Ioc
        }
    })
}

fn executor_purpose(purpose: Purpose) -> mandate_executor::Purpose {
    match purpose {
        Purpose::Open => mandate_executor::Purpose::Open,
        Purpose::Increase => mandate_executor::Purpose::Increase,
        Purpose::RiskExit => mandate_executor::Purpose::RiskExit,
        Purpose::OwnerExit => mandate_executor::Purpose::OwnerExit,
        Purpose::DiscretionaryExit => mandate_executor::Purpose::DiscretionaryExit,
        Purpose::Protective => mandate_executor::Purpose::Protective,
        Purpose::Flatten => mandate_executor::Purpose::Flatten,
    }
}

/// Trusted, effective-dated inputs used by both executor entry points.
///
/// The binding source supplies facts, never a verdict. Keeping every collaborator in one value
/// prevents `handle` and reconciliation from observing different configuration in one run.
pub struct ExecutorContext {
    ids: Rc<dyn IdGen>,
    mandates: Rc<dyn ExecutorMandateView>,
    instruments: Rc<dyn InstrumentSnapshot>,
    binding_gate: Rc<dyn BindingGateSource>,
    config: ExecutorConfig,
    fees: mandate_accounting::Config,
}

impl ExecutorContext {
    pub fn new(
        ids: Rc<dyn IdGen>,
        mandates: Rc<dyn ExecutorMandateView>,
        instruments: Rc<dyn InstrumentSnapshot>,
        binding_gate: Rc<dyn BindingGateSource>,
        config: ExecutorConfig,
        fees: mandate_accounting::Config,
    ) -> Self {
        Self {
            ids,
            mandates,
            instruments,
            binding_gate,
            config,
            fees,
        }
    }

    fn ports(&self) -> Ports<'_> {
        Ports {
            ids: &*self.ids,
            mandates: &*self.mandates,
            instruments: &*self.instruments,
            config: &self.config,
            fees: &self.fees,
        }
    }

    fn configuration(&self) -> ExecutorConfig {
        self.config
    }
}

/// `mandate_executor::handle` and `fold` over the account stream's shared state.
pub struct CoreExecutor {
    state: Rc<RefCell<ExecutorState>>,
    context: Option<Rc<ExecutorContext>>,
}

impl CoreExecutor {
    /// Builds the executor and reconciler over one replayed state and one trusted context.
    pub fn pair(
        scope: AccountScope,
        context: Option<ExecutorContext>,
    ) -> (CoreExecutor, ExecutorReconciler) {
        let state = Rc::new(RefCell::new(ExecutorState::new(scope)));
        let context = context.map(Rc::new);
        (
            CoreExecutor {
                state: Rc::clone(&state),
                context: context.clone(),
            },
            ExecutorReconciler { state, context },
        )
    }

    pub fn state(&self) -> Ref<'_, ExecutorState> {
        self.state.borrow()
    }

    fn context(&self) -> Result<&ExecutorContext, Cause> {
        self.context.as_deref().ok_or(Cause::Absent {
            what: "the trusted executor context",
        })
    }
}

impl Executor for CoreExecutor {
    fn reset(&mut self) -> Result<(), Cause> {
        let _ = self.context()?;
        let scope = self.state.borrow().scope().clone();
        *self.state.borrow_mut() = ExecutorState::new(scope);
        Ok(())
    }

    fn use_profile(&mut self, profile: CapabilityProfile) -> Result<(), Cause> {
        let scope = self.state.borrow().scope().clone();
        let mut state = self.state.borrow_mut();
        let folded = mem::replace(&mut *state, ExecutorState::new(scope));
        *state = folded.with_profile(profile);
        Ok(())
    }

    fn step(
        &mut self,
        input: mandate_executor::Input,
    ) -> Result<Vec<mandate_executor::Effect>, Cause> {
        let context = self.context()?;
        let ports = context.ports();
        let effects = mandate_executor::handle(
            &mut self.state.borrow_mut(),
            input,
            &ports,
            &*context.binding_gate,
        )?;
        Ok(effects)
    }

    fn committed(&mut self, event: &mandate_executor::FoldedEvent) -> Result<(), Cause> {
        fold(&mut self.state.borrow_mut(), event)?;
        Ok(())
    }
}

/// `mandate_alpaca::TradingClient` over `transport`: the scripted one in CI, `AlpacaPaperHttp` for
/// the manual run. The shell names no host and builds no URL (TI-5): every request is one of the
/// client's own, sent through the transport it is given.
///
/// `BrokerConnector::call` is async and the stage is not, so the shell owns the runtime that drives
/// it (task brief step 13): one current-thread tokio runtime per call, which holds nothing between
/// calls, as the client holds nothing a restart would lose.
pub struct AlpacaConnector<T> {
    pub transport: T,
}

impl<T: TradingTransport + Clone> Connector for AlpacaConnector<T> {
    fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
        TradingClient::new(self.transport.clone(), TokioPause, RetryPolicy::default()).profile()
    }

    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|_| ConnectorError::NotSent {
                code: "runtime_unavailable",
            })?;
        let mut client =
            TradingClient::new(self.transport.clone(), TokioPause, RetryPolicy::default());
        runtime.block_on(client.call(request))
    }
}

/// The connector of a process that holds no transport: every request is refused before it leaves
/// the process (DEC-166 item 3). The shipping binary never uses it (DEC-466 item 3); the
/// fail-closed tests do.
pub struct Disconnected;

impl Connector for Disconnected {
    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        let _ = request;
        Err(ConnectorError::NotSent {
            code: "no_transport",
        })
    }
}

/// `mandate_executor::reconcile` on the broker snapshot the connector reads at startup.
pub struct ExecutorReconciler {
    state: Rc<RefCell<ExecutorState>>,
    context: Option<Rc<ExecutorContext>>,
}

impl ExecutorReconciler {
    fn context(&self) -> Result<&ExecutorContext, Cause> {
        self.context.as_deref().ok_or(Cause::Absent {
            what: "the trusted executor context",
        })
    }

    fn gather_snapshot(
        &self,
        connector: &mut dyn Connector,
        requests: &[BrokerRequest],
    ) -> Result<mandate_executor::BrokerSnapshot, Cause> {
        let state = self.state.borrow();
        let [
            open_orders_request @ BrokerRequest::ListOpenOrders,
            positions_request @ BrokerRequest::ListPositions,
            account_request @ BrokerRequest::GetAccount,
            activities_request @ BrokerRequest::ListActivities { since },
        ] = requests
        else {
            return Err(Cause::Absent {
                what: "the executor's complete reconciliation request",
            });
        };
        let expected_since = match state.checkpoint() {
            Some(cursor) => cursor.clone(),
            None => mandate_executor::ActivityCursor(String::new()),
        };
        if since != &expected_since {
            return Err(Cause::Absent {
                what: "the executor's current reconciliation cursor",
            });
        }
        let open_orders = match connector.call(open_orders_request)? {
            BrokerOutcome::OpenOrders(orders) => orders,
            unexpected => {
                return Err(unexpected_snapshot(
                    unexpected,
                    "the broker's open-order snapshot",
                ));
            }
        };
        let positions = match connector.call(positions_request)? {
            BrokerOutcome::Positions(positions) => positions,
            unexpected => {
                return Err(unexpected_snapshot(
                    unexpected,
                    "the broker's position snapshot",
                ));
            }
        };
        let account = match connector.call(account_request)? {
            BrokerOutcome::Account(account) => account,
            unexpected => {
                return Err(unexpected_snapshot(
                    unexpected,
                    "the broker's account snapshot",
                ));
            }
        };
        let (fills, cursor) = match connector.call(activities_request)? {
            BrokerOutcome::Activities { fills, cursor } => (fills, cursor),
            unexpected => {
                return Err(unexpected_snapshot(
                    unexpected,
                    "the broker's activity snapshot",
                ));
            }
        };
        let scope = state.scope();
        let stream = account_stream(&scope.workspace.0, &scope.account.0);
        Ok(mandate_executor::BrokerSnapshot {
            open_orders,
            positions,
            account,
            fills,
            cursor,
            reason: mandate_executor::ReconcileReason::Startup,
            taken_at_head: match state.head(&stream) {
                Some(head) => head,
                None => Seq(0),
            },
        })
    }
}

fn unexpected_snapshot(outcome: BrokerOutcome, what: &'static str) -> Cause {
    match outcome {
        BrokerOutcome::Submitted(_)
        | BrokerOutcome::DuplicateClientOrderId { .. }
        | BrokerOutcome::Rejected(_)
        | BrokerOutcome::Order(_)
        | BrokerOutcome::Absent { .. }
        | BrokerOutcome::OpenOrders(_)
        | BrokerOutcome::Positions(_)
        | BrokerOutcome::Account(_)
        | BrokerOutcome::Activities { .. }
        | BrokerOutcome::CancelAccepted { .. }
        | BrokerOutcome::AccountWideAccepted => Cause::Absent { what },
    }
}

impl Reconciler for ExecutorReconciler {
    fn snapshot(
        &mut self,
        connector: &mut dyn Connector,
        requests: &[BrokerRequest],
    ) -> Result<mandate_executor::BrokerSnapshot, Cause> {
        self.gather_snapshot(connector, requests)
    }

    fn reconcile(
        &mut self,
        snapshot: &mandate_executor::BrokerSnapshot,
    ) -> Result<mandate_executor::Reconciliation, Cause> {
        let context = self.context()?;
        let ports = context.ports();
        let reconciliation = mandate_executor::reconcile(&self.state.borrow(), snapshot, &ports)?;
        Ok(reconciliation)
    }
}

/// Where the production stages read from.
pub struct Sources<T> {
    pub mandate: PathBuf,
    pub dataset: PathBuf,
    /// `None` is a scratch in-memory journal: a run that places nothing keeps nothing, so a later
    /// start cannot re-hand an intent a planning run proposed (DEC-157 item 6).
    pub journal: Option<String>,
    pub recorded_at: UtcNanos,
    pub agent: AgentId,
    pub workspace: String,
    pub account_ref: String,
    /// Trusted executor snapshots and effective-dated configuration. `None` refuses before the
    /// executor mutates process state or reads the broker.
    pub executor: Option<ExecutorContext>,
    /// Validation, admission, builder, and advisory-gate facts. `None` keeps the binary fail closed
    /// until live assembly can supply every effective-dated input.
    pub run: Option<RunContext>,
    /// The content-addressed store the run's configuration objects were registered in. The
    /// journal checks a version-2 record's `policy_set` and `model_registry` objects in it before
    /// it commits the record, so `None`, or a store missing either object, stops a governed run
    /// at its first version-2 append, before any intent (journal spec §5.1, §11 check 6).
    pub artifacts: Option<Arc<dyn ArtifactSource + Send + Sync>>,
    pub transport: T,
}

/// The production stages, with the Alpaca paper client over `sources.transport`.
pub fn production<T: TradingTransport + Clone + 'static>(sources: Sources<T>) -> Stages {
    let Sources {
        mandate,
        dataset,
        journal,
        recorded_at,
        agent,
        workspace,
        account_ref,
        executor,
        run,
        artifacts,
        transport,
    } = sources;
    over(Sources {
        mandate,
        dataset,
        journal,
        recorded_at,
        agent,
        workspace,
        account_ref,
        executor,
        run,
        artifacts,
        transport: Box::new(AlpacaConnector { transport }),
    })
}

/// The production stages over a caller-supplied connector.
pub fn over(sources: Sources<Box<dyn Connector>>) -> Stages {
    let run = sources.run.map(Rc::new);
    let protection_config = sources
        .executor
        .as_ref()
        .map(ExecutorContext::configuration);
    let (executor, reconciler) = CoreExecutor::pair(
        AccountScope {
            account: AccountRef(sources.account_ref),
            workspace: WorkspaceId(sources.workspace),
        },
        sources.executor,
    );
    Stages {
        exit: Box::new(RiskExitPath::new(
            sources.agent.clone(),
            sources.mandate.clone(),
        )),
        protection: Box::new(ExecutorProtection::new(
            sources.mandate.clone(),
            protection_config,
        )),
        mandate: Box::new(SpecMandate {
            path: sources.mandate.clone(),
            context: run.clone(),
        }),
        bars: Box::new(StoredBars {
            dir: sources.dataset,
        }),
        signal: Box::new(MovingAverage),
        reconciler: Box::new(reconciler),
        sizing: Box::new(BuilderPlan {
            mandate: sources.mandate.clone(),
            context: run.clone(),
        }),
        classifier: Box::new(BuilderPlan {
            mandate: sources.mandate,
            context: run.clone(),
        }),
        gate: Box::new(RiskGate { context: run }),
        journal: Box::new(StoreJournal::from_dsn(sources.journal, sources.recorded_at)),
        sink: Box::new(ExecutorSink {
            agent: sources.agent,
        }),
        executor: Box::new(executor),
        connector: sources.transport,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet, VecDeque};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    use super::{
        AdvisoryGateContext, AdvisoryOrderFacts, CoreExecutor, DecisionContext, ExecutorContext,
        ExecutorMandateView, ExecutorOrder, ExecutorProtection, ExecutorSink, Protection,
        ProtectionProbePorts, RiskGate, RunContext, StoreJournal, action_order_matches,
        is_unattributed_exit, proposed_order, working_order_of,
    };
    use mandate_backtest::{BacktestError, Signal};
    use mandate_canon::{DecStr, Digest, Value};
    use mandate_journal::AppendOutcome;
    use mandate_marketdata::actions::{RecordedActions, write_actions};
    use mandate_marketdata::dataset::{Store, partition_name};
    use mandate_marketdata::inspect::{self, ActionsReport, Problem};
    use mandate_marketdata::model::{
        AssetClass, Bar, CorporateActions, DatasetId, DayRange, Feed, Kind, Records, Split,
        SplitRatio, Symbol, TimeUnit, Timeframe,
    };
    use mandate_num::{Price, Qty, Usd};
    use mandate_runtime::{
        AgentId, EventId as RuntimeEventId, IntentBody, IntentHandoff, OrderExecution,
        ProtectionPrices, Purpose as RuntimePurpose, TimeInForce as RuntimeTimeInForce,
    };
    use mandate_time::{Date, UtcNanos};

    use crate::envelope::{DraftFields, Envelope, Writer, draft_bytes};
    use crate::stages::{Connector, Executor, Gate, JournalWriter, Reconciler, Sink};

    fn value_object(fields: &[(&str, Value)]) -> Result<Value, String> {
        let mut object = mandate_canon::Object::new();
        for (name, value) in fields {
            object.insert(
                mandate_canon::Key::new(name).map_err(|e| e.to_string())?,
                value.clone(),
            );
        }
        Ok(Value::Object(object))
    }

    struct TestExecutorSource;

    impl mandate_executor::IdGen for TestExecutorSource {
        fn event_id(
            &self,
            epoch: mandate_executor::WriterEpoch,
            head: mandate_executor::Seq,
            ordinal: u32,
        ) -> mandate_executor::EventId {
            mandate_executor::EventId(format!("{}-{}-{ordinal}", epoch.0, head.0))
        }
    }

    impl mandate_executor::MandateView for TestExecutorSource {
        fn version(
            &self,
            _agent: &mandate_executor::AgentId,
        ) -> Option<mandate_executor::MandateVersion> {
            None
        }

        fn crypto_stop_limit_offset(
            &self,
            _agent: &mandate_executor::AgentId,
        ) -> Option<mandate_num::Fraction> {
            None
        }

        fn covers(
            &self,
            _agent: &mandate_executor::AgentId,
            _instrument: &mandate_accounting::InstrumentId,
        ) -> bool {
            false
        }
    }

    impl mandate_executor::InstrumentSnapshot for TestExecutorSource {
        fn asset_class(
            &self,
            _instrument: &mandate_accounting::InstrumentId,
        ) -> Option<mandate_accounting::AssetClass> {
            None
        }

        fn increment(
            &self,
            _instrument: &mandate_accounting::InstrumentId,
        ) -> Option<mandate_num::ShareIncrement> {
            None
        }

        fn exit_tier(
            &self,
            _instrument: &mandate_accounting::InstrumentId,
        ) -> Option<mandate_executor::ExitTier> {
            None
        }
    }

    impl mandate_executor::BindingGateSource for TestExecutorSource {
        fn input(
            &self,
            _request: &mandate_executor::BindingGateRequest<'_>,
        ) -> Option<mandate_executor::BindingGateInput> {
            None
        }
    }

    fn test_executor_context() -> Result<ExecutorContext, String> {
        let first = Date::parse("2026-01-01").map_err(|e| e.to_string())?;
        let last = Date::parse("2026-12-31").map_err(|e| e.to_string())?;
        let calendar =
            mandate_time::TradingCalendar::new(first, last, [], []).map_err(|e| e.to_string())?;
        let fees = mandate_executor::paper_only_fee_config("paper", calendar, "2026-01-01")
            .map_err(|e| e.to_string())?;
        let source = Rc::new(TestExecutorSource);
        Ok(ExecutorContext::new(
            source.clone(),
            source.clone(),
            source.clone(),
            source,
            mandate_executor::ExecutorConfig::PROPOSED,
            fees,
        ))
    }

    fn account_scope() -> mandate_executor::AccountScope {
        mandate_executor::AccountScope {
            account: mandate_executor::AccountRef("tracer-paper".to_owned()),
            workspace: mandate_executor::WorkspaceId("tracer".to_owned()),
        }
    }

    fn stream_opened() -> Result<mandate_executor::FoldedEvent, String> {
        Ok(mandate_executor::FoldedEvent {
            stream: "acct:tracer:tracer-paper".to_owned(),
            seq: mandate_executor::Seq(1),
            event_id: mandate_executor::EventId("10000100000000000000000000".to_owned()),
            event_type: "StreamOpened".to_owned(),
            causation_id: None,
            payload: value_object(&[("environment", Value::Str("paper".to_owned()))])?,
        })
    }

    #[test]
    fn memory_journal_seals_with_the_injected_recorded_at() -> Result<(), String> {
        let recorded_at =
            UtcNanos::parse("2026-09-25T20:00:00.123456789Z").map_err(|e| e.to_string())?;
        let stream = "acct:tracer:tracer-paper";
        let payload = value_object(&[
            ("stream_type", Value::Str("account".to_owned())),
            ("workspace_id", Value::Str("tracer".to_owned())),
            ("account_ref", Value::Str("tracer-paper".to_owned())),
            ("broker", Value::Str("alpaca".to_owned())),
        ])?;
        let config_refs = mandate_canon::Object::new();
        let draft = draft_bytes(
            &Envelope {
                stream,
                writer: Writer::Executor,
                actor_id: "executor",
                event_time: recorded_at,
            },
            &DraftFields {
                event_id: "10000100000000000000000000",
                event_type: "StreamOpened",
                schema_version: 1,
                causation_id: None,
                config_refs: &config_refs,
                payload: &payload,
            },
        )
        .map_err(|e| e.to_string())?;
        let mut journal = StoreJournal::memory(recorded_at);
        let epoch = journal.take_ownership(stream).map_err(|e| e.to_string())?;
        assert!(matches!(
            journal.append(stream, 0, epoch, &[draft]),
            AppendOutcome::Committed(_)
        ));
        let rows = journal.read(stream).map_err(|e| e.to_string())?;
        assert_eq!(rows.len(), 1);
        let row = rows.first().ok_or("the committed row is absent")?;
        assert_eq!(row.recorded_at, recorded_at.to_string());
        Ok(())
    }

    #[test]
    fn a_dsn_parse_failure_never_falls_back_to_memory() -> Result<(), String> {
        let recorded_at =
            UtcNanos::parse("2026-09-25T20:00:00.000000000Z").map_err(|e| e.to_string())?;
        let mut journal =
            StoreJournal::from_dsn(Some("not a postgres dsn".to_owned()), recorded_at);
        assert!(journal.take_ownership("acct:tracer:tracer-paper").is_err());
        assert!(matches!(
            journal.append("acct:tracer:tracer-paper", 0, 0, &[]),
            AppendOutcome::Unavailable
        ));
        Ok(())
    }

    #[test]
    fn core_executor_replays_then_starts_through_real_handle() -> Result<(), String> {
        let (mut core, _) = CoreExecutor::pair(account_scope(), Some(test_executor_context()?));
        core.committed(&stream_opened()?)
            .map_err(|e| e.to_string())?;
        let effects = core
            .step(mandate_executor::Input::Started(
                mandate_executor::WriterEpoch(7),
            ))
            .map_err(|e| e.to_string())?;
        assert_eq!(
            effects,
            [
                mandate_executor::Effect::Broker(mandate_executor::BrokerRequest::ListOpenOrders),
                mandate_executor::Effect::Broker(mandate_executor::BrokerRequest::ListPositions),
                mandate_executor::Effect::Broker(mandate_executor::BrokerRequest::GetAccount),
                mandate_executor::Effect::Broker(mandate_executor::BrokerRequest::ListActivities {
                    since: mandate_executor::ActivityCursor(String::new())
                })
            ]
        );
        assert!(core.state().started());
        assert_eq!(core.state().epoch(), Some(mandate_executor::WriterEpoch(7)));
        assert_eq!(
            core.state().head("acct:tracer:tracer-paper"),
            Some(mandate_executor::Seq(1))
        );
        Ok(())
    }

    #[test]
    fn core_executor_reset_rebuilds_an_empty_fold_and_keeps_its_trusted_context()
    -> Result<(), String> {
        let (mut core, _) = CoreExecutor::pair(account_scope(), Some(test_executor_context()?));
        let opened = stream_opened()?;
        core.committed(&opened).map_err(|error| error.to_string())?;
        assert!(
            core.committed(&opened).is_err(),
            "without a reset sequence one cannot be folded twice"
        );
        core.reset().map_err(|error| error.to_string())?;
        core.committed(&opened).map_err(|error| error.to_string())?;
        core.step(mandate_executor::Input::Started(
            mandate_executor::WriterEpoch(7),
        ))
        .map_err(|error| error.to_string())?;
        assert_eq!(core.state().environment(), Some("paper"));
        Ok(())
    }

    #[test]
    fn core_executor_without_trusted_context_fails_before_starting() -> Result<(), String> {
        let (mut core, _) = CoreExecutor::pair(account_scope(), None);
        core.committed(&stream_opened()?)
            .map_err(|e| e.to_string())?;
        assert!(matches!(
            core.step(mandate_executor::Input::Started(
                mandate_executor::WriterEpoch(7)
            )),
            Err(crate::Cause::Absent {
                what: "the trusted executor context"
            })
        ));
        assert!(!core.state().started());
        Ok(())
    }

    struct SnapshotConnector {
        outcomes: VecDeque<mandate_executor::BrokerOutcome>,
        requests: Vec<mandate_executor::BrokerRequest>,
    }

    impl Connector for SnapshotConnector {
        fn call(
            &mut self,
            request: &mandate_executor::BrokerRequest,
        ) -> Result<mandate_executor::BrokerOutcome, mandate_executor::ConnectorError> {
            self.requests.push(request.clone());
            self.outcomes
                .pop_front()
                .ok_or(mandate_executor::ConnectorError::NotSent {
                    code: "fixture_exhausted",
                })
        }
    }

    #[test]
    fn reconciliation_reads_the_broker_at_the_shared_folded_head_and_runs_the_core()
    -> Result<(), String> {
        let account = mandate_executor::BrokerAccount {
            status: "ACTIVE".to_owned(),
            crypto_status: "ACTIVE".to_owned(),
            trading_blocked: false,
            account_blocked: false,
            trade_suspended_by_user: false,
            multiplier: 2,
            equity: mandate_num::Usd::ZERO,
            cash: mandate_num::Usd::ZERO,
            buying_power: mandate_num::Usd::ZERO,
            non_marginable_buying_power: mandate_num::Usd::ZERO,
            accrued_fees: mandate_num::Usd::ZERO,
            last_equity: mandate_num::Usd::ZERO,
            maintenance_margin: mandate_num::Usd::ZERO,
        };
        let mut connector = SnapshotConnector {
            outcomes: VecDeque::from([
                mandate_executor::BrokerOutcome::OpenOrders(Vec::new()),
                mandate_executor::BrokerOutcome::Positions(Vec::new()),
                mandate_executor::BrokerOutcome::Account(account),
                mandate_executor::BrokerOutcome::Activities {
                    fills: Vec::new(),
                    cursor: mandate_executor::ActivityCursor("cursor-1".to_owned()),
                },
            ]),
            requests: Vec::new(),
        };
        let (mut core, mut reconciler) =
            CoreExecutor::pair(account_scope(), Some(test_executor_context()?));
        core.committed(&stream_opened()?)
            .map_err(|e| e.to_string())?;
        let requests: Vec<_> = core
            .step(mandate_executor::Input::Started(
                mandate_executor::WriterEpoch(7),
            ))
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter_map(|effect| match effect {
                mandate_executor::Effect::Broker(request) => Some(request),
                mandate_executor::Effect::Journal(_)
                | mandate_executor::Effect::Timer(_)
                | mandate_executor::Effect::Notify(_) => None,
            })
            .collect();
        let snapshot = reconciler
            .snapshot(&mut connector, &requests)
            .map_err(|e| e.to_string())?;
        let result = reconciler.reconcile(&snapshot).map_err(|e| e.to_string())?;
        assert_eq!(result.expected_head, mandate_executor::Seq(1));
        assert!(result.effects.iter().any(|effect| matches!(
            effect,
            mandate_executor::Effect::Journal(draft)
                if draft.event_type == "ReconciliationRun"
        )));
        assert_eq!(
            connector.requests,
            [
                mandate_executor::BrokerRequest::ListOpenOrders,
                mandate_executor::BrokerRequest::ListPositions,
                mandate_executor::BrokerRequest::GetAccount,
                mandate_executor::BrokerRequest::ListActivities {
                    since: mandate_executor::ActivityCursor(String::new())
                }
            ]
        );
        Ok(())
    }

    #[test]
    fn incomplete_reconciliation_request_reads_nothing_from_the_broker() -> Result<(), String> {
        let mut connector = SnapshotConnector {
            outcomes: VecDeque::new(),
            requests: Vec::new(),
        };
        let (_, mut reconciler) =
            CoreExecutor::pair(account_scope(), Some(test_executor_context()?));
        assert!(matches!(
            reconciler.snapshot(&mut connector, &[]),
            Err(crate::Cause::Absent {
                what: "the executor's complete reconciliation request"
            })
        ));
        assert!(connector.requests.is_empty());
        Ok(())
    }

    #[test]
    fn executor_sink_carries_the_mandate_order_policy_without_substitution() -> Result<(), String> {
        let instrument =
            mandate_accounting::InstrumentId::new("b0b6dd9d-8b9b-48a9-ba46-b9d54906e415")
                .map_err(|e| e.to_string())?;
        let stop = Price::parse("242.44").map_err(|e| e.to_string())?;
        let mut sink = ExecutorSink {
            agent: AgentId("tracer-aapl".to_owned()),
        };
        let converted = sink
            .hand(&IntentHandoff {
                intent_id: RuntimeEventId("10000100000000000000000000".to_owned()),
                body: IntentBody::Order {
                    instrument: instrument.clone(),
                    side: mandate_accounting::Side::Buy,
                    qty: Qty::parse("1").map_err(|e| e.to_string())?,
                    limit: Price::parse("255.2").map_err(|e| e.to_string())?,
                    purpose: RuntimePurpose::Open,
                },
                execution: Some(OrderExecution {
                    asset_class: mandate_accounting::AssetClass::UsEquity,
                    tif: RuntimeTimeInForce::Gtc,
                    protection_required: true,
                    protection: Some(ProtectionPrices {
                        stop,
                        take_profit: None,
                    }),
                }),
            })
            .map_err(|e| e.to_string())?;
        assert_eq!(converted.agent.0, "tracer-aapl");
        assert_eq!(converted.tif, Some(mandate_executor::TimeInForce::Gtc));
        let mandate_executor::IntentBody::Order {
            instrument: got_instrument,
            side,
            purpose,
            protection,
            ..
        } = converted.body
        else {
            return Err("the order handoff became a flatten".to_owned());
        };
        assert_eq!(got_instrument, instrument);
        assert_eq!(side, mandate_accounting::Side::Buy);
        assert_eq!(purpose, mandate_executor::Purpose::Open);
        assert_eq!(
            protection,
            Some(mandate_executor::ProtectionPrices {
                stop,
                take_profit: None,
            })
        );
        Ok(())
    }

    #[test]
    fn executor_sink_refuses_missing_policy_missing_protection_and_short_openings()
    -> Result<(), String> {
        let instrument =
            mandate_accounting::InstrumentId::new("b0b6dd9d-8b9b-48a9-ba46-b9d54906e415")
                .map_err(|e| e.to_string())?;
        let order = |side, purpose, execution| -> Result<IntentHandoff, String> {
            Ok(IntentHandoff {
                intent_id: RuntimeEventId("10000100000000000000000000".to_owned()),
                body: IntentBody::Order {
                    instrument: instrument.clone(),
                    side,
                    qty: Qty::parse("1").map_err(|e| e.to_string())?,
                    limit: Price::parse("255.2").map_err(|e| e.to_string())?,
                    purpose,
                },
                execution,
            })
        };
        let mut sink = ExecutorSink {
            agent: AgentId("tracer-aapl".to_owned()),
        };
        assert!(
            sink.hand(&order(
                mandate_accounting::Side::Buy,
                RuntimePurpose::Open,
                None,
            )?)
            .is_err()
        );
        let required = Some(OrderExecution {
            asset_class: mandate_accounting::AssetClass::UsEquity,
            tif: RuntimeTimeInForce::Day,
            protection_required: true,
            protection: None,
        });
        assert!(
            sink.hand(&order(
                mandate_accounting::Side::Buy,
                RuntimePurpose::Open,
                required,
            )?)
            .is_err()
        );
        let unprotected = Some(OrderExecution {
            asset_class: mandate_accounting::AssetClass::UsEquity,
            tif: RuntimeTimeInForce::Day,
            protection_required: false,
            protection: None,
        });
        assert!(
            sink.hand(&order(
                mandate_accounting::Side::Sell,
                RuntimePurpose::Open,
                unprotected,
            )?)
            .is_err()
        );
        assert!(
            sink.hand(&order(
                mandate_accounting::Side::Buy,
                RuntimePurpose::RiskExit,
                unprotected,
            )?)
            .is_err()
        );
        sink.hand(&order(
            mandate_accounting::Side::Sell,
            RuntimePurpose::RiskExit,
            unprotected,
        )?)
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[test]
    fn the_protection_probe_refuses_without_its_mandate() {
        let probe = ExecutorProtection::new(
            PathBuf::from("a-mandate-that-does-not-exist.json"),
            Some(mandate_executor::ExecutorConfig::PROPOSED),
        );
        assert!(probe.probe().is_err());
    }

    #[test]
    fn the_protection_probe_ports_never_invent_mandate_coverage() -> Result<(), String> {
        let agent = mandate_executor::AgentId("agent-a".to_owned());
        let instrument =
            mandate_accounting::InstrumentId::new("b0b6dd9d-8b9b-48a9-ba46-b9d54906e415")
                .map_err(|e| e.to_string())?;
        let ports = ProtectionProbePorts {
            instrument: instrument.clone(),
        };
        assert!(ExecutorMandateView::covers(&ports, &agent, &instrument));
        let other = mandate_accounting::InstrumentId::new("7b4a1c2e-2222-4a2b-9c3d-000000000002")
            .map_err(|e| e.to_string())?;
        assert!(!ExecutorMandateView::covers(&ports, &agent, &other));
        Ok(())
    }

    /// The purpose mapping the risk crate's inputs owe (DEC-449 item 3): the executor's
    /// protective purpose is the risk crate's protective flag, and an opening purpose is the
    /// opening flag. `agent_flatten` itself cancels every working order whatever its flags, so
    /// no plan-level test can see a flip — this pin is what makes one wrong.
    #[test]
    fn the_purpose_flags_map_from_the_folds_own_order() -> Result<(), String> {
        let today = day("2026-09-21")?;
        let protective =
            working_order_of(&the_order(mandate_executor::Purpose::Protective)?, today)
                .map_err(|e| e.to_string())?;
        assert!(protective.protective);
        assert!(!protective.opening);

        let opening = working_order_of(&the_order(mandate_executor::Purpose::Open)?, today)
            .map_err(|e| e.to_string())?;
        assert!(!opening.protective);
        assert!(opening.opening);
        assert_eq!(
            opening.open_qty,
            mandate_num::Qty::parse("4").map_err(|e| e.to_string())?
        );
        Ok(())
    }

    /// §5.5 permits only ownerless protective legs and watchdog risk exits to join the
    /// deciding agent's cancels. An ownerless opening and another agent's protection remain
    /// outside the agent-scoped flatten.
    #[test]
    fn only_ownerless_protection_and_watchdog_exits_join_the_agent_flatten() -> Result<(), String> {
        let mut ownerless_protective = the_order(mandate_executor::Purpose::Protective)?;
        ownerless_protective.client_order_id =
            mandate_executor::ClientOrderId::parse("md-broker-protective")
                .map_err(|e| e.to_string())?;
        ownerless_protective.agent = None;
        let mut ownerless_watchdog = the_order(mandate_executor::Purpose::RiskExit)?;
        ownerless_watchdog.client_order_id =
            mandate_executor::ClientOrderId::parse("md-w-01J8Z3M1Q0000000000000000W")
                .map_err(|e| e.to_string())?;
        ownerless_watchdog.agent = None;
        let mut executor_sentinel_watchdog = the_order(mandate_executor::Purpose::RiskExit)?;
        executor_sentinel_watchdog.client_order_id =
            mandate_executor::ClientOrderId::parse("md-w-01J8Z3M1Q0000000000000000S")
                .map_err(|e| e.to_string())?;
        executor_sentinel_watchdog.agent = Some(mandate_executor::AgentId("*".to_owned()));
        let mut other_agent = the_order(mandate_executor::Purpose::Protective)?;
        other_agent.agent = Some(mandate_executor::AgentId("agent-b".to_owned()));
        let mut ownerless_opening = the_order(mandate_executor::Purpose::Open)?;
        ownerless_opening.agent = None;
        assert!(is_unattributed_exit(&ownerless_protective));
        assert!(is_unattributed_exit(&ownerless_watchdog));
        assert!(is_unattributed_exit(&executor_sentinel_watchdog));
        assert!(!is_unattributed_exit(&other_agent));
        assert!(!is_unattributed_exit(&ownerless_opening));
        Ok(())
    }

    /// One resting executor order of the fixture's instrument: four to sell, none filled.
    fn the_order(purpose: mandate_executor::Purpose) -> Result<ExecutorOrder, String> {
        Ok(ExecutorOrder {
            client_order_id: mandate_executor::ClientOrderId::parse("md-resting")
                .map_err(|e| e.to_string())?,
            intent_id: None,
            agent: Some(mandate_executor::AgentId("agent-a".to_owned())),
            instrument: InstrumentId::new("b0b6dd9d-8b9b-48a9-ba46-b9d54906e415")
                .map_err(|e| e.to_string())?,
            side: Side::Sell,
            qty: mandate_num::Qty::parse("4").map_err(|e| e.to_string())?,
            filled_qty: mandate_num::Qty::parse("0").map_err(|e| e.to_string())?,
            state: mandate_executor::OrderState::Submitting,
            attempt: 1,
            purpose,
            absent_lookups: 0,
            first_absence_at: None,
            cancel_unconfirmed: false,
            replaced_by: None,
            created_on: None,
        })
    }

    use mandate_accounting::{InstrumentId, Side};
    use mandate_executor::{
        BrokerRequest, ClientOrderId, ConnectorError, EventId, IntentId, OrderType, Purpose,
        SubmitOrder, TimeInForce,
    };

    use super::{Disconnected, MovingAverage, StoredBars, split_inside, trusted};
    use crate::error::Cause;
    use crate::stages::{Bars, ModelRef, SignalModel};

    /// A scratch directory, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Result<Scratch, String> {
            let path = std::env::temp_dir().join(format!(
                "mandate-shell-adapters-{name}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            Ok(Scratch(path))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn day(text: &str) -> Result<Date, String> {
        Date::parse(text).map_err(|e| e.to_string())
    }

    fn as_of() -> Result<UtcNanos, String> {
        UtcNanos::parse("2026-09-25T20:00:00.000000000Z").map_err(|error| error.to_string())
    }

    fn dataset(symbol: &str, timeframe: &str) -> Result<DatasetId, String> {
        DatasetId::new(
            AssetClass::UsEquity,
            Feed::Iex,
            Kind::Bars(timeframe.parse().map_err(|e| format!("{e:?}"))?),
            Symbol::parse(symbol).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }

    fn bar(on: Date, close: &str) -> Result<Bar, String> {
        let dec = |text: &str| DecStr::parse(text).map_err(|e| e.to_string());
        Ok(Bar {
            start: UtcNanos::parse_rfc3339(&format!("{on}T04:00:00Z"))
                .map_err(|e| e.to_string())?,
            open: dec(close)?,
            high: dec(close)?,
            low: dec(close)?,
            close: dec(close)?,
            volume: dec("50000000")?,
            vwap: dec(close)?,
            trade_count: 400_000,
        })
    }

    /// One daily bar per weekday from 2026-08-24, a Monday, with these closes; `skip` names
    /// weekdays given no partition at all, and `empty` weekdays listed with no bar.
    fn write(
        root: &Path,
        id: &DatasetId,
        closes: &[&str],
        skip: &[&str],
        empty: &[&str],
    ) -> Result<PathBuf, String> {
        let store = Store::new(root);
        let mut on = day("2026-08-24")?;
        for close in closes {
            while on.is_weekend()
                || skip.contains(&on.to_string().as_str())
                || empty.contains(&on.to_string().as_str())
            {
                if empty.contains(&on.to_string().as_str()) {
                    store
                        .put_day(id, on, &Records::Bars(Vec::new()))
                        .map_err(|e| e.to_string())?;
                }
                on = on.next().map_err(|e| e.to_string())?;
            }
            store
                .put_day(id, on, &Records::Bars(vec![bar(on, close)?]))
                .map_err(|e| e.to_string())?;
            on = on.next().map_err(|e| e.to_string())?;
        }
        Ok(store.dataset_dir(id))
    }

    fn rising() -> Vec<String> {
        (231..256).map(|whole| format!("{whole}.20")).collect()
    }

    fn aapl(root: &Path, skip: &[&str], empty: &[&str]) -> Result<PathBuf, String> {
        let closes = rising();
        let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
        write(root, &dataset("AAPL", "1Day")?, &closes, skip, empty)
    }

    fn untrusted(answer: Result<Vec<Price>, Cause>) -> Result<&'static str, String> {
        match answer {
            Err(Cause::Untrusted { what }) => Ok(what),
            other => Err(format!("expected an untrusted refusal, got {other:?}")),
        }
    }

    /// Storage order and time order differ: the partitions are written newest day first, so only
    /// a read in the manifest's date order answers oldest first.
    #[test]
    fn stored_closes_are_read_oldest_first_exactly_as_stored() -> Result<(), String> {
        let scratch = Scratch::new("read")?;
        let id = dataset("AAPL", "1Day")?;
        let store = Store::new(&scratch.0);
        let closes = rising();
        let mut days = Vec::new();
        let mut on = day("2026-08-24")?;
        while days.len() < closes.len() {
            if !on.is_weekend() {
                days.push(on);
            }
            on = on.next().map_err(|e| e.to_string())?;
        }
        for (on, close) in days.iter().zip(&closes).rev() {
            store
                .put_day(&id, *on, &Records::Bars(vec![bar(*on, close)?]))
                .map_err(|e| e.to_string())?;
        }
        store
            .put_day(&id, day("2026-08-29")?, &Records::Bars(Vec::new()))
            .map_err(|e| e.to_string())?;
        let read = StoredBars {
            dir: store.dataset_dir(&id),
        }
        .closes("AAPL", as_of()?)
        .map_err(|e| e.to_string())?;
        let expected: Vec<String> = closes.iter().map(|c| canonical(c)).collect();
        let read: Vec<String> = read.iter().map(ToString::to_string).collect();
        assert_eq!(read, expected);
        Ok(())
    }

    #[test]
    fn stored_bars_must_end_on_the_last_completed_equity_session() -> Result<(), String> {
        let scratch = Scratch::new("stale")?;
        let dir = aapl(&scratch.0, &[], &[])?;
        let before_monday_close =
            UtcNanos::parse("2026-09-28T19:59:59.000000000Z").map_err(|e| e.to_string())?;
        let fresh = StoredBars { dir: dir.clone() }
            .closes("AAPL", before_monday_close)
            .map_err(|e| e.to_string())?;
        assert_eq!(
            fresh.len(),
            25,
            "Friday remains the last completed session until Monday's regular close"
        );
        let after_monday_close =
            UtcNanos::parse("2026-09-28T20:00:00.000000000Z").map_err(|e| e.to_string())?;
        let what = untrusted(StoredBars { dir }.closes("AAPL", after_monday_close))?;
        assert_eq!(
            what,
            "the dataset does not end on the last completed equity session"
        );
        Ok(())
    }

    /// Review round 1, major 2: two bars on one listed day, newest first, would put the closes out
    /// of time order and make the windows span fewer days than the envelope names.
    #[test]
    fn a_listed_day_holding_more_than_one_bar_is_refused() -> Result<(), String> {
        let scratch = Scratch::new("two-bars")?;
        let dir = aapl(&scratch.0, &["2026-08-26"], &[])?;
        let on = day("2026-08-26")?;
        let at = |hour: &str, close: &str| -> Result<Bar, String> {
            Ok(Bar {
                start: UtcNanos::parse_rfc3339(&format!("{on}T{hour}:00:00Z"))
                    .map_err(|e| e.to_string())?,
                ..bar(on, close)?
            })
        };
        Store::new(&scratch.0)
            .put_day(
                &dataset("AAPL", "1Day")?,
                on,
                &Records::Bars(vec![at("09", "999.99")?, at("04", "111.11")?]),
            )
            .map_err(|e| e.to_string())?;
        let after_last_listed =
            UtcNanos::parse("2026-09-28T20:00:00.000000000Z").map_err(|e| e.to_string())?;
        let what = untrusted(StoredBars { dir }.closes("AAPL", after_last_listed))?;
        assert_eq!(what, "a listed day does not hold exactly one bar");
        Ok(())
    }

    /// Review round 1, major 3: the only connector the shipped binary holds refuses every request
    /// before it leaves the process.
    #[test]
    fn the_disconnected_connector_sends_nothing() -> Result<(), String> {
        let submit = BrokerRequest::Submit(SubmitOrder {
            client_order_id: ClientOrderId::for_intent(&IntentId(EventId(
                "01KFDZ3GXS0000000000000000".to_owned(),
            )))
            .map_err(|e| e.to_string())?,
            instrument: InstrumentId::new("b0b6dd9d-8b9b-48a9-ba46-b9d54906e415")
                .map_err(|e| e.to_string())?,
            side: Side::Buy,
            qty: Qty::parse("1").map_err(|e| e.to_string())?,
            order_type: OrderType::Limit,
            tif: TimeInForce::Day,
            limit_price: Some(Price::parse("255.2").map_err(|e| e.to_string())?),
            stop_price: None,
            bracket: None,
            oco: None,
            extended_hours: false,
            purpose: Purpose::Open,
        });
        for request in [
            submit,
            BrokerRequest::GetAccount,
            BrokerRequest::ListPositions,
        ] {
            assert_eq!(
                Disconnected.call(&request),
                Err(ConnectorError::NotSent {
                    code: "no_transport"
                })
            );
        }
        Ok(())
    }

    #[test]
    fn another_instruments_bars_are_refused() -> Result<(), String> {
        let scratch = Scratch::new("symbol")?;
        let dir = aapl(&scratch.0, &[], &[])?;
        let what = untrusted(StoredBars { dir }.closes("MSFT", as_of()?))?;
        assert_eq!(what, "the dataset is another instrument's");
        Ok(())
    }

    #[test]
    fn bars_that_are_not_daily_are_refused() -> Result<(), String> {
        let scratch = Scratch::new("hourly")?;
        let dir = write(
            &scratch.0,
            &dataset("AAPL", "1Hour")?,
            &["231.20"],
            &[],
            &[],
        )?;
        let what = untrusted(StoredBars { dir }.closes("AAPL", as_of()?))?;
        assert_eq!(what, "the dataset is not daily bars");
        Ok(())
    }

    #[test]
    fn an_altered_partition_is_refused() -> Result<(), String> {
        let scratch = Scratch::new("altered")?;
        let dir = aapl(&scratch.0, &[], &[])?;
        let path = dir.join(partition_name(day("2026-08-26")?));
        let mut bytes = fs::read(&path).map_err(|e| e.to_string())?;
        bytes.push(0);
        fs::write(&path, bytes).map_err(|e| e.to_string())?;
        let what = untrusted(StoredBars { dir }.closes("AAPL", as_of()?))?;
        assert_eq!(what, "a partition cannot be trusted");
        Ok(())
    }

    #[test]
    fn two_bars_with_one_start_are_refused() -> Result<(), String> {
        let scratch = Scratch::new("duplicate")?;
        let dir = aapl(&scratch.0, &["2026-08-26"], &[])?;
        let on = day("2026-08-26")?;
        Store::new(&scratch.0)
            .put_day(
                &dataset("AAPL", "1Day")?,
                on,
                &Records::Bars(vec![bar(on, "233.20")?, bar(on, "233.30")?]),
            )
            .map_err(|e| e.to_string())?;
        let what = untrusted(StoredBars { dir }.closes("AAPL", as_of()?))?;
        assert_eq!(what, "two bars share a start");
        Ok(())
    }

    #[test]
    fn a_trading_day_listed_without_a_bar_is_refused() -> Result<(), String> {
        let scratch = Scratch::new("empty")?;
        let dir = aapl(&scratch.0, &[], &["2026-08-26"])?;
        let what = untrusted(StoredBars { dir }.closes("AAPL", as_of()?))?;
        assert_eq!(what, "a trading day has no bar");
        Ok(())
    }

    #[test]
    fn a_trading_day_never_fetched_is_refused() -> Result<(), String> {
        let scratch = Scratch::new("gap")?;
        let dir = aapl(&scratch.0, &["2026-08-26"], &[])?;
        let what = untrusted(StoredBars { dir }.closes("AAPL", as_of()?))?;
        assert_eq!(what, "a trading day was never fetched");
        Ok(())
    }

    #[test]
    fn a_split_inside_the_span_or_actions_short_of_it_are_refused() -> Result<(), String> {
        let scratch = Scratch::new("split")?;
        let dir = aapl(&scratch.0, &[], &[])?;
        let mut inspection = inspect::inspect(&dir).map_err(|e| e.to_string())?;
        assert!(trusted(&inspection, "AAPL", as_of()?).is_ok());
        let span = inspection.coverage.span.ok_or("no span")?;
        let symbol = Symbol::parse("AAPL").map_err(|e| e.to_string())?;
        let split = |ex_date: Date| -> Result<Split, String> {
            Ok(Split {
                id: "split".to_owned(),
                ex_date,
                ratio: SplitRatio::new(4, 1).map_err(|e| e.to_string())?,
            })
        };
        let with = |splits: Vec<Split>| RecordedActions {
            range: span,
            actions: CorporateActions {
                splits,
                ..CorporateActions::none(symbol.clone())
            },
        };
        let cases = [
            (span.first(), false),
            (span.first().next().map_err(|e| e.to_string())?, true),
            (span.last(), true),
            (span.last().next().map_err(|e| e.to_string())?, false),
        ];
        for (ex_date, refused) in cases {
            inspection.corporate_actions = ActionsReport::Applied {
                recorded: with(vec![split(ex_date)?]),
                as_of: span.last(),
            };
            assert_eq!(split_inside(&inspection), refused, "{ex_date}");
        }
        inspection.corporate_actions = ActionsReport::Applied {
            recorded: with(Vec::new()),
            as_of: span.last(),
        };
        assert!(!split_inside(&inspection));
        inspection.corporate_actions = ActionsReport::Incomplete(with(Vec::new()));
        assert!(split_inside(&inspection));
        assert!(matches!(
            trusted(&inspection, "AAPL", as_of()?),
            Err(Cause::Untrusted {
                what: "a split lies inside the span, or the corporate actions do not cover it"
            })
        ));
        inspection.corporate_actions = ActionsReport::Applied {
            recorded: with(vec![split(span.last())?]),
            as_of: span.last(),
        };
        inspection.coverage.span = None;
        assert!(!split_inside(&inspection));
        Ok(())
    }

    #[test]
    fn stored_actions_covering_the_span_with_no_split_are_trusted() -> Result<(), String> {
        let scratch = Scratch::new("actions")?;
        let dir = aapl(&scratch.0, &[], &[])?;
        let span =
            DayRange::new(day("2026-08-24")?, day("2026-09-25")?).map_err(|e| e.to_string())?;
        let symbol = Symbol::parse("AAPL").map_err(|e| e.to_string())?;
        write_actions(
            &dir,
            &RecordedActions {
                range: span,
                actions: CorporateActions::none(symbol),
            },
        )
        .map_err(|e| e.to_string())?;
        let closes = StoredBars { dir }
            .closes("AAPL", as_of()?)
            .map_err(|e| e.to_string())?;
        assert_eq!(closes.len(), 25);
        Ok(())
    }

    fn model(id: &str, params: &[(&str, &str)]) -> ModelRef {
        ModelRef {
            id: id.to_owned(),
            version: "1.0.0".to_owned(),
            content_hash: Digest::of(format!("{id}:1.0.0").as_bytes()),
            max_output_age_s: 86_400,
            params: params
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    /// `mandate-num`'s canonical text of a decimal: trailing fractional zeros and a bare point
    /// dropped, and nothing else, so `"100"` stays `"100"` (#248 review, minor 2).
    fn canonical(text: &str) -> String {
        match text.split_once('.') {
            Some((whole, places)) => match places.trim_end_matches('0') {
                "" => whole.to_owned(),
                kept => format!("{whole}.{kept}"),
            },
            None => text.to_owned(),
        }
    }

    /// The closes as prices, in canonical text (`Price` refuses a trailing fractional zero).
    fn prices(closes: &[String]) -> Result<Vec<Price>, String> {
        closes
            .iter()
            .map(|close| Price::parse(&canonical(close)).map_err(|e| e.to_string()))
            .collect()
    }

    /// #248 review, minor 2: a close whose digits end in 0 is read as itself. The old helper
    /// trimmed every trailing `0`, so `"100"` became `"1"`.
    #[test]
    fn a_close_ending_in_zero_keeps_its_value() -> Result<(), String> {
        let closes = ["100", "250.50", "1000.000", "0.10", "99.9"].map(str::to_owned);
        let read: Vec<String> = prices(&closes)?.iter().map(ToString::to_string).collect();
        assert_eq!(read, ["100", "250.5", "1000", "0.1", "99.9"]);
        Ok(())
    }

    #[test]
    fn the_crossover_reads_the_windows_the_envelope_states() -> Result<(), String> {
        let windows = [("fast_periods", "5"), ("slow_periods", "20")];
        let crossover = model("quant.ma_crossover", &windows);
        let up = prices(&rising())?;
        let mut down = up.clone();
        down.reverse();
        let answer = |closes: &[Price]| MovingAverage.signal(&crossover, closes);
        assert!(matches!(answer(&up), Ok(Signal::Long)), "{:?}", answer(&up));
        assert!(matches!(answer(&down), Ok(Signal::Flat)));
        let short = up.get(..19).ok_or("closes")?;
        assert!(matches!(answer(short), Ok(Signal::Undecided)));
        let exactly = up.get(..20).ok_or("closes")?;
        assert!(matches!(answer(exactly), Ok(Signal::Long)));
        let crossed = model(
            "quant.ma_crossover",
            &[("fast_periods", "20"), ("slow_periods", "5")],
        );
        assert!(matches!(
            MovingAverage.signal(&crossed, &up),
            Err(Cause::Backtest(BacktestError::StrategyWindowsCrossed))
        ));
        Ok(())
    }

    /// #241 review round 2, minor 1: the fast window is the one the envelope states. Over these 20
    /// closes (14 at 101, one at 0.01, five at 100.5) the slow average is 95.8255: the last 5
    /// average 100.5, above it, but the last 6 average 83.7517 and the last 15 average 94.1007,
    /// both below it. So a fast window of 5 says `Long` and one of 6 or 15 says `Flat`.
    #[test]
    fn the_fast_window_is_the_one_the_envelope_states() -> Result<(), String> {
        let mut closes = vec!["101".to_owned(); 14];
        closes.push("0.01".to_owned());
        closes.extend(vec!["100.5".to_owned(); 5]);
        let closes = prices(&closes)?;
        let with = |fast: &str| {
            MovingAverage.signal(
                &model(
                    "quant.ma_crossover",
                    &[("fast_periods", fast), ("slow_periods", "20")],
                ),
                &closes,
            )
        };
        assert!(matches!(with("5"), Ok(Signal::Long)), "{:?}", with("5"));
        assert!(matches!(with("6"), Ok(Signal::Flat)), "{:?}", with("6"));
        assert!(matches!(with("15"), Ok(Signal::Flat)), "{:?}", with("15"));
        Ok(())
    }

    /// #241 review round 2, minor 3: a partition on disk that the manifest does not list is never
    /// read. `inspect` reports it as unlisted, so the whole dataset is refused before any partition
    /// is read, and the stray close cannot reach the windows.
    #[test]
    fn a_partition_the_manifest_does_not_list_is_never_read() -> Result<(), String> {
        let scratch = Scratch::new("unlisted")?;
        let dir = aapl(&scratch.0, &[], &[])?;
        let listed = dir.join(partition_name(day("2026-08-26")?));
        let stray = dir.join(partition_name(day("2026-08-29")?));
        fs::copy(&listed, &stray).map_err(|e| e.to_string())?;
        let problems = inspect::inspect(&dir).map_err(|e| e.to_string())?.problems;
        assert_eq!(
            problems,
            vec![Problem::Unlisted {
                file: "2026-08-29.parquet".to_owned()
            }],
            "the one problem `inspect` finds is the unlisted partition (#248 review, minor 1)"
        );
        let what = untrusted(StoredBars { dir }.closes("AAPL", as_of()?))?;
        assert_eq!(what, "a partition cannot be trusted");
        Ok(())
    }

    /// #248 review, minor 1: the loop in `closes` walks the manifest, not the directory. Called
    /// past `trusted`, over a directory holding an unlisted partition with a close of its own,
    /// it reads the listed closes and never the stray.
    #[test]
    fn the_closes_are_read_from_the_manifest_and_never_the_directory() -> Result<(), String> {
        let scratch = Scratch::new("manifest-only")?;
        let dir = aapl(&scratch.0, &[], &[])?;
        let elsewhere = Scratch::new("manifest-only-stray")?;
        let stray = write(&elsewhere.0, &dataset("AAPL", "1Day")?, &["1.23"], &[], &[])?;
        fs::copy(
            stray.join(partition_name(day("2026-08-24")?)),
            dir.join(partition_name(day("2026-08-29")?)),
        )
        .map_err(|e| e.to_string())?;
        let daily = Kind::Bars(Timeframe::new(1, TimeUnit::Day).map_err(|e| e.to_string())?);
        let read: Vec<String> = StoredBars { dir }
            .listed_closes(daily)
            .map_err(|e| e.to_string())?
            .iter()
            .map(ToString::to_string)
            .collect();
        let expected: Vec<String> = rising().iter().map(|c| canonical(c)).collect();
        assert_eq!(read, expected, "the stray 1.23 is never read");
        Ok(())
    }

    #[test]
    fn another_model_or_an_unstated_window_is_refused() -> Result<(), String> {
        let up = prices(&rising())?;
        let refused = |model: ModelRef| {
            matches!(MovingAverage.signal(&model, &up), Err(Cause::Absent { .. }))
        };
        let windows = [("fast_periods", "5"), ("slow_periods", "20")];
        assert!(refused(model("quant.other", &windows)));
        assert!(refused(model(
            "quant.ma_crossover",
            &[("slow_periods", "20")]
        )));
        assert!(refused(model(
            "quant.ma_crossover",
            &[("fast_periods", "5")]
        )));
        assert!(refused(model(
            "quant.ma_crossover",
            &[("fast_periods", "five"), ("slow_periods", "20")]
        )));
        Ok(())
    }

    #[test]
    fn classification_order_facts_must_match_the_sized_proposal() -> Result<(), String> {
        let one_share = Usd::parse("255.2").map_err(|e| e.to_string())?;
        let limit = Price::parse("255.2").map_err(|e| e.to_string())?;
        assert!(
            action_order_matches(
                one_share,
                Qty::parse("1").map_err(|e| e.to_string())?,
                limit
            )
            .map_err(|e| e.to_string())?
        );
        assert!(
            !action_order_matches(
                one_share,
                Qty::parse("2").map_err(|e| e.to_string())?,
                limit
            )
            .map_err(|e| e.to_string())?,
            "one-share autonomy facts cannot classify a two-share proposal"
        );
        Ok(())
    }

    #[test]
    fn the_advisory_order_copies_only_proposal_fields_from_the_runtime() -> Result<(), String> {
        let proposal = mandate_runtime::Proposal {
            instrument: mandate_accounting::InstrumentId::new(
                "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415",
            )
            .map_err(|e| e.to_string())?,
            asset_class: mandate_accounting::AssetClass::UsEquity,
            side: mandate_accounting::Side::Buy,
            qty: Qty::parse("3").map_err(|e| e.to_string())?,
            limit: Price::parse("12.34").map_err(|e| e.to_string())?,
            purpose: RuntimePurpose::Open,
            exit_origin: None,
            exit_conviction: Some(Value::Str("1".to_owned())),
            buy_conviction: Some(Value::Str("1".to_owned())),
            combined_score: Value::Str("0.8".to_owned()),
            outputs_used: BTreeSet::new(),
            model_weights: BTreeMap::new(),
            clips_applied: Vec::new(),
            execution: None,
        };
        let facts = AdvisoryOrderFacts {
            kind: mandate_risk::ProposedKind::Bracket {
                take_profit: Price::parse("15").map_err(|e| e.to_string())?,
                stop: Price::parse("10").map_err(|e| e.to_string())?,
            },
            tif: mandate_risk::TimeInForce::Gtc,
            extended_hours: true,
            origin: mandate_risk::Origin::OrderBuilder,
            owner_confirmed_bid: Some(Price::parse("12").map_err(|e| e.to_string())?),
            client_order_id: mandate_risk::ClientOrderId(71),
            fee_reservation: mandate_num::Usd::parse("1.23").map_err(|e| e.to_string())?,
        };
        let mapped = proposed_order(&proposal, &facts).map_err(|e| e.to_string())?;
        assert_eq!(mapped.instrument.as_str(), proposal.instrument.as_str());
        assert_eq!(mapped.side, proposal.side);
        assert_eq!(mapped.qty, proposal.qty);
        assert_eq!(mapped.limit_price, proposal.limit);
        assert_eq!(mapped.kind, facts.kind);
        assert_eq!(mapped.tif, facts.tif);
        assert_eq!(mapped.extended_hours, facts.extended_hours);
        assert_eq!(mapped.origin, facts.origin);
        assert_eq!(mapped.owner_confirmed_bid, facts.owner_confirmed_bid);
        assert_eq!(mapped.client_order_id, facts.client_order_id);
        assert_eq!(mapped.fee_reservation, facts.fee_reservation);
        Ok(())
    }

    #[test]
    fn decision_adapters_refuse_without_injected_context() -> Result<(), String> {
        let proposal = mandate_runtime::Proposal {
            instrument: mandate_accounting::InstrumentId::new(
                "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415",
            )
            .map_err(|e| e.to_string())?,
            asset_class: mandate_accounting::AssetClass::UsEquity,
            side: mandate_accounting::Side::Buy,
            qty: Qty::parse("1").map_err(|e| e.to_string())?,
            limit: Price::parse("12.34").map_err(|e| e.to_string())?,
            purpose: RuntimePurpose::Open,
            exit_origin: None,
            exit_conviction: Some(Value::Str("1".to_owned())),
            buy_conviction: Some(Value::Str("1".to_owned())),
            combined_score: Value::Str("1".to_owned()),
            outputs_used: BTreeSet::new(),
            model_weights: BTreeMap::new(),
            clips_applied: Vec::new(),
            execution: None,
        };
        assert!(matches!(
            RiskGate { context: None }.evaluate(&proposal),
            Err(Cause::Absent {
                what: "the trusted run context"
            })
        ));
        Ok(())
    }

    #[test]
    fn the_advisory_adapter_calls_the_real_gate_with_the_mapped_order() -> Result<(), String> {
        let asset = mandate_risk::AssetId::new("b0b6dd9d-8b9b-48a9-ba46-b9d54906e415")
            .map_err(|e| e.to_string())?;
        let zero_fraction = mandate_num::Fraction::parse("0").map_err(|e| e.to_string())?;
        let gate = AdvisoryGateContext {
            now: UtcNanos::parse("2026-09-25T20:00:00.000000000Z").map_err(|e| e.to_string())?,
            pass: mandate_risk::GatePass::First,
            config: mandate_risk::GateConfig {
                price_floor: mandate_num::Usd::ZERO,
                liquidity_floor_usd: mandate_num::Usd::ZERO,
                crypto_liquidity_floor_usd: mandate_num::Usd::ZERO,
                collar_liquid_threshold_usd: mandate_num::Usd::ZERO,
                collar_liquid_x: zero_fraction,
                collar_other_x: zero_fraction,
                collar_crypto_x: zero_fraction,
                collar_passive_band: zero_fraction,
                opposite_fill_interval_s: 0,
                min_resting_time_s: 0,
                order_to_fill_max: 0,
                order_to_fill_min_orders: 0,
                order_size_participation: zero_fraction,
                daily_participation: zero_fraction,
                close_window_minutes: 0,
                legacy_pdt_equity_threshold: mandate_num::Usd::ZERO,
                etp_classification_max_age_s: 0,
            },
            mandate: mandate_risk::ValidatedMandate::from_validated_parts(
                mandate_risk::spec_types::RiskLimits {
                    max_position_usd: mandate_num::Usd::ZERO,
                    max_position_fraction: zero_fraction,
                    max_order_usd: mandate_num::Usd::ZERO,
                    max_gross_exposure_usd: mandate_num::Usd::ZERO,
                    max_orders_per_day: 0,
                    reentry_cooldown_s: 0,
                    rebalance_band: zero_fraction,
                    breach_confirm_s: 0,
                    drawdown_ladder: Vec::new(),
                },
                mandate_risk::spec_types::GoalState::Running,
                false,
                false,
            ),
            risk: mandate_risk::RiskSnapshot {
                agent_equity: mandate_num::Usd::ZERO,
                high_water_mark: mandate_num::Usd::ZERO,
                day_start_equity: mandate_num::Usd::ZERO,
                capital_base: mandate_num::Usd::ZERO,
                inherited_loss: mandate_num::Usd::ZERO,
                latched: BTreeSet::new(),
                active_rungs: BTreeMap::new(),
                size_factor: mandate_num::Ratio::parse("1").map_err(|e| e.to_string())?,
                agent_mode: mandate_risk::AgentMode::Normal,
            },
            account: mandate_risk::AccountSnapshot {
                account_type: mandate_risk::AccountType::Cash,
                state: mandate_risk::AccountState::Active,
                crypto_active: false,
                regime: mandate_risk::DayTradeRegime::LegacyPdt,
                equity: mandate_num::Usd::ZERO,
                prior_close_equity: mandate_num::Usd::ZERO,
                model_buying_power: mandate_num::Usd::ZERO,
                broker_buying_power: mandate_num::Usd::ZERO,
                broker_non_marginable_buying_power: mandate_num::Usd::ZERO,
                positions: BTreeMap::new(),
                market_values: BTreeMap::new(),
                working_orders: BTreeMap::new(),
                unknown_orders: BTreeSet::new(),
                related_account_resting: BTreeMap::new(),
            },
            agent: mandate_risk::AgentSnapshot {
                agent: mandate_risk::AgentId(1),
                mode: mandate_risk::AgentMode::Normal,
                instrument_restrictions: BTreeMap::new(),
                positions: BTreeMap::new(),
                market_values: BTreeMap::new(),
                working_orders: BTreeSet::new(),
                instrument_groups: BTreeMap::new(),
                last_exit_fill_at: BTreeMap::new(),
                orders_today: 0,
                day_trades: mandate_risk::DayTradeLedger::default(),
            },
            instrument: mandate_risk::InstrumentSnapshot {
                instrument: asset,
                asset_class: mandate_risk::AssetClass::UsEquity,
                exchange: None,
                status_active: false,
                tradable: false,
                fractionable: false,
                ipo: false,
                ptp_no_exception: false,
                etp: mandate_risk::EtpClass::Plain,
                etp_classified_at: None,
                quote_currency: None,
                prior_close: None,
                median_dollar_volume_20d: None,
                median_dollar_volume_30d: None,
                min_order_size: Qty::ZERO,
                qty_increment: Qty::parse("1").map_err(|e| e.to_string())?,
                halted: false,
                status_feed_current: false,
            },
            market: mandate_risk::MarketSnapshot {
                quote: None,
                last_trade: None,
                trailing_5m_volume: None,
                adv_20d: None,
            },
            conduct: mandate_risk::ConductState::default(),
            universe: mandate_risk::WorkingUniverse::Unavailable,
            order: AdvisoryOrderFacts {
                kind: mandate_risk::ProposedKind::Plain,
                tif: mandate_risk::TimeInForce::Day,
                extended_hours: false,
                origin: mandate_risk::Origin::OrderBuilder,
                owner_confirmed_bid: None,
                client_order_id: mandate_risk::ClientOrderId(1),
                fee_reservation: mandate_num::Usd::ZERO,
            },
        };
        let context = RunContext {
            validation: mandate_spec::ValidationContext {
                account_equity_usd: mandate_num::Usd::ZERO,
                other_allocations_usd: mandate_num::Usd::ZERO,
                validation_date: Date::parse("2026-09-25").map_err(|e| e.to_string())?,
                registry: None,
                provenance: mandate_spec::document::ProvenanceMap::default(),
                workspace_users: 0,
                approver_users: 0,
                independent_approval_required: false,
                disclosures_accepted: BTreeSet::new(),
                instrument_groups: BTreeMap::new(),
                claimed_by_other_agents: BTreeSet::new(),
                connection_environment: None,
                connection_loss_carry_usd: mandate_num::Usd::ZERO,
                eligibility_failures: BTreeSet::new(),
                previous_version: None,
                current_mandate_version: None,
            },
            policies: Vec::new(),
            author: String::new(),
            restricted_instruments: BTreeSet::new(),
            decision: Some(DecisionContext {
                builder: None,
                gate: Some(gate),
            }),
            governance: None,
        };
        let proposal = mandate_runtime::Proposal {
            instrument: mandate_accounting::InstrumentId::new(
                "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415",
            )
            .map_err(|e| e.to_string())?,
            asset_class: mandate_accounting::AssetClass::UsEquity,
            side: mandate_accounting::Side::Buy,
            qty: Qty::ZERO,
            limit: Price::parse("12.34").map_err(|e| e.to_string())?,
            purpose: RuntimePurpose::Open,
            exit_origin: None,
            exit_conviction: Some(Value::Str("1".to_owned())),
            buy_conviction: Some(Value::Str("1".to_owned())),
            combined_score: Value::Str("1".to_owned()),
            outputs_used: BTreeSet::new(),
            model_weights: BTreeMap::new(),
            clips_applied: Vec::new(),
            execution: None,
        };
        assert!(matches!(
            RiskGate {
                context: Some(Rc::new(context))
            }
            .evaluate(&proposal),
            Err(Cause::Gate(mandate_risk::GateError::ZeroQuantity))
        ));
        Ok(())
    }
}
