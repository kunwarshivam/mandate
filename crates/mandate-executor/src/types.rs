//! The core's vocabulary: what goes in, what comes out, and the order states in between.

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_canon::{Object, Value};
use mandate_num::{Fraction, Price, Qty, SignedQty, Usd};
use mandate_time::Date;

use crate::ids::{ClientOrderId, IntentId};

/// The scheduler's whole-second risk clock (mandate spec §5.2). The only time the core knows:
/// `event_time` and `recorded_at` are never read for timing, and nothing reads a wall clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RiskClock(i64);

impl RiskClock {
    pub fn from_secs(secs: i64) -> Self {
        Self(secs)
    }

    pub fn secs(self) -> i64 {
        self.0
    }
}

/// A stream writer's fencing token (journal spec §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct WriterEpoch(pub u64);

/// A journal `seq`, the only ordering key within a stream (journal spec §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Seq(pub u64);

/// A ULID, opaque to the core.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventId(pub String);

/// An agent deployment's opaque id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AgentId(pub String);

/// The account stream's subject: an opaque internal ULID, **never** the broker's account number,
/// which journal spec §6.4 keeps in the personal-data vault.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AccountRef(pub String);

/// A workspace's opaque id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkspaceId(pub String);

/// A broker fill's own id, which is what makes a re-ingested fill idempotent (journal spec §5.2).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FillId(pub String);

/// A confirmed mandate version, carried as the `man` configuration reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MandateVersion(pub String);

/// Where the activities walk resumes from (trading-domain spec §11).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActivityCursor(pub String);

/// Which broker account this executor owns the ledger for (DEC-26: one serialized ledger per
/// account). The ids are given rather than inferred from the first event folded, because a state
/// that infers its own identity accepts anything (the lesson of [#134]'s round-1 review).
///
/// [#134]: https://github.com/kunwarshivam/mandate/pull/134
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountScope {
    pub account: AccountRef,
    pub workspace: WorkspaceId,
}

/// An agent mode, ordered so that `max` is the strictest (trading-domain spec §7.4, mandate spec
/// §5.9). The ordering is load-bearing: an effective mode is a maximum over it, so a wrong order
/// would let a paused agent open a position.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    #[default]
    Normal,
    ExitsOnly,
    Paused,
    Stopped,
}

/// Account state detected from statuses and rejects, evaluated **before** agent mode
/// (trading-domain spec §7.3, §9.1 check 1). Ordered so that `max` is the strictest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum AccountState {
    #[default]
    Active,
    ClosingOnly,
    Blocked,
}

/// Why an intent exists (trading-domain spec §6.1). The purpose decides what the mode may hold,
/// what a pacing control may pace, and what `AGENTS.md` rule 13 exempts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Purpose {
    Open,
    Increase,
    RiskExit,
    OwnerExit,
    DiscretionaryExit,
    Protective,
    Flatten,
}

impl Purpose {
    /// Whether the purpose adds risk, which is what the mode gate and the approval rules turn on.
    pub fn adds_risk(self) -> bool {
        matches!(self, Self::Open | Self::Increase)
    }

    /// Whether `AGENTS.md` rule 13 exempts this purpose from every pacing control: conduct
    /// controls, eligibility, day-trade budgets, buying power, and opening-session rules.
    pub fn exempt_from_pacing(self) -> bool {
        matches!(self, Self::RiskExit | Self::Protective | Self::Flatten)
    }
}

/// Who ordered a stop. `Broker` is deliberately absent: a broker-driven restriction is account
/// state (trading-domain spec §7.3), not an initiator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Initiator {
    Owner,
    RiskLimit,
    PlatformOperator,
}

impl Initiator {
    /// The final mode, applied before anything else (trading-domain spec §5.5, DEC-100).
    pub fn final_mode(self) -> Mode {
        match self {
            Self::Owner | Self::PlatformOperator => Mode::Stopped,
            Self::RiskLimit => Mode::Paused,
        }
    }

    /// The purpose of the flatten's sells. An operator cannot confirm a bid, so an operator stop
    /// sells on the automated schedule.
    pub fn sell_purpose(self) -> Purpose {
        match self {
            Self::Owner => Purpose::OwnerExit,
            Self::RiskLimit | Self::PlatformOperator => Purpose::RiskExit,
        }
    }
}

/// What a kill switch reaches. The account and workspace scopes are the only two that may reach
/// the broker's account-wide endpoints, which [`AccountWideScope`] is what enforces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KillScope {
    Agent(AgentId),
    Account(AccountRef),
    Workspace(WorkspaceId),
}

