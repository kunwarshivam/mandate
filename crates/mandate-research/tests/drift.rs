//! E17-5's input-drift detector: hand-calculated drift cases, one per DEC-266 measure and per
//! invariant of the fold.
//!
//! Every scenario names its source's times in whole seconds from
//! `UtcNanos::from_parts(secs, 0)` and its content by number, so each expected verdict is
//! recomputable from DEC-266's rules with integer arithmetic alone: a baseline of the first 32
//! observations and a recent window of the last 8, the measures in the order `NoBaseline`,
//! `DuplicateContent`, `ArrivalRate` (baseline span over 12 × recent span), `GapCollapse`
//! (4 × 31 × the recent minimum gap under the baseline span), `LengthShift` (the recent mean
//! beyond 4× or under ¼ of the baseline mean).
//!
//! Every test is pending until its story lands (DEC-77), and every one fails on the stubs because
//! the stubs return an error where the case expects a verdict.

use mandate_canon::Digest;
use mandate_research::drift::{DriftMeasure, DriftState, InputClass, InputObservation};
use mandate_research::{ContentHash, ResearchError, SourceId};
use mandate_time::UtcNanos;

/// One source's id, in the tests' own words.
fn feed(id: &str) -> SourceId {
    SourceId::new(id).expect("a fixture source id is not empty")
}

/// An instant at a whole second past the epoch.
fn t(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("a fixture instant is in range")
}

/// An instant at `secs` seconds and `nanos` past the epoch, for the sub-second gaps the
/// collapse case needs.
fn t_nanos(secs: i64, nanos: u32) -> UtcNanos {
    UtcNanos::from_parts(secs, nanos).expect("a fixture instant is in range")
}

/// Distinct content, by number: the detector sees hashes, never text (DEC-266 item 2).
fn content(n: u64) -> ContentHash {
    ContentHash::new(Digest::of_parts(&[&n.to_le_bytes()]))
}

/// One news observation from `source` at `at`, carrying content `n` of `length` bytes.
fn news(source: &SourceId, at: UtcNanos, n: u64, length: u64) -> InputObservation {
    InputObservation {
        source: source.clone(),
        class: InputClass::News,
        observed_at: at,
        content: content(n),
        length_bytes: length,
    }
}

/// DEC-266's quiet history: 40 observations a minute apart, distinct content, 1 000 bytes each.
/// The baseline (the first 32) spans 1 860 s with 60 s gaps and a 1 000-byte mean; the recent
/// window (the last 8) spans 420 s the same way, so no measure crosses.
fn quiet(source: &SourceId) -> Vec<InputObservation> {
    (0..40)
        .map(|i| {
            news(
                source,
                t(i * 60),
                u64::try_from(i).expect("a fixture index fits"),
                1_000,
            )
        })
        .collect()
}

/// Folds a history into a fresh state, in arrival order.
fn fold(history: &[InputObservation]) -> DriftState {
    let mut state = DriftState::new();
    for observation in history {
        state
            .observe(observation)
            .expect("a well-formed observation folds");
    }
    state
}

#[test]
#[ignore = "pending E17-5"]
fn a_quiet_source_never_escalates() {
    let state = fold(&quiet(&feed("feed-a")));
    let report = state.report().expect("the report is total");
    assert_eq!(
        report
            .source_count()
            .expect("the report counts its sources"),
        1,
        "a history of one source gives a report of one source"
    );
    let verdict = report
        .source(&feed("feed-a"))
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        !verdict.unusual,
        "a minute apart, distinct, 1 000 bytes: no measure crosses"
    );
    assert_eq!(
        verdict.measure, None,
        "a quiet source reports no measure, not a crossed one"
    );
}

#[test]
#[ignore = "pending E17-5"]
fn a_burst_crosses_the_gap_measure() {
    let source = feed("feed-a");
    let mut history = quiet(&source);
    for j in 0..7 {
        history.push(news(
            &source,
            t(2_400 + j * 100),
            100 + u64::try_from(j).expect("a fixture index fits"),
            1_000,
        ));
    }
    history.push(news(&source, t_nanos(3_000, 100_000_000), 200, 1_000));
    let state = fold(&history);
    let verdict = state
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        verdict.unusual,
        "a 0.1 s gap against a 60 s baseline mean crosses the gap measure"
    );
    assert_eq!(
        verdict.measure,
        Some(DriftMeasure::GapCollapse),
        "the recent window's span (600.1 s against 12 × 1860 s) does not cross the arrival measure, so the gap is the first crossing"
    );
}

