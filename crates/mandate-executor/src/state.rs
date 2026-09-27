//! The folded state and the replay that builds it.

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::InstrumentId;
use mandate_num::{Price, Qty, SignedQty, Usd};
use mandate_time::Date;

use crate::ids::{ClientOrderId, IntentId};
use crate::types::{
    AccountScope, AccountState, ActivityCursor, AgentId, EventId, FillId, IntentBody,
    MarketObservation, Mode, Order, Protection, RiskClock, Seq, SubmitOrder, UnprotectedInterval,
    WriterEpoch,
};

pub use crate::fold::fold;

/// The `fold_version` of ADR-0001 ES-21. Bumped whenever fold output changes, with the golden
/// journal regenerated in the same change.
pub const FOLD_VERSION: u32 = 1;

/// The agent name an account-wide restriction is recorded under: a restriction for every agent on
/// the account, including one the executor has not yet seen (trading-domain spec §7.3).
pub(crate) const EVERY_AGENT: &str = "*";

/// The restriction a reconciliation places for one subject — an instrument, or external activity
/// on the account — and the only one an owner acknowledgment of that subject lifts (§11).
pub(crate) fn restriction_for(subject: &str) -> String {
    format!("reconciliation:{subject}")
}

/// Everything the executor knows about one broker account, derived from journaled events and
/// nothing else.
///
/// Every field is private and every collection is ordered (ES-21). The folded position of each
/// followed stream lives here and is re-derived by replay, so nothing durable exists outside the
/// journal and a restart cannot mistake an old event for a new one. Three fields are the running
/// process's own and are never folded: the writer epoch, the latest tick, and the quotes the shell
/// observed, none of which a replay could know or needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorState {
    pub(crate) scope: AccountScope,
    pub(crate) heads: BTreeMap<String, Seq>,
    pub(crate) epoch: Option<WriterEpoch>,
    pub(crate) started: bool,
    pub(crate) environment: Option<String>,
    pub(crate) unresolved: Option<UnresolvedAppend>,
    pub(crate) risk_clock: Option<RiskClock>,
    pub(crate) intents: BTreeMap<IntentId, IntentRecord>,
    pub(crate) orders: BTreeMap<ClientOrderId, Order>,
    pub(crate) reservations: BTreeMap<ClientOrderId, Usd>,
    pub(crate) protection: BTreeMap<InstrumentId, Protection>,
    pub(crate) unprotected: Vec<UnprotectedInterval>,
    pub(crate) positions: BTreeMap<InstrumentId, SignedQty>,
    pub(crate) fills: BTreeSet<FillId>,
    pub(crate) modes: BTreeMap<AgentId, Mode>,
    pub(crate) account_state: AccountState,
    pub(crate) consecutive_403s: u32,
    pub(crate) observed: Option<ObservedAccount>,
    pub(crate) mismatched: BTreeSet<InstrumentId>,
    pub(crate) checkpoint: Option<ActivityCursor>,
    pub(crate) reconciled_through: Option<Seq>,
    pub(crate) last_submission: Option<Seq>,
    pub(crate) bodies: BTreeMap<IntentId, IntentBody>,
    pub(crate) held: BTreeSet<IntentId>,
    pub(crate) details: BTreeMap<ClientOrderId, OrderDetail>,
    pub(crate) restrictions: BTreeMap<(AgentId, String), Mode>,
    pub(crate) copied: BTreeMap<EventId, EventId>,
    pub(crate) pending_actions: BTreeSet<InstrumentId>,
    pub(crate) asset_fees: BTreeMap<InstrumentId, Qty>,
    pub(crate) unposted_fees: Usd,
    pub(crate) simulated_fees: Usd,
    pub(crate) cash_flow: Usd,
    pub(crate) fill_notional: Usd,
    pub(crate) trading_day: Option<Date>,
    pub(crate) now: Option<RiskClock>,
    pub(crate) quotes: BTreeMap<InstrumentId, Vec<MarketObservation>>,
}

/// What the fold knows about one order beyond [`Order`]: the exact request, so a resubmission
/// after a confirmed absence sends the same body; where it was journaled, so a reconciliation can
/// tell which submissions its snapshot covered; and the broker's cumulative filled quantity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OrderDetail {
    pub(crate) request: Option<SubmitOrder>,
    pub(crate) submitted_seq: Option<Seq>,
    pub(crate) unknown_since: Option<RiskClock>,
    pub(crate) last_absence: Option<RiskClock>,
    pub(crate) broker_filled: Option<Qty>,
    pub(crate) limit: Option<Price>,
}

/// A batch whose append has not been answered. The input and the drafts are kept so that the only
/// permitted next step is the same input, which re-emits the same drafts and lets the append
/// answer `AlreadyCommitted` rather than appending a second event (journal spec §5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedAppend {
    pub head: Seq,
    pub input: crate::types::Input,
    pub drafts: Vec<crate::types::EventDraft>,
}

