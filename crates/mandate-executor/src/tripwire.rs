//! The pure account-stream tripwire fold (mandate spec §6.7, MI-31; E6-13).
//!
//! The state is rebuilt by folding journaled account-stream facts in sequence. Effects are returned
//! in the order in which callers must append them before applying the projected behavior.

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{InstrumentId, Side};
use mandate_approval::{
    AssertionId, CommandAuthority, Environment, OwnerCommandKind, RiskClock, StepUp, StepUpRefusal,
    owner_command,
};
use mandate_num::{NumError, Usd};
use mandate_spec::SchemaDec;
use mandate_spec::document::{Tripwire, TripwireAction, TripwireId, TripwireMetric};
use mandate_time::Date;

use crate::ExecutorError;

/// Folded tripwire state, rebuilt from the account stream on every restart.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TripwireState {
    tripwires: Vec<Tripwire>,
    metrics: BTreeMap<TripwireId, TripwireValue>,
    accumulators: BTreeMap<TripwireId, TripwireAccumulator>,
    fired: BTreeMap<TripwireId, TripwireAction>,
    instruments_ever_filled: BTreeSet<InstrumentId>,
    risk_day: Option<Date>,
    used_assertions: BTreeSet<AssertionId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TripwireAccumulator {
    ConsecutiveLosingExits(u32),
    RealizedNet(Usd),
    NewInstruments(u32),
}

impl TripwireAccumulator {
    fn zero(metric: TripwireMetric) -> Self {
        match metric {
            TripwireMetric::ConsecutiveLosingExits => Self::ConsecutiveLosingExits(0),
            TripwireMetric::RealizedLossUsd => Self::RealizedNet(Usd::ZERO),
            TripwireMetric::NewInstruments => Self::NewInstruments(0),
        }
    }

