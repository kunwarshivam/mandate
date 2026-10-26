//! The production adapters: one per [`Stage`](crate::Stage), each binding one crate's public API.
//!
//! The implementation lands one slice at a time (DEC-77 stage 3, DEC-166). **Live** in slice 1:
//! [`AlpacaConnector`], whose upstream is fully implemented and whose every input the shell
//! holds; since DEC-449's flip also [`RiskExitPath`], which probes
//! and plans over the journal, stream, clock, and agent the run's bridge hands it; and
//! [`ExecutorProtection`], through the executor's public protection probe. The journal, sink,
//! executor, reconciliation, mandate validation, sizing, classification, and advisory-gate
//! boundaries are also live when their trusted inputs are injected. The paper adapter
//! injects the one-run paper contexts [`crate::paper`] assembles from its GET-only broker preflight
//! and the stored datasets (DEC-466, DEC-470); callers that omit either context fail closed rather
//! than inventing effective-dated inputs.
//!
//! What each binds, per the task brief's step table:
//!
//! | Adapter | Binds |
//! |---|---|
//! | [`RiskExitPath`] | `mandate_risk::agent_flatten` (E6-3) |
//! | [`ExecutorProtection`] | `mandate_executor::is_protected` (E7-4) |
//! | [`SpecMandate`] | `mandate_spec::validate`, then `ValidatedMandate::new` (stream F) |
//! | [`BuilderPlan`] | `mandate-builder`'s sizing and classification (stream H) |
//! | [`RiskGate`] | `mandate_risk::evaluate` as a dry run (E6-3, E6-6 to E6-8) |
//! | [`StoreJournal`] | `mandate_journal::MemoryJournal` in CI, `mandate-journal-pg` for the manual run |
//! | [`ExecutorSink`] | the two `IntentHandoff` types, joined with the deployment's `AgentId` |
//! | [`CoreExecutor`] | `mandate_executor::handle` and `fold` (stream K) |
//! | [`AlpacaConnector`] | `mandate_alpaca::TradingClient` over a `TradingTransport` (stream K) |
//! | [`ExecutorReconciler`] | `mandate_executor::reconcile` on the connector's snapshot (E7-3) |
//!
//! The stored daily figures and typed closes the run trusts are read by [`crate::paper`] through
//! `trusted_daily_liquidity` and `trusted_daily_closes`, `mandate_bars`'s one trust rule (DEC-2385,
//! DEC-2274)
//! over `mandate_marketdata`'s datasets (E2-1, E2-2), not by a stage: the model host owns every signal, and the
//! shell computes none (DEC-475 items 1 and 6, DEC-503).

