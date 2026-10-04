//! The production adapters: one per [`Stage`](crate::Stage), each binding one crate's public API.
//!
//! The implementation lands one slice at a time (DEC-77 stage 3, DEC-166). **Live** in slice 1:
//! [`StoredBars`], [`MovingAverage`] and [`AlpacaConnector`], whose upstreams are fully implemented
//! and whose every input the shell holds, and since DEC-449's flip also [`RiskExitPath`], which
//! probes and plans over the journal, stream, clock, and agent the run's bridge hands it.
//! **Still refusing**, each with [`Cause::Unimplemented`]: every other adapter, because its
//! upstream is a stub or no production source exists for one of its inputs, and the shell never
//! invents one (DEC-166 item 2) — so a run over [`production`] refuses at the protection probe,
//! the first stage after the exit, and places no order.
//!
//! What each will bind, per the task brief's step table:
//!
//! | Adapter | Binds |
//! |---|---|
//! | [`RiskExitPath`] | `mandate_risk::agent_flatten` (E6-3) |
//! | [`ExecutorProtection`] | `mandate_executor::handle` on a synthetic protective input (E7-4) |
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

use std::collections::BTreeMap;
use std::path::PathBuf;

use mandate_accounting::{InstrumentId, Side};
use mandate_alpaca::{RetryPolicy, TokioPause, TradingClient, TradingTransport};
use mandate_backtest::{Signal, Strategy, StrategyConfig};
use mandate_executor::{
    BrokerConnector, BrokerOutcome, BrokerRequest, ConnectorError, Order as ExecutorOrder,
};
use mandate_journal::{AppendOutcome, StoredEvent};
use mandate_marketdata::dataset;
use mandate_marketdata::inspect::{self, ActionsReport, GapClass, Inspection};
use mandate_marketdata::model::{Kind, Records, TimeUnit, Timeframe};
use mandate_num::{Bps, Fraction, Price, Qty, SignedQty, Usd};
use mandate_risk::{Decision, FlattenInitiator, FlattenInput, agent_flatten};
use mandate_runtime::{
    AgentId, Autonomy, FlattenLeg, FlattenPlan, FlattenRequest, Initiator, IntentHandoff,
    MandateView, Proposal, Purpose, RiskClock, SignalInputs,
};
use mandate_time::UtcNanos;

