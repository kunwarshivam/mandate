//! The doubles the executor process's tests share: a journal that sequences what it is given and
//! can be told to fail, a paper broker that records every request in order, and the executor
//! core's ports for one liquid equity.
//!
//! The journal checks epochs, heads and idempotency as journal spec §5.1 says, and no payload
//! schema: these tests are about the loop (what it reads, in what order, when it may send), and
//! the core's own records are proved against the journal in `mandate-executor`'s tests.

#![allow(dead_code, reason = "each test file uses its own part of the support")]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;
use std::time::Duration;

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_canon::{Digest, Object, Value, parse};
use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{
    AccountRef, AccountScope, AgentId, BindingGateInput, BindingGateRequest, BindingGateSource,
    BrokerAccount, BrokerOrder, BrokerOutcome, BrokerPosition, BrokerRequest, BrokerUnknown,
    ConnectorError, EventId, ExecutorConfig, ExitTier, IdGen, InstrumentSnapshot, MandateVersion,
    MandateView, Seq, WorkspaceId, WriterEpoch,
};
use mandate_executord::{Broker, Config, ExecutorProcess, Wiring};
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_num::{Fraction, Price, Qty, ShareIncrement, SignedQty, Usd};
use mandate_store::Store;
use mandate_store::envelope::{Envelope, Fields, draft_bytes};
use mandate_time::UtcNanos;

pub const WORKSPACE: &str = "ws1";
pub const ACCOUNT: &str = "acct1";
pub const OTHER_ACCOUNT: &str = "acct2";
pub const CONNECTION: &str = "conn1";
pub const OTHER_CONNECTION: &str = "conn2";
pub const AGENT: &str = "agenta";
pub const OTHER_AGENT: &str = "agentb";
pub const ACCOUNT_STREAM: &str = "acct:ws1:acct1";
pub const CONTROL_STREAM: &str = "ctl:ws1";
pub const CLOCK_STREAM: &str = "clock:ws1";
pub const AAPL: &str = "AAPL";
pub const VERSION: &str = "sha256:5555555555555555555555555555555555555555555555555555555555555555";
/// 11:00 New York on a Monday: the regular session, where an equity opening may go.
pub const NOON: &str = "2026-09-21T15:00:00Z";
/// The account stream an AAPL opening of 10 at 150 leaves, recorded from the executor core with
/// these doubles: the startup reconciliation, then the intent received, its intended protection,
/// the gate's allow, the request, the submission, and the broker's acceptance.
pub const OPENED: &str = include_str!("../fixtures/opened.jsonl");
/// The client order id the recorded opening was submitted under.
pub const OPENING: &str = "md-01JINTENT0000000000000000A";

pub type Error = Box<dyn std::error::Error>;

pub fn at(text: &str) -> Result<UtcNanos, Error> {
    Ok(UtcNanos::parse_rfc3339(text)?)
}

pub fn agent_stream(agent: &str) -> String {
    format!("agent:{WORKSPACE}:{agent}")
}

pub fn stream(id: &str) -> Result<StreamId, Error> {
    StreamId::parse(id).ok_or_else(|| format!("stream id {id}").into())
}

/// A failure the journal answers to the next appends, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// Nothing written; the writer retries the same drafts.
    Unavailable,
    /// Written, and the answer lost.
    LostAnswer,
}

#[derive(Default)]
struct Rows {
    streams: BTreeMap<String, Vec<StoredEvent>>,
    epochs: BTreeMap<String, u64>,
    faults: VecDeque<Fault>,
    unreadable: BTreeSet<String>,
    appends: usize,
    /// After this many appends the journal answers nothing more, as a process that crashed would
    /// find it: every append `Unavailable` and every read refused.
    crash_after: Option<usize>,
}

/// One journal shared by every process and the test, so two processes can contend for a stream.
#[derive(Clone, Default)]
pub struct Journal(Rc<RefCell<Rows>>);

impl Journal {
    pub fn rows_of(&self, stream: &str) -> Vec<StoredEvent> {
        self.0
            .borrow()
            .streams
            .get(stream)
            .cloned()
            .unwrap_or_default()
    }