#[test]
#[ignore = "pending E17-5"]
fn a_volume_spike_crosses_the_arrival_measure() {
    let source = feed("feed-a");
    let mut history = quiet(&source);
    for j in 0..8 {
        history.push(news(
            &source,
            t(2_400 + j * 10),
            100 + u64::try_from(j).expect("a fixture index fits"),
            1_000,
        ));
    }
    let state = fold(&history);
    let verdict = state
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        verdict.unusual,
        "8 observations in 70 s against a 1 860 s baseline crosses the arrival measure"
    );
    assert_eq!(
        verdict.measure,
        Some(DriftMeasure::ArrivalRate),
        "the arrival measure is evaluated before the gap measure, and its crossing is the one reported"
    );
}

#[test]
#[ignore = "pending E17-5"]
fn replayed_content_crosses_the_duplicate_measure() {
    let source = feed("feed-a");
    let mut history = quiet(&source);
    for j in 0..8 {
        let replayed = j < 5;
        let n = if replayed {
            999
        } else {
            u64::try_from(90 + j).expect("a fixture index fits")
        };
        history.push(news(&source, t(2_400 + j * 60), n, 1_000));
    }
    let state = fold(&history);
    let verdict = state
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        verdict.unusual,
        "one hash in 5 of the recent 8 observations crosses the duplicate measure"
    );
    assert_eq!(
        verdict.measure,
        Some(DriftMeasure::DuplicateContent),
        "the duplicate measure is evaluated before the timing measures, and its crossing is the one reported"
    );
}

#[test]
#[ignore = "pending E17-5"]
fn a_length_shift_crosses_the_length_measure() {
    let source = feed("feed-a");
    let grown: Vec<InputObservation> = quiet(&source)
        .into_iter()
        .enumerate()
        .map(|(i, mut observation)| {
            if i >= 32 {
                observation.length_bytes = 5_000;
            }
            observation
        })
        .collect();
    let verdict = fold(&grown)
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        verdict.unusual,
        "a 5 000-byte recent mean against a 1 000-byte baseline crosses the length measure"
    );
    assert_eq!(
        verdict.measure,
        Some(DriftMeasure::LengthShift),
        "the timing measures are quiet, so the length is the first crossing"
    );

    let shrunk: Vec<InputObservation> = quiet(&source)
        .into_iter()
        .enumerate()
        .map(|(i, mut observation)| {
            if i >= 32 {
                observation.length_bytes = 200;
            }
            observation
        })
        .collect();
    let verdict = fold(&shrunk)
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        verdict.unusual,
        "a 200-byte recent mean against a 1 000-byte baseline crosses the length measure's other direction"
    );
    assert_eq!(verdict.measure, Some(DriftMeasure::LengthShift));
}

#[test]
#[ignore = "pending E17-5"]
fn a_source_without_a_baseline_escalates_fail_safe() {
    let source = feed("feed-a");
    let history: Vec<InputObservation> = (0..5)
        .map(|i| {
            news(
                &source,
                t(i * 60),
                u64::try_from(i).expect("a fixture index fits"),
                1_000,
            )
        })
        .collect();
    let state = fold(&history);
    let verdict = state
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        verdict.unusual,
        "an unproven source escalates until its baseline fills: the fail-safe reading of a missing answer (AGENTS.md rule 3)"
    );
    assert_eq!(
        verdict.measure,
        Some(DriftMeasure::NoBaseline),
        "the baseline measure is evaluated first, and its crossing is the one reported"
    );
}

