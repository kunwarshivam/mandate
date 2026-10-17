//! Property tests for the harness (DEC-1002), each with an oracle of its own and shown to fail on a
//! seeded bug:
//!
//! - **Every thesis is accounted for.** Over all groups, closed + open + outside the window equals
//!   the records read, and the open count equals the records whose horizon ends after the cut-off,
//!   counted from the inputs.
//! - **Passing needs a scored group, and every scored group passing.**
//! - **Pure.** The same records give the same report bytes.

mod common;

use common::*;
use mandate_evaluation::EvaluationReport;
use proptest::prelude::*;

const CUT_OFF: &str = "2026-09-24T00:00:00Z";

/// The fixed horizons a generated thesis may have: closed in the window, open after the cut-off,
/// or closed before the window opened.
const HORIZONS: [(&str, &str); 3] = [
    ("2026-09-21T14:00:00Z", "2026-09-23T21:00:00Z"),
    ("2026-09-21T14:00:00Z", "2026-09-30T21:00:00Z"),
    ("2026-08-20T14:00:00Z", "2026-08-25T21:00:00Z"),
];

fn generated(shapes: &[(usize, bool, bool)]) -> Vec<Thesis> {
    shapes
        .iter()
        .enumerate()
        .map(|(index, (horizon, admitted, next_model))| {
            let (as_of, expires_at) = HORIZONS[*horizon % HORIZONS.len()];
            let mut thesis = thesis(&format!("th_{index}"), as_of, expires_at);
            thesis.admitted = *admitted;
            if *next_model {
                thesis.model_version = "0.2.0";
                thesis.content_hash = NEXT_HASH;
            }
            thesis
        })
        .collect()
}

fn accounted(report: &EvaluationReport, theses: &[Thesis]) -> Result<(), String> {
    let counted: u32 = report
        .groups
        .iter()
        .map(|group| group.closed + group.open + group.outside_window)
        .sum();
    let read = u32::try_from(theses.len()).map_err(|e| e.to_string())?;
    if counted != read {
        return Err(format!("{counted} theses counted of {read} read"));
    }
    let cut_off = at(CUT_OFF);
    let open = theses
        .iter()
        .filter(|thesis| at(thesis.expires_at) > cut_off)
        .count();
    let reported: u32 = report.groups.iter().map(|group| group.open).sum();
    if usize::try_from(reported).map_err(|e| e.to_string())? != open {
        return Err(format!(
            "{reported} open reported, {open} open by the cut-off"
        ));
    }
    Ok(())
}

fn passes_as_defined(report: &EvaluationReport) -> Result<(), String> {
    let scored: Vec<bool> = report
        .groups
        .iter()
        .filter_map(|group| group.outcome.as_ref().ok().map(|card| card.passed))
        .collect();
    let expected = !scored.is_empty() && scored.iter().all(|passed| *passed);
    if report.passed() != expected {
        return Err(format!(
            "passed is {} where the groups give {expected}",
            report.passed()
        ));
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn every_thesis_is_accounted_for_and_the_report_is_pure(
        shapes in prop::collection::vec((0usize..3, any::<bool>(), any::<bool>()), 0..8),
        minimum in 0u32..3,
    ) {
        let stored = stored_research();
        let theses = generated(&shapes);
        let records = records(stored.0, &theses);
        let report = evaluate(&records, &stored, &decision(minimum), CUT_OFF)
            .map_err(|e| TestCaseError::fail(e.code()))?;
        accounted(&report, &theses).map_err(TestCaseError::fail)?;
        passes_as_defined(&report).map_err(TestCaseError::fail)?;
        let again = evaluate(&records, &stored, &decision(minimum), CUT_OFF)
            .map_err(|e| TestCaseError::fail(e.code()))?;
        prop_assert_eq!(
            report.canonical_bytes().map_err(|e| TestCaseError::fail(e.code()))?,
            again.canonical_bytes().map_err(|e| TestCaseError::fail(e.code()))?
        );
    }
}

/// The accounting oracle catches a dropped thesis and a miscounted open one.
#[test]
fn the_accounting_oracle_catches_a_dropped_or_miscounted_thesis() {
    let stored = stored_research();
    let theses = generated(&[(0, true, false), (1, true, false), (2, false, true)]);
    let records = records(stored.0, &theses);
    let report = evaluate(&records, &stored, &decision(0), CUT_OFF)
        .unwrap_or_else(|e| panic!("{}", e.code()));
    assert!(accounted(&report, &theses).is_ok());
    let mut dropped = report.clone();
    dropped.groups[0].closed = dropped.groups[0].closed.saturating_sub(1);
    assert!(accounted(&dropped, &theses).is_err());
    let mut miscounted = report.clone();
    miscounted.groups[0].open += 1;
    miscounted.groups[0].closed = miscounted.groups[0].closed.saturating_sub(1);
    assert!(accounted(&miscounted, &theses).is_err());
}

/// DEC-1002 item 7: one scored group that fails fails the evaluation, and the pass oracle agrees.
#[test]
fn one_failing_group_fails_the_evaluation() {
    let stored = stored_research();
    let theses = generated(&[(0, true, false)]);
    let records = records(stored.0, &theses);
    let mut report = evaluate(&records, &stored, &decision(0), CUT_OFF)
        .unwrap_or_else(|e| panic!("{}", e.code()));
    assert!(passes_as_defined(&report).is_ok());
    if let Ok(card) = report.groups[0].outcome.as_mut() {
        card.passed = !card.passed;
    }
    let flipped = report.passed();
    report.groups.push(report.groups[0].clone());
    if let Ok(card) = report.groups[1].outcome.as_mut() {
        card.passed = !flipped;
    }
    assert!(!report.passed(), "one failing group fails the evaluation");
    assert!(passes_as_defined(&report).is_ok());
}