    pub fn types(&self, stream: &str) -> Vec<String> {
        self.rows_of(stream)
            .into_iter()
            .map(|row| row.event_type)
            .collect()
    }

    /// The payloads of the account stream's records of `event_type`, in order.
    pub fn payloads(&self, event_type: &str) -> Vec<Value> {
        self.rows_of(ACCOUNT_STREAM)
            .iter()
            .filter(|row| row.event_type == event_type)
            .filter_map(|row| parse(&row.body).ok()?.get("payload").cloned())
            .collect()
    }

    /// The agents the account stream's `KillSwitchActivated` records name, in order.
    pub fn switched(&self) -> Vec<String> {
        self.payloads("KillSwitchActivated")
            .iter()
            .filter_map(|p| p.get("subject")?.as_str().map(str::to_owned))
            .collect()
    }

    /// The intents the account stream received, by the `intent_id` each `IntentReceived` names.
    pub fn received(&self) -> Vec<String> {
        self.payloads("IntentReceived")
            .iter()
            .filter_map(|p| p.get("intent_id")?.as_str().map(str::to_owned))
            .collect()
    }

    pub fn fail_next(&self, faults: &[Fault]) {
        self.0.borrow_mut().faults.extend(faults.iter().copied());
    }

    pub fn clear_faults(&self) {
        self.0.borrow_mut().faults.clear();
    }

    pub fn unreadable(&self, stream: &str) {
        self.0.borrow_mut().unreadable.insert(stream.to_owned());
    }

    pub fn readable(&self, stream: &str) {
        self.0.borrow_mut().unreadable.remove(stream);
    }

    pub fn crash_after(&self, appends: Option<usize>) {
        let mut rows = self.0.borrow_mut();
        rows.crash_after = appends.map(|n| rows.appends.saturating_add(n));
    }

    pub fn crashed(&self) -> bool {
        let rows = self.0.borrow();
        rows.crash_after.is_some_and(|n| rows.appends >= n)
    }

    pub fn appends(&self) -> usize {
        self.0.borrow().appends
    }

    /// Stores `bodies` on `stream_id` as an earlier executor wrote them, for a process to replay.
    pub fn seed(&self, stream_id: &str, bodies: &str) -> Result<(), Error> {
        let mut rows = self.0.borrow_mut();
        let stored = rows.streams.entry(stream_id.to_owned()).or_default();
        for body in bodies.lines().filter(|line| !line.is_empty()) {
            let row = sealed(stream_id, stored.len(), body.as_bytes()).ok_or("seal")?;
            stored.push(row);
        }
        Ok(())
    }

    /// Writes one record of another stream's writer: the control services', a runtime's, or the
    /// scheduler's.
    pub fn write(
        &self,
        stream_id: &str,
        event_id: &str,
        event_type: &str,
        schema_version: u64,
        causation_id: Option<&str>,
        payload: Value,
    ) -> Result<(), Error> {
        let envelope = Envelope {
            stream: stream_id,
            environment: Environment::Paper,
            actor_kind: "system",
            actor_id: "test",
            build: "test/0",
            version: "0",
            event_time: at(NOON)?,
        };
        let bytes = draft_bytes(
            &envelope,
            &Fields {
                event_id,
                event_type,
                schema_version,
                causation_id,
                correlation_id: None,
                config_refs: &Object::new(),
                payload: &payload,
            },
        )
        .ok_or("draft")?;
        let mut rows = self.0.borrow_mut();
        let stored = rows.streams.entry(stream_id.to_owned()).or_default();
        let row = sealed(stream_id, stored.len(), &bytes).ok_or("seal")?;
        stored.push(row);
        Ok(())
    }
}