use std::cell::{Ref, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::mem;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_alpaca::{RetryPolicy, TokioPause, TradingClient, TradingTransport};
use mandate_canon::{Key, Object, Value};
use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{
    AccountRef, AccountScope, AgentId as ExecutorAgentId, BindingGateSource, BrokerConnector,
    BrokerOutcome, BrokerRequest, ConnectorError, EventId, ExecutorConfig, ExecutorState,
    FoldedEvent, IdGen, InstrumentSnapshot, MandateVersion, MandateView as ExecutorMandateView,
    Order as ExecutorOrder, Ports, Seq, WorkspaceId, WriterEpoch, fold, is_protected,
    paper_only_fee_config,
};
use mandate_journal::{
    AppendOutcome, ArtifactRef, ArtifactSource, ArtifactStore, MemoryJournal, StoredEvent, StreamId,
};
use mandate_journal_pg::PgJournal;
use mandate_liquidity::DailyLiquidity;
use mandate_num::{Fraction, Price, Qty, ShareIncrement, SignedQty, Usd};
use mandate_risk::{Decision, FlattenInitiator, FlattenInput, agent_flatten};
use mandate_robinhood::Tools;
use mandate_runtime::{
    AgentId, Classified, FlattenLeg, FlattenPlan, FlattenRequest, Initiator, IntentBody,
    IntentHandoff, LiveScope, MandateView, OrderExecution, Proposal, Purpose, RiskClock,
    SignalInputs, TimeInForce,
};
use mandate_spec::policy::PolicyLevel;
use mandate_spec::{ValidatedMandate, ValidationContext};
use mandate_time::{Date, TradingCalendar, UtcNanos};

use crate::approvers::{ApproverSource, Members, resolve};
use crate::control::Governance;
use crate::envelope::account_stream;
use crate::error::Cause;
use crate::robinhood::{RobinhoodBridge, RobinhoodReconciler, RobinhoodShellConnector};
use crate::stages::{
    Admitted, Classifier, Connector, Executor, ExitPath, Gate, GovernedRefs, JournalWriter,
    MandateSource, Protection, Reconciler, Sink, Sizing, Stages,
};
use crate::symbols::{BrokerSymbols, Platform};

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

pub use mandate_ports::{AdvisoryGateContext, AdvisoryOrderFacts, BuilderContext, DecisionContext};
use mandate_ports::{ViewFacts, validate_order_execution};

/// Inputs outside the mandate document for validation, admission, and one decision run.
#[derive(Debug, Clone)]
pub struct RunContext {
    pub validation: ValidationContext,
    pub policies: Vec<PolicyLevel>,
    pub author: String,
    /// The workspace's members, which [`runtime_view`] resolves the mandate's approver
    /// references against (DEC-953).
    pub members: Members,
    pub restricted_instruments: BTreeSet<InstrumentId>,
    pub decision: Option<DecisionContext>,
    /// The effective policy set and model registry (DEC-484, D4b). `Some` makes the run write
    /// `ModelOutputRecorded` and `DecisionMade` at schema version 2 with their `policy_set` and
    /// `model_registry` refs, appended through the store's artifact-aware append (journal spec
    /// v0.16). `None` keeps the version-1 records of a run nothing governs yet.
    pub governance: Option<Governance>,
    /// DEC-923's scope for a live `cli_confirm` grant, which the runtime's admission reads
    /// (DEC-970 item 1). `None` refuses every live `cli_confirm` grant `step_up_method`.
    pub live_cli_confirm: Option<LiveScope>,
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
    let facts = ViewFacts {
        approvers: resolve(
            &validated.mandate().autonomy.approval.approvers,
            &context.members,
        ),
        author: context.author.clone(),
        restricted_instruments: context.restricted_instruments.clone(),
        live_cli_confirm: context.live_cli_confirm.clone(),
    };
    Ok(mandate_ports::runtime_view(validated, facts)?)
}

/// The mandate document at `path`, validated with no violation against the injected run context.
pub struct SpecMandate {
    pub path: PathBuf,
    pub context: Option<Rc<RunContext>>,
}

impl MandateSource for SpecMandate {
    fn admitted(&self) -> Result<Admitted, Cause> {
        let context = required_context(&self.context)?;
        let validated = validated_mandate(&self.path, context)?;
        let document = validated.mandate();
        let mut instruments = document.universe.pinned_instruments.iter();
        match (instruments.next(), instruments.next()) {
            (Some(_), None) => {}
            (None, _) | (Some(_), Some(_)) => {
                return Err(Cause::Absent {
                    what: "the tracer's one pinned instrument",
                });
            }
        }
        let view = runtime_view(&validated, context)?;
        Ok(Admitted {
            environment: view.approval.environment,
            view,
            approvers: ApproverSource {
                references: document.autonomy.approval.approvers.clone(),
                connection: document.connection_id.as_str().to_owned(),
            },
            governed: context.governance.as_ref().map(|governance| GovernedRefs {
                policy_set: governance.policy_set.content_hash,
                model_registry: governance.model_registry.content_hash,
            }),
        })
    }
}

/// The daily liquidity figures the run trusts: `mandate_bars`'s one reading (DEC-2274) over its
/// trust check (DEC-2385), so the paper assembly's liquidity facts read the same span as the model
/// host's closes, and the same figures the runtime and the executor read.
pub(crate) fn trusted_daily_liquidity(
    dir: &Path,
    symbol: &str,
    now: UtcNanos,
) -> Result<DailyLiquidity, Cause> {
    Ok(mandate_bars::trusted_daily_liquidity(dir, symbol, now)?)
}

/// The pinned symbol's closes, each with its session, as `mandate_bars`'s trust check admits them
/// (DEC-846 item 2, DEC-2385).
pub(crate) fn trusted_daily_closes(
    dir: &Path,
    symbol: &str,
    now: UtcNanos,
) -> Result<Vec<(Date, Price)>, Cause> {
    Ok(mandate_bars::trusted_daily_closes(dir, symbol, now)?)
}

pub(crate) fn untrusted(what: &'static str) -> Cause {
    Cause::Untrusted { what }
}

/// Whether `view` is the run's own admitted view `own`, but for approvers narrowed to a subset of
/// its own by a later control-stream read (DEC-2384 item 6): narrowing removes approvers and
/// changes nothing sizing or classification reads, while any other difference, a wider approver
/// set included, is not the admitted view.
pub(crate) fn admitted_view(own: MandateView, view: &MandateView) -> bool {
    let narrowed = view.approval.approvers.is_subset(&own.approval.approvers);
    let mut own = own;
    own.approval.approvers.clone_from(&view.approval.approvers);
    narrowed && own == *view
}

/// `mandate-builder`'s `conviction_linear` sizing and §6 classification.
pub struct BuilderPlan {
    pub mandate: PathBuf,
    pub context: Option<Rc<RunContext>>,
}

impl Sizing for BuilderPlan {
    fn size(&self, view: &MandateView, inputs: &SignalInputs) -> Result<Option<Proposal>, Cause> {
        let context = required_context(&self.context)?;
        let validated = validated_mandate(&self.mandate, context)?;
        if !admitted_view(runtime_view(&validated, context)?, view) {
            return Err(Cause::Absent {
                what: "the admitted mandate view used for sizing",
            });
        }
        Ok(mandate_ports::size(
            &validated,
            builder_context(context)?,
            inputs,
        )?)
    }
}

impl Classifier for BuilderPlan {
    /// Mandate spec §6.2 step 5c (DEC-534 item 2, DEC-536): an opening or an increase while the
    /// confirmed mandate is nonconforming is denied, applied last so it only tightens. The overlay
    /// names itself (`policy_overlay`) only when it changed the decision, so a `deny` the rules
    /// already decided keeps their label. An exit never reads the governance, so nothing here
    /// denies it (`AGENTS.md` rule 13).
    fn classify(&self, view: &MandateView, proposal: &Proposal) -> Result<Classified, Cause> {
        let context = required_context(&self.context)?;
        let validated = validated_mandate(&self.mandate, context)?;
        if !admitted_view(runtime_view(&validated, context)?, view) {
            return Err(Cause::Absent {
                what: "the admitted mandate view used for classification",
            });
        }
        let policy_nonconforming = context
            .governance
            .as_ref()
            .is_some_and(|governance| !governance.violations.is_empty());
        Ok(mandate_ports::classify(
            &validated,
            &builder_context(context)?.action,
            proposal,
            policy_nonconforming,
        )?)
    }
}

/// `mandate_risk::evaluate`, as the runtime's advisory pass. The binding call stays inside
/// `mandate-executor` (`AGENTS.md` rules 1 and 12).
pub struct RiskGate {
    pub context: Option<Rc<RunContext>>,
}

impl Gate for RiskGate {
    fn evaluate(&self, proposal: &Proposal) -> Result<Decision, Cause> {
        let context = required_context(&self.context)?;
        let gate = gate_context(context)?;
        Ok(mandate_ports::evaluate(proposal, gate)?)
    }
}

/// The journal store: `mandate_journal::MemoryJournal` for CI and a scratch run,
/// `mandate-journal-pg` at the DSN for the manual paper run.
pub struct StoreJournal {
    backend: StoreBackend,
    recorded_at: UtcNanos,
    /// The store the run's configuration objects were registered in. `Some` routes every append
    /// through `append_with_config_artifacts`; `None` keeps the plain append, which refuses every
    /// version-2 record that names a configuration object (journal spec §5.1, §11 check 6).
    artifacts: Option<Arc<dyn ArtifactSource + Send + Sync>>,
    /// Where each appended `ApprovalRequested`'s content object is put first, or `None`.
    content_store: Option<Box<dyn ArtifactStore + Send>>,
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
            artifacts: None,
            content_store: None,
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
            artifacts: None,
            content_store: None,
        }
    }

    /// The same journal, appending through the artifact-aware append over `artifacts`.
    #[must_use]
    pub fn with_artifacts(self, artifacts: Option<Arc<dyn ArtifactSource + Send + Sync>>) -> Self {
        Self { artifacts, ..self }
    }

    /// The same journal, putting each `ApprovalRequested`'s canonical `content` in `store` before
    /// the append that lists its hash (DEC-970 item 4).
    #[must_use]
    pub fn with_content_store(self, store: Option<Box<dyn ArtifactStore + Send>>) -> Self {
        Self {
            content_store: store,
            ..self
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
        if let Some(store) = self.content_store.as_deref_mut()
            && drafts
                .iter()
                .any(|draft| put_request_content(store, draft).is_err())
        {
            return AppendOutcome::Unavailable;
        }
        let drafts: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
        let recorded_at = self.recorded_at;
        match (&mut self.backend, self.artifacts.as_deref()) {
            (StoreBackend::Memory(journal), None) => {
                journal.append(&stream, expected_head, writer_epoch, recorded_at, &drafts)
            }
            (StoreBackend::Memory(journal), Some(artifacts)) => journal
                .append_with_config_artifacts(
                    &stream,
                    expected_head,
                    writer_epoch,
                    recorded_at,
                    &drafts,
                    artifacts,
                ),
            (StoreBackend::Postgres { journal, runtime }, None) => {
                match runtime.block_on(journal.append(
                    &stream,
                    expected_head,
                    writer_epoch,
                    recorded_at,
                    &drafts,
                )) {
                    Ok(outcome) => outcome,
                    Err(_) => AppendOutcome::Unavailable,
                }
            }
            (StoreBackend::Postgres { journal, runtime }, Some(artifacts)) => {
                match runtime.block_on(journal.append_with_config_artifacts(
                    &stream,
                    expected_head,
                    writer_epoch,
                    recorded_at,
                    &drafts,
                    artifacts,
                )) {
                    Ok(outcome) => outcome,
                    Err(_) => AppendOutcome::Unavailable,
                }
            }
            (StoreBackend::Unavailable, _) => AppendOutcome::Unavailable,
        }
    }
}