#[test]
#[ignore = "pending E17-5"]
fn a_compromised_source_does_not_escalate_others() {
    let quiet_source = feed("feed-quiet");
    let loud_source = feed("feed-loud");
    let mut history = quiet(&quiet_source);
    for j in 0..8 {
        history.push(news(
            &loud_source,
            t(2_400 + j * 10),
            100 + u64::try_from(j).expect("a fixture index fits"),
            1_000,
        ));
    }
    let state = fold(&history);
    let report = state.report().expect("the report is total");
    assert_eq!(
        report
            .source_count()
            .expect("the report counts its sources"),
        2,
        "two sources observed give a report of two sources"
    );
    let unusual = report.unusual().expect("the unusual set is total");
    assert_eq!(
        unusual,
        [loud_source.clone()].into(),
        "only the bursting source is unusual: drift never crosses sources"
    );
    let quiet_verdict = report
        .source(&quiet_source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(!quiet_verdict.unusual);
}

#[test]
#[ignore = "pending E17-5"]
fn the_fold_is_deterministic() {
    let history = quiet(&feed("feed-a"));
    let first = fold(&history).report().expect("the report is total");
    let second = fold(&history).report().expect("the report is total");
    assert_eq!(
        first.source_count().expect("the report counts its sources"),
        1,
        "the report is over an observed history, never empty"
    );
    assert_eq!(
        first, second,
        "the same observations in the same order give equal reports (ES-21)"
    );
}

#[test]
#[ignore = "pending E17-5"]
fn the_fold_keeps_same_instant_arrivals() {
    let source = feed("feed-a");
    let mut history = quiet(&source);
    for j in 0..7 {
        history.push(news(
            &source,
            t(2_400 + j * 100),
            100 + u64::try_from(j).expect("a fixture index fits"),
            1_000,
        ));
    }
    history.push(news(&source, t(3_000), 200, 1_000));
    let state = fold(&history);
    let verdict = state
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        verdict.unusual,
        "two arrivals in one instant are a 0 s gap: a fold that kept a set would drop one and read quiet"
    );
    assert_eq!(
        verdict.measure,
        Some(DriftMeasure::GapCollapse),
        "the recent window's span (600 s against 12 × 1 860 s) keeps the arrival measure quiet, so the gap is the first crossing"
    );
}

#[test]
#[ignore = "pending E17-5"]
fn an_observation_before_its_source_s_last_is_refused() {
    let source = feed("feed-a");
    let other = feed("feed-b");
    let mut state = DriftState::new();
    state
        .observe(&news(&source, t(200), 1, 1_000))
        .expect("the first observation folds");
    state
        .observe(&news(&source, t(100), 2, 1_000))
        .expect_err("a timestamp before the same source's last is refused, not folded");
    state
        .observe(&news(&other, t(100), 3, 1_000))
        .expect("the order is per source: another source's earlier instant folds");
    let mut backwards = DriftState::new();
    backwards
        .observe(&news(&source, t(200), 1, 1_000))
        .expect("the first observation folds");
    match backwards.observe(&news(&source, t(100), 2, 1_000)) {
        Err(ResearchError::ObservationOutOfOrder) => {}
        other => panic!("a backwards timestamp is refused with its own error, got {other:?}"),
    }
}

#[test]
#[ignore = "pending E17-5"]
fn the_fact_is_true_only_for_a_cited_unusual_source() {
    let quiet_source = feed("feed-quiet");
    let loud_source = feed("feed-loud");
    let mut history = quiet(&quiet_source);
    for j in 0..8 {
        history.push(news(
            &loud_source,
            t(2_400 + j * 10),
            100 + u64::try_from(j).expect("a fixture index fits"),
            1_000,
        ));
    }
    let report = fold(&history).report().expect("the report is total");
    assert!(
        !report
            .unusual_input(std::slice::from_ref(&quiet_source))
            .expect("the fact is computed"),
        "a proposal citing only the quiet source reads a quiet input"
    );
    assert!(
        report
            .unusual_input(std::slice::from_ref(&loud_source))
            .expect("the fact is computed"),
        "a proposal citing the unusual source reads an unusual input"
    );
    assert!(
        report
            .unusual_input(&[quiet_source.clone(), loud_source.clone()])
            .expect("the fact is computed"),
        "one unusual cited source is enough: the fact is a disjunction over the citations"
    );
    assert!(
        !report.unusual_input(&[]).expect("the fact is computed"),
        "a proposal citing nothing reads a quiet input: no cited source is unusual"
    );
}
