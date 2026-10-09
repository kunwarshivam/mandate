//! Permissive doubles at the shell's own stage traits, and the counters the fail-closed suite's
//! oracles read (task brief, "The test that proves it").
//!
//! Every double answers "yes, proceed" with fixture values, so a path of doubles reaches the broker
//! and places one order; that is what makes a stubbed stage's zero mean something. They live in a
//! `#[cfg(test)]` module inside `src/` because a `ValidatedMandate`-free view must not be reachable
//! from a build that ships (task brief item 5).
//!
//! The counters are the suite's **independent oracles**: the connector double counts every
//! submission it is handed before anything interprets it, the sink double counts every handoff,
//! and the ledger is read back by parsing the committed bytes, never from the runner's own state.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_backtest::Signal;
use mandate_canon::{Digest, Int, Key, Object, Value};
use mandate_executor::{
    ActivityCursor, BrokerAccount, BrokerOrder, BrokerOutcome, BrokerRequest, BrokerSnapshot,
    BrokerUnknown, ClientOrderId, ConnectorError, IntentId, OrderType, ReconcileReason, Seq,
    SubmitOrder, TimeInForce,
};
use mandate_journal::{AppendOutcome, Environment, StoredEvent};
use mandate_num::{Price, Qty, Usd};
use mandate_risk::{Check, CheckOutcome, Computed, Decision, Purpose as GatePurpose, Verdict};
use mandate_runtime::{
    AgentId, ApprovalSettings, Autonomy, Classified, ConnectionId, Deployment, FlattenPlan,
    FlattenRequest, IntentBody, IntentHandoff, MandateView, OrderExecution, Proposal, Purpose,
    SignalInputs, TimeInForce as RuntimeTimeInForce, WorkspaceId,
};
use mandate_time::UtcNanos;

use super::{
    Admitted, Bars, Classifier, Connector, Executor, ExitPath, Gate, JournalWriter, MandateSource,
    ModelRef, Protection, Reconciler, SignalModel, Sink, Sizing, Stage, Stages,
};
use crate::envelope::{IdSpace, Ids};
use crate::error::Cause;
use crate::tracer::Setup;

/// The broker's asset id for AAPL, the tracer's instrument (DEC-90, internal test data).
pub const AAPL: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
pub const AGENT: &str = "tracer-aapl";
pub const NOW: &str = "2026-09-25T20:00:00.000000000Z";

/// What the doubles saw, counted where they saw it.
#[derive(Debug, Default)]
pub struct Tally {
    /// Every stage a double was asked to answer for, in order.
    pub calls: Vec<Stage>,
    /// `IntentSink` handoffs the sink double accepted.
    pub hands: u32,
    /// Handoffs whose `IntentProposed` was not in the ledger when they arrived (TI-1).
    pub unrecorded_hands: u32,
    /// `Submit` requests the connector double was handed.
    pub submissions: u32,
    /// Submissions whose `OrderSubmitted` was not in the ledger when they arrived (TI-1).
    pub unrecorded_submissions: u32,
    /// Every submission, as the connector saw it.
    pub submitted: Vec<SubmitOrder>,
    /// Order queries the connector double answered.
    pub queries: u32,
    /// Broker answers the executor double was handed.
    pub executor_broker_inputs: u32,
    /// Order intents handed to the executor after startup.
    pub executor_intents: u32,
    /// The stream of every committed event the executor double was asked to fold.
    pub executor_folds: Vec<String>,
    next_id: u64,
}

impl Tally {
    fn next_account_id(&mut self) -> String {
        self.next_id = self.next_id.saturating_add(1);
        Ids {
            space: IdSpace::Account,
        }
        .derive(9, self.next_id, 0)
    }
}

/// The committed events of every stream, as the journal double stored them.
#[derive(Debug, Default)]
pub struct Ledger {
    streams: BTreeMap<String, Vec<StoredEvent>>,
    epochs: BTreeMap<String, u64>,
}

impl Ledger {
    /// Every committed body, stream by stream, in `seq` order.
    pub fn bodies(&self) -> Vec<Vec<u8>> {
        self.streams
            .values()
            .flat_map(|rows| rows.iter().map(|row| row.body.clone()))
            .collect()
    }

    /// Every committed body parsed, of one event type, read from the bytes rather than from the
    /// stored column.
    pub fn parsed(&self, event_type: &str) -> Vec<Value> {
        self.bodies()
            .iter()
            .map(|body| parsed(body))
            .filter(|body| body.get("event_type").and_then(Value::as_str) == Some(event_type))
            .collect()
    }

    pub fn count(&self, event_type: &str) -> usize {
        self.parsed(event_type).len()
    }