/// The two kill-switch scopes that may reach the broker's account-wide endpoints, and only those
/// two. There is no `Agent` variant, so not even code inside this crate can describe an
/// agent-scoped account-wide request (trading-domain spec §5.5, `AGENTS.md` rule 13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountWide {
    Account(AccountRef),
    Workspace(WorkspaceId),
}

/// A witness that a kill switch is account- or workspace-scoped.
///
/// [`BrokerRequest::CancelAll`] and [`BrokerRequest::ClosePosition`] take one. Its field is
/// private, so nothing outside this crate can construct one, and the field is an
/// [`AccountWide`], which has no agent variant, so nothing inside it can build one for an agent
/// either. That is how `AGENTS.md` rule 13 is made unrepresentable rather than merely forbidden
/// (trading-domain spec §5.5, task brief interpretation 18). The account and workspace
/// kill-switch paths construct it in the implementation PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountWideScope {
    scope: AccountWide,
}

impl AccountWideScope {
    /// Which of the two account-wide scopes this is. There is no public constructor.
    pub fn scope(&self) -> &AccountWide {
        &self.scope
    }
}

/// The owner's confirmation of a displayed bid, which is what lets an equity sell price outside the
/// regular session (trading-domain spec §5.5, §5.6, the agent stream's `OwnerExitRequested`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerConfirmation {
    pub bid: Price,
    pub bid_size: Qty,
    pub floor: Price,
    pub user: String,
    pub step_up: String,
}

/// One sell or cancel an agent-scoped flatten asks for: this agent's orders by `client_order_id`,
/// and exactly its sub-ledger quantity. There is no account-wide variant in this type
/// (`AGENTS.md` rule 13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlattenLeg {
    pub instrument: InstrumentId,
    pub asset_class: AssetClass,
    pub qty: Qty,
    pub deferred_to_regular_session: bool,
}

/// The agent-scoped plan of trading-domain spec §5.5, as stream I hands it over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlattenPlan {
    pub cancel_client_order_ids: Vec<String>,
    pub sells: Vec<FlattenLeg>,
    pub purpose: Purpose,
    pub confirmation: Option<OwnerConfirmation>,
}

/// What the runtime asks the executor to do with an intent.
///
/// The shape is stream I's `mandate_runtime::IntentBody`, transcribed rather than imported:
/// `mandate-runtime` and `mandate-executor` are both layer 6, so neither may name the other's
/// types and the adapter that carries one across is the shell's, at layer 7 (DEC-131, ES-02).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntentBody {
    Order {
        instrument: InstrumentId,
        side: Side,
        qty: Qty,
        limit: Price,
        purpose: Purpose,
        /// The protective prices the order builder computed from the mandate's stop and
        /// take-profit distances (stream H, mandate spec §3's `protection`). The executor places
        /// and re-places exactly these and never invents one: an entry that carries them goes as a
        /// bracket (or, for crypto, is followed by a stop-limit), and one that does not goes as a
        /// plain order. Whether protection is required is the mandate's `protection_required`,
        /// which the gate reads, not this crate (DEC-133).
        protection: Option<ProtectionPrices>,
    },
    Flatten(FlattenPlan),
}

/// The protective prices one entry carries (trading-domain spec §5.4, RC-14, RC-21).
///
/// `take_profit` is `None` for crypto, whose take-profit the runtime watches rather than the
/// broker (§5.4, DEC-36); a crypto stop-limit's limit is stop × (1 − `crypto_stop_limit_offset`)
/// from the mandate view, derived by the executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtectionPrices {
    pub stop: Price,
    pub take_profit: Option<Price>,
}

/// One handoff. `intent_id` **is** the `event_id` of the agent stream's `IntentProposed`
/// (journal spec §2), which is what makes the sink safely at-least-once: a re-hand of an intent
/// the fold already carries produces an empty effect list (task brief interpretation 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentHandoff {
    pub intent_id: IntentId,
    pub agent: AgentId,
    /// The TIF an order proposal carries, which `IntentReceived` copies exactly — as proposed,
    /// never the submission's (journal spec §9.1, DEC-389 item 3). A flatten has no proposal TIF.
    pub tif: Option<TimeInForce>,
    pub body: IntentBody,
}

/// An order's internal state (trading-domain spec §5.7's diagram).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OrderState {
    Intent,
    Submitting,
    Accepted,
    PartiallyFilled,
    PendingCancel,
    PendingReplace,
    Unknown,
    Filled,
    Canceled,
    Rejected,
    Expired,
    Replaced,
    Abandoned,
}

impl OrderState {
    /// The six terminal states §5.7's diagram names, each of which releases the order's
    /// reservation — `Abandoned` and `Replaced` included (task brief interpretation 26).
    pub const TERMINAL: [Self; 6] = [
        Self::Filled,
        Self::Canceled,
        Self::Rejected,
        Self::Expired,
        Self::Replaced,
        Self::Abandoned,
    ];

