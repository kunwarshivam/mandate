//! The core's vocabulary: what goes in, what comes out, and the modes in between.

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_approval::{ActorKind, Notification};
use mandate_canon::Value;
use mandate_journal::Environment;
use mandate_num::{Price, Qty};

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

/// A broker connection's opaque id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionId(pub String);

/// A workspace's opaque id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkspaceId(pub String);

/// Which deployment this runtime is. A runtime is for exactly one agent deployment (DEC-08), and it
/// needs its own ids to tell a kill switch addressed to it from one addressed to a sibling, and its
/// own streams from another workspace's. Nothing here is secret: all three are opaque ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deployment {
    pub agent: AgentId,
    pub connection: ConnectionId,
    pub workspace: WorkspaceId,
}

impl Deployment {
    /// Whether a kill switch of this scope reaches this deployment (trading-domain spec §5.5).
    pub fn in_scope(&self, scope: &KillScope) -> bool {
        match scope {
            KillScope::Agent(agent) => *agent == self.agent,
            KillScope::Connection(connection) => *connection == self.connection,
            KillScope::Workspace(workspace) => *workspace == self.workspace,
        }
    }
}

/// What a flatten planner is asked for. The runtime never computes the plan itself
/// (`AGENTS.md` rule 13, mandate spec `MC-F01` to `MC-F04`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlattenRequest {
    pub initiator: Initiator,
    /// The one instrument an owner exit closes, or `None` for a kill switch's whole agent. The plan
    /// then sells exactly the agent's sub-ledger quantity of that instrument and cancels only its
    /// orders in it (mandate spec §6.1, trading-domain spec §5.5; DEC-257).
    pub instrument: Option<InstrumentId>,
    pub confirmation: Option<OwnerConfirmation>,
    /// The intent event ids of every intent the fold still holds outstanding, which is what
    /// `RuntimeState::working_orders` supplies; the plan's cancels name client order ids, a
    /// mapping the exit adapter derives from the journal (DEC-449 item 3).
    pub working_orders: Vec<String>,
}

/// An agent mode, ordered so that `max` is the strictest (trading-domain spec §7.4, mandate spec
/// §5.9). The ordering is load-bearing: the effective mode is a maximum over it, so a wrong order
/// would let a paused agent open a position.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    #[default]
    Normal,
    ExitsOnly,
    Paused,
    Stopped,
}

/// What the runtime's own restriction set can hold. The risk restrictions of mandate spec §5.9 are
/// the executor's and reach the runtime only as a copied mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LocalHold {
    /// The replay is not yet confirmed by the reconciliation trading-domain spec §11 runs at
    /// startup (mandate spec §5.9's reconciliation restriction, held from startup).
    AwaitingReconciliation,
    /// The owner paused this deployment (HLD "Agent lifecycle").
    OwnerPaused,
    /// The owner, a risk limit, or a platform operator stopped it. Terminal.
    Stopped,
}

impl LocalHold {
    pub fn mode(self) -> Mode {
        match self {
            Self::AwaitingReconciliation | Self::OwnerPaused => Mode::Paused,
            Self::Stopped => Mode::Stopped,
        }
    }
}

/// Why an intent exists (trading-domain spec §6.1). The purpose decides what the mode may hold and
/// what a restart may re-hand.
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
    /// Whether the purpose adds risk, which is what the mode gate turns on.
    pub fn adds_risk(self) -> bool {
        matches!(self, Self::Open | Self::Increase)
    }
}

/// Who ordered a stop. `Broker` is deliberately absent: a broker-driven restriction is account
/// state (trading-domain spec §7.3) that reaches the runtime as a copied `AgentModeApplied`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Initiator {
    Owner,
    RiskLimit,
    PlatformOperator,
}

impl Initiator {
    /// The final mode, applied before anything else (trading-domain spec §5.5, mandate spec §5.5,
    /// DEC-100). Every initiator has one, because a kill switch that refused would be worse than
    /// one that over-stopped.
    pub fn final_mode(self) -> Mode {
        match self {
            Self::Owner | Self::PlatformOperator => Mode::Stopped,
            Self::RiskLimit => Mode::Paused,
        }
    }

    /// The purpose of the flatten's sells. An operator cannot confirm a bid, so an operator stop
    /// sells on the automated schedule (DEC-131 item 21, Decisions needed 6).
    pub fn sell_purpose(self) -> Purpose {
        match self {
            Self::Owner => Purpose::OwnerExit,
            Self::RiskLimit | Self::PlatformOperator => Purpose::RiskExit,
        }
    }
}