    /// The event type of the last committed body of `stream`.
    pub fn last_type(&self, stream: &str) -> Option<String> {
        let row = self.streams.get(stream)?.last()?;
        let body = parsed(&row.body);
        body.get("event_type")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    /// How many events `stream` holds.
    pub fn len(&self, stream: &str) -> usize {
        match self.streams.get(stream) {
            Some(rows) => rows.len(),
            None => 0,
        }
    }

    pub fn types(&self, stream: &str) -> Vec<String> {
        self.streams
            .get(stream)
            .into_iter()
            .flatten()
            .map(|row| row.event_type.clone())
            .collect()
    }

    fn has(&self, event_type: &str, field: &str, value: &str) -> bool {
        self.parsed(event_type).iter().any(|body| {
            let direct = body.get(field).and_then(Value::as_str);
            let in_payload = body
                .get("payload")
                .and_then(|payload| payload.get(field))
                .and_then(Value::as_str);
            direct == Some(value) || in_payload == Some(value)
        })
    }
}

/// A committed body, parsed. A body that does not parse is `null`, an event of no type, which the
/// environment scanner (it parses every body itself) reports as a failure.
fn parsed(body: &[u8]) -> Value {
    match mandate_canon::parse(body) {
        Ok(value) => value,
        Err(_) => Value::Null,
    }
}

/// Everything a harness run shares: the counters and the journal.
#[derive(Clone, Default)]
pub struct World {
    pub tally: Rc<RefCell<Tally>>,
    pub ledger: Rc<RefCell<Ledger>>,
}

impl World {
    fn called(&self, stage: Stage) {
        self.tally.borrow_mut().calls.push(stage);
    }

    /// Every stage a permissive double.
    pub fn stages(&self) -> Stages {
        Stages {
            exit: Box::new(PermissiveExit(self.clone())),
            protection: Box::new(PermissiveProtection(self.clone())),
            mandate: Box::new(FixtureMandate {
                world: self.clone(),
                environment: Environment::Paper,
            }),
            bars: Box::new(FixtureBars(self.clone())),
            signal: Box::new(FixedSignal {
                world: self.clone(),
                signal: Signal::Long,
            }),
            reconciler: Box::new(FixedReconciler {
                world: self.clone(),
                clean: true,
            }),
            sizing: Box::new(OneShare {
                world: self.clone(),
                qty: "1",
            }),
            classifier: Box::new(FixedClassifier {
                world: self.clone(),
                autonomy: Autonomy::Auto,
            }),
            gate: Box::new(FixedGate {
                world: self.clone(),
                verdict: Verdict::Allow,
                checks: passed_checks(),
            }),
            journal: Box::new(LedgerJournal(self.clone())),
            sink: Box::new(ConvertingSink(self.clone())),
            executor: Box::new(PaperExecutor {
                world: self.clone(),
                seen: BTreeSet::new(),
                inverted: false,
                last_client_order_id: None,
            }),
            connector: Box::new(ScriptedConnector {
                world: self.clone(),
                script: Script::Accept,
            }),
            artifacts: None,
        }
    }
}

pub fn setup() -> Result<Setup, String> {
    Ok(Setup {
        deployment: Deployment {
            agent: AgentId(AGENT.to_owned()),
            connection: ConnectionId("conn_alpaca_paper_01".to_owned()),
            workspace: WorkspaceId("tracer".to_owned()),
        },
        account_ref: "tracer-paper".to_owned(),
        now: UtcNanos::parse(NOW).map_err(|e| e.to_string())?,
        place_one_order: true,
        new_cycle: false,
    })
}

pub fn agent_stream() -> String {
    crate::envelope::agent_stream("tracer", AGENT)
}

pub fn account_stream() -> String {
    crate::envelope::account_stream("tracer", "tracer-paper")
}

pub fn instrument() -> Result<InstrumentId, Cause> {
    InstrumentId::new(AAPL).map_err(|_| Cause::Absent {
        what: "instrument id",
    })
}

fn object(members: &[(&str, &str)]) -> Result<Value, Cause> {
    let mut map = Object::new();
    for (name, value) in members {
        let key = Key::new(name).map_err(|_| Cause::Absent { what: "a key" })?;
        map.insert(key, Value::Str((*value).to_owned()));
    }
    Ok(Value::Object(map))
}

fn account_payload(account: &BrokerAccount) -> Result<Value, Cause> {
    let mut map = Object::new();
    let mut insert = |name: &'static str, value: Value| -> Result<(), Cause> {
        let key = Key::new(name).map_err(|_| Cause::Absent { what: "a key" })?;
        map.insert(key, value);
        Ok(())
    };
    insert("status", Value::Str(account.status.clone()))?;
    insert("crypto_status", Value::Str(account.crypto_status.clone()))?;
    insert("trading_blocked", Value::Bool(account.trading_blocked))?;
    insert("account_blocked", Value::Bool(account.account_blocked))?;
    insert(
        "trade_suspended_by_user",
        Value::Bool(account.trade_suspended_by_user),
    )?;
    insert(
        "multiplier",
        Value::Int(
            Int::new(u64::from(account.multiplier)).ok_or(Cause::Absent {
                what: "the account multiplier",
            })?,
        ),
    )?;
    insert("equity", Value::Str(account.equity.to_string()))?;
    insert("cash", Value::Str(account.cash.to_string()))?;
    insert("buying_power", Value::Str(account.buying_power.to_string()))?;
    insert(
        "non_marginable_buying_power",
        Value::Str(account.non_marginable_buying_power.to_string()),
    )?;
    insert("accrued_fees", Value::Str(account.accrued_fees.to_string()))?;
    insert("risk_clock", Value::Str(NOW.to_owned()))?;
    Ok(Value::Object(map))
}

