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

use mandate_accounting::{AccountType, AssetClass, InstrumentId, Side};
use mandate_canon::{Digest, Object, Value, parse};
use mandate_connections::checks::{AccountRead, CheckReport, Occasion};
use mandate_connections::record::ConnectionId;
use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{
    AccountRef, AccountScope, ActivityCursor, AgentId, BindingGateConfigRefs, BindingGateInput,
    BindingGateRequest, BindingGateSource, BrokerAccount, BrokerFill, BrokerOrder, BrokerOutcome,
    BrokerPosition, BrokerReject, BrokerRequest, BrokerUnknown, ConnectorError, EventId,
    ExecutorConfig, ExitTier, FillConfigRefs, FillId, IdGen, InstrumentSnapshot, MandateVersion,
    MandateView, ProtectionPrices, Purpose, Seq, WorkspaceId, WriterEpoch, equity_bracket_prices,
};
use mandate_executord::marks::NoMarks;
use mandate_executord::quotes::NoQuotes;
use mandate_executord::risk::Unopened;
use mandate_executord::serve::ScriptedBroker;
use mandate_executord::{Broker, Config, ExecutorProcess, Protections, Unplanned, Wiring};
use mandate_journal::{AppendOutcome, Environment, Invalid, InvalidReason, StoredEvent, StreamId};
use mandate_num::{Fraction, Price, Qty, Ratio, ShareIncrement, SignedQty, Usd};
use mandate_risk::spec_types::{GoalState, RiskLimits};
use mandate_risk::{
    AccountSnapshot, AccountState as GateAccountState, AgentId as GateAgentId, AgentMode,
    AgentSnapshot, AssetId, ClientOrderId as GateOrderId, ConductState, DayTradeLedger,
    DayTradeRegime, EtpClass, Exchange, GateConfig, InstrumentSnapshot as GateInstrumentSnapshot,
    MarketSnapshot, QuoteCurrency, RiskSnapshot, SaneQuote, ValidatedMandate, WorkingUniverse,
};
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
    /// Nothing written: the journal refuses the batch as invalid, as it refuses a draft its schema
    /// does not admit (DEC-2805).
    Invalid,
    /// Nothing written: a newer writer holds the stream, and this one is fenced (journal spec §2).
    Fenced,
}

#[derive(Default)]
struct Rows {
    streams: BTreeMap<String, Vec<StoredEvent>>,
    epochs: BTreeMap<String, u64>,
    faults: VecDeque<Fault>,
    /// Failures answered, in order, to the next appends whose first draft is of the named event
    /// type, before any in `faults`.
    typed: BTreeMap<String, VecDeque<Fault>>,
    unreadable: BTreeSet<String>,
    appends: usize,
    /// After this many appends the journal answers nothing more, as a process that crashed would
    /// find it: every append `Unavailable` and every read refused.
    crash_after: Option<usize>,
    /// The journal's own rules, judging every batch appended to a stream whose rows all came
    /// through [`Store::append`]: the executor's account stream from its `StreamOpened`. A stream a
    /// test wrote or seeded by hand is not judged, since its rows were never drafts.
    rules: mandate_journal::MemoryJournal,
    judged: BTreeMap<String, u64>,
    /// Whether this journal judges at all: a test opts in with [`Journal::judge_appends`] while
    /// the others still write ids the journal would refuse (backlog `E7-executord-judged-streams`).
    judging: bool,
}

/// One journal shared by every process and the test, so two processes can contend for a stream.
#[derive(Clone, Default)]
pub struct Journal(Rc<RefCell<Rows>>);

impl Journal {
    /// The `config_refs.mandate_version` of each account-stream record of `event_type`.
    pub fn mandate_refs(&self, event_type: &str) -> Vec<Option<String>> {
        self.rows_of(ACCOUNT_STREAM)
            .iter()
            .filter(|row| row.event_type == event_type)
            .map(|row| {
                parse(&row.body)
                    .ok()?
                    .get("config_refs")?
                    .get("mandate_version")?
                    .as_str()
                    .map(str::to_owned)
            })
            .collect()
    }

    /// Keeps the first `len` rows of `stream`, as a crash that lost the rest would have left it.
    pub fn truncate(&self, stream: &str, len: usize) {
        let mut rows = self.0.borrow_mut();
        rows.judged.remove(stream);
        if let Some(rows) = rows.streams.get_mut(stream) {
            rows.truncate(len);
        }
    }

    /// Gives the last record of `stream` another hash, as a rewrite that hashes the chain again
    /// leaves the record at a held head (DEC-2854 item 4).
    pub fn rewrite_head(&self, stream: &str) {
        if let Some(row) = self
            .0
            .borrow_mut()
            .streams
            .get_mut(stream)
            .and_then(|rows| rows.last_mut())
        {
            row.hash = Digest::of(b"rewritten");
        }
    }

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