/// A stored row for `bytes` at the next sequence, its members read back from the body.
fn sealed(stream_id: &str, before: usize, bytes: &[u8]) -> Option<StoredEvent> {
    let body = parse(bytes).ok()?;
    let text = |member: &str| body.get(member)?.as_str().map(str::to_owned);
    Some(StoredEvent {
        stream_id: stream_id.to_owned(),
        seq: u64::try_from(before).ok()?.checked_add(1)?,
        event_id: text("event_id")?,
        event_type: text("event_type")?,
        schema_version: body.get("schema_version")?.as_int()?,
        environment: text("environment")?,
        recorded_at: NOON.to_owned(),
        prev_hash: Digest::ZERO,
        hash: Digest::of(bytes),
        body: bytes.to_vec(),
    })
}

impl Store for Journal {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        let mut rows = self.0.borrow_mut();
        let epoch = rows.epochs.entry(stream.as_str().to_owned()).or_insert(0);
        *epoch = epoch.checked_add(1)?;
        Some(*epoch)
    }

    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        if self.crashed() || self.0.borrow().unreadable.contains(stream.as_str()) {
            return None;
        }
        Some(self.rows_of(stream.as_str()))
    }

    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        epoch: u64,
        _recorded_at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        if self.crashed() {
            return AppendOutcome::Unavailable;
        }
        let mut rows = self.0.borrow_mut();
        rows.appends = rows.appends.saturating_add(1);
        let current = rows.epochs.get(stream.as_str()).copied().unwrap_or(0);
        if epoch < current {
            return AppendOutcome::Fenced {
                current_epoch: current,
            };
        }
        let fault = rows.faults.pop_front();
        if fault == Some(Fault::Unavailable) {
            return AppendOutcome::Unavailable;
        }
        let stored = rows.streams.entry(stream.as_str().to_owned()).or_default();
        let ids: Vec<Option<String>> = drafts
            .iter()
            .map(|d| parse(d).ok()?.get("event_id")?.as_str().map(str::to_owned))
            .collect();
        let existing: Vec<StoredEvent> = stored
            .iter()
            .filter(|row| ids.contains(&Some(row.event_id.clone())))
            .cloned()
            .collect();
        if !existing.is_empty() {
            let same = existing.len() == drafts.len()
                && existing.iter().zip(drafts).all(|(row, d)| row.body == *d);
            return if same {
                AppendOutcome::AlreadyCommitted(existing)
            } else {
                AppendOutcome::IdempotencyConflict {
                    stored_seq: existing.first().map_or(0, |row| row.seq),
                }
            };
        }
        let head = u64::try_from(stored.len()).unwrap_or(u64::MAX);
        if head != expected_head {
            return AppendOutcome::HeadMismatch {
                actual_seq: head,
                actual_hash: Digest::ZERO,
            };
        }
        let mut committed = Vec::new();
        for draft in drafts {
            let Some(row) = sealed(stream.as_str(), stored.len(), draft) else {
                return AppendOutcome::Unavailable;
            };
            stored.push(row.clone());
            committed.push(row);
        }
        if fault == Some(Fault::LostAnswer) {
            AppendOutcome::Ambiguous
        } else {
            AppendOutcome::Committed(committed)
        }
    }
}

/// One request the broker saw, and whether, as it arrived, the account stream already held the
/// `OrderSubmitted` naming it (`AGENTS.md` rule 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub request: BrokerRequest,
    pub journaled_first: bool,
}

#[derive(Default)]
struct Books {
    seen: Vec<Seen>,
    orders: BTreeMap<String, BrokerOrder>,
    positions: Vec<BrokerPosition>,
    /// Requests answered `Unknown` after they took effect: the answer is lost on the way back.
    lose_answers: usize,
    sequence: u64,
}

/// A paper broker in process: an order is accepted and rests, nothing fills, and every request is
/// recorded in arrival order. It reads the journal only to record whether each submission was
/// journaled first, and answers nothing once the journal has crashed, as the process is gone.
#[derive(Clone)]
pub struct Paper {
    books: Rc<RefCell<Books>>,
    journal: Journal,
}

impl Paper {
    pub fn new(journal: &Journal) -> Self {
        Self {
            books: Rc::default(),
            journal: journal.clone(),
        }
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.books.borrow().seen.clone()
    }