pub fn passed_checks() -> Vec<CheckOutcome> {
    [
        Check::AccountAndMode,
        Check::UniverseAndLimits,
        Check::SessionAndHalt,
        Check::OrderConstraints,
        Check::MarkAndCollar,
        Check::ConductControls,
        Check::BuyingPowerAndExposure,
        Check::DayTradeBudget,
    ]
    .into_iter()
    .map(CheckOutcome::Passed)
    .collect()
}

/// The permissive exit: probes and plans succeed so the fail-closed suite can drive the paths
/// around a working exit. Its plan is fixture-shaped, not a mapping example — it echoes the
/// request's `working_orders` as cancels, which under DEC-449 item 3 is exactly the copy a
/// production adapter must never make (the cancels are the fold's working orders for the
/// agent, as `md-` client order ids, never the request's intent ids).
pub struct PermissiveExit(pub World);

impl ExitPath for PermissiveExit {
    fn probe(&self) -> Result<(), Cause> {
        self.0.called(Stage::FlattenProbe);
        Ok(())
    }

    fn plan(
        &self,
        request: &FlattenRequest,
        journal: &dyn JournalWriter,
        stream: &str,
        clock: Option<mandate_runtime::RiskClock>,
    ) -> Result<FlattenPlan, Cause> {
        let _ = (journal, stream, clock);
        Ok(FlattenPlan {
            cancel_client_order_ids: request.working_orders.clone(),
            sells: Vec::new(),
            purpose: Purpose::Flatten,
            confirmation: request.confirmation.clone(),
        })
    }
}

/// Probes like the permissive exit, and then cannot plan: the mid-run failure of PB-8.
pub struct PlanlessExit(pub World);

impl ExitPath for PlanlessExit {
    fn probe(&self) -> Result<(), Cause> {
        self.0.called(Stage::FlattenProbe);
        Ok(())
    }

    fn plan(
        &self,
        request: &FlattenRequest,
        journal: &dyn JournalWriter,
        stream: &str,
        clock: Option<mandate_runtime::RiskClock>,
    ) -> Result<FlattenPlan, Cause> {
        let _ = (request, journal, stream, clock);
        Err(Cause::Gate(mandate_risk::GateError::Unimplemented(
            "agent_flatten",
            "E6-3",
        )))
    }
}

pub struct PermissiveProtection(pub World);

impl Protection for PermissiveProtection {
    fn probe(&self) -> Result<(), Cause> {
        self.0.called(Stage::ProtectionProbe);
        Ok(())
    }
}

pub struct FixtureMandate {
    pub world: World,
    pub environment: Environment,
}

impl MandateSource for FixtureMandate {
    fn admitted(&self) -> Result<Admitted, Cause> {
        self.world.called(Stage::Validate);
        Ok(Admitted {
            view: MandateView {
                version: "1".to_owned(),
                working_universe: BTreeSet::from([instrument()?]),
                restricted_instruments: BTreeSet::new(),
                approval: ApprovalSettings {
                    approvers: BTreeSet::new(),
                    author: "user-author".to_owned(),
                    timeout_s: 300,
                    environment: self.environment,
                },
            },
            environment: self.environment,
            model: ModelRef {
                id: "quant.ma_crossover".to_owned(),
                version: "1.0.0".to_owned(),
                content_hash: Digest::of(b"quant.ma_crossover:1.0.0"),
                max_output_age_s: 86_400,
                params: BTreeMap::from([
                    ("fast_periods".to_owned(), "5".to_owned()),
                    ("slow_periods".to_owned(), "20".to_owned()),
                ]),
            },
            symbol: "AAPL".to_owned(),
            governed: None,
        })
    }
}

pub struct FixtureBars(pub World);

impl Bars for FixtureBars {
    fn closes(&self, symbol: &str, now: UtcNanos) -> Result<Vec<Price>, Cause> {
        let _ = (symbol, now);
        self.0.called(Stage::MarketData);
        let close = Price::parse("255.2").map_err(|_| Cause::Absent { what: "a close" })?;
        Ok(vec![close; 25])
    }
}

pub struct FixedSignal {
    pub world: World,
    pub signal: Signal,
}

impl SignalModel for FixedSignal {
    fn signal(&self, model: &ModelRef, closes: &[Price]) -> Result<Signal, Cause> {
        let _ = (model, closes);
        self.world.called(Stage::Signal);
        Ok(self.signal)
    }
}

pub struct FixedReconciler {
    pub world: World,
    pub clean: bool,
}

