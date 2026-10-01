//! E17-5's input-drift detector (`unusual_input`, V-018, DEC-101, DEC-266): a per-source fold
//! over the typed observations the shell records as the research agent's inputs arrive. Content
//! never reaches the detector as text — an observation carries a source, a class, a timestamp, a
//! content hash, and a byte length, nothing else — so the R-05 injection surface (a payload
//! written to be read) is unrepresentable in the input type: the detector measures the *shape* of
//! a source's traffic, which is what deterministic code can see without a model.
//!
//! Every measure is integer arithmetic over nanosecond timestamps, byte counts, and observation
//! counts (ES-21): no clock, no randomness, no floats. Each source keeps a baseline of its first
//! [`BASELINE_WINDOW`] observations and a recent window of its last [`RECENT_WINDOW`] (the two may
//! overlap; requiring them disjoint would read "not yet measurable" as "not unusual", the
//! fail-open direction AGENTS.md rule 3 bars). The measures, in evaluation order, whose first
//! crossing is the reported one:
//!
//! 1. [`DriftMeasure::NoBaseline`] — fewer than [`BASELINE_WINDOW`] observations ever seen:
//!    unusual, fail-safe, because an unproven source's inputs escalate until its baseline fills;
//! 2. [`DriftMeasure::DuplicateContent`] — one content hash at least [`DUPLICATE_MINIMUM`] of the
//!    recent window: a replayed or spammed payload;
//! 3. [`DriftMeasure::ArrivalRate`] — the recent window's span under [`ARRIVAL_FACTOR`] of the
//!    baseline's: a flood;
//! 4. [`DriftMeasure::GapCollapse`] — the recent window's smallest inter-arrival gap under
//!    [`GAP_FACTOR`] of the baseline's mean gap;
//! 5. [`DriftMeasure::LengthShift`] — the recent window's mean byte length beyond
//!    [`LENGTH_FACTOR`] times the baseline's, or under its inverse.
//!
//! The thresholds are DEC-266's, pinned by a live test in the implementation PR; moving them
//! into envelope fields is Proposed there, because an owner cannot be talked out of a threshold
//! they never confirmed. The escalation seam — how an unusual verdict reaches the owner before
//! the agent acts — is Proposed to the founder in DEC-266 item 4 and is **not wired here**:
//! [`crate::admit`] is untouched, and the §6.3 fact becomes rule-usable only when stream F lifts
//! V-018.

use std::collections::{BTreeMap, BTreeSet};

use mandate_time::UtcNanos;

use crate::{ContentHash, ResearchError, SourceId};

/// The baseline a source must fill before its drift is measured: its first 32 observations
/// (DEC-266 item 1).
pub const BASELINE_WINDOW: usize = 32;

/// The recent window every measure reads: a source's last 8 observations (DEC-266 item 1).
pub const RECENT_WINDOW: usize = 8;

/// The duplicate measure's bar: one content hash at least 4 of the recent 8 observations
/// (DEC-266 item 4.2).
pub const DUPLICATE_MINIMUM: usize = 4;

/// The arrival measure's factor: the recent window's span under a twelfth of the baseline's
/// (DEC-266 item 4.3).
pub const ARRIVAL_FACTOR: u64 = 12;

/// The gap measure's factor: the recent window's smallest inter-arrival gap under a quarter of
/// the baseline's mean gap, itself the baseline span over its 31 gaps (DEC-266 item 4.4).
pub const GAP_FACTOR: u64 = 4;

/// The length measure's factor: the recent window's mean byte length beyond four times the
/// baseline's, or under a quarter of it (DEC-266 item 4.5).
pub const LENGTH_FACTOR: u64 = 4;

/// §8.4's input classes, kept apart because the shell records what kind of input a source
/// delivered; a source's drift is measured within its own history whatever the class mix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InputClass {
    /// News from an allowlisted source.
    News,
    /// A filing from an allowlisted source.
    Filing,
    /// A screen over the eligible universe.
    Screen,
    /// Market data.
    MarketData,
    /// The agent's own memory: positions, past theses, their outcomes.
    Memory,
}

/// One observed input, as the shell recorded it: never text, so a payload written to be read has
/// no field to land in (DEC-266 item 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputObservation {
    /// The source the input arrived from; every allowlisted source has one id.
    pub source: SourceId,
    /// What kind of input it was.
    pub class: InputClass,
    /// When the shell observed it.
    pub observed_at: UtcNanos,
    /// The content's hash, the only form the content takes here.
    pub content: ContentHash,
    /// The content's length in bytes.
    pub length_bytes: u64,
}