    /// The client order ids of every submission, in order.
    pub fn submitted(&self) -> Vec<String> {
        self.seen()
            .into_iter()
            .filter_map(|seen| match seen.request {
                BrokerRequest::Submit(order) => Some(order.client_order_id.as_str().to_owned()),
                _ => None,
            })
            .collect()
    }

    pub fn holding(&self, instrument: &str, qty: &str) -> Result<(), Error> {
        self.books.borrow_mut().positions.push(BrokerPosition {
            instrument: InstrumentId::new(instrument)?,
            qty: SignedQty::parse(qty)?,
            avg_entry_price: Price::parse("150")?,
        });
        Ok(())
    }

    /// An order the broker already holds, accepted and resting, under our `client_order_id`.
    pub fn resting(
        &self,
        client_order_id: &str,
        instrument: &str,
        qty: &str,
        limit: &str,
    ) -> Result<(), Error> {
        let mut books = self.books.borrow_mut();
        books.sequence = books.sequence.saturating_add(1);
        let order = BrokerOrder {
            broker_order_id: format!("b{}", books.sequence),
            client_order_id: Some(client_order_id.to_owned()),
            instrument: InstrumentId::new(instrument)?,
            side: Side::Buy,
            qty: Qty::parse(qty)?,
            filled_qty: Qty::ZERO,
            limit_price: Some(Price::parse(limit)?),
            stop_price: None,
            status: "new".to_owned(),
            reject_code: None,
            replaced_by_broker_order_id: None,
            legs: Vec::new(),
            created_on: None,
        };
        books.orders.insert(client_order_id.to_owned(), order);
        Ok(())
    }

    pub fn lose_next_answers(&self, n: usize) {
        self.books.borrow_mut().lose_answers = n;
    }
}

impl Broker for Paper {
    fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
        mandate_alpaca::alpaca_profile()
    }

    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        if self.journal.crashed() {
            return Err(ConnectorError::NotSent { code: "crashed" });
        }
        let journaled_first = match request {
            BrokerRequest::Submit(order) => {
                self.journal.payloads("OrderSubmitted").iter().any(|p| {
                    p.get("client_order_id").and_then(Value::as_str)
                        == Some(order.client_order_id.as_str())
                })
            }
            _ => true,
        };
        let mut books = self.books.borrow_mut();
        books.seen.push(Seen {
            request: request.clone(),
            journaled_first,
        });
        let outcome = answer(&mut books, request)?;
        if books.lose_answers > 0 {
            books.lose_answers = books.lose_answers.saturating_sub(1);
            return Err(ConnectorError::Unknown(BrokerUnknown::Timeout));
        }
        Ok(outcome)
    }
}

fn answer(books: &mut Books, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
    let not_sent = ConnectorError::NotSent { code: "fixture" };
    Ok(match request {
        BrokerRequest::Submit(order) => {
            let id = order.client_order_id.as_str().to_owned();
            if books.orders.contains_key(&id) {
                return Ok(BrokerOutcome::DuplicateClientOrderId {
                    client_order_id: id,
                });
            }
            books.sequence = books.sequence.saturating_add(1);
            let accepted = BrokerOrder {
                broker_order_id: format!("b{}", books.sequence),
                client_order_id: Some(id.clone()),
                instrument: order.instrument.clone(),
                side: order.side,
                qty: order.qty,
                filled_qty: Qty::ZERO,
                limit_price: order.limit_price,
                stop_price: order.stop_price,
                status: "new".to_owned(),
                reject_code: None,
                replaced_by_broker_order_id: None,
                legs: Vec::new(),
                created_on: None,
            };
            books.orders.insert(id, accepted.clone());
            BrokerOutcome::Submitted(accepted)
        }
        BrokerRequest::GetOrderByClientId(id) => match books.orders.get(id.as_str()) {
            Some(order) => BrokerOutcome::Order(order.clone()),
            None => BrokerOutcome::Absent {
                client_order_id: id.as_str().to_owned(),
            },
        },
        BrokerRequest::Cancel { client_order_id } => BrokerOutcome::CancelAccepted {
            client_order_id: client_order_id.as_str().to_owned(),
        },
        BrokerRequest::ListOpenOrders => {
            BrokerOutcome::OpenOrders(books.orders.values().cloned().collect())
        }
        BrokerRequest::ListPositions => BrokerOutcome::Positions(books.positions.clone()),
        BrokerRequest::GetAccount => BrokerOutcome::Account(account().map_err(|_| not_sent)?),
        BrokerRequest::ListActivities { since } => BrokerOutcome::Activities {
            fills: Vec::new(),
            cursor: since.clone(),
        },
        _ => return Err(not_sent),
    })
}