use crate::error::Cause;
use crate::stages::{
    Admitted, Bars, Classifier, Connector, Executor, ExitPath, Gate, JournalWriter, MandateSource,
    ModelRef, Protection, Reconciled, Reconciler, SignalModel, Sink, Sizing, Stages,
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
        let bytes = std::fs::read(&self.mandate).map_err(|_| {
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

    /// Each pinned instrument's asset class, keyed by the id the journal's records carry. The
    /// journal's `instrument_id` must carry the mandate's `asset_id` — the strict
    /// `8-4-4-4-12` uuid, never the vendor's symbol — and every id is proven readable as the
    /// risk crate's own `AssetId`: the probe refuses a mandate whose universe the flatten could
    /// never read, and a plan over an instrument the map does not carry is a refusal, never a
    /// guess at its class.
    fn classes(
        document: &mandate_spec::document::Mandate,
    ) -> Result<BTreeMap<String, mandate_risk::AssetClass>, Cause> {
        let mut classes = BTreeMap::new();
        for pinned in &document.universe.pinned_instruments {
            mandate_risk::AssetId::new(pinned.asset_id.as_str()).map_err(|_| {
                Cause::Spec(mandate_spec::SpecError::InvalidInput {
                    what: "a universe instrument the risk crate cannot read",
                })
            })?;
            classes.insert(pinned.asset_id.as_str().to_owned(), pinned.asset_class);
        }
        Ok(classes)
    }
}

impl ExitPath for RiskExitPath {
    fn probe(&self) -> Result<(), Cause> {
        let document = self.document()?;
        Self::classes(&document)?;
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
        let classes = Self::classes(&document)?;
        let stored = journal.read(stream)?;
        let state = fold_of(&scope_of(stream)?, &stored)?;
        let mine: Vec<&mandate_executor::Order> = state
            .orders()
            .values()
            .filter(|order| {
                order
                    .agent
                    .as_ref()
                    .is_some_and(|agent| agent.0.as_str() == self.agent.0.as_str())
            })
            .collect();
        let session = session_of(clock, &mine, &classes)?;
        let positions = sub_ledger_of(&mine, &classes, request.instrument.as_ref())?;
        let (open_orders, order_ids) =
            working_orders_of(&mine, clock, request.instrument.as_ref())?;
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
        Ok(FlattenPlan {
            cancel_client_order_ids: planned
                .cancel_client_order_ids
                .iter()
                .map(|id| {
                    order_ids.get(id).cloned().ok_or(Cause::Absent {
                        what: "a working order the plan named but the fold does not hold",
                    })
                })
                .collect::<Result<Vec<String>, Cause>>()?,
            sells,
            purpose,
            confirmation: request.confirmation.clone(),
        })
    }
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
/// as the journal normalized them: each row parses through the journal's own draft validation,
/// and the fold sees the draft's normalized payload, never the stored bytes' form (DEC-449
/// item 1).
fn fold_of(
    scope: &mandate_executor::AccountScope,
    stored: &[StoredEvent],
) -> Result<mandate_executor::ExecutorState, Cause> {
    let mut state = mandate_executor::ExecutorState::new(scope.clone());
    for row in stored {
        let draft = mandate_journal::Draft::parse(&row.body).map_err(|_| {
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
    if calendar.is_trading_day(today).is_ok() {
        return Ok(mandate_risk::Session::Overnight);
    }
    Err(Cause::Absent {
        what: "a session the committed calendar names",
    })
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
        if scope.is_some_and(|instrument| order.instrument != *instrument) {
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
    let mut open_orders = BTreeMap::new();
    let mut order_ids = BTreeMap::new();
    for (index, order) in mine
        .iter()
        .filter(|order| !order.state.is_terminal() && order.filled_qty < order.qty)
        .filter(|order| !scope.is_some_and(|instrument| order.instrument != *instrument))
        .enumerate()
    {
        let (today, _) = mandate_time::new_york_date_and_hour(
            UtcNanos::from_parts(clock.secs(), 0).map_err(|_| Cause::Absent {
                what: "a clock the calendar reads",
            })?,
        )
        .map_err(|_| Cause::Absent {
            what: "a New York date",
        })?;
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

/// The executor's protective sequence, probed through `mandate_executor::handle`, since
/// `mod protection` is private (task brief, Decisions needed 4).
pub struct ExecutorProtection;

impl Protection for ExecutorProtection {
    fn probe(&self) -> Result<(), Cause> {
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// The mandate document at `path`, validated with no violation.
pub struct SpecMandate {
    pub path: PathBuf,
}

impl MandateSource for SpecMandate {
    fn admitted(&self) -> Result<Admitted, Cause> {
        Err(Cause::Unimplemented { story: "E7-7" })
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
    fn closes(&self, symbol: &str) -> Result<Vec<Price>, Cause> {
        let inspection = inspect::inspect(&self.dir)?;
        trusted(&inspection, symbol)?;
        self.listed_closes(inspection.dataset.kind())
    }
}

impl StoredBars {
    /// One close per day the manifest lists with a file, in the manifest's date order, each read
    /// from that day's own partition. The loop walks the manifest, never the directory, so a
    /// partition the manifest does not list is not opened even if `trusted` were to let it pass
    /// (#248 review, minor 1).
    fn listed_closes(&self, kind: Kind) -> Result<Vec<Price>, Cause> {
        let (_, days) = dataset::read_manifest(&self.dir)?;
        let mut closes = Vec::new();
        for listed in days.iter().filter(|listed| listed.file.is_some()) {
            let path = self.dir.join(dataset::partition_name(listed.day));
            match dataset::read(&path, kind)? {
                Records::Bars(bars) => match bars.as_slice() {
                    [bar] => closes.push(Price::parse(bar.close.as_str())?),
                    [] | [_, _, ..] => {
                        return Err(untrusted("a listed day does not hold exactly one bar"));
                    }
                },
                Records::Trades(_) | Records::Quotes(_) => {
                    return Err(untrusted("the dataset holds no bars"));
                }
            }
        }
        Ok(closes)
    }
}

/// Whether `inspect` found the dataset fit to decide on: the pinned symbol's daily bars, every
/// partition intact, no trading day without its bar, and no split inside the span, whose prices
/// the shell may not adjust (DEC-138 item 3). `inspect`'s quality warnings are the vendor's
/// records as stored (DEC-89) and are not refusals.
fn trusted(inspection: &Inspection, symbol: &str) -> Result<(), Cause> {
    let daily = Kind::Bars(Timeframe::new(1, TimeUnit::Day)?);
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

fn untrusted(what: &'static str) -> Cause {
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
pub struct BuilderPlan;

impl Sizing for BuilderPlan {
    fn size(&self, view: &MandateView, inputs: &SignalInputs) -> Result<Option<Proposal>, Cause> {
        let _ = (view, inputs);
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

impl Classifier for BuilderPlan {
    fn classify(&self, view: &MandateView, proposal: &Proposal) -> Result<Autonomy, Cause> {
        let _ = (view, proposal);
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// `mandate_risk::evaluate`, as the runtime's advisory pass. The binding call stays inside
/// `mandate-executor` (`AGENTS.md` rules 1 and 12).
pub struct RiskGate;

impl Gate for RiskGate {
    fn evaluate(&self, proposal: &Proposal) -> Result<Decision, Cause> {
        let _ = proposal;
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// The journal store: `mandate_journal::MemoryJournal` for CI and a scratch run,
/// `mandate-journal-pg` at the DSN for the manual paper run.
pub struct StoreJournal {
    pub dsn: Option<String>,
}

impl JournalWriter for StoreJournal {
    fn take_ownership(&mut self, stream: &str) -> Result<u64, Cause> {
        let _ = stream;
        Err(Cause::Unimplemented { story: "E7-7" })
    }

    fn read(&self, stream: &str) -> Result<Vec<StoredEvent>, Cause> {
        let _ = stream;
        Err(Cause::Unimplemented { story: "E7-7" })
    }

    fn append(
        &mut self,
        stream: &str,
        expected_head: u64,
        writer_epoch: u64,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        let _ = (stream, expected_head, writer_epoch, drafts);
        AppendOutcome::Unavailable
    }
}

/// The runtime's `IntentSink`, handing to the executor with this deployment's `AgentId`.
pub struct ExecutorSink {
    pub agent: AgentId,
}

impl Sink for ExecutorSink {
    fn hand(&mut self, handoff: &IntentHandoff) -> Result<mandate_executor::IntentHandoff, Cause> {
        let _ = handoff;
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// `mandate_executor::handle` and `fold` over the account stream's state.
pub struct CoreExecutor;

impl Executor for CoreExecutor {
    fn step(
        &mut self,
        input: mandate_executor::Input,
    ) -> Result<Vec<mandate_executor::Effect>, Cause> {
        let _ = input;
        Err(Cause::Unimplemented { story: "E7-7" })
    }

    fn committed(&mut self, event: &mandate_executor::FoldedEvent) -> Result<(), Cause> {
        let _ = event;
        Err(Cause::Unimplemented { story: "E7-7" })
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
    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
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
/// the process. The binary uses it until the slice that makes the exit probes real, since every run
/// refuses at the first probe before any request (DEC-166 item 3).
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
pub struct ExecutorReconciler;

impl Reconciler for ExecutorReconciler {
    fn reconcile(&mut self) -> Result<Reconciled, Cause> {
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// Where the production stages read from.
pub struct Sources<T> {
    pub mandate: PathBuf,
    pub dataset: PathBuf,
    /// `None` is a scratch in-memory journal: a run that places nothing keeps nothing, so a later
    /// start cannot re-hand an intent a planning run proposed (DEC-157 item 6).
    pub journal: Option<String>,
    pub agent: AgentId,
    pub transport: T,
}

/// The production stages, with the Alpaca paper client over `sources.transport`.
pub fn production<T: TradingTransport + Clone + 'static>(sources: Sources<T>) -> Stages {
    let Sources {
        mandate,
        dataset,
        journal,
        agent,
        transport,
    } = sources;
    over(Sources {
        mandate,
        dataset,
        journal,
        agent,
        transport: Box::new(AlpacaConnector { transport }),
    })
}

/// The production stages over the given connector: the binary's [`Disconnected`] one, or
/// [`production`]'s.
pub fn over(sources: Sources<Box<dyn Connector>>) -> Stages {
    Stages {
        exit: Box::new(RiskExitPath::new(
            sources.agent.clone(),
            sources.mandate.clone(),
        )),
        protection: Box::new(ExecutorProtection),
        mandate: Box::new(SpecMandate {
            path: sources.mandate,
        }),
        bars: Box::new(StoredBars {
            dir: sources.dataset,
        }),
        signal: Box::new(MovingAverage),
        reconciler: Box::new(ExecutorReconciler),
        sizing: Box::new(BuilderPlan),
        classifier: Box::new(BuilderPlan),
        gate: Box::new(RiskGate),
        journal: Box::new(StoreJournal {
            dsn: sources.journal,
        }),
        sink: Box::new(ExecutorSink {
            agent: sources.agent,
        }),
        executor: Box::new(CoreExecutor),
        connector: sources.transport,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{ExecutorOrder, working_order_of};
    use mandate_backtest::{BacktestError, Signal};
    use mandate_canon::DecStr;
    use mandate_marketdata::actions::{RecordedActions, write_actions};
    use mandate_marketdata::dataset::{Store, partition_name};
    use mandate_marketdata::inspect::{self, ActionsReport, Problem};
    use mandate_marketdata::model::{
        AssetClass, Bar, CorporateActions, DatasetId, DayRange, Feed, Kind, Records, Split,
        SplitRatio, Symbol, TimeUnit, Timeframe,
    };
    use mandate_num::{Price, Qty};
    use mandate_time::{Date, UtcNanos};

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
    use crate::stages::{Bars, Connector, ModelRef, SignalModel};

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
        .closes("AAPL")
        .map_err(|e| e.to_string())?;
        let expected: Vec<String> = closes.iter().map(|c| canonical(c)).collect();
        let read: Vec<String> = read.iter().map(ToString::to_string).collect();
        assert_eq!(read, expected);
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
        let what = untrusted(StoredBars { dir }.closes("AAPL"))?;
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
        let what = untrusted(StoredBars { dir }.closes("MSFT"))?;
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
        let what = untrusted(StoredBars { dir }.closes("AAPL"))?;
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
        let what = untrusted(StoredBars { dir }.closes("AAPL"))?;
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
        let what = untrusted(StoredBars { dir }.closes("AAPL"))?;
        assert_eq!(what, "two bars share a start");
        Ok(())
    }

    #[test]
    fn a_trading_day_listed_without_a_bar_is_refused() -> Result<(), String> {
        let scratch = Scratch::new("empty")?;
        let dir = aapl(&scratch.0, &[], &["2026-08-26"])?;
        let what = untrusted(StoredBars { dir }.closes("AAPL"))?;
        assert_eq!(what, "a trading day has no bar");
        Ok(())
    }

    #[test]
    fn a_trading_day_never_fetched_is_refused() -> Result<(), String> {
        let scratch = Scratch::new("gap")?;
        let dir = aapl(&scratch.0, &["2026-08-26"], &[])?;
        let what = untrusted(StoredBars { dir }.closes("AAPL"))?;
        assert_eq!(what, "a trading day was never fetched");
        Ok(())
    }

    #[test]
    fn a_split_inside_the_span_or_actions_short_of_it_are_refused() -> Result<(), String> {
        let scratch = Scratch::new("split")?;
        let dir = aapl(&scratch.0, &[], &[])?;
        let mut inspection = inspect::inspect(&dir).map_err(|e| e.to_string())?;
        assert!(trusted(&inspection, "AAPL").is_ok());
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
            trusted(&inspection, "AAPL"),
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
            .closes("AAPL")
            .map_err(|e| e.to_string())?;
        assert_eq!(closes.len(), 25);
        Ok(())
    }

    fn model(id: &str, params: &[(&str, &str)]) -> ModelRef {
        ModelRef {
            id: id.to_owned(),
            version: "1.0.0".to_owned(),
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
        let what = untrusted(StoredBars { dir }.closes("AAPL"))?;
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
}