/// Puts an `ApprovalRequested` draft's canonical `content` in `store`, under the `content_hash`
/// the draft states and lists in `artifact_refs` (journal spec §3, §9.7, §11 check 6; DEC-970
/// item 4). A draft of any other type puts nothing. A request whose content is missing, or whose
/// hash does not name exactly that content, or a put the store refuses, is an error, and the batch
/// is not appended, so no record names an object the store does not hold and the ask is never
/// made (`AGENTS.md` rule 3).
fn put_request_content(store: &mut (dyn ArtifactStore + Send), draft: &[u8]) -> Result<(), ()> {
    let body = mandate_canon::parse(draft).map_err(|_| ())?;
    if body.get("event_type").and_then(Value::as_str) != Some("ApprovalRequested") {
        return Ok(());
    }
    let payload = body.get("payload").ok_or(())?;
    let content = payload.get("content").ok_or(())?;
    let stated = payload
        .get("content_hash")
        .and_then(Value::as_str)
        .and_then(ArtifactRef::parse)
        .ok_or(())?;
    let bytes = mandate_canon::to_canonical(content);
    if ArtifactRef::of(&bytes) != stated {
        return Err(());
    }
    store.put_artifact(&bytes).map(|_| ()).map_err(|_| ())
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
            confirmation: None,
        })
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

    fn protected(&mut self, instrument: &InstrumentId) -> Result<bool, Cause> {
        let context = self.context()?;
        let ports = context.ports();
        Ok(is_protected(&self.state.borrow(), instrument, &ports)?)
    }

    fn committed(&mut self, event: &mandate_executor::FoldedEvent) -> Result<(), Cause> {
        fold(&mut self.state.borrow_mut(), event)?;
        Ok(())
    }

    fn owed(
        &self,
    ) -> Result<
        (
            Vec<mandate_executor::ClientOrderId>,
            mandate_executor::ActivityCursor,
        ),
        Cause,
    > {
        let _ = self.context()?;
        let state = self.state.borrow();
        let cursor = match state.checkpoint() {
            Some(cursor) => cursor.clone(),
            None => mandate_executor::ActivityCursor(String::new()),
        };
        Ok((mandate_executor::owed_orders(&state), cursor))
    }

    fn book_own(
        &mut self,
        epoch: mandate_executor::WriterEpoch,
        at: mandate_executor::RiskClock,
        orders: &[mandate_executor::BrokerOrder],
        fills: &[mandate_executor::BrokerFill],
    ) -> Result<Vec<mandate_executor::Effect>, Cause> {
        let context = self.context()?;
        let ports = context.ports();
        let effects = mandate_executor::book_own_fills(
            &self.state.borrow(),
            epoch,
            at,
            orders,
            fills,
            &ports,
        )?;
        Ok(effects)
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
            declared: None,
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
        | BrokerOutcome::AccountWideAccepted
        | BrokerOutcome::Listed { .. } => Cause::Absent { what },
    }
}

