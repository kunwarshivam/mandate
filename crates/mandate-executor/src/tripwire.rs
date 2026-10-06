//! The pure account-stream tripwire fold (mandate spec §6.7, MI-31; E6-13).
//!
//! This tests-PR module names the values shared by the executor and reference harness. [`fold`]
//! refuses every input until the implementation PR can preserve the complete latch and journal
//! contract; no caller can mistake an empty effect list for an evaluated tripwire.

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{InstrumentId, Side};
use mandate_approval::{AssertionId, Environment, RiskClock, StepUp};
use mandate_num::Usd;
use mandate_spec::SchemaDec;
use mandate_spec::document::{Tripwire, TripwireAction, TripwireId, TripwireMetric};
use mandate_time::Date;

use crate::ExecutorError;

/// Folded tripwire state, rebuilt from the account stream on every restart.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TripwireState {
    tripwires: Vec<Tripwire>,
    metrics: BTreeMap<TripwireId, TripwireValue>,
    fired: BTreeMap<TripwireId, TripwireAction>,
    instruments_ever_filled: BTreeSet<InstrumentId>,
    risk_day: Option<Date>,
    used_assertions: BTreeSet<AssertionId>,
}

/// A fill after accounting has derived its net realized P&L.
///
/// Cost-basis arithmetic remains in `mandate-accounting`; this boundary carries its answer. A buy's
/// answer is the negative of its fee. The fold derives first-ever status from its private fill
/// history, never from a caller claim or the current position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TripwireFill {
    pub instrument: InstrumentId,
    pub side: Side,
    pub net_realized_usd: Usd,
}

/// Step-up and independence evidence on an owner acknowledgment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TripwireAcknowledgment {
    pub tripwire: TripwireId,
    pub step_up: Option<StepUp>,
    pub committed_at: RiskClock,
    pub processed_at: RiskClock,
    pub environment: Environment,
    pub requester: Option<String>,
    pub acknowledging_user: Option<String>,
    pub independent_required_at_request: bool,
    pub independent_required_now: bool,
}

/// Every account-stream fact §6.7 says can affect, or must not affect, tripwire state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TripwireInput {
    MandateVersionApplied { tripwires: Vec<Tripwire> },
    FillApplied { fill: TripwireFill },
    LateFillApplied { fill: TripwireFill },
    RiskDayStarted { day: Date },
    OwnerAcknowledged(TripwireAcknowledgment),
    Mark,
    Clock,
    Restart,
}

/// Why an owner acknowledgment did not lift a fired tripwire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcknowledgmentRefusal {
    StepUpMissing,
    StepUpStale,
    StepUpReused,
    StepUpMethod,
    NotIndependent,
}

impl AcknowledgmentRefusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StepUpMissing => "step_up_missing",
            Self::StepUpStale => "step_up_stale",
            Self::StepUpReused => "step_up_reused",
            Self::StepUpMethod => "step_up_method",
            Self::NotIndependent => "not_independent",
        }
    }
}

/// A metric value whose variant fixes the metric's unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TripwireValue {
    Count(u32),
    Usd(Usd),
}

/// The fixed-content owner alert emitted immediately after a trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TripwireAlert {
    triggered_event_index: usize,
}

impl TripwireAlert {
    /// Index of the preceding trigger in this transition's journal effects.
    pub fn triggered_event_index(&self) -> usize {
        self.triggered_event_index
    }

    /// Notification text is fixed and carries no mandate or account data.
    pub fn generic_text(&self) -> &'static str {
        "tripwire_fired"
    }
}

/// Journal effects emitted by one tripwire input, in append order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TripwireEvent {
    RiskLimitTriggered {
        id: TripwireId,
        action: TripwireAction,
        metric: TripwireMetric,
        threshold: SchemaDec,
        value: TripwireValue,
    },
    OwnerAlertSent(TripwireAlert),
    RiskLimitLifted {
        id: TripwireId,
    },
    OwnerCommandRefused {
        reason: AcknowledgmentRefusal,
    },
}

/// The externally relevant projection after one input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TripwireSnapshot {
    pub fired: BTreeMap<TripwireId, TripwireAction>,
    pub metrics: BTreeMap<TripwireId, TripwireValue>,
    pub effective_action: Option<TripwireAction>,
    pub delegations_suspended: bool,
    pub allocation_increase_blocked: bool,
}

/// One pure transition and the journal records that must be appended before its behavior is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TripwireOutcome {
    pub state: TripwireState,
    pub snapshot: TripwireSnapshot,
    pub journal: Vec<TripwireEvent>,
}

/// Applies one account-stream fact.
///
/// E6-13's tests PR is deliberately fail-closed. The implementation PR replaces this body; no
/// production path may catch this error and continue as though no tripwire fired.
pub fn fold(
    _state: &TripwireState,
    _input: &TripwireInput,
) -> Result<TripwireOutcome, ExecutorError> {
    Err(ExecutorError::Unimplemented { story: "E6-13" })
}

#[cfg(test)]
mod tests {
    use super::TripwireAlert;

    /// The notification boundary exposes only the trigger link and fixed generic text.
    #[test]
    fn an_alert_exposes_only_its_trigger_index_and_generic_text() {
        let alert = TripwireAlert {
            triggered_event_index: 7,
        };
        assert_eq!(alert.triggered_event_index(), 7);
        assert_eq!(alert.generic_text(), "tripwire_fired");
    }
}
