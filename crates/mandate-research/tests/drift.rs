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
//!
//! The round-1 additions pin every bar from both sides: each measure has an at-the-bar case
//! (equality does not cross) and a just-under neighbour (it does), a cited source never observed
//! is unusual, a refused observation quarantines its source's baseline until a new one forms, and
//! the report ignores which source arrived first or how two sources' observations interleave.

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

/// A burst of 8 observations 10 s apart from `start`, distinct content from `first`: 70 s of
/// arrivals against a 1 860 s baseline crosses the arrival measure.
fn burst(source: &SourceId, start: i64, first: u64) -> Vec<InputObservation> {
    (0..8)
        .map(|j| {
            news(
                source,
                t(start + j * 10),
                first + u64::try_from(j).expect("a fixture index fits"),
                1_000,
            )
        })
        .collect()
}

/// Eight instants from `start`, advancing by the 7 `gaps` between them: the boundary cases name
/// their spans exactly, so the bar sits on integer seconds.
fn eight_at(start: i64, gaps: [i64; 7]) -> Vec<UtcNanos> {
    let mut at = start;
    let mut times = vec![t(at)];
    for gap in gaps {
        at += gap;
        times.push(t(at));
    }
    times
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
        history.push(news(&source, t(2_400 + j * 10), n, 1_000));
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
        "the window crosses the arrival measure too (8 observations in 70 s against a 1 860 s baseline), and the duplicate measure is evaluated first: its crossing is the one reported"
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
    history.extend(quiet(&loud_source));
    history.extend(burst(&loud_source, 2_400, 100));
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
    let loud_verdict = report
        .source(&loud_source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        loud_verdict.unusual,
        "the loud source carries a full quiet baseline and then bursts: unusual by a measured anomaly, not by a missing baseline"
    );
    assert_eq!(
        loud_verdict.measure,
        Some(DriftMeasure::ArrivalRate),
        "8 observations in 70 s against a 1 860 s baseline crosses the arrival measure, as the scenario claims"
    );
}

#[test]
#[ignore = "pending E17-5"]
fn the_report_is_independent_of_the_sources_arrival_order() {
    let loud = feed("feed-loud");
    let quiet_source = feed("feed-quiet");
    let mut loud_history = quiet(&loud);
    loud_history.extend(burst(&loud, 2_400, 100));
    let quiet_history = quiet(&quiet_source);
    let mut loud_then_quiet = DriftState::new();
    for observation in loud_history.iter().chain(quiet_history.iter()) {
        loud_then_quiet
            .observe(observation)
            .expect("a well-formed observation folds");
    }
    let mut quiet_then_loud = DriftState::new();
    for observation in quiet_history.iter().chain(loud_history.iter()) {
        quiet_then_loud
            .observe(observation)
            .expect("a well-formed observation folds");
    }
    let mut interleaved = DriftState::new();
    for (index, observation) in loud_history.iter().enumerate() {
        interleaved
            .observe(observation)
            .expect("a well-formed observation folds");
        if let Some(other) = quiet_history.get(index) {
            interleaved
                .observe(other)
                .expect("a well-formed observation folds");
        }
    }
    let report_loud_then_quiet = loud_then_quiet.report().expect("the report is total");
    let report_quiet_then_loud = quiet_then_loud.report().expect("the report is total");
    let report_interleaved = interleaved.report().expect("the report is total");
    assert_eq!(
        report_loud_then_quiet
            .source_count()
            .expect("the report counts its sources"),
        2,
        "the report is over an observed history, never empty"
    );
    assert_eq!(
        report_loud_then_quiet, report_quiet_then_loud,
        "which source arrived first changes nothing: the report is keyed by source, not by fold order (ES-21)"
    );
    assert_eq!(
        report_loud_then_quiet, report_interleaved,
        "interleaving the sources' observations, each source's own order kept, changes nothing"
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
    history.extend(quiet(&loud_source));
    history.extend(burst(&loud_source, 2_400, 100));
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

#[test]
#[ignore = "pending E17-5"]
fn a_cited_source_never_observed_is_unusual() {
    let quiet_source = feed("feed-quiet");
    let unseen = feed("feed-unseen");
    let report = fold(&quiet(&quiet_source))
        .report()
        .expect("the report is total");
    assert!(
        report
            .unusual_input(std::slice::from_ref(&unseen))
            .expect("the fact is computed"),
        "a cited source never observed is unusual: zero observations is fewer than the baseline of 32, the fail-safe reading of an unproven citation"
    );
    assert!(
        report
            .unusual_input(&[quiet_source, unseen])
            .expect("the fact is computed"),
        "the fact is a disjunction over the citations: a quiet source does not mask an unseen one"
    );
}

#[test]
#[ignore = "pending E17-5"]
fn the_baseline_bar_is_pinned_at_32() {
    let source = feed("feed-a");
    let full: Vec<InputObservation> = (0..32)
        .map(|i| {
            news(
                &source,
                t(i * 60),
                u64::try_from(i).expect("a fixture index fits"),
                1_000,
            )
        })
        .collect();
    let at_bar = fold(&full)
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        !at_bar.unusual,
        "32 observations is the bar itself: the baseline is full (1 860 s span, 60 s gaps, 1 000 bytes), and a quiet history reads quiet"
    );
    assert_eq!(
        at_bar.measure, None,
        "with the baseline exactly full and every measure quiet, no measure is reported"
    );
    let one_short: Vec<InputObservation> = (0..31)
        .map(|i| {
            news(
                &source,
                t(i * 60),
                u64::try_from(i).expect("a fixture index fits"),
                1_000,
            )
        })
        .collect();
    let just_under = fold(&one_short)
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        just_under.unusual,
        "31 observations is one under the bar: unproven, fail-safe"
    );
    assert_eq!(
        just_under.measure,
        Some(DriftMeasure::NoBaseline),
        "the baseline measure is evaluated first, and 31 crosses it"
    );
}