    fn value(&self) -> TripwireValue {
        match self {
            Self::ConsecutiveLosingExits(value) | Self::NewInstruments(value) => {
                TripwireValue::Count(*value)
            }
            Self::RealizedNet(value) => TripwireValue::Usd(if value.is_negative() {
                value.negated()
            } else {
                Usd::ZERO
            }),
        }
    }
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
pub fn fold(
    state: &TripwireState,
    input: &TripwireInput,
) -> Result<TripwireOutcome, ExecutorError> {
    let mut next = state.clone();
    let mut journal = Vec::new();

    match input {
        TripwireInput::MandateVersionApplied { tripwires } => {
            apply_version(&mut next, tripwires);
        }
        TripwireInput::FillApplied { fill } | TripwireInput::LateFillApplied { fill } => {
            apply_fill(&mut next, fill)?;
        }
        TripwireInput::RiskDayStarted { day } => {
            next.risk_day = Some(*day);
            reset_realized_loss(&mut next);
        }
        TripwireInput::OwnerAcknowledged(acknowledgment) => {
            apply_acknowledgment(&mut next, acknowledgment, &mut journal)?;
        }
        TripwireInput::Mark | TripwireInput::Clock | TripwireInput::Restart => {}
    }

    refresh_metrics(&mut next);
    fire_reached_tripwires(&mut next, &mut journal)?;
    let snapshot = snapshot(&next);
    Ok(TripwireOutcome {
        state: next,
        snapshot,
        journal,
    })
}

fn apply_version(state: &mut TripwireState, tripwires: &[Tripwire]) {
    let previous_metrics: BTreeMap<TripwireId, TripwireMetric> = state
        .tripwires
        .iter()
        .map(|tripwire| (tripwire.id.clone(), tripwire.metric))
        .collect();
    let mut ordered = tripwires.to_vec();
    ordered.sort_by(|left, right| left.id.cmp(&right.id));
    let current_ids: BTreeSet<TripwireId> =
        ordered.iter().map(|tripwire| tripwire.id.clone()).collect();

    state.accumulators.retain(|id, _| current_ids.contains(id));
    for tripwire in &ordered {
        if previous_metrics.get(&tripwire.id) != Some(&tripwire.metric) {
            state.accumulators.insert(
                tripwire.id.clone(),
                TripwireAccumulator::zero(tripwire.metric),
            );
        }
    }
    state.tripwires = ordered;
}

fn apply_fill(state: &mut TripwireState, fill: &TripwireFill) -> Result<(), ExecutorError> {
    let first_fill = state
        .instruments_ever_filled
        .insert(fill.instrument.clone());
    for accumulator in state.accumulators.values_mut() {
        match accumulator {
            TripwireAccumulator::ConsecutiveLosingExits(streak) if fill.side == Side::Sell => {
                *streak = if fill.net_realized_usd.is_negative() {
                    streak.checked_add(1).ok_or(NumError::Overflow)?
                } else {
                    0
                };
            }
            TripwireAccumulator::RealizedNet(net) => {
                *net = net.checked_add(fill.net_realized_usd)?;
            }
            TripwireAccumulator::NewInstruments(count) if first_fill => {
                *count = count.checked_add(1).ok_or(NumError::Overflow)?;
            }
            TripwireAccumulator::ConsecutiveLosingExits(_)
            | TripwireAccumulator::NewInstruments(_) => {}
        }
    }
    Ok(())
}

fn reset_realized_loss(state: &mut TripwireState) {
    for accumulator in state.accumulators.values_mut() {
        if let TripwireAccumulator::RealizedNet(net) = accumulator {
            *net = Usd::ZERO;
        }
    }
}

fn apply_acknowledgment(
    state: &mut TripwireState,
    acknowledgment: &TripwireAcknowledgment,
    journal: &mut Vec<TripwireEvent>,
) -> Result<(), ExecutorError> {
    let authority = owner_command(
        OwnerCommandKind::Acknowledge,
        acknowledgment.step_up.as_ref(),
        acknowledgment.committed_at,
        acknowledgment.processed_at,
        acknowledgment.environment,
        &state.used_assertions,
    )
    .map_err(|_| ExecutorError::NotInterpreted {
        what: "tripwire acknowledgment authority".to_owned(),
        story: "E6-13",
    })?;

    if let Some(step_up) = &acknowledgment.step_up {
        state.used_assertions.insert(step_up.assertion.clone());
    }

    if let CommandAuthority::Refused(reason) = authority {
        journal.push(TripwireEvent::OwnerCommandRefused {
            reason: acknowledgment_refusal(reason),
        });
        return Ok(());
    }

    if independence_required(acknowledgment)
        && !independent_users(
            acknowledgment.requester.as_deref(),
            acknowledgment.acknowledging_user.as_deref(),
        )
    {
        journal.push(TripwireEvent::OwnerCommandRefused {
            reason: AcknowledgmentRefusal::NotIndependent,
        });
        return Ok(());
    }

    if state.fired.remove(&acknowledgment.tripwire).is_some() {
        if let Some(tripwire) = state
            .tripwires
            .iter()
            .find(|tripwire| tripwire.id == acknowledgment.tripwire)
        {
            state.accumulators.insert(
                tripwire.id.clone(),
                TripwireAccumulator::zero(tripwire.metric),
            );
        }
        journal.push(TripwireEvent::RiskLimitLifted {
            id: acknowledgment.tripwire.clone(),
        });
    }
    Ok(())
}

fn acknowledgment_refusal(reason: StepUpRefusal) -> AcknowledgmentRefusal {
    match reason {
        StepUpRefusal::Missing => AcknowledgmentRefusal::StepUpMissing,
        StepUpRefusal::Stale => AcknowledgmentRefusal::StepUpStale,
        StepUpRefusal::Reused => AcknowledgmentRefusal::StepUpReused,
        StepUpRefusal::Method => AcknowledgmentRefusal::StepUpMethod,
    }
}

fn independence_required(acknowledgment: &TripwireAcknowledgment) -> bool {
    acknowledgment.independent_required_at_request || acknowledgment.independent_required_now
}

fn independent_users(requester: Option<&str>, acknowledging_user: Option<&str>) -> bool {
    matches!(
        (requester, acknowledging_user),
        (Some(requester), Some(acknowledging_user)) if requester != acknowledging_user
    )
}

fn refresh_metrics(state: &mut TripwireState) {
    state.metrics = state
        .accumulators
        .iter()
        .map(|(id, accumulator)| (id.clone(), accumulator.value()))
        .collect();
}

fn fire_reached_tripwires(
    state: &mut TripwireState,
    journal: &mut Vec<TripwireEvent>,
) -> Result<(), ExecutorError> {
    for tripwire in &state.tripwires {
        if state.fired.contains_key(&tripwire.id) {
            continue;
        }
        let Some(value) = state.metrics.get(&tripwire.id).copied() else {
            continue;
        };
        if !reached(value, &tripwire.threshold)? {
            continue;
        }
        state.fired.insert(tripwire.id.clone(), tripwire.action);
        let triggered_event_index = journal.len();
        journal.push(TripwireEvent::RiskLimitTriggered {
            id: tripwire.id.clone(),
            action: tripwire.action,
            metric: tripwire.metric,
            threshold: tripwire.threshold.clone(),
            value,
        });
        journal.push(TripwireEvent::OwnerAlertSent(TripwireAlert {
            triggered_event_index,
        }));
    }
    Ok(())
}

fn reached(value: TripwireValue, threshold: &SchemaDec) -> Result<bool, ExecutorError> {
    match value {
        TripwireValue::Count(value) => {
            let threshold = threshold.as_str().parse::<u32>().map_err(|_| {
                ExecutorError::NonCanonicalPayload {
                    field: "tripwire.threshold".to_owned(),
                }
            })?;
            Ok(value >= threshold)
        }
        TripwireValue::Usd(value) => Ok(value >= Usd::parse(threshold.as_str())?),
    }
}

fn snapshot(state: &TripwireState) -> TripwireSnapshot {
    let fired: BTreeMap<TripwireId, TripwireAction> = state
        .fired
        .iter()
        .map(|(id, fired_action)| {
            let current_action = state
                .tripwires
                .iter()
                .find(|tripwire| tripwire.id == *id)
                .map(|tripwire| tripwire.action);
            (
                id.clone(),
                current_action.map_or(*fired_action, |action| action.max(*fired_action)),
            )
        })
        .collect();
    let effective_action = fired.values().copied().max();
    let any_fired = !fired.is_empty();
    TripwireSnapshot {
        fired,
        metrics: state.metrics.clone(),
        effective_action,
        delegations_suspended: any_fired,
        allocation_increase_blocked: any_fired,
    }
}

#[cfg(test)]
mod tests {
    use mandate_approval::StepUpMethod;
    use mandate_spec::DecGrammar;

