//! The folded state and the replay that builds it.

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::InstrumentId;
use mandate_domain::CapabilityProfile;
use mandate_num::{Price, Qty, Rounding, SignedQty, Usd};
use mandate_time::Date;

use crate::error::ExecutorError;
use crate::ids::{ClientOrderId, IntentId};
use crate::shape::transitional_alpaca;
use crate::types::{
    AccountScope, AccountState, ActivityCursor, AgentId, BracketLegs, EventId, FillId, IntentBody,
    MarketObservation, Mode, OcoLegs, Order, OrderState, Protection, ProtectionPrices, Purpose,
    RiskClock, Seq, SubmitOrder, UnprotectedInterval, WriterEpoch,
};

pub use crate::fold::fold;

/// The `fold_version` of ADR-0001 ES-21. Bumped whenever fold output changes, with the golden
/// journal regenerated in the same change.
pub const FOLD_VERSION: u32 = 1;

/// The agent name an account-wide restriction is recorded under: a restriction for every agent on
/// the account, including one the executor has not yet seen (trading-domain spec §7.3).
pub(crate) const EVERY_AGENT: &str = "*";

/// Everything the executor knows about one broker account, derived from journaled events and
/// nothing else.
///
/// Every field is `pub(crate)`, private to this crate but not to its modules, and every collection
/// is ordered (ES-21). The folded position of each followed stream lives here and is re-derived by
/// replay, so nothing durable exists outside the journal and a restart cannot mistake an old event
/// for a new one. Only `fold.rs` writes folded state. `step.rs` writes only the running process's
/// own fields, which are never folded: the writer epoch, `started`, the head it started at, the
/// latest tick, and the unresolved append (which the fold clears once the append's events are
/// folded back). That split
/// is a convention the review holds (rung 3 of the trust ladder), not a guarantee the types give;
/// the backlog carries making it one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorState {
    pub(crate) scope: AccountScope,
    pub(crate) heads: BTreeMap<String, Seq>,
    pub(crate) epoch: Option<WriterEpoch>,
    pub(crate) started: bool,
    pub(crate) started_at: Option<Seq>,
    pub(crate) environment: Option<String>,
    pub(crate) unresolved: Option<UnresolvedAppend>,
    pub(crate) risk_clock: Option<RiskClock>,
    pub(crate) intents: BTreeMap<IntentId, IntentRecord>,
    pub(crate) orders: BTreeMap<ClientOrderId, Order>,
    pub(crate) reservations: BTreeMap<ClientOrderId, Usd>,
    pub(crate) protection: BTreeMap<InstrumentId, Protection>,
    pub(crate) unprotected: Vec<UnprotectedInterval>,
    /// The protective orders each instrument's open interval waits on: placed when a sequence, a
    /// re-placement or a partly filled bracket ended, and acknowledged by the broker before the
    /// interval ends (DEC-348 item 2).
    pub(crate) awaiting: BTreeMap<InstrumentId, BTreeSet<ClientOrderId>>,
    pub(crate) copied: BTreeMap<EventId, EventId>,
    /// Each instrument's running exit sequence, `unprotected_start` to `unprotected_end` (§5.4).
    pub(crate) exiting: BTreeMap<InstrumentId, ExitSequence>,
    /// Each instrument whose protection is being re-placed before its GTC expiry: cancelled, and
    /// re-placed once the cancels are confirmed, `unprotected_start` to `unprotected_end` (§5.4).
    pub(crate) replacing: BTreeMap<InstrumentId, Replacement>,
    /// The latest trading day a copied `TradingDayStarted` began: the creation date of the GTC
    /// protection placed from then on, and the day §5.4's re-placement buffer is counted from.
    pub(crate) trading_day: Option<Date>,
    /// Each exit laddered outside a sequence (§5.6: extended hours, the closing auction window, a
    /// presumed halt), by its instrument and its intent, folded from its rungs' `OrderSubmitted`:
    /// one for each exit, so a second exit laddered in the instrument never replaces the first
    /// one's (DEC-424).
    pub(crate) ladders: BTreeMap<(InstrumentId, IntentId), LoneLadder>,
    /// The latest quote per instrument: process-local, an input never journaled (like the tick).
    pub(crate) quotes: BTreeMap<InstrumentId, MarketObservation>,
    /// The latest-observed sane quote with a bid per instrument, kept past newer quotes that are
    /// not and never replaced by an older one: the exit ladder's "last sane bid within 5 minutes"
    /// (§5.6). Process-local, like `quotes`.
    pub(crate) sane_bids: BTreeMap<InstrumentId, MarketObservation>,
    /// The latest-observed sane quote with a last trade per instrument, kept like `sane_bids`: the
    /// exit ladder's "the last trade", which it takes only within 5 minutes (§5.6, DEC-260 (5)).
    /// Process-local, like `quotes`.
    pub(crate) trades: BTreeMap<InstrumentId, MarketObservation>,
    /// When each instrument's current breach began: the first sane mark at or below its resting
    /// stop since a sane mark above it, the triggered-stop watchdog's clock (§5.4). Process-local,
    /// like `quotes`, so a restart starts the clock again from the next breaching mark.
    pub(crate) breaches: BTreeMap<InstrumentId, RiskClock>,
    /// When the triggered-stop watchdog last fired in each instrument, from its journaled record:
    /// a breach fires only if it began after, so one breach is watchdogged once (§5.4, DEC-260
    /// (11)).
    pub(crate) watchdogged: BTreeMap<InstrumentId, RiskClock>,
    pub(crate) positions: BTreeMap<InstrumentId, SignedQty>,
    pub(crate) fills: BTreeSet<FillId>,
    /// Sell fills applied with no order named (`FillApplied` with no `client_order_id`), by the
    /// journal position that applied them and what of each is not yet netted against an ended
    /// sell's reported fill that no update has applied: the same shares may be both, so they come
    /// off the held position once (DEC-421 item 5). Entries shrink only by netting: one that never
    /// meets an ended sell's unapplied report stays for the life of the journal, bounded by how much
    /// the owner trades the instrument outside the platform, not by anything in this crate.
    pub(crate) unattributed: BTreeMap<InstrumentId, Vec<(Seq, Qty)>>,
    pub(crate) modes: BTreeMap<AgentId, Mode>,
    pub(crate) account_state: AccountState,
    pub(crate) consecutive_403s: u32,
    pub(crate) observed: Option<ObservedAccount>,
    pub(crate) cash_flow: Usd,
    pub(crate) fill_notional: Usd,
    pub(crate) cash_out_of_band: u32,
    pub(crate) unposted_fees: Usd,
    pub(crate) asset_fees: BTreeMap<InstrumentId, Qty>,
    pub(crate) simulated_fees: BTreeMap<(String, String), Usd>,
    pub(crate) mismatched: BTreeSet<InstrumentId>,
    pub(crate) checkpoint: Option<ActivityCursor>,
    pub(crate) reconciled_through: Option<Seq>,
    pub(crate) last_submission: Option<Seq>,
    pub(crate) bodies: BTreeMap<IntentId, IntentBody>,
    pub(crate) held: BTreeSet<IntentId>,
    /// Held exits whose hold past `max_intent_age_s` was journaled and alerted, once (D2).
    pub(crate) held_long: BTreeSet<IntentId>,
    pub(crate) details: BTreeMap<ClientOrderId, OrderDetail>,
    pub(crate) restrictions: BTreeMap<(AgentId, String), Mode>,
    pub(crate) uncompensated: BTreeMap<EventId, Adoption>,
    /// The companions an `OrderSubmitted` names: each `OrderRequestRecorded` folded, keyed by its
    /// event id, consumed by the submission that names it as its `causation_id` (rule 45). The
    /// exact request the fold rebuilds a resubmission from (§5.7).
    pub(crate) pending_requests: BTreeMap<EventId, PendingRequest>,
    /// Each agent-scoped kill switch as its `KillSwitchActivated` journaled it, by that record's
    /// id, with the closes it has not yet raised as flatten intents (§5.5, DEC-485).
    pub(crate) switches: BTreeMap<EventId, Switch>,
    pub(crate) now: Option<RiskClock>,
    /// The broker's capability profile protection reads (DEC-838 item 5): set by the shell from
    /// `connector.profile()` at every start ([`ExecutorState::with_profile`]), and otherwise the
    /// transitional Alpaca table. `None` only if that table failed to build, which protection
    /// reads as no shape offered (journaled and alerted, DEC-838 item 1).
    pub(crate) profile: Option<CapabilityProfile>,
}

