//! The replay: one journaled event into the state, effect-free (journal spec §8).

use mandate_canon::Value;

use crate::error::ExecutorError;
use crate::payload::required_text;
use crate::state::ExecutorState;
use crate::types::FoldedEvent;

/// Replays one journaled event into the state.
///
/// Total over the account-stream catalogue and **effect-free**: a replay can never re-send
/// anything, which is half of crash safety (the other half is that only [`crate::handle`]
/// produces effects). An event type, payload field, or stream this crate does not interpret is
/// [`ExecutorError::NotInterpreted`] naming the owning story, never a silent no-op (DEC-85).
pub fn fold(state: &mut ExecutorState, event: &FoldedEvent) -> Result<(), ExecutorError> {
    let account = event.stream == state.account_stream();
    if !account && !follows(state, &event.stream) {
        return Err(ExecutorError::ForeignStream {
            stream: event.stream.clone(),
        });
    }
    let expected = state
        .head(&event.stream)
        .map_or(1, |head| head.0.saturating_add(1));
    if event.seq.0 != expected {
        return Err(ExecutorError::SequenceOutOfOrder {
            stream: event.stream.clone(),
            expected,
            found: event.seq.0,
        });
    }
    if account {
        account_event(state, event)?;
    }
    state.heads.insert(event.stream.clone(), event.seq);
    Ok(())
}

/// The streams an account's executor reads without writing: its workspace's agent streams, the
/// control stream, and the clock stream (journal spec §2).
fn follows(state: &ExecutorState, stream: &str) -> bool {
    let workspace = &state.scope.workspace.0;
    stream.starts_with(&format!("agent:{workspace}:"))
        || stream == format!("ctl:{workspace}")
        || stream == format!("clock:{workspace}")
}

/// The account-stream catalogue events this crate interprets in a later slice, each with the story
/// that interprets it. Until then an event here answers that story's stub (DEC-137); an event
/// another stream owns, or a name nobody wrote, is not interpreted at all.
const LATER: [(&str, &str); 31] = [
    ("IntentReceived", "E7-2"),
    ("GateDecided", "E7-2"),
    ("OrderSubmitted", "E7-2"),
    ("OrderStateChanged", "E7-2"),
    ("OrderAbandoned", "E7-2"),
    ("AgentModeApplied", "E7-2"),
    ("TradingDayStarted", "E7-2"),
    ("ClockAdvanced", "E7-2"),
    ("RiskDayStarted", "E7-2"),
    ("MarkUpdated", "E7-2"),
    ("FillApplied", "E7-3"),
    ("LateFillApplied", "E7-3"),
    ("FeesCharged", "E7-3"),
    ("SettlementPosted", "E7-3"),
    ("DividendPaid", "E7-3"),
    ("CashInLieuPosted", "E7-3"),
    ("CorporateActionPrepared", "E7-3"),
    ("CorporateActionApplied", "E7-3"),
    ("ExternalActivityIngested", "E7-3"),
    ("AccountStateObserved", "E7-3"),
    ("AccountSnapshotRecorded", "E7-3"),
    ("AccountRestrictionChanged", "E7-3"),
    ("RejectObserved", "E7-3"),
    ("BrokerExchangeRecorded", "E7-3"),
    ("ConductBreachDetected", "E7-3"),
    ("CompensatingEvent", "E7-3"),
    ("ReconciliationRun", "E7-3"),
    ("BrokerPositionObserved", "E7-3"),
    ("OwnerAcknowledged", "E7-3"),
    ("ProtectionChanged", "E7-4"),
    ("KillSwitchActivated", "E7-4"),
];

fn account_event(state: &mut ExecutorState, event: &FoldedEvent) -> Result<(), ExecutorError> {
    let kind = event.event_type.as_str();
    if kind == "StreamOpened" {
        return stream_opened(state, &event.payload);
    }
    if let Some((_, story)) = LATER.iter().find(|(name, _)| *name == kind) {
        return Err(ExecutorError::Unimplemented { story });
    }
    Err(ExecutorError::NotInterpreted {
        what: kind.to_owned(),
        story: elsewhere(kind),
    })
}

/// The story that interprets an event this crate does not. Answered before anything else is read,
/// so an uninterpreted event fails as uninterpreted rather than on a field it was never going to
/// be read for (DEC-85).
fn elsewhere(kind: &str) -> &'static str {
    match kind {
        "RelatedAccountsCoordination" => "E7-5",
        "MandateVersionApplied"
        | "RiskLimitTriggered"
        | "RiskLimitLifted"
        | "HighWaterMarkReset"
        | "PositionReleased"
        | "InstrumentRestrictionChanged"
        | "GoalCompleted" => "E6-4",
        "UniverseChanged" => "E17-3",
        _ => "E7-2",
    }
}

/// `StreamOpened` fixes the stream's `environment` for good (ADR-0001 ES-23).
fn stream_opened(state: &mut ExecutorState, payload: &Value) -> Result<(), ExecutorError> {
    let found = required_text(payload, "environment")?;
    if let Some(opened) = &state.environment {
        return Err(ExecutorError::EnvironmentMismatch {
            opened: opened.clone(),
            found: found.to_owned(),
        });
    }
    state.environment = Some(found.to_owned());
    Ok(())
}