fn account() -> Result<BrokerAccount, Error> {
    let usd = |text: &str| Usd::parse(text);
    Ok(BrokerAccount {
        status: "ACTIVE".to_owned(),
        crypto_status: "ACTIVE".to_owned(),
        trading_blocked: false,
        account_blocked: false,
        trade_suspended_by_user: false,
        multiplier: 1,
        equity: usd("20000")?,
        cash: usd("20000")?,
        buying_power: usd("20000")?,
        non_marginable_buying_power: usd("20000")?,
        accrued_fees: usd("0")?,
        last_equity: usd("20000")?,
        maintenance_margin: usd("0")?,
    })
}

/// Event ids derived from `(epoch, head, ordinal)`, alphanumeric as a ULID is.
pub struct Ids;

impl IdGen for Ids {
    fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId {
        EventId(format!("e{}h{}o{}", epoch.0, head.0, ordinal))
    }
}

pub struct Mandate;

impl MandateView for Mandate {
    fn version(&self, _agent: &AgentId) -> Option<MandateVersion> {
        Some(MandateVersion(VERSION.to_owned()))
    }

    fn crypto_stop_limit_offset(&self, _agent: &AgentId) -> Option<Fraction> {
        Fraction::parse("0.005").ok()
    }

    fn covers(&self, _agent: &AgentId, instrument: &InstrumentId) -> bool {
        instrument.as_str() == AAPL
    }
}

pub struct Instruments;

impl InstrumentSnapshot for Instruments {
    fn asset_class(&self, instrument: &InstrumentId) -> Option<AssetClass> {
        (instrument.as_str() == AAPL).then_some(AssetClass::UsEquity)
    }

    fn increment(&self, instrument: &InstrumentId) -> Option<ShareIncrement> {
        (instrument.as_str() == AAPL).then_some(ShareIncrement::Whole)
    }

    fn exit_tier(&self, instrument: &InstrumentId) -> Option<ExitTier> {
        (instrument.as_str() == AAPL).then_some(ExitTier {
            exit_offset: Fraction::parse("0.005").ok()?,
            exit_offset_step: Fraction::parse("0.005").ok()?,
            max_exit_offset: Fraction::parse("0.03").ok()?,
        })
    }
}

/// The binding gate's facts for no order: every lookup is unavailable, so the core can admit no
/// opening (the next slice brings the facts an opening needs).
pub struct NoFacts;

impl BindingGateSource for NoFacts {
    fn input(&self, _request: &BindingGateRequest<'_>) -> Option<BindingGateInput> {
        None
    }
}

pub fn config() -> ExecutorConfig {
    ExecutorConfig::PROPOSED
}

pub fn fees() -> Result<mandate_accounting::Config, Error> {
    let date = |text: &str| mandate_time::Date::parse(text);
    Ok(mandate_accounting::Config {
        equities: mandate_accounting::EquityFees {
            sec_rate: mandate_num::FeeRate::parse("0.00003")?,
            taf_per_share: mandate_num::FeePerShare::parse("0.0002")?,
            taf_cap: mandate_num::FeeCap::parse("9.79")?,
            fee_rounding: mandate_accounting::FeeRounding::DailyTotal,
            taf_cap_basis: mandate_accounting::TafCapBasis::PerExecution,
            cat_per_share: mandate_num::FeePerShare::parse("0.00001")?,
        },
        crypto: mandate_accounting::CryptoFees {
            maker: mandate_num::Bps::parse("15")?,
            taker: mandate_num::Bps::parse("25")?,
        },
        calendar: mandate_time::TradingCalendar::new(
            date("2026-09-01")?,
            date("2026-12-31")?,
            [date("2026-11-26")?, date("2026-12-25")?],
            [date("2026-10-12")?, date("2026-11-11")?],
        )?,
    })
}

