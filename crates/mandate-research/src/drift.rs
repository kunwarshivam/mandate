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
//! 2. [`DriftMeasure::DegenerateBaseline`] — the baseline's 32 observations all arrived in one
//!    instant, so its zero span can be crossed by no fraction of itself and the timing measures
//!    below are unreachable for exactly the fastest flood, the R-05 shape: unusual, failing
//!    closed (DEC-267 item 6, tightened in round 1);
//! 3. [`DriftMeasure::DuplicateContent`] — one content hash at least [`DUPLICATE_MINIMUM`] of the
//!    recent window: a replayed or spammed payload;
//! 4. [`DriftMeasure::ArrivalRate`] — the recent window's span under [`ARRIVAL_FACTOR`] of the
//!    baseline's: a flood;
//! 5. [`DriftMeasure::GapCollapse`] — the recent window's smallest inter-arrival gap under
//!    [`GAP_FACTOR`] of the baseline's mean gap;
//! 6. [`DriftMeasure::LengthShift`] — the recent window's mean byte length beyond
//!    [`LENGTH_FACTOR`] times the baseline's, or under its inverse.
//!
//! A refused observation quarantines its source: a timestamp before the source's last breaks
//! the history's timing integrity, so both windows read only the observations after the last
//! refusal and the source reads unproven — `NoBaseline`, unusual — until a full
//! [`BASELINE_WINDOW`]-observation baseline has formed after it (DEC-266's tightening reading:
//! the pre-refusal timing is not trusted, and the source escalates while its baseline is
//! incomplete).
//!
//! The thresholds are DEC-266's, pinned by the boundary cases in `tests/drift.rs` — each
//! measure's bar with its just-under neighbour, so a threshold moved by one in either direction
//! fails a test — and by the live pin test in this module; moving them into envelope fields is
//! Proposed in DEC-266 item 7, because an owner cannot be talked out of a threshold they never
//! confirmed. The escalation seam — how an unusual verdict reaches the owner before
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

/// The baseline's inter-arrival gaps: one fewer than its [`BASELINE_WINDOW`] observations, so the
/// mean gap is the baseline span over 31 (DEC-266 item 4.4's divisor, pinned from both sides by
/// `the_gap_divisor_is_the_baseline_s_31_gaps`).
const BASELINE_GAPS: u64 = 31;

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
    /// The baseline's observations all arrived in one instant, so its zero span cannot be crossed
    /// by a fraction of itself and the timing measures cannot exonerate the source: unusual,
    /// failing closed (DEC-267 item 6, round 1's major 2).
    DegenerateBaseline,
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
///
/// No `Default` beside [`DriftState::new`]: a `new` whose body a `Default::default()` mutant
/// could replace is a mutant no test can tell apart, and the gate would count it missed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftState {
    observed: BTreeMap<SourceId, SourceHistory>,
}

impl DriftState {
    /// An empty detector, before any input has arrived.
    #[allow(
        clippy::new_without_default,
        reason = "a Default beside new would let a Default::default() body mutant replace new with no visible effect, and the mutation gate would count it missed"
    )]
    #[must_use]
    pub fn new() -> Self {
        Self {
            observed: BTreeMap::new(),
        }
    }

    /// Folds one observation into the source's history, in arrival order. A timestamp before
    /// the source's last — the last observation ever folded for it, pre-refusal ones included —
    /// is refused, and the refusal quarantines the source: both windows read only the
    /// observations after the last refusal, so the source reads unproven until a new baseline
    /// forms (DEC-266's tightening reading).
    ///
    /// # Errors
    /// Returns [`ResearchError::ObservationOutOfOrder`] — folding nothing — when the
    /// observation's timestamp precedes the last one already folded for its source. The order
    /// check is per source: another source's timeline never constrains this one's.
    pub fn observe(&mut self, observation: &InputObservation) -> Result<(), ResearchError> {
        self.observed
            .entry(observation.source.clone())
            .or_default()
            .fold(observation)
    }

    /// The drift verdict for every source ever observed, each from its own history alone: a
    /// compromised source never escalates another's theses (DEC-266 item 2). Quarantined
    /// sources are covered — their verdict is the fail-safe [`DriftMeasure::NoBaseline`] until
    /// a new baseline forms — so the report's map holds every source the fold has seen.
    ///
    /// # Errors
    /// None: the report is total over the folded history.
    pub fn report(&self) -> Result<DriftReport, ResearchError> {
        let sources = self
            .observed
            .iter()
            .map(|(source, history)| (source.clone(), history.verdict(source)))
            .collect();
        Ok(DriftReport { sources })
    }
}