    /// Whether the state is terminal, and therefore final.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Filled
                | Self::Canceled
                | Self::Rejected
                | Self::Expired
                | Self::Replaced
                | Self::Abandoned
        )
    }
}

/// What §5.7's broker-status table says one raw Alpaca status means. The table is **total**: a
/// value outside it is [`crate::ExecutorError::UnmappedBrokerStatus`], which the step turns into a
/// pause and an alert, never a silent no-op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusMapping {
    /// `new`, `accepted`, `pending_new`, `accepted_for_bidding`, `held`, and (flagged) `suspended`.
    Becomes(OrderState),
    /// `done_for_day`, `stopped`, `calculated`: the internal state is unchanged.
    Unchanged,
    /// `suspended`: Accepted, flagged restricted, and triggers a reconciliation.
    AcceptedFlaggedRestricted,
    /// `replaced`: the old order is `Replaced` and a new, linked order is `Accepted`, with the
    /// gate re-run on the new one.
    ReplacedPair,
}

/// One order the fold carries, keyed by its client order id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub client_order_id: ClientOrderId,
    pub intent_id: Option<IntentId>,
    /// The agent the order belongs to. `None` only for a protective leg the broker created that
    /// DEC-160's leg-agent rule could not attribute: it is in the order set like every other leg,
    /// so a reconciliation finds it present and its lifecycle folds, but it counts toward no
    /// agent's sub-ledger and holds openings in its instrument until it is attributed or done.
    pub agent: Option<AgentId>,
    pub instrument: InstrumentId,
    pub side: Side,
    pub qty: Qty,
    pub filled_qty: Qty,
    pub state: OrderState,
    pub attempt: u32,
    pub purpose: Purpose,
    /// Consecutive confirmed absences, and when the first of them was seen: together they are
    /// §5.7's "N lookups over T seconds" (task brief interpretation 9).
    pub absent_lookups: u32,
    pub first_absence_at: Option<RiskClock>,
    /// Set while a cancel request is outstanding and not yet confirmed. Nothing is submitted in
    /// the instrument while it is true (trading-domain spec §5.4).
    pub cancel_unconfirmed: bool,
    pub replaced_by: Option<ClientOrderId>,
    /// The `TradingDayStarted` date a GTC order was created on, from which its 90-day expiry and
    /// its re-placement buffer are computed (§5.2, §5.4).
    pub created_on: Option<Date>,
}

/// What one instrument's protection looks like: the resting protective orders and the quantity
/// they cover. Σ protective sell quantity ≤ position is a property, not a check (§5.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Protection {
    pub instrument: InstrumentId,
    pub resting: Vec<ClientOrderId>,
    pub covered_qty: Qty,
    /// The last placement's stop and, unless crypto's, take-profit (§5.4, DEC-36), which a
    /// re-placement re-uses; `None` when no placement named a stop.
    pub prices: Option<ProtectionPrices>,
}

/// One interval in which an instrument's position was not fully covered, journaled from start to
/// end as `ProtectionChanged` and bounded by `max_unprotected_s` (§5.4, task brief
/// interpretation 21). It is an interval and not a flag, so it cannot be closed by forgetting it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnprotectedInterval {
    pub instrument: InstrumentId,
    pub started_at: RiskClock,
    pub ended_at: Option<RiskClock>,
    pub alerted: bool,
    /// Its sequence or re-placement ended with no prices to place protection at, journaled and
    /// alerted (`expiry_unreplaceable`): the position is still not covered, so the interval stays
    /// open and bounded until the next interval in the instrument starts (DEC-367 item 4, #468's
    /// round-4 review, m2).
    pub uncovered: bool,
    /// The bracket entry whose partial fill opened it, when one did. Only an `unprotected_end`
    /// naming that entry, the acknowledgment of the OCO placed for it, or a new interval's start
    /// while that OCO's acknowledgment is awaited ends this interval, so a second bracket in the
    /// instrument never closes it (§5.4; DEC-521 item 3, backlog E4 and E4b).
    pub bracket: Option<ClientOrderId>,
}

/// One difference a reconciliation found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    pub kind: DifferenceKind,
    /// The client order id, fill id, or instrument the difference is about, as text, so a payload
    /// can carry it without the type.
    pub subject: String,
    /// Whether the broker's value was adopted, read through [`Difference::adopted`]. Private, so
    /// nothing outside this module writes a `Difference` literal or assigns the flag: every one is
    /// made by [`Difference::unexplained`] or [`Difference::adopting`], whose kinds hold §11's
    /// on-mismatch column in their types (#205 review, round 1, finding 3, and round 2, major 2).
    adopted: bool,
}