    use super::{
        AssertionId, Environment, InstrumentId, RiskClock, SchemaDec, Side, StepUp, Tripwire,
        TripwireAcknowledgment, TripwireAction, TripwireAlert, TripwireEvent, TripwireFill,
        TripwireId, TripwireInput, TripwireMetric, TripwireState, Usd, fold,
    };

    /// The notification boundary exposes only the trigger link and fixed generic text.
    #[test]
    fn an_alert_exposes_only_its_trigger_index_and_generic_text() {
        let alert = TripwireAlert {
            triggered_event_index: 7,
        };
        assert_eq!(alert.triggered_event_index(), 7);
        assert_eq!(alert.generic_text(), "tripwire_fired");
    }

    /// A later version's stronger action is held only while that action remains current.
    #[test]
    fn a_version_does_not_rewrite_the_action_that_fired() -> Result<(), String> {
        let id = TripwireId::parse("wire").map_err(|error| error.to_string())?;
        let wire = |action| -> Result<Tripwire, String> {
            Ok(Tripwire {
                id: id.clone(),
                metric: TripwireMetric::ConsecutiveLosingExits,
                threshold: SchemaDec::parse("1", DecGrammar::PositiveDecimal)
                    .map_err(|error| error.to_string())?,
                action,
            })
        };
        let armed = fold(
            &TripwireState::default(),
            &TripwireInput::MandateVersionApplied {
                tripwires: vec![wire(TripwireAction::EndDelegations)?],
            },
        )
        .map_err(|error| error.to_string())?;
        let fired = fold(
            &armed.state,
            &TripwireInput::FillApplied {
                fill: TripwireFill {
                    instrument: InstrumentId::new("AAPL").map_err(|error| error.to_string())?,
                    side: Side::Sell,
                    net_realized_usd: Usd::parse("-1").map_err(|error| error.to_string())?,
                },
            },
        )
        .map_err(|error| error.to_string())?;
        let strengthened = fold(
            &fired.state,
            &TripwireInput::MandateVersionApplied {
                tripwires: vec![wire(TripwireAction::ExitsOnly)?],
            },
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(
            strengthened.snapshot.effective_action,
            Some(TripwireAction::ExitsOnly)
        );
        let restored = fold(
            &strengthened.state,
            &TripwireInput::MandateVersionApplied {
                tripwires: vec![wire(TripwireAction::EndDelegations)?],
            },
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(
            restored.snapshot.effective_action,
            Some(TripwireAction::EndDelegations)
        );
        Ok(())
    }

    /// A workspace that does not require independence needs no requester or second user to lift.
    #[test]
    fn acknowledgment_without_an_independence_requirement_needs_no_user_pair() -> Result<(), String>
    {
        let id = TripwireId::parse("wire").map_err(|error| error.to_string())?;
        let tripwire = Tripwire {
            id: id.clone(),
            metric: TripwireMetric::ConsecutiveLosingExits,
            threshold: SchemaDec::parse("1", DecGrammar::PositiveDecimal)
                .map_err(|error| error.to_string())?,
            action: TripwireAction::ExitsOnly,
        };
        let armed = fold(
            &TripwireState::default(),
            &TripwireInput::MandateVersionApplied {
                tripwires: vec![tripwire],
            },
        )
        .map_err(|error| error.to_string())?;
        let fired = fold(
            &armed.state,
            &TripwireInput::FillApplied {
                fill: TripwireFill {
                    instrument: InstrumentId::new("AAPL").map_err(|error| error.to_string())?,
                    side: Side::Sell,
                    net_realized_usd: Usd::parse("-1").map_err(|error| error.to_string())?,
                },
            },
        )
        .map_err(|error| error.to_string())?;
        let lifted = fold(
            &fired.state,
            &TripwireInput::OwnerAcknowledged(TripwireAcknowledgment {
                tripwire: id.clone(),
                step_up: Some(StepUp {
                    assertion: AssertionId("assertion-no-independence".to_owned()),
                    authenticated_at: RiskClock(100),
                    method: StepUpMethod::CliConfirm,
                }),
                committed_at: RiskClock(100),
                processed_at: RiskClock(100),
                environment: Environment::Paper,
                requester: None,
                acknowledging_user: None,
                independent_required_at_request: false,
                independent_required_now: false,
            }),
        )
        .map_err(|error| error.to_string())?;

        assert!(lifted.snapshot.fired.is_empty());
        assert_eq!(lifted.journal, vec![TripwireEvent::RiskLimitLifted { id }]);
        Ok(())
    }
}