/// What the fold carries about one intent, and the whole of the executor's deduplication: an
/// intent whose id is already here produces an empty effect list on a re-hand
/// (task brief interpretation 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentRecord {
    pub intent_id: IntentId,
    pub agent: AgentId,
    /// The risk-clock second `IntentReceived` was journaled at, which is what `max_intent_age`
    /// is measured from at **every** `Intent → Submitting` transition (interpretation 11).
    pub received_at: RiskClock,
    pub outcome: IntentOutcome,
}

/// How far an intent has got. `Abandoned` and `Denied` are terminal: a later handoff of the same
/// intent id hits the fold lookup and produces nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentOutcome {
    Received,
    Denied,
    Submitted,
    Abandoned,
}

/// The last account snapshot the broker reported, from which buying power is taken as the lower
/// of the model and the broker (trading-domain spec §7.2, DEC-34, DEC-104).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedAccount {
    pub state: AccountState,
    pub multiplier: u32,
    pub equity: Usd,
    pub cash: Usd,
    pub buying_power: Usd,
    pub non_marginable_buying_power: Usd,
    pub accrued_fees: Usd,
}

impl ExecutorState {
    /// An empty state for one broker account, before any event is folded. Reads nothing: there is
    /// no constructor that touches a clock, a file, or a random number.
    pub fn new(scope: AccountScope) -> Self {
        Self {
            scope,
            heads: BTreeMap::new(),
            epoch: None,
            started: false,
            environment: None,
            unresolved: None,
            risk_clock: None,
            intents: BTreeMap::new(),
            orders: BTreeMap::new(),
            reservations: BTreeMap::new(),
            protection: BTreeMap::new(),
            unprotected: Vec::new(),
            positions: BTreeMap::new(),
            fills: BTreeSet::new(),
            modes: BTreeMap::new(),
            account_state: AccountState::Active,
            consecutive_403s: 0,
            observed: None,
            mismatched: BTreeSet::new(),
            checkpoint: None,
            reconciled_through: None,
            last_submission: None,
            bodies: BTreeMap::new(),
            held: BTreeSet::new(),
            details: BTreeMap::new(),
            restrictions: BTreeMap::new(),
            copied: BTreeMap::new(),
            pending_actions: BTreeSet::new(),
            asset_fees: BTreeMap::new(),
            unposted_fees: Usd::ZERO,
            simulated_fees: Usd::ZERO,
            cash_flow: Usd::ZERO,
            fill_notional: Usd::ZERO,
            trading_day: None,
            now: None,
            quotes: BTreeMap::new(),
        }
    }

    /// Which broker account this executor owns the ledger for.
    pub fn scope(&self) -> &AccountScope {
        &self.scope
    }

    /// The `environment` fixed at `StreamOpened`, which nothing in this stream can change and no
    /// append may contradict (ADR-0001 ES-23).
    pub fn environment(&self) -> Option<&str> {
        self.environment.as_deref()
    }

    /// The folded `seq` of one stream, or `None` for a stream with nothing folded yet.
    pub fn head(&self, stream: &str) -> Option<Seq> {
        self.heads.get(stream).copied()
    }

    /// The latest risk-clock second the fold has seen.
    pub fn risk_clock(&self) -> Option<RiskClock> {
        self.risk_clock
    }

    /// What the fold knows about one intent. `Some` is the whole of the deduplication: the
    /// handoff is at-least-once by design, and an intent already here costs an empty effect list.
    pub fn intent(&self, intent: &IntentId) -> Option<&IntentRecord> {
        self.intents.get(intent)
    }

    /// Every order the fold carries, by client order id.
    pub fn orders(&self) -> &BTreeMap<ClientOrderId, Order> {
        &self.orders
    }

    /// One order, or `None` for an id this executor never derived.
    pub fn order(&self, client_order_id: &ClientOrderId) -> Option<&Order> {
        self.orders.get(client_order_id)
    }

    /// The orders whose `OrderSubmitted` committed with no acknowledgment. `Input::Started`
    /// queries each by `client_order_id` and never resubmits blindly (journal spec §5.2).
    pub fn unacknowledged(&self) -> Vec<&Order> {
        self.orders
            .values()
            .filter(|order| order.state == crate::types::OrderState::Submitting)
            .collect()
    }

    /// Reservations by client order id. An `Unknown` order reserves its maximum cost, and only
    /// one of §5.7's six terminal states releases it (interpretation 26).
    pub fn reservations(&self) -> &BTreeMap<ClientOrderId, Usd> {
        &self.reservations
    }

    /// One instrument's resting protection and the quantity it covers.
    pub fn protection(&self, instrument: &InstrumentId) -> Option<&Protection> {
        self.protection.get(instrument)
    }

    /// Σ protective sell quantity for one instrument, which §5.4 requires never to exceed the
    /// position.
    pub fn protective_sell_qty(&self, instrument: &InstrumentId) -> Qty {
        self.protection
            .get(instrument)
            .map_or(Qty::ZERO, |protection| protection.covered_qty)
    }

    /// Every unprotected interval the fold has seen, open and closed.
    pub fn unprotected_intervals(&self) -> &[UnprotectedInterval] {
        &self.unprotected
    }

    /// The model's net position per instrument, folded from fills and corporate actions.
    pub fn positions(&self) -> &BTreeMap<InstrumentId, SignedQty> {
        &self.positions
    }