/// The rows §11 never adopts: a difference of one of these kinds is recorded and alerted, and the
/// ledger keeps its own value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unexplained {
    ExternalActivity,
    Position,
    Cash,
    Fee,
}

/// The rows §11 adopts: the broker's order state, and a fill the ledger had not seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Adopted {
    OrderState,
    MissingFill,
}

impl From<Unexplained> for DifferenceKind {
    fn from(kind: Unexplained) -> Self {
        match kind {
            Unexplained::ExternalActivity => Self::ExternalActivity,
            Unexplained::Position => Self::Position,
            Unexplained::Cash => Self::Cash,
            Unexplained::Fee => Self::Fee,
        }
    }
}

impl From<Adopted> for DifferenceKind {
    fn from(kind: Adopted) -> Self {
        match kind {
            Adopted::OrderState => Self::OrderState,
            Adopted::MissingFill => Self::MissingFill,
        }
    }
}

impl Difference {
    /// A difference the ledger keeps: an adoptable kind has no [`Unexplained`] variant, so this
    /// constructor cannot be handed one.
    pub(crate) fn unexplained(kind: Unexplained, subject: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            subject: subject.into(),
            adopted: false,
        }
    }

    /// A difference whose broker value was adopted, of a kind §11 adopts.
    pub(crate) fn adopting(kind: Adopted, subject: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            subject: subject.into(),
            adopted: true,
        }
    }

    /// Whether the broker's value was adopted. Only [`DifferenceKind::OrderState`] and
    /// [`DifferenceKind::MissingFill`] are ever adopted (trading-domain spec §11's on-mismatch
    /// column, task brief interpretation 13), and no assignment can change it.
    pub fn adopted(&self) -> bool {
        self.adopted
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DifferenceKind {
    OrderState,
    MissingFill,
    ExternalActivity,
    Position,
    Cash,
    Fee,
}

impl DifferenceKind {
    /// Whether §11's on-mismatch column adopts the broker's value for this row. A position, cash,
    /// or fee difference is never adopted: writing it away would make the ledger agree with the
    /// broker while destroying the evidence that they disagreed.
    pub fn adoptable(self) -> bool {
        matches!(self, Self::OrderState | Self::MissingFill)
    }
}

/// What a reconciliation concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconciliationVerdict {
    /// Everything matched.
    Clean,
    /// Order-set differences were adopted with compensating events, and missing fills ingested;
    /// nothing outside the order set differed.
    Adopted,
    /// A position, cash, or fee difference remains. The agents holding the instrument are paused
    /// and the owner is alerted, and nothing in this crate lifts that (interpretation 14).
    Mismatch,
}

/// What a reconciliation produced: the drafts and effects to run, and what it found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reconciliation {
    pub effects: Vec<Effect>,
    pub verdict: ReconciliationVerdict,
    pub differences: Vec<Difference>,
    /// The head the snapshot was taken at, which the `ReconciliationRun` append uses as its
    /// `expected_head` so a submission that landed in between answers `HeadMismatch`
    /// (interpretation 15).
    pub expected_head: Seq,
}

/// Why a reconciliation is running (trading-domain spec §11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileReason {
    Startup,
    UnknownOrder,
    SessionBoundary,
    FeePosting,
    LateFill,
    Scheduled,
}

/// The gate's verdict. Unlike stream I's dry run, this one **decides**: it is `mandate-risk`
/// called as a crate-private library, not an injected port (`AGENTS.md` rule 1, interpretation 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateVerdict {
    Allow,
    Deny { reason_code: String },
}

/// One row of `GateDecided`'s `checks` list (journal spec §9). The ids are trading-domain spec
/// §9.1's, in its evaluation order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateCheck {
    pub id: &'static str,
    pub passed: bool,
    pub inputs: Value,
    pub computed: Value,
}

/// One order as the broker describes it. `client_order_id` is **raw text**: an id that does not
/// parse is not ours, which is how external activity is detected rather than guessed
/// (trading-domain spec §7.1, §11).
///
/// There is no `account_number` and no account `id` in this type, and there is nowhere for one to
/// go: journal spec §6.4 keeps both in the vault, and the account stream's subject is an opaque
/// [`AccountRef`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerOrder {
    pub broker_order_id: String,
    pub client_order_id: Option<String>,
    pub instrument: InstrumentId,
    pub side: Side,
    pub qty: Qty,
    pub filled_qty: Qty,
    pub limit_price: Option<Price>,
    pub stop_price: Option<Price>,
    /// The raw status text, mapped by §5.7's total table and never interpreted here.
    pub status: String,
    pub reject_code: Option<String>,
    pub replaced_by_broker_order_id: Option<String>,
    /// A bracket's or OCO's legs, each as the broker describes it, nested under their parent as
    /// the open-orders read asks (`nested=true`). The broker creates them and names each with its
    /// own `client_order_id`, never one of ours (the recorded `submit_bracket_accepted`), so a
    /// leg is found through its parent, by side and status, and never by that id (DEC-878).
    pub legs: Vec<BrokerOrder>,
    pub created_on: Option<Date>,
}