impl Reconciler for FixedReconciler {
    fn snapshot(
        &mut self,
        connector: &mut dyn Connector,
        requests: &[BrokerRequest],
    ) -> Result<BrokerSnapshot, Cause> {
        let _ = (connector, requests);
        self.world.called(Stage::Reconcile);
        Ok(BrokerSnapshot {
            open_orders: Vec::new(),
            positions: Vec::new(),
            account: BrokerAccount {
                status: "ACTIVE".to_owned(),
                crypto_status: "ACTIVE".to_owned(),
                trading_blocked: false,
                account_blocked: false,
                trade_suspended_by_user: false,
                multiplier: 2,
                equity: Usd::ZERO,
                cash: Usd::ZERO,
                buying_power: Usd::ZERO,
                non_marginable_buying_power: Usd::ZERO,
                accrued_fees: Usd::ZERO,
                last_equity: Usd::ZERO,
                maintenance_margin: Usd::ZERO,
            },
            fills: Vec::new(),
            cursor: ActivityCursor(String::new()),
            reason: ReconcileReason::Startup,
            taken_at_head: Seq(0),
        })
    }

    fn reconcile(
        &mut self,
        snapshot: &BrokerSnapshot,
    ) -> Result<mandate_executor::Reconciliation, Cause> {
        let _ = snapshot;
        let result = if self.clean { "clean" } else { "mismatch" };
        let event_id = self.world.tally.borrow_mut().next_account_id();
        Ok(mandate_executor::Reconciliation {
            effects: vec![mandate_executor::Effect::Journal(
                mandate_executor::EventDraft {
                    event_id: mandate_executor::EventId(event_id),
                    event_type: "ReconciliationRun".to_owned(),
                    schema_version: 1,
                    config_refs: Object::new(),
                    causation_id: None,
                    payload: object(&[("result", result)])?,
                },
            )],
            verdict: if self.clean {
                mandate_executor::ReconciliationVerdict::Clean
            } else {
                mandate_executor::ReconciliationVerdict::Mismatch
            },
            differences: Vec::new(),
            expected_head: mandate_executor::Seq(0),
        })
    }
}

/// Sizes one share of AAPL once the fold holds a model output, and holds before.
pub struct OneShare {
    pub world: World,
    pub qty: &'static str,
}

impl Sizing for OneShare {
    fn size(&self, view: &MandateView, inputs: &SignalInputs) -> Result<Option<Proposal>, Cause> {
        let _ = view;
        self.world.called(Stage::Size);
        if inputs.outputs.is_empty() {
            return Ok(None);
        }
        Ok(Some(Proposal {
            instrument: instrument()?,
            asset_class: AssetClass::UsEquity,
            side: Side::Buy,
            qty: Qty::parse(self.qty).map_err(|_| Cause::Absent { what: "a qty" })?,
            limit: Price::parse("255.2").map_err(|_| Cause::Absent { what: "a limit" })?,
            purpose: Purpose::Open,
            exit_origin: None,
            exit_conviction: Some(Value::Str("1".to_owned())),
            buy_conviction: Some(Value::Str("1".to_owned())),
            combined_score: Value::Str("1".to_owned()),
            outputs_used: inputs.outputs.keys().cloned().collect(),
            model_weights: inputs
                .outputs
                .keys()
                .cloned()
                .map(|model| (model, Value::Str("1".to_owned())))
                .collect(),
            clips_applied: Vec::new(),
            execution: Some(OrderExecution {
                asset_class: AssetClass::UsEquity,
                tif: RuntimeTimeInForce::Day,
                protection_required: false,
                protection: None,
            }),
        }))
    }
}

pub struct FixedClassifier {
    pub world: World,
    pub autonomy: Autonomy,
}

impl Classifier for FixedClassifier {
    fn classify(&self, view: &MandateView, proposal: &Proposal) -> Result<Classified, Cause> {
        let _ = (view, proposal);
        self.world.called(Stage::Classify);
        Ok(Classified {
            autonomy: self.autonomy,
            decided_by: Some("default".to_owned()),
        })
    }
}

pub struct FixedGate {
    pub world: World,
    pub verdict: Verdict,
    pub checks: Vec<CheckOutcome>,
}

impl Gate for FixedGate {
    fn evaluate(&self, proposal: &Proposal) -> Result<Decision, Cause> {
        let _ = proposal;
        self.world.called(Stage::GateDryRun);
        let reason = match self.verdict {
            Verdict::Allow => None,
            Verdict::Deny | Verdict::Defer | Verdict::Hold => {
                Some(mandate_risk::ReasonCode::MaxOrderSize)
            }
        };
        Ok(Decision {
            verdict: self.verdict,
            reason,
            purpose: GatePurpose::Open,
            pacing: None,
            checks: self.checks.clone(),
            computed: Computed::default(),
        })
    }
}

/// The append protocol's head and epoch checks over an in-memory ledger. It checks no payload
/// schema: `mandate_journal::Draft::parse` registers none for the agent stream yet, which is a
/// finding for stream E rather than something a double should paper over (DEC-157 item 7).
pub struct LedgerJournal(pub World);