/// What a kill switch reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KillScope {
    Agent(AgentId),
    Connection(ConnectionId),
    Workspace(WorkspaceId),
}

/// The owner's confirmation of a displayed bid, which is what lets an equity sell outside the
/// regular session (trading-domain spec §5.5, §5.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerConfirmation {
    pub bid: Price,
    pub bid_size: Qty,
    pub floor: Price,
    pub user: String,
    pub step_up: String,
}

/// A command addressed to this deployment. Kill switches and stops are read from a priority channel
/// before anything else (ADR-0001 ES-06).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    KillSwitch {
        scope: KillScope,
        initiator: Initiator,
        confirmation: Option<OwnerConfirmation>,
    },
    Pause,
    Resume,
    Stop,
    OwnerExit {
        instrument: InstrumentId,
        confirmation: Option<OwnerConfirmation>,
    },
}

/// One event, already validated and sequenced by the journal, replayed into the fold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldedEvent {
    pub stream: String,
    pub seq: Seq,
    pub event_id: EventId,
    pub event_type: String,
    pub causation_id: Option<EventId>,
    /// The envelope's `actor.kind` (journal spec §3). Admission admits a response only from a
    /// `user` (check 3, EI-10), so a reader that cannot tell gives anything but `User`.
    pub actor: ActorKind,
    pub payload: Value,
}

/// A market-data or news observation, journaled as `ObservationRecorded`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub source: String,
    pub instrument: Option<InstrumentId>,
    pub at: RiskClock,
    pub data: Value,
}

/// A signal model's output (mandate spec §8.1, §8.2). Freshness is judged from `expires_at`
/// against the risk-clock second, never against a wall clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOutput {
    pub model: String,
    pub version: String,
    pub instrument: InstrumentId,
    pub as_of: RiskClock,
    pub expires_at: RiskClock,
    pub content: Value,
}

/// Everything that can reach the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// The process has folded every followed stream and taken `writer_epoch`. The only input that
    /// may re-hand an intent, and the only one that journals the startup hold.
    Started(WriterEpoch),
    /// An event tailed from a followed stream in `seq` order. The only variant carrying a position.
    /// The owner's answers and commands arrive this way, as the control stream's
    /// `ApprovalResponseSubmitted` and `OwnerCommandIssued`, whose `event_id` is the idempotency key
    /// and whose `actor` admission checks (DEC-155 item 2, mandate spec §6.1).
    Journal(FoldedEvent),
    /// The scheduler's tick.
    Tick(RiskClock),
    Observation(Observation),
    ModelOutput(ModelOutput),
    Command(Command),
}

/// A draft for the agent stream. The shell wraps it in the journal envelope; the core never
/// composes an envelope, so it cannot forge `seq`, `prev_hash`, or `recorded_at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDraft {
    pub event_id: EventId,
    pub event_type: String,
    pub causation_id: Option<EventId>,
    pub payload: Value,
}

/// One sell or cancel a flatten plan asks for. The plan itself is `mandate-risk`'s (family F); the
/// runtime carries it without computing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlattenLeg {
    pub instrument: InstrumentId,
    pub asset_class: AssetClass,
    pub qty: Qty,
    pub deferred_to_regular_session: bool,
}

/// The agent-scoped plan of trading-domain spec §5.5: only this agent's orders, by
/// `client_order_id`, and exactly its sub-ledger quantity. There is no account-wide variant in this
/// type, which is how `AGENTS.md` rule 13 is made unrepresentable rather than merely tested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlattenPlan {
    pub cancel_client_order_ids: Vec<String>,
    pub sells: Vec<FlattenLeg>,
    pub purpose: Purpose,
    pub confirmation: Option<OwnerConfirmation>,
}

/// What the runtime asks the executor to do with an intent, handed across `IntentSink`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntentBody {
    Order {
        instrument: InstrumentId,
        side: Side,
        qty: Qty,
        limit: Price,
        purpose: Purpose,
    },
    Flatten(FlattenPlan),
}

/// The proposal's time in force, copied into the account stream exactly as proposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeInForce {
    Day,
    Gtc,
    Ioc,
}

/// Protective prices computed before the shell boundary from the confirmed mandate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtectionPrices {
    pub stop: Price,
    pub take_profit: Option<Price>,
}

/// Mandate-derived order inputs that are not present in [`IntentBody`].
///
/// `None` on a replay means the journaled intent predates this explicit source. The sink refuses
/// such an order instead of choosing a time in force, asset class, or protection policy itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderExecution {
    pub asset_class: AssetClass,
    pub tif: TimeInForce,
    pub protection_required: bool,
    pub protection: Option<ProtectionPrices>,
}