impl ExecutorReconciler {
    /// A startup snapshot of a broker that publishes no account activity (DEC-933 item 1): the
    /// broker's open orders, positions and account, the executor's own cursor unmoved, taken at
    /// the executor's head of the account stream. Its fills are the ones the broker's order
    /// records and positions imply on a ledger flat in the instrument, and no other
    /// (`mandate_executor::recorded_fills`, DEC-1151): the reconciliation books them before it
    /// compares positions, and with none the position stays unexplained.
    pub(crate) fn without_activities(
        &self,
        open_orders: Vec<mandate_executor::BrokerOrder>,
        positions: Vec<mandate_executor::BrokerPosition>,
        account: mandate_executor::BrokerAccount,
    ) -> mandate_executor::BrokerSnapshot {
        let state = self.state.borrow();
        let scope = state.scope();
        let stream = account_stream(&scope.workspace.0, &scope.account.0);
        mandate_executor::BrokerSnapshot {
            fills: mandate_executor::recorded_fills(&state, &positions),
            open_orders,
            positions,
            account,
            declared: None,
            cursor: match state.checkpoint() {
                Some(cursor) => cursor.clone(),
                None => mandate_executor::ActivityCursor(String::new()),
            },
            reason: mandate_executor::ReconcileReason::Startup,
            taken_at_head: match state.head(&stream) {
                Some(head) => head,
                None => Seq(0),
            },
        }
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
    /// Where the journal puts the content object of each `ApprovalRequested` it appends, before the
    /// append, so the `content_hash` the record lists in `artifact_refs` names a stored object
    /// (journal spec §3, §11 check 6; DEC-970 item 4). `None` stores nothing: a scratch run.
    pub content_store: Option<Box<dyn ArtifactStore + Send>>,
    /// The broker's ticker for each instrument the run registered, so the connector's answers
    /// name it by the asset id the executor sent (DEC-938 item 1). Empty maps nothing.
    pub symbols: BrokerSymbols,
    pub transport: T,
}

/// The production stages, with the Alpaca paper client over `sources.transport`.
pub fn production<T: TradingTransport + Clone + 'static>(sources: Sources<T>) -> Stages {
    let Sources {
        mandate,
        journal,
        recorded_at,
        agent,
        workspace,
        account_ref,
        executor,
        run,
        artifacts,
        content_store,
        symbols,
        transport,
    } = sources;
    over(Sources {
        mandate,
        journal,
        recorded_at,
        agent,
        workspace,
        account_ref,
        executor,
        run,
        artifacts,
        content_store,
        symbols,
        transport: Box::new(AlpacaConnector { transport }),
    })
}