impl JournalWriter for LedgerJournal {
    fn take_ownership(&mut self, stream: &str) -> Result<u64, Cause> {
        let mut ledger = self.0.ledger.borrow_mut();
        let epoch = ledger.epochs.entry(stream.to_owned()).or_default();
        *epoch = epoch.saturating_add(1);
        Ok(*epoch)
    }

    fn read(&self, stream: &str) -> Result<Vec<StoredEvent>, Cause> {
        match self.0.ledger.borrow().streams.get(stream) {
            Some(rows) => Ok(rows.clone()),
            None => Ok(Vec::new()),
        }
    }

    fn append(
        &mut self,
        stream: &str,
        expected_head: u64,
        writer_epoch: u64,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        self.0.called(Stage::Journal);
        let mut ledger = self.0.ledger.borrow_mut();
        let current_epoch = match ledger.epochs.get(stream) {
            Some(epoch) => *epoch,
            None => 0,
        };
        if writer_epoch != current_epoch {
            return AppendOutcome::Fenced { current_epoch };
        }
        let rows = ledger.streams.entry(stream.to_owned()).or_default();
        let (head, prev_hash) = match rows.last() {
            Some(last) => (last.seq, last.hash),
            None => (0, Digest::ZERO),
        };
        if expected_head != head {
            return AppendOutcome::HeadMismatch {
                actual_seq: head,
                actual_hash: prev_hash,
            };
        }
        let mut committed = Vec::new();
        let mut seq = head;
        let mut prev = prev_hash;
        for bytes in drafts {
            let Ok(body) = mandate_canon::parse(bytes) else {
                return AppendOutcome::Unavailable;
            };
            let field = |name: &str| match body.get(name).and_then(Value::as_str) {
                Some(text) => text.to_owned(),
                None => String::new(),
            };
            seq = seq.saturating_add(1);
            let schema_version = match body.get("schema_version").and_then(Value::as_int) {
                Some(version) => version,
                None => return AppendOutcome::Unavailable,
            };
            let row = StoredEvent {
                stream_id: stream.to_owned(),
                seq,
                event_id: field("event_id"),
                event_type: field("event_type"),
                schema_version,
                environment: field("environment"),
                recorded_at: NOW.to_owned(),
                prev_hash: prev,
                hash: Digest::of(bytes),
                body: bytes.clone(),
            };
            prev = row.hash;
            committed.push(row);
        }
        rows.extend(committed.iter().cloned());
        AppendOutcome::Committed(committed)
    }
}

/// Converts the runtime's handoff into the executor's, joining the deployment's `AgentId`, and
/// checks the ledger for the `IntentProposed` before counting the hand.
pub struct ConvertingSink(pub World);

impl Sink for ConvertingSink {
    fn hand(&mut self, handoff: &IntentHandoff) -> Result<mandate_executor::IntentHandoff, Cause> {
        self.0.called(Stage::Sink);
        let recorded =
            self.0
                .ledger
                .borrow()
                .has("IntentProposed", "event_id", &handoff.intent_id.0);
        {
            let mut tally = self.0.tally.borrow_mut();
            tally.hands = tally.hands.saturating_add(1);
            if !recorded {
                tally.unrecorded_hands = tally.unrecorded_hands.saturating_add(1);
            }
        }
        let execution = handoff.execution.ok_or(Cause::Absent {
            what: "the doubled proposal's execution policy",
        })?;
        let body = match &handoff.body {
            IntentBody::Order {
                instrument,
                side,
                qty,
                limit,
                purpose,
            } => mandate_executor::IntentBody::Order {
                instrument: instrument.clone(),
                side: *side,
                qty: *qty,
                limit: *limit,
                purpose: executor_purpose(*purpose),
                protection: execution
                    .protection
                    .map(|prices| mandate_executor::ProtectionPrices {
                        stop: prices.stop,
                        take_profit: prices.take_profit,
                    }),
            },
            IntentBody::Flatten(plan) => {
                let _ = plan;
                return Err(Cause::Absent {
                    what: "the tracer hands no flatten",
                });
            }
        };
        Ok(mandate_executor::IntentHandoff {
            intent_id: IntentId(mandate_executor::EventId(handoff.intent_id.0.clone())),
            agent: mandate_executor::AgentId(AGENT.to_owned()),
            tif: Some(match execution.tif {
                RuntimeTimeInForce::Day => mandate_executor::TimeInForce::Day,
                RuntimeTimeInForce::Gtc => mandate_executor::TimeInForce::Gtc,
                RuntimeTimeInForce::Ioc => mandate_executor::TimeInForce::Ioc,
            }),
            body,
        })
    }
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

/// The executor's write-ahead protocol for one opening: `IntentReceived`, `OrderSubmitted`, then
/// the submission; a query after an unknown outcome; nothing after an absence. `inverted` puts the
/// submission before its `OrderSubmitted`, which is step 12's ordering bug.
pub struct PaperExecutor {
    pub world: World,
    pub seen: BTreeSet<String>,
    pub inverted: bool,
    pub last_client_order_id: Option<ClientOrderId>,
}

impl PaperExecutor {
    fn draft(
        &self,
        event_type: &str,
        causation: Option<&str>,
        members: &[(&str, &str)],
    ) -> Result<mandate_executor::Effect, Cause> {
        let event_id = self.world.tally.borrow_mut().next_account_id();
        let schema_version = if matches!(
            event_type,
            "IntentReceived" | "GateDecided" | "OrderSubmitted"
        ) {
            2
        } else if event_type == "OrderStateChanged" {
            1
        } else {
            return Err(Cause::Absent {
                what: "the doubled executor event's schema version",
            });
        };
        Ok(mandate_executor::Effect::Journal(
            mandate_executor::EventDraft {
                event_id: mandate_executor::EventId(event_id),
                event_type: event_type.to_owned(),
                schema_version,
                config_refs: Object::new(),
                causation_id: causation.map(|id| mandate_executor::EventId(id.to_owned())),
                payload: object(members)?,
            },
        ))
    }