/// The measure a source's recent window crossed, in evaluation order: the first crossing is the
/// one a [`SourceDrift`] reports (the house style of mandate spec §8.5, where the first failure
/// decides).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DriftMeasure {
    /// The source has not filled its baseline: unusual, fail-safe.
    NoBaseline,
    /// One content hash repeats through the recent window.
    DuplicateContent,
    /// The recent window arrived in a fraction of the baseline's time.
    ArrivalRate,
    /// The recent window's smallest gap collapsed against the baseline's mean.
    GapCollapse,
    /// The recent window's mean length shifted far from the baseline's.
    LengthShift,
}

/// One source's drift verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDrift {
    /// The source the verdict is about.
    pub source: SourceId,
    /// Whether the source's inputs are unusual: the §6.3 fact's backing set is every source
    /// where this is true.
    pub unusual: bool,
    /// The first measure the recent window crossed, in evaluation order; `None` when the source
    /// is quiet.
    pub measure: Option<DriftMeasure>,
}

/// The detector's fold state: one ordered history per source. Arrival order is the timing
/// measures' signal, so the history is never sorted (DEC-266 interpretation 5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DriftState {
    observed: BTreeMap<SourceId, Vec<InputObservation>>,
}

impl DriftState {
    /// An empty detector, before any input has arrived.
    #[must_use]
    pub fn new() -> Self {
        Self {
            observed: BTreeMap::new(),
        }
    }

    /// Folds one observation into the source's history, in arrival order.
    ///
    /// # Errors
    /// Returns [`ResearchError::Unimplemented`] until E17-5's implementation lands: the pending
    /// tests in `tests/drift.rs` are the contract this stub waits behind.
    pub fn observe(&mut self, observation: &InputObservation) -> Result<(), ResearchError> {
        let _ = observation;
        Err(ResearchError::Unimplemented("drift::observe", "E17-5"))
    }

    /// The drift verdict for every source ever observed, each from its own history alone: a
    /// compromised source never escalates another's theses (DEC-266 item 2).
    ///
    /// # Errors
    /// Returns [`ResearchError::Unimplemented`] until E17-5's implementation lands: the pending
    /// tests in `tests/drift.rs` are the contract this stub waits behind.
    pub fn report(&self) -> Result<DriftReport, ResearchError> {
        Err(ResearchError::Unimplemented("drift::report", "E17-5"))
    }
}

/// Every source's drift verdict, one entry per source ever observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftReport {
    sources: BTreeMap<SourceId, SourceDrift>,
}

impl DriftReport {
    /// The sources whose inputs are unusual: the §6.3 fact's backing set.
    ///
    /// # Errors
    /// Returns [`ResearchError::Unimplemented`] until E17-5's implementation lands.
    pub fn unusual(&self) -> Result<BTreeSet<SourceId>, ResearchError> {
        Err(ResearchError::Unimplemented("drift::unusual", "E17-5"))
    }

    /// Mandate spec §6.3's `unusual_input` fact (V-018): true exactly when a source the proposal
    /// cites is unusual. Rule-usability waits on stream F's lift of V-018.
    ///
    /// # Errors
    /// Returns [`ResearchError::Unimplemented`] until E17-5's implementation lands.
    pub fn unusual_input(&self, cited: &[SourceId]) -> Result<bool, ResearchError> {
        let _ = cited;
        Err(ResearchError::Unimplemented(
            "drift::unusual_input",
            "E17-5",
        ))
    }

    /// One source's verdict, `None` for a source never observed.
    ///
    /// # Errors
    /// Returns [`ResearchError::Unimplemented`] until E17-5's implementation lands.
    pub fn source(&self, source: &SourceId) -> Result<Option<SourceDrift>, ResearchError> {
        let _ = source;
        Err(ResearchError::Unimplemented("drift::source", "E17-5"))
    }

    /// How many sources the report covers; a report over an observed history is never empty.
    ///
    /// # Errors
    /// Returns [`ResearchError::Unimplemented`] until E17-5's implementation lands.
    pub fn source_count(&self) -> Result<usize, ResearchError> {
        Err(ResearchError::Unimplemented("drift::source_count", "E17-5"))
    }
}

#[cfg(test)]
mod tests {
    use crate::ResearchError;

    /// The stable code (ES-09) of the one error this module mints, pinned live because the
    /// pending E17-5 tests do not run under the mutation gate (DEC-253 item 2), so a code arm
    /// no live test reads would be a mutant nothing catches.
    #[test]
    fn the_out_of_order_code_is_pinned() {
        assert_eq!(
            ResearchError::ObservationOutOfOrder.code(),
            "observation_out_of_order"
        );
    }
}