/// The production stages over one Robinhood session (DEC-928 item 2, DEC-947 item 4): the
/// run's connector is [`RobinhoodShellConnector`] and its startup reconciliation
/// [`RobinhoodReconciler`] around the run's own executor reconciler, both over `sources`'
/// bridge, so every read and every order goes through the one MCP session the login opened.
pub fn robinhood_stages<T: Tools + 'static>(sources: Sources<Rc<RobinhoodBridge<T>>>) -> Stages {
    let Sources {
        mandate,
        journal,
        recorded_at,
        agent,
        workspace,
        account_ref,
        executor,
        run,
        artifacts,
        content_store,
        symbols,
        transport: bridge,
    } = sources;
    let connector: Box<dyn Connector> = Box::new(RobinhoodShellConnector::new(Rc::clone(&bridge)));
    assembled(
        Sources {
            mandate,
            journal,
            recorded_at,
            agent,
            workspace,
            account_ref,
            executor,
            run,
            artifacts,
            content_store,
            symbols,
            transport: connector,
        },
        |reconciler| Box::new(RobinhoodReconciler::new(bridge, reconciler)),
    )
}

/// The production stages over a caller-supplied connector.
pub fn over(sources: Sources<Box<dyn Connector>>) -> Stages {
    assembled(sources, |reconciler| Box::new(reconciler))
}