pub fn wiring() -> Result<Wiring, Error> {
    Ok(Wiring {
        ids: Box::new(Ids),
        mandates: Box::new(Mandate),
        instruments: Box::new(Instruments),
        config: config(),
        fees: fees()?,
        gate: Box::new(NoFacts),
    })
}

pub fn process_config(environment: Environment) -> Config {
    chained(environment, 64)
}

pub fn chained(environment: Environment, max_chain: usize) -> Config {
    Config {
        scope: AccountScope {
            account: AccountRef(ACCOUNT.to_owned()),
            workspace: WorkspaceId(WORKSPACE.to_owned()),
        },
        environment,
        liveness: Duration::from_secs(10),
        max_chain,
    }
}

pub fn start(journal: &Journal, broker: &Paper) -> Result<ExecutorProcess<Journal, Paper>, Error> {
    Ok(ExecutorProcess::start(
        journal.clone(),
        broker.clone(),
        wiring()?,
        &process_config(Environment::Paper),
        at(NOON)?,
    ))
}

pub fn object(members: Vec<(&'static str, Value)>) -> Result<Value, Error> {
    mandate_store::envelope::object(members).ok_or_else(|| "object".into())
}

fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

/// The control stream of a workspace whose connection `conn1` is account `acct1`, with agent
/// `agenta` deployed on it, and the scheduler's tick at noon.
pub fn deployed() -> Result<Journal, Error> {
    let journal = Journal::default();
    connect(&journal, "c1", CONNECTION, ACCOUNT)?;
    deploy(&journal, "c2", AGENT, CONNECTION)?;
    tick(&journal, "t1", NOON)?;
    Ok(journal)
}

pub fn connect(journal: &Journal, id: &str, connection: &str, account: &str) -> Result<(), Error> {
    journal.write(
        CONTROL_STREAM,
        id,
        "ConnectionEstablished",
        2,
        None,
        object(vec![
            ("account_ref", text(account)),
            ("connection_id", text(connection)),
            ("environment", text("paper")),
        ])?,
    )
}

pub fn deploy(journal: &Journal, id: &str, agent: &str, connection: &str) -> Result<(), Error> {
    journal.write(
        CONTROL_STREAM,
        id,
        "AgentDeployed",
        2,
        None,
        object(vec![
            ("agent_id", text(agent)),
            ("connection_id", text(connection)),
            ("mandate_version", text(VERSION)),
        ])?,
    )
}

pub fn tick(journal: &Journal, id: &str, clock: &str) -> Result<(), Error> {
    journal.write(
        CLOCK_STREAM,
        id,
        "ClockAdvanced",
        1,
        None,
        object(vec![("clock", text(&at(clock)?.to_string()))])?,
    )
}

/// The runtime's copy of an owner's kill switch of the whole agent (journal spec §9.1).
pub fn kill(journal: &Journal, agent: &str, id: &str) -> Result<(), Error> {
    kill_scoped(journal, agent, id, "agent", agent)
}

/// The runtime's copy of a kill switch of `scope` and `subject` on `agent`'s stream.
pub fn kill_scoped(
    journal: &Journal,
    agent: &str,
    id: &str,
    scope: &str,
    subject: &str,
) -> Result<(), Error> {
    journal.write(
        &agent_stream(agent),
        id,
        "KillSwitchActivated",
        1,
        Some("command"),
        object(vec![
            ("initiator", text("owner")),
            ("mode_event", Value::Null),
            ("scope", text(scope)),
            ("subject", text(subject)),
        ])?,
    )
}