/// One fill as the broker reports it (trading-domain spec §6.1's fill record).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerFill {
    pub fill_id: FillId,
    pub client_order_id: Option<String>,
    pub instrument: InstrumentId,
    pub side: Side,
    pub qty: Qty,
    pub price: Price,
    pub fees: Usd,
    pub trade_date: Date,
}

/// One position as the broker reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerPosition {
    pub instrument: InstrumentId,
    pub qty: SignedQty,
    pub avg_entry_price: Price,
}

/// The account fields trading-domain spec §7.2 and §7.3 name, and only those.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerAccount {
    pub status: String,
    pub crypto_status: String,
    pub trading_blocked: bool,
    pub account_blocked: bool,
    pub trade_suspended_by_user: bool,
    pub multiplier: u32,
    pub equity: Usd,
    pub cash: Usd,
    pub buying_power: Usd,
    pub non_marginable_buying_power: Usd,
    pub accrued_fees: Usd,
    /// The broker's equity at the prior session's close (§7.2's `last_equity`): the prior-close
    /// equity §9.2's `legacy_pdt` regime compares with its threshold. Read from the broker, never
    /// journaled: `AccountStateObserved` and `AccountSnapshotRecorded` are closed schemas (DEC-524).
    pub last_equity: Usd,
    /// The broker's maintenance margin requirement, from which [`BrokerAccount::maintenance_excess`]
    /// derives the excess §9.2's `intraday_margin` regime checks. Never journaled (DEC-524).
    pub maintenance_margin: Usd,
}

/// A reject the broker answered with, from which §7.3's restriction table is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerReject {
    pub client_order_id: Option<String>,
    pub http_status: u16,
    pub code: Option<String>,
    /// The broker's message, which the restriction table matches against. It never reaches a
    /// notification (`AGENTS.md` rule 6).
    pub message: String,
}

/// Everything the shell gathered in one reconciliation pass (trading-domain spec §11's sources).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerSnapshot {
    pub open_orders: Vec<BrokerOrder>,
    pub positions: Vec<BrokerPosition>,
    pub account: BrokerAccount,
    pub fills: Vec<BrokerFill>,
    pub cursor: ActivityCursor,
    pub reason: ReconcileReason,
    /// The stream head the snapshot was taken at. A `ReconciliationRun` is appended with this as
    /// its `expected_head`, so a submission that landed in between makes the append answer
    /// `HeadMismatch` rather than publishing a run that never saw it (interpretation 15).
    pub taken_at_head: Seq,
}

/// What the connector answered. The only way a broker fact enters the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerOutcome {
    Submitted(BrokerOrder),
    /// The broker refused our own `client_order_id` because it already has that order. Folded as
    /// "already submitted", never as a failure (E7-2 step 6).
    DuplicateClientOrderId {
        client_order_id: String,
    },
    Rejected(BrokerReject),
    Order(BrokerOrder),
    /// The query answered that the broker does not have the order. One absence never resubmits.
    Absent {
        client_order_id: String,
    },
    OpenOrders(Vec<BrokerOrder>),
    Positions(Vec<BrokerPosition>),
    Account(BrokerAccount),
    Activities {
        fills: Vec<BrokerFill>,
        cursor: ActivityCursor,
    },
    /// The broker confirmed the order canceled: trading-domain spec §5.7's
    /// `PendingCancel --> Canceled: confirmed` (DEC-867 item 4). A connector returns it only when
    /// the broker's own answer shows the order canceled; a cancel request the broker merely
    /// accepted is answered with the order as the broker shows it, never with this.
    CancelAccepted {
        client_order_id: String,
    },
    AccountWideAccepted,
    /// The answer to [`BrokerRequest::ListOrders`]: every record the broker listed for it, all
    /// pages, unfiltered beyond the listing. It names the order the listing was asked for.
    Listed {
        client_order_id: String,
        orders: Vec<ListedOrder>,
    },
}

/// One record of a [`BrokerOutcome::Listed`] answer: the order as the broker describes it, with
/// the two members [`BrokerOrder`] lacks and DEC-529 item 4 matches on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedOrder {
    pub order: BrokerOrder,
    pub order_type: OrderType,
    pub tif: TimeInForce,
}

