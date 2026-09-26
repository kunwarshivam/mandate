#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The agent runtime core ([task brief](../../../docs/project/tasks/E6-1-agent-runtime-and-kill-switches.md),
//! backlog E6-1 and E6-5): one agent deployment's loop as a deterministic state machine, plus the
//! kill switches an owner, a risk limit, or a platform operator can pull.
//!
//! # Two entry points
//!
//! [`fold`] replays one journaled event and produces no effects. [`handle`] is the only producer of
//! effects (ADR-0001 ES-06). Together they are the whole of crash safety: a replay cannot re-send,
//! and recovery — which is [`Input::Started`], not a third entry point — cannot re-journal. State is
//! a fold of events and nothing else, so the runtime keeps no durable state outside the journal and
//! a restart re-derives every stream position by replaying (journal spec §8, DEC-131 items 2 and 17).
//!
//! # What crosses the boundary
//!
//! Inputs are journaled events in `seq` order, the scheduler's whole-second risk clock, market
//! observations, model outputs, approval responses, and commands. Outputs are [`Effect`]s: drafts for
//! the agent stream, handoffs to an [`IntentSink`], timer requests, and owner alerts. **No transport
//! payload is ever an input**: the journal is the channel and a `LISTEN`/`NOTIFY` or a message is
//! only a hint, so a lost notification costs latency rather than correctness and the messaging
//! decision (DEC-17) reaches only the shell (ADR-0001 ES-20, DEC-131 item 5).
//!
//! The runtime never talks to a broker (`AGENTS.md` rule 12). Its gate call is a dry run that can
//! only narrow a proposal away; the binding gate runs on the account stream, whose owner is the
//! executor. Its kill switch applies the final mode first, cancels every pending approval, and hands
//! exactly one agent-scoped [`FlattenPlan`] to the sink — a type with no account-wide variant, so
//! `cancel-all` and `close-position` are unrepresentable here rather than merely untested
//! (`AGENTS.md` rule 13, trading-domain spec §5.5).
//!
//! # Determinism
//!
//! No wall clock, no randomness, no floats, `BTreeMap` and `BTreeSet` only, exact `mandate-num`
//! arithmetic, and `IdGen` injected (ES-21). The only time the core knows is the risk-clock second
//! on its input; `event_time` and `recorded_at` are never read for timing. Durations are sums of
//! whole seconds over the intervals between inputs, each credited by the state at the interval's
//! start, so inserting ticks cannot change a journaled draft (mandate spec §5.2, MI-13).

mod error;
mod ports;
mod state;
mod step;
mod types;

pub use error::{JsonError, RuntimeError};
pub use ports::{
    FlattenPlanner, GateDryRun, IdGen, IntentSink, OrderPlan, Ports, SinkError, TimerSource,
};
pub use state::{FOLD_VERSION, PendingApproval, RuntimeState, UnresolvedAppend, fold};
pub use step::handle;
pub use types::{
    AgentId, ApprovalOutcome, ApprovalVerdict, Autonomy, Command, ConnectionId, Deployment,
    DryRunVerdict, Effect, EventDraft, EventId, FlattenLeg, FlattenPlan, FlattenRequest,
    FoldedEvent, Handoff, Initiator, Input, IntentBody, IntentHandoff, KillScope, LocalHold,
    MandateView, Mode, ModelOutput, NotificationRef, Observation, Outstanding, OwnerConfirmation,
    Proposal, Purpose, RiskClock, Seq, SignalInputs, TimerId, TimerRequest, WorkspaceId,
    WriterEpoch,
};