/// The production stages over `sources`' connector, with the run's executor reconciler handed
/// to `reconciled` for the reconciler stage.
fn assembled(
    sources: Sources<Box<dyn Connector>>,
    reconciled: impl FnOnce(ExecutorReconciler) -> Box<dyn Reconciler>,
) -> Stages {
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
        reconciler: reconciled(reconciler),
        sizing: Box::new(BuilderPlan {
            mandate: sources.mandate.clone(),
            context: run.clone(),
        }),
        classifier: Box::new(BuilderPlan {
            mandate: sources.mandate,
            context: run.clone(),
        }),
        gate: Box::new(RiskGate { context: run }),
        journal: Box::new(
            StoreJournal::from_dsn(sources.journal, sources.recorded_at)
                .with_artifacts(sources.artifacts.clone())
                .with_content_store(sources.content_store),
        ),
        sink: Box::new(ExecutorSink {
            agent: sources.agent,
        }),
        executor: Box::new(executor),
        connector: Box::new(Platform {
            inner: sources.transport,
            symbols: sources.symbols,
        }),
        artifacts: sources.artifacts,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet, VecDeque};
    use std::path::PathBuf;
    use std::rc::Rc;

    use super::{
        AdvisoryGateContext, AdvisoryOrderFacts, CoreExecutor, DecisionContext, ExecutorContext,
        ExecutorMandateView, ExecutorOrder, ExecutorProtection, ExecutorSink, Protection,
        ProtectionProbePorts, RiskGate, RunContext, StoreJournal, is_unattributed_exit,
        working_order_of,
    };
    use mandate_canon::Value;
    use mandate_journal::AppendOutcome;
    use mandate_journal::{ArtifactSource, ArtifactStore};
    use mandate_num::{Price, Qty};
    use mandate_ports::proposed_order;
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
                environment: mandate_journal::Environment::Paper,
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

    /// A content store the test keeps a handle on, refusing every put when `refusing`.
    #[derive(Clone, Default)]
    struct Contents {
        objects: std::sync::Arc<std::sync::Mutex<BTreeMap<mandate_canon::Digest, Vec<u8>>>>,
        refusing: bool,
    }

    impl ArtifactSource for Contents {
        fn read_artifact(
            &self,
            reference: &mandate_journal::ArtifactRef,
        ) -> Result<Vec<u8>, mandate_journal::ArtifactError> {
            self.objects
                .lock()
                .map_err(|_| mandate_journal::ArtifactError::Unavailable)?
                .read_artifact(reference)
        }
    }

    impl ArtifactStore for Contents {
        fn put_artifact(
            &mut self,
            bytes: &[u8],
        ) -> Result<mandate_journal::ArtifactRef, mandate_journal::ArtifactError> {
            if self.refusing {
                return Err(mandate_journal::ArtifactError::Unavailable);
            }
            self.objects
                .lock()
                .map_err(|_| mandate_journal::ArtifactError::Unavailable)?
                .put_artifact(bytes)
        }
    }

    impl Contents {
        fn len(&self) -> usize {
            self.objects
                .lock()
                .map_or(usize::MAX, |objects| objects.len())
        }
    }

    /// An agent-stream draft of `event_type` whose payload carries `content` and `content_hash`.
    fn content_draft(
        event_type: &str,
        content: &Value,
        content_hash: &str,
    ) -> Result<Vec<u8>, String> {
        let at = UtcNanos::parse("2026-10-20T15:00:00.000000000Z").map_err(|e| e.to_string())?;
        let payload = value_object(&[
            ("content", content.clone()),
            ("content_hash", Value::Str(content_hash.to_owned())),
        ])?;
        let config_refs = mandate_canon::Object::new();
        draft_bytes(
            &Envelope {
                stream: "agent:tracer:agent",
                writer: Writer::Agent,
                actor_id: "agent",
                event_time: at,
                environment: mandate_journal::Environment::Live,
            },
            &DraftFields {
                event_id: "10000100000000000000000001",
                event_type,
                schema_version: 1,
                causation_id: None,
                config_refs: &config_refs,
                payload: &payload,
            },
        )
        .map_err(|e| e.to_string())
    }

    fn appended(contents: &Contents, draft: Vec<u8>) -> Result<AppendOutcome, String> {
        let at = UtcNanos::parse("2026-10-20T15:00:00.000000000Z").map_err(|e| e.to_string())?;
        let mut journal =
            StoreJournal::memory(at).with_content_store(Some(Box::new(contents.clone())));
        let epoch = journal
            .take_ownership("agent:tracer:agent")
            .map_err(|e| e.to_string())?;
        Ok(journal.append("agent:tracer:agent", 0, epoch, &[draft]))
    }

    /// Journal spec §3, §11 check 6 (DEC-970 item 4): a request's canonical `content` is put under
    /// its `content_hash` before the append; a hash that does not name that content, or a store
    /// that refuses the put, appends nothing (`Unavailable`, so the ask is never made); a record of
    /// another type puts nothing.
    #[test]
    fn a_request_puts_its_content_object_before_the_append() -> Result<(), String> {
        let content = value_object(&[("default", Value::Str("skip".to_owned()))])?;
        let bytes = mandate_canon::to_canonical(&content);
        let hash = mandate_journal::ArtifactRef::of(&bytes).to_string();
        let contents = Contents::default();
        let outcome = appended(
            &contents,
            content_draft("ApprovalRequested", &content, &hash)?,
        )?;
        assert!(
            !matches!(outcome, AppendOutcome::Unavailable),
            "the put succeeded, so the append is the journal's to judge: {outcome:?}"
        );
        let reference = mandate_journal::ArtifactRef::parse(&hash).ok_or("the hash parses")?;
        assert_eq!(
            mandate_journal::get_artifact(&contents, &reference).map_err(|e| format!("{e:?}"))?,
            bytes
        );
        assert_eq!(contents.len(), 1);

        let other = Contents::default();
        let wrong = format!("sha256:{}", "0".repeat(64));
        let outcome = appended(
            &other,
            content_draft("ApprovalRequested", &content, &wrong)?,
        )?;
        assert!(matches!(outcome, AppendOutcome::Unavailable), "{outcome:?}");
        assert_eq!(other.len(), 0, "a mismatched hash puts nothing");

        let refusing = Contents {
            refusing: true,
            ..Contents::default()
        };
        let outcome = appended(
            &refusing,
            content_draft("ApprovalRequested", &content, &hash)?,
        )?;
        assert!(matches!(outcome, AppendOutcome::Unavailable), "{outcome:?}");

        let untouched = Contents::default();
        appended(&untouched, content_draft("DecisionMade", &content, &hash)?)?;
        assert_eq!(untouched.len(), 0, "only a request's content is put");
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

    /// DEC-838 item 5: the profile handed to the core executor replaces its transitional one in
    /// the state it has folded, which it keeps.
    #[test]
    fn core_executor_keeps_its_fold_and_takes_the_profile_it_is_handed() -> Result<(), String> {
        let (mut core, _) = CoreExecutor::pair(account_scope(), Some(test_executor_context()?));
        let opened = stream_opened()?;
        core.committed(&opened).map_err(|e| e.to_string())?;
        let cell = mandate_domain::Cell {
            order_type: mandate_domain::OrderType::Limit,
            quantity_form: mandate_domain::QuantityForm::Whole,
            times_in_force: [mandate_domain::TimeInForce::Gtc].into(),
            protection_forms: [mandate_domain::ProtectionForm::Oco].into(),
        };
        let row = mandate_domain::Row {
            asset_class: mandate_accounting::AssetClass::UsEquity,
            session: mandate_domain::MarketSession::Regular,
            cells: vec![cell],
        };
        let idempotency = mandate_domain::Idempotency {
            client_order_id: true,
            retry: mandate_domain::Retry::Idempotent,
            query_by_client_order_id: true,
        };
        let profile = mandate_domain::CapabilityProfile::new(1, vec![row], idempotency)
            .map_err(|e| e.to_string())?;
        core.use_profile(profile.clone())
            .map_err(|e| e.to_string())?;
        let mut folded = mandate_executor::ExecutorState::new(account_scope());
        mandate_executor::fold(&mut folded, &opened).map_err(|e| e.to_string())?;
        assert_ne!(
            *core.state(),
            folded,
            "the transitional profile is replaced"
        );
        assert_eq!(*core.state(), folded.with_profile(profile));
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

    /// One bought share folded through the account stream's own `FillApplied` (§9.5), no
    /// protection resting over it: the position the fold holds.
    fn fill_applied_held_share() -> Result<mandate_executor::FoldedEvent, String> {
        Ok(mandate_executor::FoldedEvent {
            stream: "acct:tracer:tracer-paper".to_owned(),
            seq: mandate_executor::Seq(2),
            event_id: mandate_executor::EventId("10000100000000000000000001".to_owned()),
            event_type: "FillApplied".to_owned(),
            causation_id: None,
            payload: value_object(&[
                (
                    "fill_id",
                    Value::Str("the folded fill of the one held share".to_owned()),
                ),
                ("instrument_id", Value::Str("SPY".to_owned())),
                ("qty_gross", Value::Str("1".to_owned())),
                ("price", Value::Str("255.2".to_owned())),
                ("side", Value::Str("buy".to_owned())),
                (
                    "risk_clock",
                    Value::Str("2026-09-29T19:49:45.000000000Z".to_owned()),
                ),
            ])?,
        })
    }

    /// The watch's covered fact is the executor's own, which this row's review round 2 holds
    /// against the CI-missed `CoreExecutor::protected → Ok(true)` mutant: no test had a held
    /// position against the real executor's answer, so a lying always-covered body lived with
    /// the whole suite green — the exact release LT-10's first half exists to refuse
    /// (DEC-912 item 3: "The run may end when the entry is **terminal** and any filled quantity
    /// is covered… Until either half holds the run keeps reading"). One held share with no
    /// protection resting over it answers `unprotected` — `is_protected`'s own rule 3, the
    /// snapshot that knows neither class nor increment reporting unprotected rather than
    /// guessing covered — and the adapter's body must carry that fact to the seam's
    /// `uncovered`, never stand in front of it. The mutant answers `Ok(true)` here and the
    /// watch ends a run it must keep reading.
    #[test]
    fn the_core_executors_protection_answer_is_its_own_fact_over_a_held_unprotected_position()
    -> Result<(), String> {
        let (mut core, _) = CoreExecutor::pair(account_scope(), Some(test_executor_context()?));
        core.committed(&stream_opened()?)
            .map_err(|error| error.to_string())?;
        core.committed(&fill_applied_held_share()?)
            .map_err(|error| error.to_string())?;
        let answer = core
            .protected(
                &mandate_accounting::InstrumentId::new("SPY").map_err(|error| error.to_string())?,
            )
            .map_err(|error| format!("the executor answers its own fold: {error:?}"))?;
        assert!(
            !answer,
            "a held share with no protection resting over it is uncovered, never guessed covered: {answer}"
        );
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
                    limit: Some(Price::parse("255.2").map_err(|e| e.to_string())?),
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
                    limit: Some(Price::parse("255.2").map_err(|e| e.to_string())?),
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

    use super::Disconnected;
    use crate::error::Cause;

    fn day(text: &str) -> Result<Date, String> {
        Date::parse(text).map_err(|e| e.to_string())
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
                stop_limit_asset_classes: None,
            },
            policies: Vec::new(),
            author: String::new(),
            members: crate::approvers::Members::Unread,
            restricted_instruments: BTreeSet::new(),
            decision: Some(DecisionContext {
                builder: None,
                gate: Some(gate),
            }),
            governance: None,
            live_cli_confirm: None,
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
