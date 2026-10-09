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
//! The account stream's single writer
//! ([task brief](../../../docs/project/tasks/M6-K-executor-and-connector.md), backlog E7-2, E7-3,
//! and E7-4): the idempotent executor, the reconciliation that proves it worked, and the
//! protective-order sequences every one of those orders is wrapped in.
//!
//! The three stories are one state machine over one stream, which is why they are one crate: the
//! idempotency key, the reconciliation, and the protective sequences all read and write the same
//! order set, and splitting them would put the account stream's single writer in three crates.
//!
//! # Two entry points
//!
//! [`fold`] replays one journaled event and produces no effects. [`handle`] is the only producer
//! of effects (ADR-0001 ES-06). Together they are the whole of crash safety: a replay cannot
//! re-send, and recovery — which is [`Input::Started`], not a third entry point — cannot
//! re-journal. State is a fold of events and nothing else (journal spec §8).
//!
//! # The idempotency chain (E7-2)
//!
//! 1. The intent's identity comes from the agent stream: `intent_id` **is** the `event_id` of
//!    `IntentProposed` (journal spec §2), so nothing here mints it.
//! 2. `IntentReceived` is journaled first, and an intent the fold already carries produces an
//!    empty effect list — the whole of the deduplication is a fold lookup, not a set in memory.
//! 3. The binding gate runs against fresh folded state and `GateDecided` is journaled. A deny
//!    ends the intent; nothing is sent.
//! 4. The client order id is **derived**: [`ClientOrderId::for_intent`] is a pure function of the
//!    intent id alone, not of a counter and not of the attempt number, because §5.7 requires a
//!    resubmission after a confirmed absence to carry the **same** id.
//! 5. `OrderSubmitted` is journaled **before** the request leaves, as the effect immediately
//!    before the [`Effect::Broker`] that names it, and the shell submits only on `Committed` or
//!    `AlreadyCommitted` (journal spec §5.2, `AGENTS.md` rule 5, DEC-07).
//! 6. The submission carries our id, so the broker rejects a duplicate itself; that rejection is
//!    folded as "already submitted", not as a failure.
//! 7. A crash anywhere between the journal and the response is resolved by a **query**, never by
//!    a blind resubmit, and an absence must be confirmed over a window before the order returns
//!    to `Intent`.
//!
//! # What this crate is not
//!
//! It declares no `IntentSink` and names no `mandate-runtime` type. The two crates are both at
//! layer 6, so neither can implement the other's trait; an intent enters only as
//! [`Input::Intent`], and the adapter that implements stream I's sink on this executor's behalf
//! lives in the shell, at layer 7, the one place that may depend on both (DEC-131, ES-02).
//!
//! # The binding gate
//!
//! [`handle`] requires a [`BindingGateSource`] that supplies the trusted mandate, configuration,
//! account, instrument, market, and universe snapshots outside this crate's fold. The source
//! cannot return a verdict: the executor derives `mandate-risk`'s proposal and calls
//! `mandate_risk::evaluate` directly. Missing or invalid snapshots fail closed for a risk-adding
//! order. A risk reduction retains the executor's local account-stream decision when an external
//! snapshot is unavailable, preserving `AGENTS.md` rule 13. A completed binding run records its
//! full evidence; a local denial records only its reached checks under the `account_stream_only`
//! data profile.
//!
//! # Determinism
//!
//! No wall clock, no randomness, no floats, `BTreeMap` and `BTreeSet` only, exact `mandate-num`
//! arithmetic, and `IdGen` injected (ES-21, DEC-89). The only time the core knows is the
//! risk-clock second on its input.
//!
//! # Safety
//!
//! `AGENTS.md` rule 13 is a type rule here rather than a review rule: [`BrokerRequest::CancelAll`]
//! and [`BrokerRequest::ClosePosition`] take an [`AccountWideScope`] that only the account and
//! workspace kill-switch paths can construct, so no agent-scoped code path can name the
//! account-wide endpoints. There is no deposit, withdrawal, or transfer request, no live base
//! URL anywhere in this stream, and no path that could build one (`AGENTS.md` rule 8, ES-23).

mod batch;
mod codec;
mod error;
mod fees;
mod fold;
mod gate;
mod ids;
mod intent;
mod kill;
mod listing;
mod margin;
mod opening;
mod orders;
mod payload;
mod ports;
mod protection;
mod reconcile;
mod session;
mod shape;
mod state;
mod step;
pub mod tripwire;
mod types;

pub use error::{ExecutorError, JsonError};
pub use fees::{FeeSchedule, fee_config, paper_only_fee_config};
pub use gate::PartialGateDecision;
pub use ids::{ClientOrderId, IntentId, PREFIX};
pub use opening::equity_bracket_prices;
pub use ports::{
    BindingGateConfigRefs, BindingGateInput, BindingGateRequest, BindingGateSource,
    BrokerConnector, ConnectorError, IdGen, InstrumentSnapshot, MandateView, Ports,
};
pub use protection::{LadderPrice, LadderReference, is_protected};
pub use reconcile::reconcile;
pub use shape::{ProtectiveShape, protective_shape};
pub use state::{
    ExecutorState, FOLD_VERSION, IntentOutcome, IntentRecord, ObservedAccount, UnresolvedAppend,
    fold,
};
pub use step::handle;
pub use types::{
    AccountRef, AccountScope, AccountState, AccountWide, AccountWideScope, ActivityCursor, AgentId,
    BracketLegs, BrokerAccount, BrokerFill, BrokerOrder, BrokerOutcome, BrokerPosition,
    BrokerReject, BrokerRequest, BrokerSnapshot, BrokerUnknown, BrokerUpdate, Command, Difference,
    DifferenceKind, Effect, EventDraft, EventId, ExecutorConfig, ExitTier, FillId, FlattenLeg,
    FlattenPlan, FoldedEvent, GateCheck, GateVerdict, Initiator, Input, IntentBody, IntentHandoff,
    KillScope, ListedOrder, MandateVersion, MarketObservation, Mode, NotificationRef, OcoLegs,
    Order, OrderListing, OrderOrigin, OrderState, OrderType, OwnerConfirmation, Protection,
    ProtectionPrices, Purpose, ReconcileReason, Reconciliation, ReconciliationVerdict, RiskClock,
    Seq, StatusMapping, SubmitOrder, TimeInForce, TimerId, TimerRequest, UnprotectedInterval,
    WorkspaceId, WriterEpoch,
};