/// One source's folded history: every observation ever folded for it, in arrival order, and
/// where the windows' trusted part begins. A refusal sets the trust boundary past everything
/// folded so far, and the history keeps the refused-out prefix — the order check still reads the
/// whole history's last, so a post-refusal stamp before the pre-refusal last is refused too, and
/// the report stays a function of the fold alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SourceHistory {
    observations: Vec<InputObservation>,
    trusted_from: usize,
}

impl SourceHistory {
    /// Folds one observation in arrival order, refusing a timestamp strictly before the
    /// history's last; an equal instant folds, because same-instant arrivals are a real signal
    /// the timing measures read (interpretation 5).
    fn fold(&mut self, observation: &InputObservation) -> Result<(), ResearchError> {
        if self
            .observations
            .last()
            .is_some_and(|last| observation.observed_at < last.observed_at)
        {
            self.trusted_from = self.observations.len();
            return Err(ResearchError::ObservationOutOfOrder);
        }
        self.observations.push(observation.clone());
        Ok(())
    }

    /// The source's verdict over its trusted history: unusual exactly when a measure crosses.
    fn verdict(&self, source: &SourceId) -> SourceDrift {
        let effective = self.observations.get(self.trusted_from..).unwrap_or(&[]);
        let measure = first_crossed(effective);
        SourceDrift {
            source: source.clone(),
            unusual: measure.is_some(),
            measure,
        }
    }
}

/// The first measure the trusted history crosses, in evaluation order; `None` is quiet. A
/// history too short to supply the baseline is the fail-safe crossing itself,
/// [`DriftMeasure::NoBaseline`] (AGENTS.md rule 3): the recent window is the shorter of the two,
/// so the arm covers exactly the histories under [`BASELINE_WINDOW`] observations, whichever
/// window the split fails on.
fn first_crossed(effective: &[InputObservation]) -> Option<DriftMeasure> {
    let (Some((baseline, _)), Some((_, recent))) =
        (effective.split_first_chunk(), effective.split_last_chunk())
    else {
        return Some(DriftMeasure::NoBaseline);
    };
    measured(baseline, recent)
}

/// The measured verdicts for a history that supplies both windows: the baseline's own health
/// first — a span of zero can exonerate nothing — then the window measures in DEC-266's order;
/// the first crossing is the one reported (the house style of mandate spec §8.5).
fn measured(
    baseline: &[InputObservation; BASELINE_WINDOW],
    recent: &[InputObservation; RECENT_WINDOW],
) -> Option<DriftMeasure> {
    if window_span(baseline) == 0 {
        return Some(DriftMeasure::DegenerateBaseline);
    }
    if duplicate_repeats(recent) {
        return Some(DriftMeasure::DuplicateContent);
    }
    if arrival_collapsed(baseline, recent) {
        return Some(DriftMeasure::ArrivalRate);
    }
    if gap_collapsed(baseline, recent) {
        return Some(DriftMeasure::GapCollapse);
    }
    if length_shifted(baseline, recent) {
        return Some(DriftMeasure::LengthShift);
    }
    None
}