    fn intent(
        &mut self,
        handoff: &mandate_executor::IntentHandoff,
    ) -> Result<Vec<mandate_executor::Effect>, Cause> {
        let intent = handoff.intent_id.0.0.clone();
        if !self.seen.insert(intent.clone()) {
            return Ok(Vec::new());
        }
        let mandate_executor::IntentBody::Order {
            instrument,
            side,
            qty,
            limit,
            purpose,
            ..
        } = &handoff.body
        else {
            return Err(Cause::Absent {
                what: "the tracer hands no flatten",
            });
        };
        let client = ClientOrderId::for_intent(&handoff.intent_id)?;
        self.last_client_order_id = Some(client.clone());
        let qty_text = qty.to_string();
        let limit_text = limit.to_string();
        let received = self.draft(
            "IntentReceived",
            Some(&intent),
            &[
                ("intent_id", intent.as_str()),
                ("instrument_id", instrument.as_str()),
            ],
        )?;
        let submitted = self.draft(
            "OrderSubmitted",
            Some(&intent),
            &[
                ("client_order_id", client.as_str()),
                ("instrument_id", instrument.as_str()),
                ("qty", qty_text.as_str()),
                ("limit_price", limit_text.as_str()),
            ],
        )?;
        let submit = mandate_executor::Effect::Broker(BrokerRequest::Submit(SubmitOrder {
            client_order_id: client,
            instrument: instrument.clone(),
            side: *side,
            qty: *qty,
            order_type: OrderType::Limit,
            tif: TimeInForce::Day,
            limit_price: Some(*limit),
            stop_price: None,
            bracket: None,
            oco: None,
            extended_hours: false,
            purpose: *purpose,
        }));
        if self.inverted {
            Ok(vec![received, submit, submitted])
        } else {
            Ok(vec![received, submitted, submit])
        }
    }

    fn answered(
        &self,
        answer: &Result<BrokerOutcome, BrokerUnknown>,
    ) -> Result<Vec<mandate_executor::Effect>, Cause> {
        if let Ok(BrokerOutcome::Submitted(order)) = answer {
            let id = match &order.client_order_id {
                Some(id) => id.clone(),
                None => String::new(),
            };
            return Ok(vec![self.draft(
                "OrderStateChanged",
                None,
                &[("client_order_id", id.as_str()), ("status", "accepted")],
            )?]);
        }
        if answer.is_err()
            && let Some(client) = &self.last_client_order_id
        {
            return Ok(vec![mandate_executor::Effect::Broker(
                BrokerRequest::GetOrderByClientId(client.clone()),
            )]);
        }
        Ok(Vec::new())
    }
}

impl Executor for PaperExecutor {
    fn reset(&mut self) -> Result<(), Cause> {
        self.seen.clear();
        self.last_client_order_id = None;
        Ok(())
    }

    fn step(
        &mut self,
        input: mandate_executor::Input,
    ) -> Result<Vec<mandate_executor::Effect>, Cause> {
        if !matches!(&input, mandate_executor::Input::Started(_)) {
            self.world.called(Stage::Executor);
        }
        if let mandate_executor::Input::Intent(handoff) = &input {
            let mut tally = self.world.tally.borrow_mut();
            tally.executor_intents = tally.executor_intents.saturating_add(1);
            drop(tally);
            return self.intent(handoff);
        }
        if let mandate_executor::Input::Broker(answer) = &input {
            {
                let mut tally = self.world.tally.borrow_mut();
                tally.executor_broker_inputs = tally.executor_broker_inputs.saturating_add(1);
            }
            return self.answered(answer);
        }
        if let mandate_executor::Input::BrokerUpdate(mandate_executor::BrokerUpdate::Account(
            account,
        )) = &input
        {
            let event_id = self.world.tally.borrow_mut().next_account_id();
            return Ok(vec![mandate_executor::Effect::Journal(
                mandate_executor::EventDraft {
                    event_id: mandate_executor::EventId(event_id),
                    event_type: "AccountStateObserved".to_owned(),
                    schema_version: 1,
                    config_refs: Object::new(),
                    causation_id: None,
                    payload: account_payload(account)?,
                },
            )]);
        }
        Ok(Vec::new())
    }

