//! DEC-2249 item 1: `LineageState::replay` applies a recorded admission decision to its lineage,
//! never judging, so the cell's research source can rebuild an agent's lineages from its journal
//! with the core's one implementation. Every expected state below is recomputed from §8.6 by hand.

mod common;

use common::{INSTRUMENT_1, INSTRUMENT_5, asset, lineage_id};
use mandate_research::{AdmissionChange, AdmissionDecision, Lineage, LineageState, RefusalReason};

fn admitted() -> AdmissionDecision {
    AdmissionDecision::Admitted {
        change: AdmissionChange::Admitted,
    }
}

fn renewed() -> AdmissionDecision {
    AdmissionDecision::Admitted {
        change: AdmissionChange::Renewed,
    }
}

fn refused(reason: RefusalReason) -> AdmissionDecision {
    AdmissionDecision::Refused { reason }
}

fn lineage(revisions: u32, admitted: u32, retired: bool) -> Lineage {
    Lineage {
        revisions,
        admitted,
        retired,
    }
}

/// MC-N17's sequence, replayed from its recorded decisions: revisions 0 to 3 admitted, then the
/// fourth refused `lineage_retired`. Each step returns the lineage after it, and only the retiring
/// step reports that it retired the lineage.
#[test]
fn mc_n17_replayed_raises_the_revision_counts_admissions_and_retires_once() {
    let mut state = LineageState::default();
    let (l, x) = (lineage_id("th-20"), asset(INSTRUMENT_5));
    let steps = [
        (0, admitted()),
        (1, renewed()),
        (2, renewed()),
        (3, renewed()),
        (4, refused(RefusalReason::LineageRetired)),
    ];
    let after: Vec<(Lineage, bool)> = steps
        .into_iter()
        .map(|(revision, decision)| state.replay(&l, revision, &x, decision))
        .collect();
    assert_eq!(
        after,
        [
            (lineage(0, 1, false), false),
            (lineage(1, 2, false), false),
            (lineage(2, 3, false), false),
            (lineage(3, 4, false), false),
            (lineage(3, 4, true), true),
        ],
        "the refused fourth revision raises nothing and counts nothing; it only retires"
    );
    assert_eq!(state.lineage(&l), Some(&lineage(3, 4, true)));
    assert_eq!(
        state.holder_of(&l),
        Some(&x),
        "a retired lineage keeps its holder entry"
    );
}

/// MC-N19: a refusal other than `lineage_retired` leaves the lineage as it was, though known, and
/// gives it no holder.
#[test]
fn another_refusal_makes_the_lineage_known_and_changes_nothing() {
    let mut state = LineageState::default();
    let (l, x) = (lineage_id("th-30"), asset(INSTRUMENT_5));
    let step = state.replay(&l, 0, &x, refused(RefusalReason::NotInDataUniverse));
    assert_eq!(step, (lineage(0, 0, false), false));
    assert_eq!(state.lineage(&l), Some(&lineage(0, 0, false)));
    assert_eq!(state.holder_of(&l), None);
}

/// An admission moves the instrument's holder to the admitting lineage: the lineage that held it
/// before loses its holder entry, and a lineage on another instrument keeps its own.
#[test]
fn an_admission_moves_the_instrument_s_holder_to_its_lineage() {
    let mut state = LineageState::default();
    let (a, b, c) = (lineage_id("th-a"), lineage_id("th-b"), lineage_id("th-c"));
    let (x, y) = (asset(INSTRUMENT_5), asset(INSTRUMENT_1));
    state.replay(&a, 0, &x, admitted());
    state.replay(&c, 0, &y, admitted());
    state.replay(&b, 0, &x, admitted());
    assert_eq!(
        state.holder_of(&a),
        None,
        "a no longer holds the instrument"
    );
    assert_eq!(state.holder_of(&b), Some(&x));
    assert_eq!(
        state.holder_of(&c),
        Some(&y),
        "c's own instrument is untouched"
    );
    assert_eq!(state.held_by(&x), Some(&b));
    assert_eq!(
        state.lineage(&a),
        Some(&lineage(0, 1, false)),
        "a keeps its count"
    );
}

/// A second `lineage_retired` refusal on a retired lineage changes nothing and does not report a
/// retirement again, and a refusal never moves a holder.
#[test]
fn a_lineage_retires_only_once_and_a_refusal_moves_no_holder() {
    let mut state = LineageState::default();
    let (a, b, x) = (lineage_id("th-a"), lineage_id("th-b"), asset(INSTRUMENT_5));
    state.replay(&a, 0, &x, admitted());
    let retired = refused(RefusalReason::LineageRetired);
    assert_eq!(
        state.replay(&a, 2, &x, retired),
        (lineage(0, 1, true), true)
    );
    assert_eq!(
        state.replay(&a, 3, &x, retired),
        (lineage(0, 1, true), false)
    );
    state.replay(&b, 0, &x, refused(RefusalReason::UniverseFull));
    assert_eq!(
        state.held_by(&x),
        Some(&a),
        "b's refusal took nothing from a"
    );
}