/// The duplicate measure: one content hash at least [`DUPLICATE_MINIMUM`] of the recent window
/// (DEC-266 item 4.2) — a replayed or spammed payload. Counting matches pairwise keeps the
/// window's multiplicity: a fold over a set would read a replay as one arrival.
fn duplicate_repeats(recent: &[InputObservation; RECENT_WINDOW]) -> bool {
    recent.iter().any(|probe| {
        recent
            .iter()
            .filter(|other| other.content == probe.content)
            .count()
            >= DUPLICATE_MINIMUM
    })
}

/// The arrival measure: the recent window's span under [`ARRIVAL_FACTOR`] of the baseline's
/// (DEC-266 item 4.3) — a flood. The factor compares spans, not rates: 8 arrivals at the
/// baseline's own pace already span 7/31 of the baseline's span, so crossing needs roughly a
/// 2.7× rate, not a 12× flood. The bar is strict: a recent span of exactly a twelfth reads
/// quiet, as the boundary case pins.
fn arrival_collapsed(
    baseline: &[InputObservation; BASELINE_WINDOW],
    recent: &[InputObservation; RECENT_WINDOW],
) -> bool {
    window_span(baseline) > i128::from(ARRIVAL_FACTOR).saturating_mul(window_span(recent))
}

/// The gap measure: the recent window's smallest inter-arrival gap under [`GAP_FACTOR`] of the
/// baseline's mean gap, itself the baseline span over its [`BASELINE_GAPS`] gaps (DEC-266 item
/// 4.4). The comparison multiplies rather than divides — `GAP_FACTOR × BASELINE_GAPS × gap`
/// against the baseline span — so no rounding can move the bar, and a same-instant pair, a gap
/// of zero, crosses whenever the baseline span is positive.
fn gap_collapsed(
    baseline: &[InputObservation; BASELINE_WINDOW],
    recent: &[InputObservation; RECENT_WINDOW],
) -> bool {
    i128::from(GAP_FACTOR)
        .saturating_mul(i128::from(BASELINE_GAPS))
        .saturating_mul(smallest_gap(recent))
        < window_span(baseline)
}

/// A `usize` threshold as the length comparison's `u128`: the constants sit far inside `u128`
/// on every supported target, so the `0` fallback is unreachable and kept so the conversion
/// stays checked rather than assumed.
fn as_u128(value: usize) -> u128 {
    u128::try_from(value).unwrap_or(0)
}

/// The length measure: the recent window's mean byte length beyond [`LENGTH_FACTOR`] times the
/// baseline's, or under its inverse (DEC-266 item 4.5) — an injected payload, or a stripped
/// feed. The means stay as exact totals and the comparison cross-multiplies, so it is the mean
/// comparison without a division (ES-21) and a rounding can never move the bar.
fn length_shifted(
    baseline: &[InputObservation; BASELINE_WINDOW],
    recent: &[InputObservation; RECENT_WINDOW],
) -> bool {
    let recent_total = total_length(recent);
    let baseline_total = total_length(baseline);
    let grew = as_u128(BASELINE_WINDOW).saturating_mul(recent_total)
        > u128::from(LENGTH_FACTOR)
            .saturating_mul(as_u128(RECENT_WINDOW))
            .saturating_mul(baseline_total);
    let shrank = u128::from(LENGTH_FACTOR)
        .saturating_mul(as_u128(BASELINE_WINDOW))
        .saturating_mul(recent_total)
        < as_u128(RECENT_WINDOW).saturating_mul(baseline_total);
    grew || shrank
}

/// A window's span: its last arrival's instant minus its first's. Both windows [`measured`]
/// reads have a fixed length, so the `0` fallback is unreachable; a real span of `0` — every
/// arrival in one instant — reaches the measures as-is.
fn window_span(window: &[InputObservation]) -> i128 {
    window
        .first()
        .zip(window.last())
        .map_or(0, |(first, last)| {
            span_nanos(last.observed_at, first.observed_at)
        })
}

