//! The folded state and the replay that builds it.

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::InstrumentId;

use crate::error::RuntimeError;
use crate::types::{
    Deployment, EventId, FoldedEvent, LocalHold, Mode, ModelOutput, Outstanding, RiskClock, Seq,
    WriterEpoch,
};

/// The `fold_version` of ADR-0001 ES-21. Bumped whenever fold output changes, with the golden
/// journal regenerated in the same change.
pub const FOLD_VERSION: u32 = 1;

/// What the runtime knows, derived from journaled events and nothing else.
///
/// Every field is private and every collection is ordered (ES-21). The folded position of each
/// followed stream lives here and is re-derived by replay, so nothing durable exists outside the
/// journal and a restart cannot mistake an old event for a new one (DEC-131 item 17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeState {
    deployment: Deployment,
    heads: BTreeMap<String, Seq>,
    epoch: Option<WriterEpoch>,
    started: bool,
    unresolved: Option<UnresolvedAppend>,
    risk_clock: Option<RiskClock>,
    copied_mode: Mode,
    local_holds: BTreeSet<LocalHold>,
    pending_approvals: BTreeMap<EventId, PendingApproval>,
    outstanding: BTreeMap<EventId, Outstanding>,
    outputs: BTreeMap<String, BTreeMap<InstrumentId, ModelOutput>>,
    last_submission: Option<Seq>,
    reconciled_through: Option<Seq>,
}

/// A batch whose append has not been answered. The input and the drafts are kept so that the only
/// permitted next step is the same input, which re-emits the same drafts and lets the append answer
/// `AlreadyCommitted` rather than appending a second event (journal spec §5.1, DEC-131 item 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedAppend {
    pub head: Seq,
    pub input: crate::types::Input,
    pub drafts: Vec<crate::types::EventDraft>,
}

/// An approval the runtime is waiting on, with what it binds (mandate spec §6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingApproval {
    pub instrument: InstrumentId,
    pub mandate_version: String,
    pub deadline: RiskClock,
    pub adds_risk: bool,
}

impl RuntimeState {
    /// An empty state for one deployment, before any event is folded. Reads nothing: there is no
    /// constructor that touches a clock, a file, or a random number. The deployment's ids are what
    /// let the fold reject another workspace's stream and the kill switch tell its own scope from a
    /// sibling's, so they are given rather than inferred from the first event folded.
    pub fn new(deployment: Deployment) -> Self {
        Self {
            deployment,
            heads: BTreeMap::new(),
            epoch: None,
            started: false,
            unresolved: None,
            risk_clock: None,
            copied_mode: Mode::Normal,
            local_holds: BTreeSet::new(),
            pending_approvals: BTreeMap::new(),
            outstanding: BTreeMap::new(),
            outputs: BTreeMap::new(),
            last_submission: None,
            reconciled_through: None,
        }
    }

    /// Which deployment this runtime is.
    pub fn deployment(&self) -> &Deployment {
        &self.deployment
    }

    /// The effective mode: the strictest of the copied account-stream mode and every local hold
    /// (mandate spec §5.9, MI-6). A maximum over [`Mode`]'s ordering, so the runtime is never less
    /// strict than the account stream and never lifts what the account stream set.
    pub fn effective_mode(&self) -> Mode {
        let _ = &self.local_holds;
        Mode::Normal
    }

    /// Whether an intent of this purpose may be **proposed** in the effective mode.
    pub fn permits(&self, _purpose: crate::types::Purpose) -> bool {
        false
    }

    /// The intents a restart would hand again: those the account stream has not yet taken.
    pub fn pending_handoffs(&self) -> Vec<&Outstanding> {
        let _ = &self.outstanding;
        Vec::new()
    }

    /// Whether a restart may **re-hand** an intent whose `IntentProposed` already committed.
    ///
    /// The startup hold is excluded from this gate, which [`Self::permits`] applies: the hold means
    /// "the broker's truth is not yet confirmed", which is a reason not to *decide*, while dropping
    /// an exit already journaled would remove protection rather than add it, and trading-domain spec
    /// §5.5 lets only `paused`, `stopped`, an `Unknown` order, or the broker hold an exit. The copied
    /// account mode and the lifecycle state still govern, so `stopped` re-hands a flatten alone and
    /// `exits_only` re-hands no opening (DEC-131 item 22).
    pub fn permits_rehand(&self, _purpose: crate::types::Purpose) -> bool {
        false
    }

    /// The folded `seq` of one stream, or `None` for a stream with nothing folded yet.
    pub fn head(&self, _stream: &str) -> Option<Seq> {
        None
    }

    /// The latest risk-clock second the fold has seen.
    pub fn risk_clock(&self) -> Option<RiskClock> {
        self.risk_clock
    }

    /// The intents whose `IntentProposed` committed and which have no terminal outcome yet.
    pub fn outstanding(&self) -> &BTreeMap<EventId, Outstanding> {
        &self.outstanding
    }

    /// The approvals the runtime is waiting on.
    pub fn pending_approvals(&self) -> &BTreeMap<EventId, PendingApproval> {
        &self.pending_approvals
    }
}

/// Replays one journaled event into the state.
///
/// Total over the catalogue and **effect-free**: a replay can never re-send anything, which is half
/// of crash safety (the other half is that only [`crate::handle`] produces effects). An event type,
/// payload field, or stream the core does not interpret is [`RuntimeError::NotInterpreted`], never a
/// silent no-op (DEC-85).
pub fn fold(state: &mut RuntimeState, event: &FoldedEvent) -> Result<(), RuntimeError> {
    let _ = (state, event);
    Err(RuntimeError::Unimplemented { story: "E6-1" })
}