#[test]
#[ignore = "pending E17-5"]
fn the_duplicate_bar_is_pinned_at_4_of_the_recent_8() {
    let source = feed("feed-a");
    let mut at_bar = quiet(&source);
    for j in 0..8 {
        let replayed = j < 4;
        let n = if replayed {
            999
        } else {
            u64::try_from(90 + j).expect("a fixture index fits")
        };
        at_bar.push(news(&source, t(2_400 + j * 60), n, 1_000));
    }
    let verdict = fold(&at_bar)
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        verdict.unusual,
        "one hash in exactly 4 of the recent 8 is the bar: 4 crosses, and the bar is not 5 — and the four replays sit in the window's older half, so a window of 7 would read 3 of 7 and stay quiet: the case pins the window at 8 too"
    );
    assert_eq!(verdict.measure, Some(DriftMeasure::DuplicateContent));
    let mut just_under = quiet(&source);
    for j in 0..8 {
        let replayed = j < 3;
        let n = if replayed {
            999
        } else {
            u64::try_from(90 + j).expect("a fixture index fits")
        };
        just_under.push(news(&source, t(2_400 + j * 60), n, 1_000));
    }
    let verdict = fold(&just_under)
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        !verdict.unusual,
        "3 of the recent 8 is one under the bar: quiet, and the timing measures are quiet too (60 s apart against a 1 860 s baseline)"
    );
    assert_eq!(verdict.measure, None);
}

#[test]
#[ignore = "pending E17-5"]
fn the_arrival_bar_is_pinned_at_a_twelfth_of_the_baseline_span() {
    let source = feed("feed-a");
    let mut at_bar = quiet(&source);
    for (j, at) in eight_at(2_400, [22, 22, 22, 22, 22, 22, 23])
        .into_iter()
        .enumerate()
    {
        at_bar.push(news(
            &source,
            at,
            u64::try_from(90 + j).expect("a fixture index fits"),
            1_000,
        ));
    }
    let verdict = fold(&at_bar)
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        !verdict.unusual,
        "a recent span of 155 s is exactly a twelfth of the baseline's 1 860 s: the bar is strict, and equality does not cross"
    );
    assert_eq!(verdict.measure, None);
    let mut just_under = quiet(&source);
    for (j, at) in eight_at(2_400, [22, 22, 22, 22, 22, 22, 22])
        .into_iter()
        .enumerate()
    {
        just_under.push(news(
            &source,
            at,
            u64::try_from(90 + j).expect("a fixture index fits"),
            1_000,
        ));
    }
    let verdict = fold(&just_under)
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        verdict.unusual,
        "a recent span of 154 s puts 12 x 154 s under the baseline's 1 860 s: just under the bar, and it crosses"
    );
    assert_eq!(
        verdict.measure,
        Some(DriftMeasure::ArrivalRate),
        "the gap measure is quiet (the smallest gap is 22 s against a 15 s bar), so the arrival crossing is the one reported"
    );
}