/// The smallest inter-arrival gap in a window, in nanoseconds. A window of fewer than two
/// arrivals has no gaps and reads `0` — a collapse — so even that unreachable case stays on the
/// unusual side (AGENTS.md rule 3).
fn smallest_gap(window: &[InputObservation]) -> i128 {
    window
        .windows(2)
        .filter_map(|pair| match pair {
            [earlier, later] => Some(span_nanos(later.observed_at, earlier.observed_at)),
            _ => None,
        })
        .min()
        .unwrap_or(0)
}

/// A window's total byte length, the exact form its mean takes in the length comparison: a
/// `u128` total, because a window of `u64` lengths cannot overflow it.
fn total_length(window: &[InputObservation]) -> u128 {
    window
        .iter()
        .map(|observation| u128::from(observation.length_bytes))
        .fold(0_u128, |total, length| total.saturating_add(length))
}

/// The nanoseconds from `earlier` to `later`. `UtcNanos::from_parts` bounds both instants to
/// 1970 through 9999, so the difference fits `i128` nanoseconds with nine orders of magnitude
/// to spare and no saturating fallback below can fire; they keep the arithmetic checked rather
/// than assumed. A `later` before `earlier`, which the monotone histories never hold, would
/// read as the negative span it is, never as a wrap.
fn span_nanos(later: UtcNanos, earlier: UtcNanos) -> i128 {
    i128::from(later.secs())
        .saturating_sub(i128::from(earlier.secs()))
        .saturating_mul(1_000_000_000)
        .saturating_add(i128::from(later.nanos()))
        .saturating_sub(i128::from(earlier.nanos()))
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
    /// None: the set is total over the report.
    pub fn unusual(&self) -> Result<BTreeSet<SourceId>, ResearchError> {
        Ok(self
            .sources
            .values()
            .filter(|drift| drift.unusual)
            .map(|drift| drift.source.clone())
            .collect())
    }

    /// Mandate spec §6.3's `unusual_input` fact (V-018): true exactly when a source the proposal
    /// cites is unusual — and a cited source never observed is unusual, because zero
    /// observations is fewer than [`BASELINE_WINDOW`] (the fail-safe reading of an unproven
    /// citation; the report's verdicts cover observed sources, and a citation with no verdict
    /// was never observed, so the fold's own coverage answers for it). Rule-usability waits on
    /// stream F's lift of V-018.
    ///
    /// # Errors
    /// None: the fact is total over the citations and the fold.
    pub fn unusual_input(&self, cited: &[SourceId]) -> Result<bool, ResearchError> {
        Ok(cited
            .iter()
            .any(|source| self.sources.get(source).is_none_or(|drift| drift.unusual)))
    }

    /// One source's verdict, `None` for a source never observed.
    ///
    /// # Errors
    /// None: the lookup is total.
    pub fn source(&self, source: &SourceId) -> Result<Option<SourceDrift>, ResearchError> {
        Ok(self.sources.get(source).cloned())
    }

    /// How many sources the report covers; a report over an observed history is never empty.
    ///
    /// # Errors
    /// None: the count is total.
    pub fn source_count(&self) -> Result<usize, ResearchError> {
        Ok(self.sources.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mandate_canon::Digest;
    use proptest::prelude::*;

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

    /// DEC-266 item 7: the thresholds are code constants, pinned here live (the mutation gate
    /// runs only live tests) and from both sides by `tests/drift.rs`'s boundary cases — a
    /// threshold moved by one in either direction, or a strict bar relaxed to equality, fails
    /// one of those.
    #[test]
    fn the_thresholds_are_pinned() {
        assert_eq!(BASELINE_WINDOW, 32);
        assert_eq!(RECENT_WINDOW, 8);
        assert_eq!(DUPLICATE_MINIMUM, 4);
        assert_eq!(ARRIVAL_FACTOR, 12);
        assert_eq!(GAP_FACTOR, 4);
        assert_eq!(LENGTH_FACTOR, 4);
        assert_eq!(BASELINE_GAPS, 31);
        assert_eq!(
            usize::try_from(BASELINE_GAPS).unwrap_or(0),
            BASELINE_WINDOW.saturating_sub(1),
            "the gap divisor is the baseline's own gaps: one fewer than its observations"
        );
    }

    /// DEC-266's quiet history in the module's own words: 40 observations a minute apart,
    /// distinct content, 1 000 bytes each — the same shape `tests/drift.rs`'s `quiet` builds.
    fn quiet_history(source: &SourceId) -> Result<Vec<InputObservation>, ResearchError> {
        let arrivals: Vec<Arrival> = (0..40)
            .map(|index| (60, 0, u64::try_from(index).unwrap_or(0), 1_000))
            .collect();
        observations_of(source, &arrivals, BASE_SECS)
    }

    /// Eight instants from `start`, advancing by the seven `gaps` between them, on whole
    /// seconds: the boundary cases in `tests/drift.rs` name their spans the same way.
    fn eight_at(start: i64, gaps: [i64; 7]) -> Result<Vec<UtcNanos>, ResearchError> {
        let mut at = start;
        let mut times = vec![UtcNanos::from_parts(at, 0)?];
        for gap in gaps {
            at = at.saturating_add(gap);
            times.push(UtcNanos::from_parts(at, 0)?);
        }
        Ok(times)
    }

    /// DEC-267 item 1: the order check reads the whole history's last, not the trusted
    /// suffix's. After a refusal, a stamp between the refused one and the pre-refusal last is
    /// refused too — it precedes an instant the quarantine refuses to trust — and
    /// re-quarantines; the trusted-suffix reading would fold it and seed the new baseline from
    /// timing the quarantine rejects, and the plant that reads the check that way fails here
    /// (round 1's major 1).
    #[test]
    fn a_stamp_between_the_refused_one_and_the_pre_refusal_last_is_refused_too()
    -> Result<(), ResearchError> {
        let source = SourceId::new("feed-a")?;
        let mut state = DriftState::new();
        for observation in &quiet_history(&source)? {
            state.observe(observation)?;
        }
        let refused = UtcNanos::from_parts(BASE_SECS, 0)?;
        let between = UtcNanos::from_parts(BASE_SECS, 1)?;
        assert!(matches!(
            state.observe(&observation(&source, refused, 999, 1_000)),
            Err(ResearchError::ObservationOutOfOrder)
        ));
        assert!(
            matches!(
                state.observe(&observation(&source, between, 998, 1_000)),
                Err(ResearchError::ObservationOutOfOrder)
            ),
            "the whole history's last still binds after a refusal: a stamp before it is \
             refused, not folded into the post-refusal baseline"
        );
        Ok(())
    }

    /// DEC-267 item 6, tightened in round 1 (major 2): a baseline whose 32 observations all
    /// arrived in one instant has a zero span, and a zero span cannot be crossed by a fraction
    /// of itself, so both timing measures are unreachable for exactly the fastest flood — the
    /// R-05 shape. Such a baseline reads unusual in its own right, whatever follows it: the
    /// reviewer's witness is 32 distinct inputs at one instant and then an 8-input burst a
    /// second apart 23 days later, which the pre-tightening code read quiet.
    #[test]
    fn a_baseline_that_never_spread_is_unusual_whatever_follows() -> Result<(), ResearchError> {
        let source = SourceId::new("feed-a")?;
        let mut state = DriftState::new();
        for index in 0..32 {
            state.observe(&observation(
                &source,
                UtcNanos::from_parts(BASE_SECS, 0)?,
                u64::try_from(index).unwrap_or(0).saturating_add(500),
                1_000,
            ))?;
        }
        let burst_start = BASE_SECS.saturating_add(23_i64.saturating_mul(86_400));
        for (index, gap) in (0..8).zip([0, 1, 1, 1, 1, 1, 1, 1]) {
            let at = UtcNanos::from_parts(burst_start.saturating_add(gap), 0)?;
            state.observe(&observation(
                &source,
                at,
                u64::try_from(index).unwrap_or(0).saturating_add(600),
                1_000,
            ))?;
        }
        let quiet_fallback = SourceDrift {
            source: source.clone(),
            unusual: false,
            measure: None,
        };
        let verdict = state.report()?.source(&source)?.unwrap_or(quiet_fallback);
        assert!(
            verdict.unusual,
            "a baseline that never spread cannot exonerate its source: the timing measures \
             have no span to compare against"
        );
        assert_eq!(
            verdict.measure,
            Some(DriftMeasure::DegenerateBaseline),
            "the baseline's own health is checked before the window measures, so the reported \
             crossing names the degenerate baseline rather than a window that happens to be quiet"
        );
        Ok(())
    }

    /// Round 1's minor 1: DEC-266 fixes the evaluation order, and the gap-before-length pair is
    /// the one no integration case pins. This window crosses both — a 1 s smallest gap against
    /// a 60 s baseline mean, and a 5 000-byte recent mean against a 1 000-byte baseline — while
    /// the arrival measure stays quiet (a 701 s span against a 155 s bar), so the reported
    /// measure is the gap, and the arm swap fails here.
    #[test]
    fn the_gap_measure_is_reported_when_the_length_crosses_too() -> Result<(), ResearchError> {
        let source = SourceId::new("feed-a")?;
        let mut history = quiet_history(&source)?;
        let gaps = [1, 100, 100, 100, 100, 100, 100];
        for (index, at) in eight_at(BASE_SECS.saturating_add(2_400), gaps)?
            .into_iter()
            .enumerate()
        {
            history.push(observation(
                &source,
                at,
                u64::try_from(index).unwrap_or(0).saturating_add(90),
                5_000,
            ));
        }
        let quiet_fallback = SourceDrift {
            source: source.clone(),
            unusual: false,
            measure: None,
        };
        let verdict = fold_of(&history)?
            .report()?
            .source(&source)?
            .unwrap_or(quiet_fallback);
        assert!(verdict.unusual);
        assert_eq!(
            verdict.measure,
            Some(DriftMeasure::GapCollapse),
            "the window crosses the gap and the length measures both; DEC-266's order reports \
             the gap"
        );
        Ok(())
    }

    /// The base instant every generated history walks forward from: 2025-01-01, far inside
    /// `UtcNanos`'s range however the generated gaps accumulate.
    const BASE_SECS: i64 = 1_735_689_600;

    /// One generated arrival: a forward gap in seconds and nanoseconds (ties and sub-second
    /// gaps both occur), content drawn from a pool of six hashes so replays occur, and a byte
    /// length.
    type Arrival = (u64, u32, u64, u64);

    fn arrival() -> impl Strategy<Value = Arrival> {
        (0..600_u64, 0..1_000_000_000_u32, 0..6_u64, 1..5_000_u64)
    }

    fn arrivals() -> impl Strategy<Value = Vec<Arrival>> {
        proptest::collection::vec(arrival(), 0..48)
    }

    /// One news observation, the tests' own words: content by number, so the detector sees
    /// hashes, never text (DEC-266 item 2).
    fn observation(source: &SourceId, at: UtcNanos, content: u64, length: u64) -> InputObservation {
        InputObservation {
            source: source.clone(),
            class: InputClass::News,
            observed_at: at,
            content: ContentHash::new(Digest::of_parts(&[&content.to_le_bytes()])),
            length_bytes: length,
        }
    }

    /// Builds one source's observations from generated arrivals: the instant walks forward by
    /// each arrival's gap, carrying the nanosecond part, so the history is monotone with ties
    /// and never leaves `UtcNanos`'s range (48 arrivals of at most 600 s from the 2025 base).
    fn observations_of(
        source: &SourceId,
        arrivals: &[Arrival],
        base_secs: i64,
    ) -> Result<Vec<InputObservation>, ResearchError> {
        let mut secs = base_secs;
        let mut nanos = 0_u64;
        let mut observations = Vec::with_capacity(arrivals.len());
        for (gap_secs, gap_nanos, content, length) in arrivals {
            secs = secs.saturating_add(i64::try_from(*gap_secs).unwrap_or(0));
            nanos = nanos.saturating_add(u64::from(*gap_nanos));
            let carry = i64::try_from(nanos / 1_000_000_000).unwrap_or(0);
            let sub_nanos = u32::try_from(nanos % 1_000_000_000).unwrap_or(0);
            let at = UtcNanos::from_parts(secs.saturating_add(carry), sub_nanos)?;
            observations.push(observation(source, at, *content, *length));
        }
        Ok(observations)
    }

    /// An instant strictly before `last`'s: one whole second off, so whatever `last`'s
    /// nanosecond part, the instant precedes it.
    fn before(last: UtcNanos) -> Result<UtcNanos, ResearchError> {
        Ok(UtcNanos::from_parts(last.secs().saturating_sub(1), 0)?)
    }

    /// Folds a history into a fresh state, in arrival order.
    fn fold_of(history: &[InputObservation]) -> Result<DriftState, ResearchError> {
        let mut state = DriftState::new();
        for observation in history {
            state.observe(observation)?;
        }
        Ok(state)
    }

    /// Interleaves two histories observation by observation, each source's own order kept.
    fn interleave(left: &[InputObservation], right: &[InputObservation]) -> Vec<InputObservation> {
        let mut out = Vec::new();
        for index in 0..left.len().max(right.len()) {
            if let Some(observation) = left.get(index) {
                out.push(observation.clone());
            }
            if let Some(observation) = right.get(index) {
                out.push(observation.clone());
            }
        }
        out
    }

    proptest! {
        /// A source's verdict is its own history's: folding another source's observations
        /// before, after, or between the first's changes neither verdict, and the report equals
        /// the one from the reversed interleaving (DEC-266 item 2, interpretation 6). The
        /// oracle folds each source alone and answers from the two solo reports.
        #[test]
        fn interleaving_never_changes_a_source_s_own_verdict(
            (left, right) in (arrivals(), arrivals()),
        ) {
            let source_a = SourceId::new("feed-a")?;
            let source_b = SourceId::new("feed-b")?;
            let a = observations_of(&source_a, &left, BASE_SECS)?;
            let b = observations_of(&source_b, &right, BASE_SECS)?;
            let solo_a = fold_of(&a)?.report()?;
            let solo_b = fold_of(&b)?.report()?;
            let mixed = fold_of(&interleave(&a, &b))?.report()?;
            prop_assert_eq!(mixed.source(&source_a)?, solo_a.source(&source_a)?);
            prop_assert_eq!(mixed.source(&source_b)?, solo_b.source(&source_b)?);
            prop_assert_eq!(mixed, fold_of(&interleave(&b, &a))?.report()?);
        }

        /// A generated monotone history folds clean, ties included, and a stamp strictly before
        /// the source's last is refused while another source's earlier stamp still folds
        /// (interpretation 5); a source with no history yet accepts any first stamp.
        #[test]
        fn monotone_times_fold_and_a_backwards_stamp_is_refused(generated in arrivals()) {
            let source = SourceId::new("feed-a")?;
            let other = SourceId::new("feed-b")?;
            let history = observations_of(&source, &generated, BASE_SECS)?;
            let mut state = DriftState::new();
            for observation in &history {
                state.observe(observation)?;
            }
            if let Some(last) = history.last() {
                prop_assert!(matches!(
                    state.observe(&observation(&source, before(last.observed_at)?, 999, 1_000)),
                    Err(ResearchError::ObservationOutOfOrder)
                ));
                prop_assert!(state
                    .observe(&observation(&other, UtcNanos::EPOCH, 3, 1_000))
                    .is_ok());
            } else {
                prop_assert!(state
                    .observe(&observation(&source, UtcNanos::EPOCH, 1, 1_000))
                    .is_ok());
            }
        }

        /// After a refusal the source reads `NoBaseline` — unusual — until exactly 32 further
        /// observations have folded, whatever the pre-refusal history held (interpretation 7).
        /// The oracle counts the post-refusal arrivals itself; the recovery history starts a
        /// second after the pre-refusal last, so it folds clean, and a verdict lookup that
        /// wrongly finds nothing answers as the quiet fallback, which the `NoBaseline`
        /// assertions fail.
        #[test]
        fn a_refusal_quarantines_until_32_further_observations_fold(
            (pre, recovery) in (
                proptest::collection::vec(arrival(), 1..48),
                proptest::collection::vec(arrival(), 32..33),
            ),
        ) {
            let source = SourceId::new("feed-a")?;
            let history = observations_of(&source, &pre, BASE_SECS)?;
            prop_assume!(!history.is_empty());
            let Some(last) = history.last() else {
                return Ok(());
            };
            let mut state = DriftState::new();
            for observation in &history {
                state.observe(observation)?;
            }
            prop_assert!(matches!(
                state.observe(&observation(&source, before(last.observed_at)?, 999, 1_000)),
                Err(ResearchError::ObservationOutOfOrder)
            ));
            let after = observations_of(
                &source,
                &recovery,
                last.observed_at.secs().saturating_add(1),
            )?;
            for (index, observation) in after.iter().enumerate() {
                state.observe(observation)?;
                let quiet_fallback = SourceDrift {
                    source: source.clone(),
                    unusual: false,
                    measure: None,
                };
                let verdict = state
                    .report()?
                    .source(&source)?
                    .unwrap_or(quiet_fallback);
                if index < 31 {
                    prop_assert_eq!(verdict.measure, Some(DriftMeasure::NoBaseline));
                } else {
                    prop_assert_ne!(verdict.measure, Some(DriftMeasure::NoBaseline));
                }
            }
        }

        /// The §6.3 fact is the disjunction over the citations, and a cited source never
        /// observed is unusual (DEC-266 item 3): the oracle answers from the report's own
        /// per-source verdicts, true for a citation the report does not cover, so a fact that
        /// ORs over the wrong set or reads an unproven citation quiet fails.
        #[test]
        fn the_fact_is_the_disjunction_over_citations(
            (left, right, cite_left, cite_right, cite_unseen) in (
                arrivals(),
                arrivals(),
                any::<bool>(),
                any::<bool>(),
                any::<bool>(),
            ),
        ) {
            let source_a = SourceId::new("feed-a")?;
            let source_b = SourceId::new("feed-b")?;
            let unseen = SourceId::new("feed-unseen")?;
            let a = observations_of(&source_a, &left, BASE_SECS)?;
            let b = observations_of(&source_b, &right, BASE_SECS)?;
            let report = fold_of(&interleave(&a, &b))?.report()?;
            let unusual_a = report.source(&source_a)?.is_none_or(|drift| drift.unusual);
            let unusual_b = report.source(&source_b)?.is_none_or(|drift| drift.unusual);
            let mut cited = Vec::new();
            if cite_left {
                cited.push(source_a.clone());
            }
            if cite_right {
                cited.push(source_b.clone());
            }
            if cite_unseen {
                cited.push(unseen.clone());
            }
            let expected = (cite_left && unusual_a) || (cite_right && unusual_b) || cite_unseen;
            prop_assert_eq!(report.unusual_input(&cited)?, expected);
            prop_assert!(!report.unusual_input(&[])?);
        }
    }
}