    /// Each agent switch the account stream records, as the agent and the record it copies.
    pub fn applications(&self) -> Vec<(String, String)> {
        self.payloads("KillSwitchActivated")
            .iter()
            .filter_map(|p| {
                Some((
                    p.get("subject")?.as_str()?.to_owned(),
                    p.get("copied_from")?.as_str()?.to_owned(),
                ))
            })
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

    /// Answers `faults`, in order, to the next appends whose first draft is a `event_type`.
    pub fn fail_next_of(&self, event_type: &str, faults: &[Fault]) {
        self.0
            .borrow_mut()
            .typed
            .entry(event_type.to_owned())
            .or_default()
            .extend(faults.iter().copied());
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

    /// Judges every later batch on a stream the executor opens by the journal's own rules, and
    /// writes every record's id and cause as [`ulid`] maps them, as the real journal would take
    /// them (DEC-2846).
    pub fn judge_appends(&self) {
        self.0.borrow_mut().judging = true;
    }

    pub fn appends(&self) -> usize {
        self.0.borrow().appends
    }

    /// Stores `bodies` on `stream_id` as an earlier executor wrote them, for a process to replay.
    pub fn seed(&self, stream_id: &str, bodies: &str) -> Result<(), Error> {
        let mut rows = self.0.borrow_mut();
        rows.judged.remove(stream_id);
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
        let judging = self.0.borrow().judging;
        let canonical = |id: &str| if judging { ulid(id) } else { id.to_owned() };
        let event_id = canonical(event_id);
        let causation_id = causation_id.map(canonical);
        let bytes = draft_bytes(
            &envelope,
            &Fields {
                event_id: &event_id,
                event_type,
                schema_version,
                causation_id: causation_id.as_deref(),
                correlation_id: None,
                config_refs: &Object::new(),
                payload: &payload,
            },
        )
        .ok_or("draft")?;
        let mut rows = self.0.borrow_mut();
        rows.judged.remove(stream_id);
        let stored = rows.streams.entry(stream_id.to_owned()).or_default();
        let row = sealed(stream_id, stored.len(), &bytes).ok_or("seal")?;
        stored.push(row);
        Ok(())
    }
}

/// An artifact source that holds nothing: no account-stream record reads one (journal spec §9.1's
/// artifact checks are the agent stream's).
struct NoArtifacts;

impl mandate_journal::ArtifactSource for NoArtifacts {
    fn read_artifact(
        &self,
        _reference: &mandate_journal::ArtifactRef,
    ) -> Result<Vec<u8>, mandate_journal::ArtifactError> {
        Err(mandate_journal::ArtifactError::Missing)
    }
}

impl Rows {
    /// Judges `drafts`, appended at `before` rows, by the journal's own rules, as the Postgres and
    /// in-memory journals do: the refusal they would answer, or `None` when they would commit it
    /// or the stream is not judged. The executor's tests otherwise accept any bytes, so a batch
    /// the real journal refuses, and the exit inside it, passed every test while it was lost in
    /// the cell (DEC-2845).
    fn judge(
        &mut self,
        stream: &StreamId,
        before: usize,
        recorded_at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> Option<AppendOutcome> {
        if !self.judging {
            return None;
        }
        let name = stream.as_str().to_owned();
        if before == 0 && !self.judged.contains_key(&name) {
            let epoch = self.rules.take_ownership(stream);
            self.judged.insert(name.clone(), epoch);
        }
        let epoch = *self.judged.get(&name)?;
        let head = self.rules.head(stream).seq;
        let slices: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
        match self.rules.append_with_config_artifacts(
            stream,
            head,
            epoch,
            recorded_at,
            &slices,
            &NoArtifacts,
        ) {
            AppendOutcome::Committed(_) | AppendOutcome::AlreadyCommitted(_) => None,
            refused => Some(refused),
        }
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
        recorded_at: UtcNanos,
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
        let first_type = drafts.first().and_then(|d| {
            parse(d)
                .ok()?
                .get("event_type")?
                .as_str()
                .map(str::to_owned)
        });
        let typed = first_type
            .and_then(|kind| rows.typed.get_mut(&kind))
            .and_then(VecDeque::pop_front);
        let fault = typed.or_else(|| rows.faults.pop_front());
        if fault == Some(Fault::Unavailable) {
            return AppendOutcome::Unavailable;
        }
        if fault == Some(Fault::Fenced) {
            return AppendOutcome::Fenced {
                current_epoch: epoch.saturating_add(1),
            };
        }
        if fault == Some(Fault::Invalid) {
            return AppendOutcome::Invalid {
                draft: 0,
                error: Invalid {
                    reason: InvalidReason::Schema,
                    path: "payload".to_owned(),
                },
            };
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
        let before = stored.len();
        if let Some(refused) = rows.judge(stream, before, recorded_at, drafts) {
            return refused;
        }
        let stored = rows.streams.entry(stream.as_str().to_owned()).or_default();
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
    /// How many appends the journal had taken when the request was sent: what the process had
    /// journaled of every answer handed before it.
    pub appended: usize,
}

#[derive(Default)]
struct Books {
    seen: Vec<Seen>,
    orders: BTreeMap<String, BrokerOrder>,
    positions: Vec<BrokerPosition>,
    /// The fills the account's activities report, in the order they executed.
    fills: Vec<BrokerFill>,
    /// Requests answered `Unknown` after they took effect: the answer is lost on the way back.
    lose_answers: usize,
    /// Whether the account reads as blocked from trading (trading spec §7.3).
    blocked: bool,
    /// How a cancel-all or a close-position is answered: refused unsent while `None`, accepted
    /// while `Some`. A close is an order of the broker's own, working while `Some(false)` and
    /// filled whole at 150 on arrival while `Some(true)`.
    account_wide: Option<bool>,
    /// Whether every close-position's answer is lost after the close takes effect.
    lose_closes: bool,
    /// Requests refused with this HTTP status before they reach the books, as the Alpaca connector
    /// answers them: a `401` as an unknown outcome, `Unauthorized` (connections spec §5.4), and any
    /// other status as a rejection.
    refuse_with: Option<(u16, usize)>,
    /// The status every account read is refused with from now on, every other request answered.
    refuse_account: Option<u16>,
    /// Whether the broker says it runs no later permission checks, as one not built yet.
    no_later_checks: bool,
    /// The rate limit every response reports from now on, as the limit and what it leaves.
    rate_limit: Option<(u32, u32)>,
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

    /// A buy the broker already holds, accepted and resting, under our `client_order_id`.
    pub fn resting(
        &self,
        client_order_id: &str,
        instrument: &str,
        qty: &str,
        limit: &str,
    ) -> Result<(), Error> {
        self.resting_on(Side::Buy, client_order_id, instrument, qty, limit)
    }

    /// A sell resting at the broker, as a protective leg rests (R3's review of #2764): the sell
    /// side of [`Self::resting`].
    pub fn resting_sell(
        &self,
        client_order_id: &str,
        instrument: &str,
        qty: &str,
        limit: &str,
    ) -> Result<(), Error> {
        self.resting_on(Side::Sell, client_order_id, instrument, qty, limit)
    }

    fn resting_on(
        &self,
        side: Side,
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
            side,
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

    /// The order under `client_order_id` fills whole at `price`: the order reads as filled, the
    /// position grows by its quantity, and the account's activities report the fill.
    pub fn fill(&self, client_order_id: &str, price: &str) -> Result<(), Error> {
        let mut books = self.books.borrow_mut();
        let order = books
            .orders
            .get_mut(client_order_id)
            .ok_or("no such order")?;
        order.status = "filled".to_owned();
        order.filled_qty = order.qty;
        let fill = BrokerFill {
            fill_id: FillId(format!("f-{}", order.broker_order_id)),
            client_order_id: None,
            broker_order_id: Some(order.broker_order_id.clone()),
            instrument: order.instrument.clone(),
            side: order.side,
            qty: order.qty,
            price: Price::parse(price)?,
            fees: Usd::parse("0")?,
            trade_date: mandate_time::Date::parse("2026-09-21")?,
        };
        let signed = match fill.side {
            Side::Buy => SignedQty::parse(&fill.qty.to_string())?,
            Side::Sell => SignedQty::parse(&format!("-{}", fill.qty))?,
        };
        books.positions.push(BrokerPosition {
            instrument: fill.instrument.clone(),
            qty: signed,
            avg_entry_price: fill.price,
        });
        books.fills.push(fill);
        Ok(())
    }

    /// The broker confirms the cancel of the order under `client_order_id`: it reads as canceled.
    pub fn canceled(&self, client_order_id: &str) -> Result<(), Error> {
        let mut books = self.books.borrow_mut();
        let order = books
            .orders
            .get_mut(client_order_id)
            .ok_or("no such order")?;
        order.status = "canceled".to_owned();
        Ok(())
    }

    /// The account reads as blocked from trading from now on.
    pub fn block(&self) {
        self.books.borrow_mut().blocked = true;
    }

    pub fn lose_next_answers(&self, n: usize) {
        self.books.borrow_mut().lose_answers = n;
    }

    /// Every response from now on reports a rate limit of `limit` leaving `remaining` (DEC-2842).
    pub fn report_rate_limit(&self, limit: u32, remaining: u32) {
        self.books.borrow_mut().rate_limit = Some((limit, remaining));
    }

    /// The broker says it runs no later permission checks from now on (DEC-2840).
    pub fn without_later_checks(&self) {
        self.books.borrow_mut().no_later_checks = true;
    }

    /// Every account read from now on is refused with `status`; every other request is answered.
    pub fn refuse_account_with(&self, status: u16) {
        self.books.borrow_mut().refuse_account = Some(status);
    }

    /// The next `n` requests are refused with `status`, and nothing they asked takes effect.
    pub fn refuse_next_with(&self, status: u16, n: usize) {
        self.books.borrow_mut().refuse_with = Some((status, n));
    }

    /// Every cancel-all and close-position is accepted from now on. Each close is an order of the
    /// broker's own that names no client order id, for the whole position: if `closes_fill`, it
    /// sells it at once, else it works until [`Paper::expire_closes`].
    pub fn accept_account_wide(&self, closes_fill: bool) {
        self.books.borrow_mut().account_wide = Some(closes_fill);
    }

    /// Every close-position from now on takes effect and its answer is lost.
    pub fn lose_close_answers(&self) {
        self.books.borrow_mut().lose_closes = true;
    }

    /// The broker expires every close still working, unfilled: the position stays held.
    pub fn expire_closes(&self) {
        for order in self.books.borrow_mut().orders.values_mut() {
            if order.client_order_id.is_none() && order.status == "new" {
                order.status = "expired".to_owned();
            }
        }
    }
}

impl ScriptedBroker for Paper {}

impl Broker for Paper {
    fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
        mandate_alpaca::alpaca_profile()
    }

    fn has_later_checks(&self) -> bool {
        !self.books.borrow().no_later_checks
    }

    fn rate_limit(&self) -> Option<mandate_alpaca::RateLimit> {
        self.books
            .borrow()
            .rate_limit
            .map(|(limit, remaining)| mandate_alpaca::RateLimit { limit, remaining })
    }

    /// The paper Alpaca connection's own later checks (DEC-2840), as the process's Alpaca broker
    /// runs them.
    fn later_check(
        &self,
        connection: ConnectionId,
        occasion: Occasion,
        account: AccountRead,
    ) -> Option<Result<CheckReport, &'static str>> {
        Some(mandate_executord::connect::alpaca_report(
            connection, occasion, account,
        ))
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
            appended: self.journal.appends(),
        });
        if let Some((status, left)) = books.refuse_with
            && left > 0
        {
            books.refuse_with = Some((status, left.saturating_sub(1)));
            if status == 401 {
                return Err(ConnectorError::Unknown(BrokerUnknown::Unauthorized));
            }
            return Ok(BrokerOutcome::Rejected(BrokerReject {
                client_order_id: None,
                http_status: status,
                code: None,
                message: "refused".to_owned(),
            }));
        }
        if let (Some(status), BrokerRequest::GetAccount) = (books.refuse_account, request) {
            return Ok(BrokerOutcome::Rejected(BrokerReject {
                client_order_id: None,
                http_status: status,
                code: None,
                message: "refused".to_owned(),
            }));
        }
        let outcome = answer(&mut books, request)?;
        if books.lose_closes && matches!(request, BrokerRequest::ClosePosition(..)) {
            return Err(ConnectorError::Unknown(BrokerUnknown::Timeout));
        }
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
        BrokerRequest::GetAccount => {
            let mut held = account().map_err(|_| not_sent)?;
            held.trading_blocked = books.blocked;
            BrokerOutcome::Account(held)
        }
        BrokerRequest::ListActivities { since } => {
            let from = since.0.parse::<usize>().unwrap_or(0);
            BrokerOutcome::Activities {
                fills: books.fills.iter().skip(from).cloned().collect(),
                cursor: ActivityCursor(books.fills.len().to_string()),
            }
        }
        BrokerRequest::CancelAll(_) if books.account_wide.is_some() => {
            BrokerOutcome::AccountWideAccepted
        }
        BrokerRequest::ClosePosition(_, instrument) if books.account_wide.is_some() => {
            let fills = books.account_wide == Some(true);
            close(books, instrument, fills).map_err(|_| not_sent)?;
            BrokerOutcome::AccountWideAccepted
        }
        _ => return Err(not_sent),
    })
}

/// The broker's close of `instrument`: a sell of the whole position as an order of its own with no
/// client order id, working, or, if `fills`, filled at 150, its fill in the account's activities
/// and the position gone.
fn close(books: &mut Books, instrument: &InstrumentId, fills: bool) -> Result<(), Error> {
    let held = books
        .positions
        .iter()
        .filter(|position| &position.instrument == instrument)
        .try_fold(SignedQty::ZERO, |sum, position| {
            sum.checked_add(position.qty)
        })?;
    let qty = held.abs();
    books.sequence = books.sequence.saturating_add(1);
    let broker_order_id = format!("b{}", books.sequence);
    let price = Price::parse("150")?;
    books.orders.insert(
        broker_order_id.clone(),
        BrokerOrder {
            broker_order_id: broker_order_id.clone(),
            client_order_id: None,
            instrument: instrument.clone(),
            side: Side::Sell,
            qty,
            filled_qty: if fills { qty } else { Qty::ZERO },
            limit_price: None,
            stop_price: None,
            status: if fills { "filled" } else { "new" }.to_owned(),
            reject_code: None,
            replaced_by_broker_order_id: None,
            legs: Vec::new(),
            created_on: None,
        },
    );
    if !fills {
        return Ok(());
    }
    books.fills.push(BrokerFill {
        fill_id: FillId(format!("f-{broker_order_id}")),
        client_order_id: None,
        broker_order_id: Some(broker_order_id),
        instrument: instrument.clone(),
        side: Side::Sell,
        qty,
        price,
        fees: Usd::parse("0")?,
        trade_date: mandate_time::Date::parse("2026-09-21")?,
    });
    books
        .positions
        .retain(|position| &position.instrument != instrument);
    Ok(())
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

/// `id` as an event id the journal takes (journal spec §3): itself when it is a canonical ULID,
/// otherwise a ULID derived from it, the same every time, so a test may name a record `t1` and
/// find it again as `ulid("t1")`.
pub fn ulid(id: &str) -> String {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let canonical = id.len() == 26
        && id.bytes().all(|b| ALPHABET.contains(&b))
        && id.bytes().next().is_some_and(|b| b <= b'7');
    if canonical {
        return id.to_owned();
    }
    let digest = Digest::of(id.as_bytes()).to_hex();
    let tail: String = digest
        .bytes()
        .take(24)
        .map(|b| char::from(ALPHABET[usize::from(b) % ALPHABET.len()]))
        .collect();
    format!("7T{tail}")
}

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

    /// AAPL's fill configuration, the test deployment's (journal spec §9: a fill of an order of
    /// ours names all four, rule 300), the same objects its gate decisions name.
    fn fill_refs(&self, instrument: &InstrumentId) -> Option<FillConfigRefs> {
        let hash = |digit: &str| format!("sha256:{}", digit.repeat(64));
        (instrument.as_str() == AAPL).then(|| FillConfigRefs {
            fee_config: hash("1"),
            trading_calendar: hash("2"),
            settlement_calendar: hash("2"),
            instrument_snapshot: hash("3"),
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

    fn config_refs(
        &self,
        _agent: &AgentId,
        _instrument: &InstrumentId,
    ) -> Option<BindingGateConfigRefs> {
        Some(deployed_refs())
    }
}

/// The configuration the test deployment registers, which every gate decision of its agent is
/// journaled under, a binding run's or not (journal spec §9, DEC-2846).
pub fn deployed_refs() -> BindingGateConfigRefs {
    let hash = |digit: &str| format!("sha256:{}", digit.repeat(64));
    BindingGateConfigRefs::complete(hash("1"), hash("2"), hash("3"), hash("4"), VERSION)
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
    wired(Box::new(NoFacts), Box::new(TwoPercentStop))
}

/// The wiring with this gate and these protections.
pub fn wired(
    gate: Box<dyn BindingGateSource>,
    protections: Box<dyn Protections>,
) -> Result<Wiring, Error> {
    Ok(Wiring {
        ids: Box::new(Ids),
        mandates: Box::new(Mandate),
        instruments: Box::new(Instruments),
        config: config(),
        fees: fees()?,
        gate,
        protections,
        quotes: Box::new(NoQuotes),
        marked: Box::new(NoMarks),
        risk: Box::new(Unopened),
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
    start_wired(journal, broker, wiring()?)
}

/// A process whose gate admits every AAPL opening and protects it 2 % below.
pub fn start_allowing(
    journal: &Journal,
    broker: &Paper,
) -> Result<ExecutorProcess<Journal, Paper>, Error> {
    start_wired(
        journal,
        broker,
        wired(Box::new(Allowing), Box::new(TwoPercentStop))?,
    )
}

pub fn start_wired(
    journal: &Journal,
    broker: &Paper,
    wiring: Wiring,
) -> Result<ExecutorProcess<Journal, Paper>, Error> {
    Ok(ExecutorProcess::start(
        journal.clone(),
        broker.clone(),
        wiring,
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

/// A version-3 `AgentDeployed` (journal spec v0.84, DEC-2131): version 2's members, then the
/// passkey step-up that authorized it.
pub fn deploy_stepped_up(
    journal: &Journal,
    id: &str,
    agent: &str,
    connection: &str,
) -> Result<(), Error> {
    journal.write(
        CONTROL_STREAM,
        id,
        "AgentDeployed",
        3,
        None,
        object(vec![
            ("agent_id", text(agent)),
            ("connection_id", text(connection)),
            ("mandate_version", text(VERSION)),
            (
                "step_up",
                object(vec![
                    ("assertion_id", text("01JAA3P0A000000000000000H1")),
                    ("authenticated_at", text("2026-10-17T15:59:50.000000000Z")),
                    ("method", text("passkey")),
                ])?,
            ),
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

/// An `AgentModeApplied` an earlier executor journaled on the account stream (journal spec §9.6):
/// `agent` (or `*`) set `to` a mode under `restriction`, originated here, at risk clock `at`.
pub fn mode_applied(
    journal: &Journal,
    id: &str,
    agent: &str,
    to: &str,
    restriction: &str,
    at: u64,
) -> Result<(), Error> {
    journal.write(
        ACCOUNT_STREAM,
        id,
        "AgentModeApplied",
        1,
        None,
        object(vec![
            ("agent", text(agent)),
            ("originated", Value::Bool(true)),
            ("restriction", text(restriction)),
            (
                "risk_clock",
                Value::Int(mandate_canon::Int::new(at).ok_or("clock")?),
            ),
            ("to", text(to)),
        ])?,
    )
}

/// The account stream's `StreamOpened`, as an earlier executor journaled it.
pub fn opened(journal: &Journal) -> Result<(), Error> {
    journal.write(
        ACCOUNT_STREAM,
        "e0h0o0",
        "StreamOpened",
        1,
        None,
        object(vec![
            ("account_ref", text(ACCOUNT)),
            ("broker", text("alpaca")),
            ("stream_type", text("account")),
            ("workspace_id", text(WORKSPACE)),
        ])?,
    )
}

/// The runtime's copy of an owner's kill switch of the whole agent (journal spec §9.1).
pub fn kill(journal: &Journal, agent: &str, id: &str) -> Result<(), Error> {
    kill_scoped(journal, agent, id, "agent", agent)
}

/// The runtime's copy of a kill switch of the whole agent that `initiator` started.
pub fn kill_by(journal: &Journal, agent: &str, id: &str, initiator: &str) -> Result<(), Error> {
    journal.write(
        &agent_stream(agent),
        id,
        "KillSwitchActivated",
        1,
        Some("command"),
        object(vec![
            ("initiator", text(initiator)),
            ("mode_event", Value::Null),
            ("scope", text("agent")),
            ("subject", text(agent)),
        ])?,
    )
}

/// The control stream's `OwnerCommandIssued` of a kill switch of `scope` (`connection` or
/// `workspace`) and `subject` (journal spec §9.1, workspace API §5.6).
pub fn kill_command(journal: &Journal, id: &str, scope: &str, subject: &str) -> Result<(), Error> {
    journal.write(
        CONTROL_STREAM,
        id,
        "OwnerCommandIssued",
        1,
        None,
        object(vec![
            ("command", text("kill_switch")),
            ("scope", text(scope)),
            ("subject", text(subject)),
            ("step_up", Value::Null),
            ("user", text("usr_owner")),
        ])?,
    )
}

/// The control stream's `ConnectionRevoked` of `connection`, ordinary or compromised.
pub fn revoke(
    journal: &Journal,
    id: &str,
    connection: &str,
    compromised: bool,
) -> Result<(), Error> {
    journal.write(
        CONTROL_STREAM,
        id,
        "ConnectionRevoked",
        1,
        None,
        object(vec![
            ("connection_id", text(connection)),
            (
                "reason",
                text(if compromised { "compromised" } else { "owner" }),
            ),
        ])?,
    )
}

/// The control stream's `ConnectionRequested` of an OAuth connect of `connection` for `account`
/// (journal spec §9.8), with the members the executor reads.
pub fn requested(
    journal: &Journal,
    id: &str,
    connection: &str,
    account: &str,
) -> Result<(), Error> {
    journal.write(
        CONTROL_STREAM,
        id,
        "ConnectionRequested",
        1,
        None,
        object(vec![
            ("account_ref", text(account)),
            ("connection_id", text(connection)),
            ("environment", text("paper")),
        ])?,
    )
}

/// The manager's `ProcessStartIssued` of `account`'s executor on its connect's token exchange's
/// success, cause `exchanged` (journal spec §9.21, DEC-2335).
pub fn exchanged(journal: &Journal, id: &str, account: &str) -> Result<(), Error> {
    start_issued(journal, id, "executor", account, "exchanged")
}

/// The manager's `ProcessStartIssued` of `process` for `subject`, at generation 1, for `cause`.
pub fn start_issued(
    journal: &Journal,
    id: &str,
    process: &str,
    subject: &str,
    cause: &str,
) -> Result<(), Error> {
    journal.write(
        CONTROL_STREAM,
        id,
        "ProcessStartIssued",
        1,
        None,
        object(vec![
            ("cause", text(cause)),
            (
                "generation",
                Value::Int(mandate_canon::Int::new(1).ok_or("generation")?),
            ),
            ("process", text(process)),
            ("subject", text(subject)),
        ])?,
    )
}

/// The control stream's `ConnectionRefused` of `connection`'s connect, by a teardown.
pub fn refused(journal: &Journal, id: &str, connection: &str) -> Result<(), Error> {
    journal.write(
        CONTROL_STREAM,
        id,
        "ConnectionRefused",
        1,
        None,
        object(vec![
            ("check", Value::Null),
            ("connection_id", text(connection)),
            ("occasion", text("connect")),
            ("reason", text("timeout")),
        ])?,
    )
}

/// The control stream's `AgentStopped` of `agent`.
pub fn stopped(journal: &Journal, id: &str, agent: &str) -> Result<(), Error> {
    journal.write(
        CONTROL_STREAM,
        id,
        "AgentStopped",
        1,
        None,
        object(vec![("agent_id", text(agent))])?,
    )
}

/// The runtime's copy on `agent`'s stream of the kill-switch command `command`, at its scope.
pub fn copy_of(
    journal: &Journal,
    agent: &str,
    id: &str,
    command: &str,
    scope: &str,
    subject: &str,
) -> Result<(), Error> {
    journal.write(
        &agent_stream(agent),
        id,
        "KillSwitchActivated",
        1,
        Some(command),
        object(vec![
            ("initiator", text("owner")),
            ("mode_event", Value::Null),
            ("scope", text(scope)),
            ("subject", text(subject)),
        ])?,
    )
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

/// The mandate's protection, 2 % below and 5 % above the entry, through the same function the
/// executor derives it with in one process.
pub struct TwoPercentStop;

impl Protections for TwoPercentStop {
    fn protection(
        &self,
        _agent: &AgentId,
        _instrument: &InstrumentId,
        entry: Price,
    ) -> Result<Option<ProtectionPrices>, Unplanned> {
        let stop = Fraction::parse("0.02").map_err(|_| Unplanned)?;
        let take = Fraction::parse("0.05").map_err(|_| Unplanned)?;
        let bracket = equity_bracket_prices(entry, stop, Some(take)).map_err(|_| Unplanned)?;
        Ok(Some(ProtectionPrices {
            stop: bracket.stop,
            take_profit: bracket.take_profit,
        }))
    }
}

pub struct CannotProtect;

impl Protections for CannotProtect {
    fn protection(
        &self,
        _agent: &AgentId,
        _instrument: &InstrumentId,
        _entry: Price,
    ) -> Result<Option<ProtectionPrices>, Unplanned> {
        Err(Unplanned)
    }
}

/// The runtime's `IntentProposed` of an AAPL limit order (journal spec §9.1).
pub fn propose(
    journal: &Journal,
    agent: &str,
    id: &str,
    side: &str,
    qty: &str,
    limit: &str,
    purpose: &str,
) -> Result<(), Error> {
    journal.write(
        &agent_stream(agent),
        id,
        "IntentProposed",
        1,
        Some("decision"),
        object(vec![
            ("instrument_id", text(AAPL)),
            ("limit_price", text(limit)),
            ("purpose", text(purpose)),
            ("qty", text(qty)),
            ("side", text(side)),
            ("tif", text("day")),
            ("type", text("limit")),
        ])?,
    )
}

/// The binding gate's facts for an AAPL opening that every limit admits: a large, empty margin
/// account, a fresh quote at the order's limit, and a mandate whose limits do not bind.
pub struct Allowing;

impl BindingGateSource for Allowing {
    fn config_refs(
        &self,
        _agent: &AgentId,
        _instrument: &InstrumentId,
    ) -> Option<BindingGateConfigRefs> {
        Some(deployed_refs())
    }

    fn input(&self, request: &BindingGateRequest<'_>) -> Option<BindingGateInput> {
        if !request.purpose.adds_risk() {
            return None;
        }
        let asset = AssetId::new(request.instrument.as_str()).ok()?;
        let now = UtcNanos::parse_rfc3339(NOON).ok()?;
        let quote_at = UtcNanos::parse_rfc3339("2026-09-21T14:59:59Z").ok()?;
        let held = Qty::parse("1000000").ok()?;
        let equity = Usd::parse("1000000000").ok()?;
        let one = Fraction::parse("1").ok()?;
        let half = Fraction::parse("0.5").ok()?;
        let gate_agent = GateAgentId(1);
        Some(BindingGateInput {
            config_refs: deployed_refs(),
            now,
            config: GateConfig {
                price_floor: Usd::parse("0.01").ok()?,
                liquidity_floor_usd: Usd::ZERO,
                crypto_liquidity_floor_usd: Usd::ZERO,
                collar_liquid_threshold_usd: Usd::ZERO,
                collar_liquid_x: half,
                collar_other_x: half,
                collar_crypto_x: half,
                collar_passive_band: half,
                opposite_fill_interval_s: 0,
                min_resting_time_s: 0,
                order_to_fill_max: u32::MAX,
                order_to_fill_min_orders: u32::MAX,
                order_size_participation: one,
                daily_participation: one,
                close_window_minutes: 0,
                legacy_pdt_equity_threshold: Usd::ZERO,
                etp_classification_max_age_s: u32::MAX,
            },
            mandate: ValidatedMandate::from_validated_parts(
                RiskLimits {
                    max_position_usd: equity,
                    max_position_fraction: one,
                    max_order_usd: equity,
                    max_gross_exposure_usd: equity,
                    max_orders_per_day: u32::MAX,
                    reentry_cooldown_s: 0,
                    rebalance_band: one,
                    breach_confirm_s: 0,
                    drawdown_ladder: Vec::new(),
                },
                GoalState::Running,
                true,
                true,
            ),
            risk: RiskSnapshot {
                agent_equity: equity,
                high_water_mark: equity,
                day_start_equity: equity,
                capital_base: equity,
                inherited_loss: Usd::ZERO,
                latched: BTreeSet::new(),
                active_rungs: BTreeMap::new(),
                size_factor: Ratio::parse("1").ok()?,
                agent_mode: AgentMode::Normal,
            },
            account: AccountSnapshot {
                account_type: AccountType::Margin,
                state: GateAccountState::Active,
                crypto_active: true,
                regime: DayTradeRegime::IntradayMargin {
                    maintenance_excess: equity,
                },
                equity,
                prior_close_equity: equity,
                model_buying_power: equity,
                broker_buying_power: equity,
                broker_non_marginable_buying_power: equity,
                positions: BTreeMap::new(),
                market_values: BTreeMap::new(),
                working_orders: BTreeMap::new(),
                unknown_orders: BTreeSet::new(),
                related_account_resting: BTreeMap::new(),
            },
            agent: AgentSnapshot {
                agent: gate_agent,
                mode: AgentMode::Normal,
                instrument_restrictions: BTreeMap::new(),
                positions: BTreeMap::new(),
                market_values: BTreeMap::new(),
                working_orders: BTreeSet::new(),
                instrument_groups: BTreeMap::new(),
                last_exit_fill_at: BTreeMap::new(),
                orders_today: 0,
                day_trades: DayTradeLedger::default(),
            },
            instrument: GateInstrumentSnapshot {
                instrument: asset.clone(),
                asset_class: AssetClass::UsEquity,
                exchange: Some(Exchange::Nasdaq),
                status_active: true,
                tradable: true,
                fractionable: true,
                ipo: false,
                ptp_no_exception: false,
                etp: EtpClass::Plain,
                etp_classified_at: Some(now),
                quote_currency: Some(QuoteCurrency::Usd),
                prior_close: Some(request.limit),
                median_dollar_volume_20d: Some(equity),
                median_dollar_volume_30d: Some(equity),
                min_order_size: Qty::parse("1").ok()?,
                qty_increment: Qty::parse("1").ok()?,
                halted: false,
                status_feed_current: true,
            },
            market: MarketSnapshot {
                quote: Some(SaneQuote {
                    bid: request.limit,
                    ask: request.limit,
                    at: quote_at,
                }),
                last_trade: Some((request.limit, quote_at)),
                trailing_5m_volume: Some(held),
                adv_20d: Some(held),
            },
            conduct: ConductState::default(),
            universe: WorkingUniverse::Known {
                instruments: BTreeSet::from([asset.clone()]),
                pinned: true,
            },
            asset,
            gate_agent,
            gate_client_order_id: GateOrderId(1),
            owner_confirmed_bid: None,
            fee_reservation: Usd::ZERO,
            data_profile: "test".to_owned(),
            feed: "test".to_owned(),
        })
    }
}

/// The binding gate's facts for any AAPL order, an exit's included: [`Allowing`]'s, looked up as
/// an opening, so the gate runs and records every check on an exit too.
pub struct ExitFacts;

impl BindingGateSource for ExitFacts {
    fn config_refs(
        &self,
        _agent: &AgentId,
        _instrument: &InstrumentId,
    ) -> Option<BindingGateConfigRefs> {
        Some(deployed_refs())
    }

    fn input(&self, request: &BindingGateRequest<'_>) -> Option<BindingGateInput> {
        Allowing.input(&BindingGateRequest {
            agent: request.agent,
            instrument: request.instrument,
            side: request.side,
            qty: request.qty,
            limit: request.limit,
            purpose: Purpose::Open,
            tif: request.tif,
            protection: request.protection,
        })
    }
}

/// The runtime's copy of the owner's exit of the AAPL position (journal spec §9.1): with the bid,
/// bid size and floor the owner confirmed, or none confirmed.
pub fn owner_exit(
    journal: &Journal,
    agent: &str,
    id: &str,
    floor: Option<&str>,
) -> Result<(), Error> {
    let confirmed = |value: &str| floor.map_or(Value::Null, |_| text(value));
    journal.write(
        &agent_stream(agent),
        id,
        "OwnerExitRequested",
        1,
        Some("command"),
        object(vec![
            ("bid", confirmed("150")),
            ("bid_size", confirmed("100")),
            ("confirmed", Value::Bool(floor.is_some())),
            ("floor", floor.map_or(Value::Null, text)),
            ("scope", text("instrument")),
            ("step_up", Value::Null),
            ("step_up_status", text("absent")),
            ("subject", text(AAPL)),
            ("user", text("u1")),
        ])?,
    )
}

/// The owner's `OwnerExitRequested` of `AAPL` at `id`, confirming a bid of 150 for 100 and
/// `floor`, with valid step-up authenticated at `authenticated_at` (DEC-2040 item 1).
pub fn owner_exit_stepped_up(
    journal: &Journal,
    agent: &str,
    id: &str,
    floor: &str,
    authenticated_at: &str,
) -> Result<(), Error> {
    journal.write(
        &agent_stream(agent),
        id,
        "OwnerExitRequested",
        1,
        Some("command"),
        object(vec![
            ("bid", text("150")),
            ("bid_size", text("100")),
            ("confirmed", Value::Bool(true)),
            ("floor", text(floor)),
            ("scope", text("instrument")),
            (
                "step_up",
                object(vec![
                    ("assertion_id", text("a1")),
                    ("authenticated_at", text(authenticated_at)),
                    ("method", text("passkey")),
                ])?,
            ),
            ("step_up_status", text("valid")),
            ("subject", text(AAPL)),
            ("user", text("u1")),
        ])?,
    )
}

/// The runtime's `IntentProposed` of the owner's exit of 10 AAPL at the market, following from
/// `cause`.
pub fn propose_owner_market_exit(
    journal: &Journal,
    agent: &str,
    id: &str,
    cause: &str,
) -> Result<(), Error> {
    propose_market_exit(journal, agent, id, "owner_exit", cause)
}

/// The runtime's `IntentProposed` of an exit of 10 AAPL at the market, of `purpose`, following
/// from `cause`.
pub fn propose_market_exit(
    journal: &Journal,
    agent: &str,
    id: &str,
    purpose: &str,
    cause: &str,
) -> Result<(), Error> {
    journal.write(
        &agent_stream(agent),
        id,
        "IntentProposed",
        1,
        Some(cause),
        object(vec![
            ("instrument_id", text(AAPL)),
            ("limit_price", Value::Null),
            ("purpose", text(purpose)),
            ("qty", text("10")),
            ("side", text("sell")),
            ("tif", text("day")),
            ("type", text("market")),
        ])?,
    )
}

/// A mandate port that knows no agent's version, as the binary's fixture port does today.
pub struct NoVersion;

impl MandateView for NoVersion {
    fn version(&self, _agent: &AgentId) -> Option<MandateVersion> {
        None
    }

    fn crypto_stop_limit_offset(&self, agent: &AgentId) -> Option<Fraction> {
        Mandate.crypto_stop_limit_offset(agent)
    }

    fn covers(&self, agent: &AgentId, instrument: &InstrumentId) -> bool {
        Mandate.covers(agent, instrument)
    }
}