/// One agent-scoped kill switch (trading-domain spec §5.5): whose sub-ledger it sells, as what
/// purpose, the owner's confirmed floor if any, and the instruments it closes, in the
/// order the record named them, which is what a flatten's ordinal is derived from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Switch {
    pub(crate) agent: AgentId,
    pub(crate) purpose: Purpose,
    pub(crate) floor: Option<Price>,
    pub(crate) instruments: Vec<InstrumentId>,
    pub(crate) pending: BTreeSet<InstrumentId>,
}

/// What one `OrderRequestRecorded` carries (§9.5): the executor-only members of the order that
/// follows it, from which the fold rebuilds the exact request and the order's owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingRequest {
    pub(crate) agent: AgentId,
    pub(crate) intent: Option<IntentId>,
    pub(crate) purpose: Purpose,
    pub(crate) extended_hours: bool,
    pub(crate) stop_price: Option<Price>,
    pub(crate) bracket: Option<BracketLegs>,
    pub(crate) oco: Option<OcoLegs>,
    pub(crate) rung: Option<u32>,
    pub(crate) at_floor: bool,
}

/// What the fold knows about one order beyond [`Order`]: the exact request, so a resubmission
/// after a confirmed absence sends the same body; where it was journaled, so a reconciliation can
/// tell which submissions its snapshot covered; and the broker's cumulative filled quantity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OrderDetail {
    pub(crate) request: Option<SubmitOrder>,
    pub(crate) submitted_seq: Option<Seq>,
    /// The risk clock of the `OrderSubmitted` this attempt was journaled with, from which an
    /// `Unknown` order's listing starts (DEC-862 item 2).
    pub(crate) submitted_at: Option<RiskClock>,
    pub(crate) unknown_since: Option<RiskClock>,
    pub(crate) last_absence: Option<RiskClock>,
    /// An exit waited on the order past rule 5's bound, or the broker refused its cancel, and the
    /// order was queried. From then on no exit waits on this submission attempt again, answered
    /// or not: whatever the query finds is the gate's to hold (`unknown_order_in_flight`) and the
    /// broker's to refuse. A resubmission after a confirmed absence is a new attempt, with a new
    /// detail, which an exit waits on afresh (#174 ruling 5863046153, DEC-160 (18)).
    pub(crate) cancel_overdue: bool,
    /// The broker's own cumulative filled quantity for the order, from the `filled_qty` its
    /// latest report carried: a bracket entry's filled quantity before its fills are ingested
    /// (§5.4's "the filled quantity", DEC-346 item 3).
    pub(crate) reported_filled: Option<Qty>,
    /// What of `reported_filled` beyond the applied `filled_qty` an unattributed sell fill applied
    /// after this order was submitted has already taken off the position (DEC-421 item 5).
    pub(crate) netted: Option<Qty>,
    /// When a bracket entry's unprotected interval started: its first partial fill, from the
    /// `ProtectionChanged unprotected_start` that names it as `bracket` (§5.4, DEC-346 item 4).
    pub(crate) bracket_since: Option<RiskClock>,
    /// Whether a bracket entry is done with: its legs placed on completion, or its interval ended
    /// once it was terminal partly filled, with the OCO for its filled quantity or with nothing
    /// left to cover, from the `ProtectionChanged placed` or `unprotected_end` that names it as
    /// `bracket`. Each entry is protected once (§5.4, DEC-346 item 4).
    pub(crate) bracket_placed: bool,
}