    /// Every fill id already applied, which is what makes a re-ingested fill idempotent.
    pub fn fills(&self) -> &BTreeSet<FillId> {
        &self.fills
    }

    /// Each agent's mode on this account, as `AgentModeApplied` set it: the strictest of its
    /// active restrictions, each lifting independently (mandate spec §5.9).
    pub fn modes(&self) -> &BTreeMap<AgentId, Mode> {
        &self.modes
    }

    /// One agent's effective mode: the strictest of the account state, the restrictions on every
    /// agent of the account, and its own mode (trading-domain spec §7.3 evaluates account state
    /// **before** agent mode).
    pub fn effective_mode(&self, agent: &AgentId) -> Mode {
        let account = match self.account_state {
            AccountState::Active => Mode::Normal,
            AccountState::ClosingOnly => Mode::ExitsOnly,
            AccountState::Blocked => Mode::Paused,
        };
        let every = self
            .modes
            .get(&AgentId(EVERY_AGENT.to_owned()))
            .copied()
            .unwrap_or_default();
        let own = self.modes.get(agent).copied().unwrap_or_default();
        account.max(every).max(own)
    }

    /// The account state detected from statuses and rejects (§7.3).
    pub fn account_state(&self) -> AccountState {
        self.account_state
    }

    /// Consecutive 403 rejects with no known order-level cause, against
    /// `restriction_403_threshold` (§7.3).
    pub fn consecutive_403s(&self) -> u32 {
        self.consecutive_403s
    }

    /// The last account snapshot the broker reported.
    pub fn observed_account(&self) -> Option<&ObservedAccount> {
        self.observed.as_ref()
    }

    /// The gate's buying power: the lower of the model and the broker, reservations included and
    /// uncleared deposits excluded (§7.2, DEC-34, DEC-104).
    ///
    /// The model is the broker's last reported cash moved by every fill since, less unposted fees,
    /// paper's simulated ones included (§10, R-22: paper must not look flatter than live). `None`
    /// until the broker has reported an account, or if the arithmetic overflows: no buying power
    /// is ever guessed.
    pub fn buying_power(&self) -> Option<Usd> {
        let observed = self.observed.as_ref()?;
        let model = observed
            .cash
            .checked_add(self.cash_flow)
            .and_then(|cash| cash.checked_sub(self.unposted_fees))
            .and_then(|cash| cash.checked_sub(self.simulated_fees))
            .ok()?;
        let reserved = self
            .reservations
            .values()
            .try_fold(Usd::ZERO, |total, amount| total.checked_add(*amount))
            .ok()?;
        model.min(observed.buying_power).checked_sub(reserved).ok()
    }

    /// The instruments whose reconciliation difference is unexplained. Only an owner
    /// acknowledgment with step-up evidence clears one; no tick, no restart, and no later
    /// agreeing reconciliation does (interpretation 14).
    pub fn mismatched(&self) -> &BTreeSet<InstrumentId> {
        &self.mismatched
    }

    /// The activities cursor the last `ReconciliationRun` advanced to.
    pub fn checkpoint(&self) -> Option<&ActivityCursor> {
        self.checkpoint.as_ref()
    }

    /// The `seq` of the last `ReconciliationRun`, and of the last `OrderSubmitted`. Stream I's
    /// startup hold lifts on a run positioned at or after the last submission, which is why the
    /// executor never appends a run positioned after a submission it did not cover
    /// (DEC-131 item 13, interpretation 15).
    pub fn reconciled_through(&self) -> Option<Seq> {
        self.reconciled_through
    }

    pub fn last_submission(&self) -> Option<Seq> {
        self.last_submission
    }

    /// The batch whose append is still unresolved, if any. No new input may be handled at an
    /// unresolved head (journal spec §5.1).
    pub fn unresolved(&self) -> Option<&UnresolvedAppend> {
        self.unresolved.as_ref()
    }

    /// The writer epoch this state was folded under, once `Input::Started` has taken one.
    pub fn epoch(&self) -> Option<WriterEpoch> {
        self.epoch
    }

    pub fn started(&self) -> bool {
        self.started
    }

    /// Whether the causation chain of a copied fact is recorded for `event`.
    pub fn copied_origin(&self, event: &EventId) -> Option<&EventId> {
        self.copied.get(event)
    }

    /// The account stream this state is the single writer of, named by its opaque ids alone
    /// (journal spec §2, §6.4).
    pub(crate) fn account_stream(&self) -> String {
        format!("acct:{}:{}", self.scope.workspace.0, self.scope.account.0)
    }

    /// The account stream's folded head, zero before anything is folded.
    pub(crate) fn account_head(&self) -> Seq {
        self.head(&self.account_stream()).unwrap_or(Seq(0))
    }

    /// The time the core acts at: the later of the folded risk clock and the scheduler's latest
    /// tick, or zero before either exists. Never a wall clock (ES-21).
    pub(crate) fn clock(&self) -> RiskClock {
        self.risk_clock
            .max(self.now)
            .unwrap_or(RiskClock::from_secs(0))
    }
}