/// Who placed an order, as a broker that tags it records it. Only `Agentic` exists: the
/// fallback lists only orders an agent placed (DEC-529 item 4's `placed_agent`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderOrigin {
    Agentic,
}

/// The shared fallback for an `Unknown` order on a profile with no query by client order id
/// ([DEC-529](../../../docs/project/decisions/DEC-529.md) item 4, connections spec §6.2,
/// [DEC-862](../../../docs/project/decisions/DEC-862.md)): list the account's orders in the
/// order's instrument, placed by an agent, created at or after `created_since`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderListing {
    /// The `Unknown` order this listing is for; the answer names it back.
    pub client_order_id: ClientOrderId,
    pub instrument: InstrumentId,
    pub origin: OrderOrigin,
    /// The order's `OrderSubmitted` risk clock less DEC-862 item 2's margin.
    pub created_since: RiskClock,
}

/// Why a request produced no usable answer. An `Err` from the connector is **not** a rejection: it
/// means the outcome is unknown and recovery must query (task brief interpretation 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BrokerUnknown {
    #[error("the request timed out")]
    Timeout,
    #[error("the broker answered ambiguously")]
    Ambiguous,
    #[error("the connection failed")]
    Transport,
}

impl BrokerUnknown {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Ambiguous => "ambiguous",
            Self::Transport => "transport",
        }
    }
}

/// A broker-pushed order, fill, account, or reject update the shell polled or streamed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerUpdate {
    Order(BrokerOrder),
    Fill(BrokerFill),
    Account(BrokerAccount),
    Reject(BrokerReject),
}

/// A quote or mark the shell observed, used for collar and ladder pricing, **never** for time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketObservation {
    pub instrument: InstrumentId,
    pub bid: Option<Price>,
    pub bid_size: Option<Qty>,
    pub ask: Option<Price>,
    pub last_trade: Option<Price>,
    pub mark: Option<Price>,
    /// Whether the quote passed the sanity checks of trading-domain spec §4: the ladder's
    /// reference bid is a *fresh, sane* quote first, then the last sane bid within five minutes,
    /// then the last trade.
    pub sane: bool,
    /// The risk-clock second the observation was made at, which is what the five-minute fallback
    /// window is measured against.
    pub observed_at: RiskClock,
}

/// One order to submit, described and not performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitOrder {
    pub client_order_id: ClientOrderId,
    pub instrument: InstrumentId,
    pub side: Side,
    pub qty: Qty,
    pub order_type: OrderType,
    pub tif: TimeInForce,
    pub limit_price: Option<Price>,
    pub stop_price: Option<Price>,
    pub bracket: Option<BracketLegs>,
    pub oco: Option<OcoLegs>,
    pub extended_hours: bool,
    pub purpose: Purpose,
}

/// The order types v1 uses. Trailing stops, FOK, and notional market buys are absent because §5.1
/// does not use them, so no code path can build one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderType {
    Limit,
    Market,
    StopLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeInForce {
    Day,
    Gtc,
    /// Crypto adds only (§5.1's "IOC (except crypto adds)").
    Ioc,
}

/// A bracket's two protective legs, which share the entry's TIF and carry no extended hours
/// (§5.2's capability matrix).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BracketLegs {
    pub take_profit: Price,
    pub stop: Price,
}

/// A GTC OCO for a filled quantity, at the bracket's prices (§5.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcoLegs {
    pub take_profit: Price,
    pub stop: Price,
    pub qty: Qty,
}

/// Every request this executor can make, described and not performed.
///
/// `CancelAll` and `ClosePosition` take an [`AccountWideScope`], which only the account and
/// workspace kill-switch paths can construct. There is no deposit, withdrawal, or transfer
/// variant, and no code path that could build one (`AGENTS.md` rule 8: no custody of funds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerRequest {
    Submit(SubmitOrder),
    Cancel {
        client_order_id: ClientOrderId,
    },
    /// Broker-initiated replacements only (§5.1 forbids ours); carried so the new order can be
    /// linked to the one it replaced.
    AcknowledgeReplace {
        replaced: ClientOrderId,
    },
    GetOrderByClientId(ClientOrderId),
    /// The fallback in place of [`Self::GetOrderByClientId`] where the profile cannot query by
    /// client order id (DEC-529 item 4). Never sent on any other profile.
    ListOrders(OrderListing),
    ListOpenOrders,
    ListPositions,
    GetAccount,
    ListActivities {
        since: ActivityCursor,
    },
    CancelAll(AccountWideScope),
    ClosePosition(AccountWideScope, InstrumentId),
}