#[test]
#[ignore = "pending E17-5"]
fn the_gap_bar_is_pinned_at_a_quarter_of_the_baseline_mean_gap() {
    let source = feed("feed-a");
    let mut at_bar = quiet(&source);
    for (j, at) in eight_at(2_400, [15, 100, 100, 100, 100, 100, 100])
        .into_iter()
        .enumerate()
    {
        at_bar.push(news(
            &source,
            at,
            u64::try_from(90 + j).expect("a fixture index fits"),
            1_000,
        ));
    }
    let verdict = fold(&at_bar)
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        !verdict.unusual,
        "a smallest recent gap of 15 s is exactly a quarter of the baseline's 60 s mean (1 860 s over 31 gaps): the bar is strict, and equality does not cross"
    );
    assert_eq!(verdict.measure, None);
    let mut just_under = quiet(&source);
    for (j, at) in eight_at(2_400, [14, 100, 100, 100, 100, 100, 100])
        .into_iter()
        .enumerate()
    {
        just_under.push(news(
            &source,
            at,
            u64::try_from(90 + j).expect("a fixture index fits"),
            1_000,
        ));
    }
    let verdict = fold(&just_under)
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        verdict.unusual,
        "a smallest recent gap of 14 s is just under the bar, and it crosses"
    );
    assert_eq!(
        verdict.measure,
        Some(DriftMeasure::GapCollapse),
        "the arrival measure is quiet (the window spans 714 s against a 155 s bar), so the gap crossing is the one reported"
    );
}

#[test]
#[ignore = "pending E17-5"]
fn the_length_bar_is_pinned_at_4x_the_baseline_mean() {
    let source = feed("feed-a");
    let with_recent = |length: u64, last: u64| -> Vec<InputObservation> {
        quiet(&source)
            .into_iter()
            .enumerate()
            .map(|(i, mut observation)| {
                if i >= 32 {
                    observation.length_bytes = if i == 39 { last } else { length };
                }
                observation
            })
            .collect()
    };
    let at_bar_up = fold(&with_recent(4_000, 4_000))
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        !at_bar_up.unusual,
        "a recent mean of 4 000 bytes is exactly 4x the baseline's 1 000: the bar is strict, and equality does not cross"
    );
    assert_eq!(at_bar_up.measure, None);
    let just_over = fold(&with_recent(4_000, 4_008))
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        just_over.unusual,
        "a recent mean of 4 001 bytes (seven at 4 000 and one at 4 008) is just over 4x, and it crosses"
    );
    assert_eq!(just_over.measure, Some(DriftMeasure::LengthShift));
    let at_bar_down = fold(&with_recent(250, 250))
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        !at_bar_down.unusual,
        "a recent mean of 250 bytes is exactly a quarter of the baseline's 1 000: equality does not cross"
    );
    assert_eq!(at_bar_down.measure, None);
    let just_under = fold(&with_recent(250, 242))
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        just_under.unusual,
        "a recent mean of 249 bytes (seven at 250 and one at 242) is just under a quarter, and it crosses"
    );
    assert_eq!(just_under.measure, Some(DriftMeasure::LengthShift));
}

#[test]
#[ignore = "pending E17-5"]
fn a_refused_observation_quarantines_the_baseline_until_a_new_one_forms() {
    let source = feed("feed-a");
    let mut state = DriftState::new();
    for observation in &quiet(&source) {
        state
            .observe(observation)
            .expect("a well-formed observation folds");
    }
    state
        .observe(&news(&source, t(100), 999, 1_000))
        .expect_err("a timestamp before the source's last is refused, not folded");
    let quarantined = state
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        quarantined.unusual,
        "the refusal breaks the history's timing integrity: the source escalates until a new baseline forms after it"
    );
    assert_eq!(
        quarantined.measure,
        Some(DriftMeasure::NoBaseline),
        "a quarantined source reads as unproven: the fail-safe measure"
    );
    for j in 0..31 {
        state
            .observe(&news(
                &source,
                t(2_400 + j * 60),
                u64::try_from(500 + j).expect("a fixture index fits"),
                1_000,
            ))
            .expect("a well-formed observation folds");
    }
    let still_short = state
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        still_short.unusual,
        "31 observations after the refusal are one short of a new baseline"
    );
    assert_eq!(still_short.measure, Some(DriftMeasure::NoBaseline));
    state
        .observe(&news(&source, t(2_400 + 31 * 60), 531, 1_000))
        .expect("a well-formed observation folds");
    let recovered = state
        .report()
        .expect("the report is total")
        .source(&source)
        .expect("the verdict is looked up")
        .expect("the observed source has a verdict");
    assert!(
        !recovered.unusual,
        "32 quiet observations after the refusal form a new baseline (1 860 s span, 60 s gaps, 1 000 bytes), and the source reads quiet again"
    );
    assert_eq!(recovered.measure, None);
}