/// An adoption on the journal (`OrderStateChanged` with `adopted`) whose `CompensatingEvent` has
/// not been folded yet: a crash between the two leaves one here, and the next reconciliation
/// journals the missing compensation before anything else, so no adoption stays silent (§11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Adoption {
    pub(crate) subject: ClientOrderId,
    pub(crate) from: OrderState,
    pub(crate) to: OrderState,
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
    /// The risk-clock second of the latest `GateDecided` that allowed the intent: for an exit,
    /// the start of its wait on its cancels, which rule 5's bound runs from (§5.3, DEC-160 (13)).
    pub(crate) allowed_at: Option<RiskClock>,
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
    /// Whether the report carried every field buying power reads.
    pub(crate) complete: bool,
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
            started_at: None,
            environment: None,
            unresolved: None,
            risk_clock: None,
            intents: BTreeMap::new(),
            orders: BTreeMap::new(),
            reservations: BTreeMap::new(),
            protection: BTreeMap::new(),
            unprotected: Vec::new(),
            awaiting: BTreeMap::new(),
            copied: BTreeMap::new(),
            exiting: BTreeMap::new(),
            replacing: BTreeMap::new(),
            trading_day: None,
            ladders: BTreeMap::new(),
            quotes: BTreeMap::new(),
            sane_bids: BTreeMap::new(),
            trades: BTreeMap::new(),
            breaches: BTreeMap::new(),
            watchdogged: BTreeMap::new(),
            positions: BTreeMap::new(),
            fills: BTreeSet::new(),
            unattributed: BTreeMap::new(),
            modes: BTreeMap::new(),
            account_state: AccountState::Active,
            consecutive_403s: 0,
            observed: None,
            cash_flow: Usd::ZERO,
            fill_notional: Usd::ZERO,
            cash_out_of_band: 0,
            unposted_fees: Usd::ZERO,
            asset_fees: BTreeMap::new(),
            simulated_fees: BTreeMap::new(),
            mismatched: BTreeSet::new(),
            checkpoint: None,
            reconciled_through: None,
            last_submission: None,
            bodies: BTreeMap::new(),
            held: BTreeSet::new(),
            held_long: BTreeSet::new(),
            details: BTreeMap::new(),
            restrictions: BTreeMap::new(),
            uncompensated: BTreeMap::new(),
            pending_requests: BTreeMap::new(),
            switches: BTreeMap::new(),
            now: None,
            profile: transitional_alpaca().ok(),
        }
    }

    /// This state with the connector's capability profile, which protection reads in place of the
    /// transitional Alpaca default (DEC-838 item 5). The shell sets it after the fold at every
    /// start; a state built without it is Alpaca-only by contract until B3 deletes the default.
    pub fn with_profile(mut self, profile: CapabilityProfile) -> Self {
        self.profile = Some(profile);
        self
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

    /// One instrument's resting protection and the quantity it covers, as `ProtectionChanged`
    /// folds it.
    pub fn protection(
        &self,
        instrument: &InstrumentId,
    ) -> Result<Option<&Protection>, ExecutorError> {
        Ok(self.protection.get(instrument))
    }

    /// Σ protective sell quantity for one instrument, which §5.4 requires never to exceed the
    /// position.
    pub fn protective_sell_qty(&self, instrument: &InstrumentId) -> Result<Qty, ExecutorError> {
        Ok(self
            .protection
            .get(instrument)
            .map_or(Qty::ZERO, |protection| protection.covered_qty))
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
        let account = self.account_mode();
        let every = self
            .modes
            .get(&AgentId(EVERY_AGENT.to_owned()))
            .copied()
            .unwrap_or_default();
        let own = self.modes.get(agent).copied().unwrap_or_default();
        account.max(every).max(own)
    }

    /// The mode the account's own state imposes on every agent (§7.3): the one part of the
    /// effective mode a kill switch's flatten is not exempt from (§5.5, DEC-485).
    pub(crate) fn account_mode(&self) -> Mode {
        match self.account_state {
            AccountState::Active => Mode::Normal,
            AccountState::ClosingOnly => Mode::ExitsOnly,
            AccountState::Blocked => Mode::Paused,
        }
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
    /// The model is §7.2's margin row, since an Alpaca account is always a margin account: the
    /// broker's last reported cash moved by every fill since (a buy lowers it by quantity × price,
    /// and a sell's proceeds count, unsettled or not), less paper's simulated fees charged per
    /// `(family, day)` bucket at `round(bucket, 2, ceiling)`, and less the broker's own unposted
    /// fees at their ceiling cent (§6.2, §7.2, §10, DEC-104). `None` until the broker has reported
    /// a complete account, or if the arithmetic overflows: no buying power is ever guessed. This is
    /// §7.2's equities row; a crypto order reads [`Self::crypto_buying_power`].
    pub fn buying_power(&self) -> Option<Usd> {
        let observed = self
            .observed
            .as_ref()
            .filter(|observed| observed.complete)?;
        let fees = self
            .simulated_fees
            .values()
            .try_fold(Usd::ZERO, |total, bucket| {
                total.checked_add(bucket.round(2, Rounding::Ceiling)?)
            })
            .ok()?;
        let model = observed
            .cash
            .checked_add(self.cash_flow)
            .and_then(|cash| cash.checked_sub(fees))
            .and_then(|cash| cash.checked_sub(self.unposted_fees.round(2, Rounding::Ceiling)?))
            .ok()?;
        let reserved = self
            .reservations
            .values()
            .try_fold(Usd::ZERO, |total, amount| total.checked_add(*amount))
            .ok()?;
        model.min(observed.buying_power).checked_sub(reserved).ok()
    }

    /// §7.2's crypto row: the equities figure, no more than the broker's
    /// `non_marginable_buying_power`, since a crypto buy cannot use margin.
    pub fn crypto_buying_power(&self) -> Option<Usd> {
        let observed = self.observed.as_ref()?;
        Some(
            self.buying_power()?
                .min(observed.non_marginable_buying_power),
        )
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

    /// Whether a reconciliation has run since this process started: a `ReconciliationRun` folded
    /// after the head `Input::Started` found. A run an earlier process appended sits at or before
    /// that head, so a restart always waits for its own. Until one runs, and until an account is
    /// journaled, the gate holds every opening (§11, the coordinator's ruling on #174, comment
    /// 5857742391).
    pub(crate) fn reconciled_since_start(&self) -> bool {
        self.started_at
            .zip(self.reconciled_through)
            .is_some_and(|(start, run)| run > start)
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

    /// The origin a copied fact cites, as folded from its `causation_id` (journal spec §2).
    pub fn copied_origin(&self, event: &EventId) -> Result<Option<&EventId>, ExecutorError> {
        Ok(self.copied.get(event))
    }

    /// The exact request an order's `OrderSubmitted` named, rebuilt at replay from its
    /// `OrderRequestRecorded` companion (§9.5, rule 45) — what a resubmission after a confirmed
    /// absence sends again under the same id (§5.7).
    pub fn request_of(&self, client_order_id: &ClientOrderId) -> Option<&SubmitOrder> {
        self.details
            .get(client_order_id)
            .and_then(|detail| detail.request.as_ref())
    }

    /// The protective prices an intent was handed in with, restored at replay from its `intended`
    /// `ProtectionChanged` (`AGENTS.md` rule 13: protection never lost to a restart).
    pub fn intent_protection(&self, intent: &IntentId) -> Option<ProtectionPrices> {
        match self.bodies.get(intent) {
            Some(IntentBody::Order { protection, .. }) => *protection,
            _ => None,
        }
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

/// One exit sequence as its `unprotected_start` journaled it (§5.4): the exit's intent, and the
/// entry, agent and prices a re-placement takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExitSequence {
    pub(crate) intent: IntentId,
    pub(crate) entry: ClientOrderId,
    pub(crate) agent: AgentId,
    pub(crate) prices: Option<ProtectionPrices>,
    /// A passive exit's (`passive_start`): it keeps the stop, so no interval opens.
    pub(crate) passive: bool,
    /// Where the exit's price ladder stands, from its journaled rungs (§5.6).
    pub(crate) ladder: Ladder,
}

/// One re-placement before expiry as its `unprotected_start` journaled it (§5.4): the entry,
/// agent and prices the new protection takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Replacement {
    pub(crate) entry: ClientOrderId,
    pub(crate) agent: AgentId,
    pub(crate) prices: Option<ProtectionPrices>,
}

/// An exit §5.6 ladders with no protection to cancel first: its intent, agent and progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoneLadder {
    pub(crate) intent: IntentId,
    pub(crate) agent: AgentId,
    pub(crate) ladder: Ladder,
}

/// The exit price ladder's progress (§5.6), folded from each rung's `OrderSubmitted` and its
/// stepping cancel, so a restart resumes it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Ladder {
    /// The current rung: 0 is the exit's own order, n its `-l{n}` (§2.3).
    pub(crate) rung: u32,
    /// When the current rung was submitted, from which `exit_step_s` runs.
    pub(crate) since: Option<RiskClock>,
    /// The current rung is at `max_exit_offset`: it rests, and never steps again.
    pub(crate) floored: bool,
    /// The current rung's cancel was a step's, so its confirmation submits the next rung.
    pub(crate) stepping: bool,
    /// The step's confirmation came while no session was open: the next rung waits for the next
    /// open, priced fresh then, and the hold was journaled (DEC-260 (18)).
    pub(crate) parked: bool,
}

/// The restriction a reconciliation places for one subject — an instrument, or external activity
/// on the account — and the only one an owner acknowledgment of that subject lifts (§11).
pub(crate) fn restriction_for(subject: &str) -> String {
    format!("reconciliation:{subject}")
}

/// A restriction is the one an acknowledgment of its own subject lifts, so no two subjects share
/// one (§11): the account-wide cash, fee and incomplete-run restrictions included.
#[cfg(test)]
mod restriction_tests {
    use super::restriction_for;

    #[test]
    fn each_subject_has_its_own_restriction() {
        let subjects = [
            "AAPL",
            "BTC/USD",
            "external_activity",
            "cash",
            "fees",
            "incomplete",
        ];
        for (i, subject) in subjects.iter().enumerate() {
            let own = restriction_for(subject);
            assert!(
                own.ends_with(subject),
                "the restriction names its subject: {own}"
            );
            for other in subjects.iter().skip(i + 1) {
                assert_ne!(
                    own,
                    restriction_for(other),
                    "an acknowledgment of {subject} never lifts {other}'s restriction"
                );
            }
        }
    }
}

/// The accessors read their own fields. These cases set the private fields directly, so a mutant
/// that answers the fresh value dies even for a field no merged slice folds yet, which no test
/// outside the crate can reach until that slice lands (DEC-137).
#[cfg(test)]
mod tests {
    use mandate_accounting::Side;
    use mandate_time::Date;

    use super::*;
    use crate::types::{AccountRef, Input, OrderState, Purpose, WorkspaceId};

    fn id(raw: &str) -> ClientOrderId {
        ClientOrderId::seeded_for_tests(raw)
    }

    fn apple() -> Option<InstrumentId> {
        InstrumentId::new("AAPL").ok()
    }

    fn held() -> Option<ExecutorState> {
        let instrument = apple()?;
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        state.epoch = Some(WriterEpoch(7));
        state.started = true;
        state.environment = Some("paper".to_owned());
        state.risk_clock = Some(RiskClock::from_secs(42));
        state.unresolved = Some(UnresolvedAppend {
            head: Seq(3),
            input: Input::Tick(RiskClock::from_secs(42)),
            drafts: Vec::new(),
        });
        state.orders.insert(
            id("md-a"),
            Order {
                client_order_id: id("md-a"),
                intent_id: None,
                agent: Some(AgentId("agent-a".to_owned())),
                instrument: instrument.clone(),
                side: Side::Buy,
                qty: Qty::parse("1").ok()?,
                filled_qty: Qty::ZERO,
                state: OrderState::Accepted,
                attempt: 1,
                purpose: Purpose::Open,
                absent_lookups: 0,
                first_absence_at: None,
                cancel_unconfirmed: false,
                replaced_by: None,
                created_on: Date::parse("2026-09-22").ok(),
            },
        );
        state
            .reservations
            .insert(id("md-a"), Usd::parse("150").ok()?);
        state.unprotected.push(UnprotectedInterval {
            instrument: instrument.clone(),
            started_at: RiskClock::from_secs(40),
            ended_at: None,
            alerted: false,
            uncovered: false,
            bracket: None,
        });
        state
            .positions
            .insert(instrument.clone(), SignedQty::parse("10").ok()?);
        state.fills.insert(FillId("f-1".to_owned()));
        state
            .modes
            .insert(AgentId("agent-a".to_owned()), Mode::Paused);
        state.account_state = AccountState::ClosingOnly;
        state.consecutive_403s = 2;
        state.observed = Some(ObservedAccount {
            state: AccountState::ClosingOnly,
            multiplier: 2,
            equity: Usd::parse("1").ok()?,
            cash: Usd::parse("1").ok()?,
            buying_power: Usd::parse("1").ok()?,
            non_marginable_buying_power: Usd::parse("1").ok()?,
            accrued_fees: Usd::ZERO,
            complete: true,
        });
        state.mismatched.insert(instrument);
        state.checkpoint = Some(ActivityCursor("cursor-9".to_owned()));
        state.reconciled_through = Some(Seq(5));
        state.last_submission = Some(Seq(4));
        Some(state)
    }

    #[test]
    fn every_field_accessor_answers_its_own_field() {
        let state = held();
        assert!(state.is_some(), "the held state builds");
        let Some(state) = state else { return };
        assert_eq!(state.epoch(), Some(WriterEpoch(7)));
        assert!(state.started());
        assert_eq!(state.environment(), Some("paper"));
        assert_eq!(state.risk_clock(), Some(RiskClock::from_secs(42)));
        assert_eq!(state.unresolved().map(|u| u.head), Some(Seq(3)));
        assert_eq!(state.orders().len(), 1);
        assert_eq!(state.reservations().len(), 1);
        assert_eq!(state.unprotected_intervals().len(), 1);
        assert_eq!(state.positions().len(), 1);
        assert_eq!(state.fills().len(), 1);
        assert_eq!(state.modes().len(), 1);
        assert_eq!(state.account_state(), AccountState::ClosingOnly);
        assert_eq!(state.consecutive_403s(), 2);
        assert_eq!(state.observed_account().map(|a| a.multiplier), Some(2));
        assert_eq!(state.mismatched().len(), 1);
        assert_eq!(
            state.checkpoint(),
            Some(&ActivityCursor("cursor-9".to_owned()))
        );
        assert_eq!(state.reconciled_through(), Some(Seq(5)));
        assert_eq!(state.last_submission(), Some(Seq(4)));
    }
}