/// Which deadline a timer is for. Keyed so that arming twice replaces rather than duplicates.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TimerId {
    /// §5.6's `exit_step_s` between ladder steps.
    LadderStep(ClientOrderId),
    /// §5.4's `bracket_partial_fill_timeout`.
    BracketPartialFill(ClientOrderId),
    /// §5.4's `max_unprotected_s` bound on one instrument's unprotected interval.
    UnprotectedBound(InstrumentId),
    /// §5.4's `stop_watchdog_s` on a resting stop.
    StopWatchdog(ClientOrderId),
    /// §5.7's `unknown_absent_window_s` between lookups of an `Unknown` order.
    UnknownLookup(ClientOrderId),
    /// §11's scheduled reconciliation.
    Reconciliation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimerRequest {
    Arm { id: TimerId, at: RiskClock },
    Cancel { id: TimerId },
}

/// An owner alert. It carries an opaque id and a message key only: no instrument, quantity, price,
/// reject message, or account field ever reaches a notification (`AGENTS.md` rule 6, DEC-11). The
/// type is the enforcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationRef {
    pub subject_event: EventId,
    pub message_key: &'static str,
}

/// A draft for the account stream. The shell wraps it in the journal envelope, so the core cannot
/// forge `seq`, `prev_hash`, or `recorded_at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDraft {
    pub event_id: EventId,
    pub event_type: String,
    /// The registered journal payload schema selected by the executor that owns this draft.
    pub schema_version: u64,
    /// The trusted configuration artifacts used for this exact event.
    pub config_refs: Object,
    pub causation_id: Option<EventId>,
    pub payload: Value,
}

/// One event, already validated and sequenced by the journal, replayed into the fold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldedEvent {
    pub stream: String,
    pub seq: Seq,
    pub event_id: EventId,
    pub event_type: String,
    pub causation_id: Option<EventId>,
    pub payload: Value,
}

/// A command addressed to this account's executor. The shell reads kill switches and risk exits
/// from a priority channel before ordinary inputs, and what gets journaled is the order in which
/// [`crate::handle`] saw them (ADR-0001 ES-06, interpretation 30).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    KillSwitch {
        scope: KillScope,
        initiator: Initiator,
        confirmation: Option<OwnerConfirmation>,
    },
    Reconcile(ReconcileReason),
    /// Cancels `agent`'s working **opening** orders in `instrument`, and nothing else
    /// ([DEC-853](../../../docs/project/decisions/DEC-853.md)): the paper adapter's bounded watch
    /// sends it at the close-window bound so no entry is left working (first paper trade FT-11).
    /// It goes through the cancel path an exit and a stricter mode already take, so each cancel is
    /// journaled as `OrderStateChanged` (`pending_cancel`, `cancel_requested`) before it leaves
    /// (`AGENTS.md` rule 5). It only reduces risk, so no gate, conduct, budget, session or mode rule
    /// denies it (rule 13); protective orders, exits, other agents and other instruments are never
    /// touched, an `Unknown` opening is left to its query, and with nothing working it changes
    /// nothing.
    CancelOpenings {
        agent: AgentId,
        instrument: InstrumentId,
    },
}

/// Everything that can reach the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// The process folded the account stream and took `writer_epoch`. The only input that may
    /// re-resolve an unacknowledged submission, and the input that opens the startup
    /// reconciliation. It never resubmits: it queries (journal spec §5.2).
    Started(WriterEpoch),
    /// An event tailed from a stream the executor follows, in `seq` order. The agent streams it
    /// follows are where `IntentProposed` arrives from.
    Journal(FoldedEvent),
    /// An intent the shell's adapter took from stream I's `IntentSink`. It carries no authority:
    /// it is journaled as `IntentReceived` and then gated like any other.
    Intent(IntentHandoff),
    /// What the connector answered, or that it answered nothing.
    Broker(Result<BrokerOutcome, BrokerUnknown>),
    /// A broker-pushed order or account update.
    BrokerUpdate(BrokerUpdate),
    /// A snapshot for reconciliation, gathered by the shell in one pass.
    BrokerSnapshot(BrokerSnapshot),
    /// The scheduler's tick: a whole-second risk clock and nothing else.
    Tick(RiskClock),
    /// A quote or mark, used for pricing and never for time.
    Market(MarketObservation),
    Command(Command),
}

/// What one step asks the shell to do, in order. The shell runs the journal appends first and
/// submits only on `Committed` or `AlreadyCommitted` (ES-06), which is what makes write-before-
/// acting structural rather than a convention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Journal(EventDraft),
    Broker(BrokerRequest),
    Timer(TimerRequest),
    Notify(NotificationRef),
}

/// The three offsets of one exit-price tier (trading-domain spec §5.6's table), read from the
/// effective-dated instrument snapshot rather than hard-coded (interpretation 22).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitTier {
    pub exit_offset: Fraction,
    pub exit_offset_step: Fraction,
    pub max_exit_offset: Fraction,
}