    fn committed(&mut self, event: &mandate_executor::FoldedEvent) -> Result<(), Cause> {
        self.world
            .tally
            .borrow_mut()
            .executor_folds
            .push(event.stream.clone());
        Ok(())
    }
}

/// What the connector double answers a submission with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Accept,
    /// The submission times out, and the query finds no order.
    UnknownThenAbsent,
    /// The broker answers something the connector cannot read.
    Unreadable,
    /// The connector refuses the submission before it leaves the process.
    NotSent,
}

/// The broker, scripted. It counts every submission before anything interprets it and checks the
/// ledger for the `OrderSubmitted` that should precede it.
pub struct ScriptedConnector {
    pub world: World,
    pub script: Script,
}

impl Connector for ScriptedConnector {
    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        self.world.called(Stage::Connector);
        if let BrokerRequest::Submit(order) = request {
            let recorded = self.world.ledger.borrow().has(
                "OrderSubmitted",
                "client_order_id",
                order.client_order_id.as_str(),
            );
            {
                let mut tally = self.world.tally.borrow_mut();
                tally.submissions = tally.submissions.saturating_add(1);
                tally.submitted.push(order.clone());
                if !recorded {
                    tally.unrecorded_submissions = tally.unrecorded_submissions.saturating_add(1);
                }
            }
            return match self.script {
                Script::Accept => Ok(BrokerOutcome::Submitted(accepted(order)?)),
                Script::UnknownThenAbsent => Err(ConnectorError::Unknown(BrokerUnknown::Timeout)),
                Script::Unreadable => Err(ConnectorError::Unreadable { code: "wire" }),
                Script::NotSent => Err(ConnectorError::NotSent {
                    code: "refused_path",
                }),
            };
        }
        if let BrokerRequest::GetOrderByClientId(client) = request {
            let mut tally = self.world.tally.borrow_mut();
            tally.queries = tally.queries.saturating_add(1);
            return Ok(BrokerOutcome::Absent {
                client_order_id: client.as_str().to_owned(),
            });
        }
        Err(ConnectorError::NotSent {
            code: "not_scripted",
        })
    }
}

fn accepted(order: &SubmitOrder) -> Result<BrokerOrder, ConnectorError> {
    Ok(BrokerOrder {
        broker_order_id: "569fca5f-d21f-461a-9e3f-21311cf912f0".to_owned(),
        client_order_id: Some(order.client_order_id.as_str().to_owned()),
        instrument: order.instrument.clone(),
        side: order.side,
        qty: order.qty,
        filled_qty: Qty::parse("0").map_err(|_| ConnectorError::Unreadable { code: "qty" })?,
        limit_price: order.limit_price,
        stop_price: None,
        status: "new".to_owned(),
        reject_code: None,
        replaced_by_broker_order_id: None,
        legs: Vec::new(),
        created_on: None,
    })
}

/// Each stage's own crate's refusal, the stub the fail-closed suite puts in that stage's place.
pub fn stub(stages: &mut Stages, stage: Stage, world: &World) {
    match stage {
        Stage::FlattenProbe => stages.exit = Box::new(Stubbed(world.clone(), stage)),
        Stage::ProtectionProbe => stages.protection = Box::new(Stubbed(world.clone(), stage)),
        Stage::Validate => stages.mandate = Box::new(Stubbed(world.clone(), stage)),
        Stage::MarketData => stages.bars = Box::new(Stubbed(world.clone(), stage)),
        Stage::Signal => stages.signal = Box::new(Stubbed(world.clone(), stage)),
        Stage::Reconcile => stages.reconciler = Box::new(Stubbed(world.clone(), stage)),
        Stage::Size => stages.sizing = Box::new(Stubbed(world.clone(), stage)),
        Stage::Classify => stages.classifier = Box::new(Stubbed(world.clone(), stage)),
        Stage::GateDryRun => stages.gate = Box::new(Stubbed(world.clone(), stage)),
        Stage::Journal => stages.journal = Box::new(Stubbed(world.clone(), stage)),
        Stage::Sink => stages.sink = Box::new(Stubbed(world.clone(), stage)),
        Stage::Executor => stages.executor = Box::new(Stubbed(world.clone(), stage)),
        Stage::Connector => stages.connector = Box::new(Stubbed(world.clone(), stage)),
    }
}

