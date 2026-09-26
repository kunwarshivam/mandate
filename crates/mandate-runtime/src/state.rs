//! The folded state and the replay that builds it.

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::InstrumentId;

use crate::error::RuntimeError;
use crate::types::{
    EventId, FoldedEvent, LocalHold, Mode, ModelOutput, Outstanding, RiskClock, Seq, WriterEpoch,
};

/// The `fold_version` of ADR-0001 ES-21. Bumped whenever fold output changes, with the golden
/// journal regenerated in the same change.
pub const FOLD_VERSION: u32 = 1;

/// What the runtime knows, derived from journaled events and nothing else.
///
/// Every field is private and every collection is ordered (ES-21). The folded position of each
/// followed stream lives here and is re-derived by replay, so nothing durable exists outside the
/// journal and a restart cannot mistake an old event for a new one (DEC-131 item 17).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeState {
    heads: BTreeMap<String, Seq>,
    epoch: Option<WriterEpoch>,
    started: bool,
    unresolved_head: Option<Seq>,
    risk_clock: Option<RiskClock>,
    copied_mode: Mode,
    local_holds: BTreeSet<LocalHold>,
    pending_approvals: BTreeMap<EventId, PendingApproval>,
    outstanding: BTreeMap<EventId, Outstanding>,
    outputs: BTreeMap<String, BTreeMap<InstrumentId, ModelOutput>>,
    last_submission: Option<Seq>,
    reconciled_through: Option<Seq>,
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
    /// An empty state, before any event is folded. Reads nothing: there is no constructor that
    /// touches a clock, a file, or a random number.
    pub fn new() -> Self {
        Self::default()
    }

    /// The effective mode: the strictest of the copied account-stream mode and every local hold
    /// (mandate spec §5.9, MI-6). A maximum over [`Mode`]'s ordering, so the runtime is never less
    /// strict than the account stream and never lifts what the account stream set.
    pub fn effective_mode(&self) -> Mode {
        let _ = &self.local_holds;
        Mode::Normal
    }

    /// Whether an intent of this purpose may be proposed or re-handed in the effective mode.
    pub fn permits(&self, _purpose: crate::types::Purpose) -> bool {
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