/// The parameters trading-domain spec §5.4, §5.6, and §5.7 name.
///
/// They are read from the effective-dated configuration the gate already reads, so changing one is
/// a configuration change with a content hash in `config_refs`, not a release (interpretation 22).
/// The values this stream proposes for the five the spec names **without** one are in the task
/// brief's Decisions needed 2 and are still `Proposed (founder)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutorConfig {
    /// An intent older than this is abandoned rather than submitted, at **every**
    /// `Intent → Submitting` transition (interpretation 11).
    pub max_intent_age_s: i64,
    pub unknown_absent_lookups: u32,
    pub unknown_absent_window_s: i64,
    pub protective_replace_buffer_trading_days: u32,
    pub restriction_403_threshold: u32,
    pub bracket_partial_fill_timeout_s: i64,
    pub max_unprotected_s: i64,
    pub stop_watchdog_s: i64,
    pub exit_step_s: i64,
    pub gtc_expiry_days: u32,
}

impl ExecutorConfig {
    /// The values a deployment starts from until the effective-dated configuration says otherwise.
    ///
    /// Five are the spec's own defaults: `bracket_partial_fill_timeout` 60 s, `max_unprotected_s`
    /// 60 s and `stop_watchdog_s` 60 s (§5.4), `exit_step_s` 5 s (§5.6), and the 90-day GTC expiry
    /// (§5.2). The other five have no spec value and are **Proposed (founder)**, not accepted
    /// (DEC-133 item 22; task brief Decisions needed 2), each at the value the brief argues is the
    /// conservative one: `max_intent_age` 120 s (a stale intent is abandoned, never acted on),
    /// three absent lookups over 15 s before an `Unknown` order is resubmitted (never early),
    /// re-placement 5 trading days before expiry (never late), and `closing_only` after three
    /// unexplained 403s (sooner reduces risk). The founder's ruling replaces them here.
    pub const PROPOSED: Self = Self {
        max_intent_age_s: 120,
        unknown_absent_lookups: 3,
        unknown_absent_window_s: 15,
        protective_replace_buffer_trading_days: 5,
        restriction_403_threshold: 3,
        bracket_partial_fill_timeout_s: 60,
        max_unprotected_s: 60,
        stop_watchdog_s: 60,
        exit_step_s: 5,
        gtc_expiry_days: 90,
    };
}

#[cfg(test)]
mod config_tests {
    use super::ExecutorConfig;

    /// The five spec defaults, and the five values still Proposed (founder) at the brief's
    /// conservative choice (DEC-133 item 22): a change to either set is a decision, not a refactor.
    #[test]
    fn the_proposed_configuration_is_the_spec_defaults_and_the_briefs_conservative_values() {
        let proposed = ExecutorConfig::PROPOSED;
        assert_eq!(
            (
                proposed.bracket_partial_fill_timeout_s,
                proposed.max_unprotected_s,
                proposed.stop_watchdog_s,
                proposed.exit_step_s,
                proposed.gtc_expiry_days,
            ),
            (60, 60, 60, 5, 90),
            "the spec's own defaults (§5.2, §5.4, §5.6)"
        );
        assert_eq!(
            (
                proposed.max_intent_age_s,
                proposed.unknown_absent_lookups,
                proposed.unknown_absent_window_s,
                proposed.protective_replace_buffer_trading_days,
                proposed.restriction_403_threshold,
            ),
            (120, 3, 15, 5, 3),
            "Proposed (founder), DEC-133 item 22"
        );
    }
}

/// §11's on-mismatch column, held by the constructors: every [`Unexplained`] kind is one
/// [`DifferenceKind::adoptable`] refuses, every [`Adopted`] kind one it accepts, and each
/// constructor records what its kind says.
#[cfg(test)]
mod difference_tests {
    use super::{Adopted, Difference, DifferenceKind, Unexplained};

    #[test]
    fn a_difference_is_adopted_exactly_when_its_kind_is_adoptable() {
        for kind in [
            Unexplained::ExternalActivity,
            Unexplained::Position,
            Unexplained::Cash,
            Unexplained::Fee,
        ] {
            let difference = Difference::unexplained(kind, "s");
            assert!(!DifferenceKind::from(kind).adoptable(), "{kind:?}");
            assert_eq!(
                (difference.kind, difference.adopted()),
                (DifferenceKind::from(kind), false)
            );
        }
        for kind in [Adopted::OrderState, Adopted::MissingFill] {
            let difference = Difference::adopting(kind, "s");
            assert!(DifferenceKind::from(kind).adoptable(), "{kind:?}");
            assert_eq!(
                (difference.kind, difference.adopted()),
                (DifferenceKind::from(kind), true)
            );
        }
    }
}