/// One handoff: the intent id is the `event_id` of the `IntentProposed` that recorded it, which is
/// what makes the sink safely at-least-once (journal spec §2, §5.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentHandoff {
    pub intent_id: EventId,
    pub body: IntentBody,
    pub execution: Option<OrderExecution>,
}

/// Which deadline a timer is for. Keyed so that arming twice replaces rather than duplicates.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TimerId {
    ApprovalDeadline(EventId),
    ProtectionRenewal(InstrumentId),
    Evaluation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimerRequest {
    Arm { id: TimerId, at: RiskClock },
    Cancel { id: TimerId },
}

/// An owner alert. It carries opaque ids and a message key only: no instrument, quantity, price, or
/// thesis content ever reaches a notification (`AGENTS.md` rule 6). The type is the enforcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationRef {
    pub subject_event: EventId,
    pub message_key: &'static str,
}

/// What one step asks the shell to do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Journal(EventDraft),
    Intent(IntentHandoff),
    Timer(TimerRequest),
    Notify(NotificationRef),
    /// An approval request's notification: its opaque id and one closed generic text, with no field
    /// for anything else (`AGENTS.md` rule 6, EI-9). It follows the `ApprovalRequested` draft it
    /// names in the same list, so it never points at an uncommitted request (rule 5).
    NotifyApproval(Notification),
}

/// A proposal the order plan produced, before the dry run and the autonomy classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub instrument: InstrumentId,
    /// What the drift band reads at re-validation: 100 bp for `us_equity`, 200 bp for `crypto`
    /// (DEC-156 item 3).
    pub asset_class: AssetClass,
    pub side: Side,
    pub qty: Qty,
    pub limit: Price,
    pub purpose: Purpose,
    pub combined_score: Value,
    pub execution: Option<OrderExecution>,
}

/// The dry run's answer. `Allow` authorises nothing: the binding gate runs on the account stream
/// (`AGENTS.md` rules 1 and 12), so this can only narrow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DryRunVerdict {
    Allow,
    Deny { reason_code: String },
}

/// How the mandate's autonomy rules classified a proposal (mandate spec §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Autonomy {
    Auto,
    Ask,
    Deny,
}

/// `OrderPlan::classify`'s answer: the classification and the `DecidedBy` label of what decided it
/// (`rule:<id>`, `default`, `admission_ceiling`, …). An approval request binds the label and check 10
/// compares it with the label at re-validation (DEC-156 item 4). `None` when the classifier cannot
/// name what decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub autonomy: Autonomy,
    pub decided_by: Option<String>,
}

/// The mandate view `mandate-spec` will provide. Only what the runtime reads is here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MandateView {
    pub version: String,
    pub working_universe: BTreeSet<InstrumentId>,
    pub restricted_instruments: BTreeSet<InstrumentId>,
    pub approval: ApprovalSettings,
}

/// What admission reads of the mandate version and its connection (mandate spec §6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalSettings {
    /// `autonomy.approval.approvers`, as the opaque user ids admission compares (check 3).
    pub approvers: BTreeSet<String>,
    /// The version's author, whom independent approval excludes (check 7).
    pub author: String,
    /// `autonomy.approval.timeout_s`: the deadline is the request's risk clock plus this.
    pub timeout_s: i64,
    /// The connection's environment: `cli_confirm` step-up is admitted on `paper` only (DEC-155
    /// item 4).
    pub environment: Environment,
}

/// The signal inputs a decision reads: the outputs the fold holds, by model then instrument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalInputs {
    pub outputs: BTreeMap<String, BTreeMap<InstrumentId, ModelOutput>>,
    pub now: RiskClock,
}

/// Whether the executor is known to hold an intent yet.
///
/// The two states answer different questions and conflating them loses one of them. `Pending` means
/// the handoff may not have arrived, so `Input::Started` hands it again; `Taken` means the account
/// stream carries `IntentReceived` for it, so re-handing is pointless. **Both are still live orders**
/// until a terminal outcome, so both belong in a flatten's `working_orders`: an order the executor
/// took and is working is exactly the one a kill switch must name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handoff {
    Pending,
    Taken,
}

/// An intent the fold still holds live: proposed, and with no terminal outcome on the account stream
/// (an `OrderAbandoned`, a `GateDecided` denial, or a terminal order state).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outstanding {
    pub intent_id: EventId,
    pub purpose: Purpose,
    pub handoff: Handoff,
}