/// The refusal each stage's own crate gives while it cannot answer. Where the crate is already
/// implemented (market data, the signal) it is the error that crate gives for missing input.
pub fn crate_refusal(stage: Stage) -> Cause {
    match stage {
        Stage::FlattenProbe => Cause::Gate(mandate_risk::GateError::Unimplemented(
            "agent_flatten",
            "E6-3",
        )),
        Stage::ProtectionProbe => {
            Cause::Executor(mandate_executor::ExecutorError::Unimplemented { story: "E7-4" })
        }
        Stage::Validate => Cause::Spec(mandate_spec::SpecError::Unimplemented),
        Stage::MarketData => Cause::Absent {
            what: "the dataset has no partition for the instrument",
        },
        Stage::Signal => Cause::Backtest(mandate_backtest::BacktestError::NoBars),
        Stage::Reconcile => {
            Cause::Executor(mandate_executor::ExecutorError::Unimplemented { story: "E7-3" })
        }
        Stage::Size | Stage::Classify => {
            Cause::Builder(mandate_builder::BuilderError::Unimplemented)
        }
        Stage::GateDryRun => {
            Cause::Gate(mandate_risk::GateError::Unimplemented("evaluate", "E6-6"))
        }
        Stage::Journal => Cause::Append {
            outcome: "Unavailable",
        },
        Stage::Sink => Cause::Sink(mandate_runtime::SinkError::Unavailable),
        Stage::Executor => {
            Cause::Executor(mandate_executor::ExecutorError::Unimplemented { story: "E7-3" })
        }
        Stage::Connector => Cause::Connector(ConnectorError::NotSent {
            code: "unimplemented",
        }),
    }
}

/// One stubbed stage: it records that it was asked, then refuses with its crate's error.
pub struct Stubbed(pub World, pub Stage);

impl Stubbed {
    fn refuse<T>(&self) -> Result<T, Cause> {
        self.0.called(self.1);
        Err(crate_refusal(self.1))
    }
}

impl ExitPath for Stubbed {
    fn probe(&self) -> Result<(), Cause> {
        self.refuse()
    }

    fn plan(
        &self,
        request: &FlattenRequest,
        journal: &dyn JournalWriter,
        stream: &str,
        clock: Option<mandate_runtime::RiskClock>,
    ) -> Result<FlattenPlan, Cause> {
        let _ = (request, journal, stream, clock);
        self.refuse()
    }
}

impl Protection for Stubbed {
    fn probe(&self) -> Result<(), Cause> {
        self.refuse()
    }
}

impl MandateSource for Stubbed {
    fn admitted(&self) -> Result<Admitted, Cause> {
        self.refuse()
    }
}

impl Bars for Stubbed {
    fn closes(&self, symbol: &str, now: UtcNanos) -> Result<Vec<Price>, Cause> {
        let _ = (symbol, now);
        self.refuse()
    }
}

impl SignalModel for Stubbed {
    fn signal(&self, model: &ModelRef, closes: &[Price]) -> Result<Signal, Cause> {
        let _ = (model, closes);
        self.refuse()
    }
}

impl Reconciler for Stubbed {
    fn snapshot(
        &mut self,
        connector: &mut dyn Connector,
        requests: &[BrokerRequest],
    ) -> Result<BrokerSnapshot, Cause> {
        let _ = (connector, requests);
        self.refuse()
    }

    fn reconcile(
        &mut self,
        snapshot: &BrokerSnapshot,
    ) -> Result<mandate_executor::Reconciliation, Cause> {
        let _ = snapshot;
        self.refuse()
    }
}

impl Sizing for Stubbed {
    fn size(&self, view: &MandateView, inputs: &SignalInputs) -> Result<Option<Proposal>, Cause> {
        let _ = (view, inputs);
        self.refuse()
    }
}

impl Classifier for Stubbed {
    fn classify(&self, view: &MandateView, proposal: &Proposal) -> Result<Classified, Cause> {
        let _ = (view, proposal);
        self.refuse()
    }
}

impl Gate for Stubbed {
    fn evaluate(&self, proposal: &Proposal) -> Result<Decision, Cause> {
        let _ = proposal;
        self.refuse()
    }
}

impl JournalWriter for Stubbed {
    fn take_ownership(&mut self, stream: &str) -> Result<u64, Cause> {
        let _ = stream;
        Ok(1)
    }

    fn read(&self, stream: &str) -> Result<Vec<StoredEvent>, Cause> {
        let _ = stream;
        Ok(Vec::new())
    }

    fn append(
        &mut self,
        stream: &str,
        expected_head: u64,
        writer_epoch: u64,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        let _ = (stream, expected_head, writer_epoch, drafts);
        self.0.called(self.1);
        AppendOutcome::Unavailable
    }
}

impl Sink for Stubbed {
    fn hand(&mut self, handoff: &IntentHandoff) -> Result<mandate_executor::IntentHandoff, Cause> {
        let _ = handoff;
        self.refuse()
    }
}

impl Executor for Stubbed {
    fn reset(&mut self) -> Result<(), Cause> {
        Ok(())
    }

    fn step(
        &mut self,
        input: mandate_executor::Input,
    ) -> Result<Vec<mandate_executor::Effect>, Cause> {
        let _ = input;
        self.refuse()
    }

    fn committed(&mut self, event: &mandate_executor::FoldedEvent) -> Result<(), Cause> {
        let _ = event;
        Ok(())
    }
}

impl Connector for Stubbed {
    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        let _ = request;
        self.0.called(self.1);
        Err(ConnectorError::NotSent {
            code: "unimplemented",
        })
    }
}
