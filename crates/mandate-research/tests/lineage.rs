//! Hand-calculated lineage folds: the revision cap, retirement, and the three cases that pin what
//! retirement does and does not remove (MC-N17 to MC-N19, MC-N24, MC-N27, MC-N28).
//!
//! Each fold is recomputed from §8.6 before it is asserted. The cap comes from
//! `behavior.research.max_revisions_per_lineage`, and the fixture bases give two values: 3
//! (`research_equity`) and 1 (`research_equity_cap_one`).

mod common;

use common::{
    FoldScenario, INSTRUMENT_5, asset, lineage_id, revision, thesis, thesis_id, universe_of,
};
use mandate_research::{
    AdmissionChange, AdmissionDecision, Direction, RefusalReason, ResearchEvent, UniverseChange,
    UniverseChangeReason, fold_theses,
};

/// MC-N17: revision 0 admits and enters; revisions 1, 2, and 3 are renewals of the same instrument,
/// so each journals only its `ThesisRevised`; revision 4 is over the cap of 3, so it is refused with
/// `lineage_retired` and the retirement removes the instrument in the same step.
#[test]
fn mc_n17_revisions_one_to_three_are_admitted_and_the_fourth_retires_the_lineage() {
    let f = FoldScenario::new(
        3,
        vec![
            thesis("th-20", INSTRUMENT_5),
            revision("th-21", "th-20", 1, "th-20", INSTRUMENT_5),
            revision("th-22", "th-20", 2, "th-21", INSTRUMENT_5),
            revision("th-23", "th-20", 3, "th-22", INSTRUMENT_5),
            revision("th-24", "th-20", 4, "th-23", INSTRUMENT_5),
        ],
    );

    let fold = fold_theses(&f.input()).expect("the fold decides");
    let verdicts: Vec<(bool, Option<RefusalReason>, u32, bool, usize)> = fold
        .steps
        .iter()
        .map(|s| {
            (
                s.decision.admitted(),
                s.decision.reason(),
                s.lineage_revisions,
                s.lineage_retired,
                s.journal.len(),
            )
        })
        .collect();
    assert_eq!(
        verdicts,
        vec![
            (true, None, 0, false, 2),
            (true, None, 1, false, 1),
            (true, None, 2, false, 1),
            (true, None, 3, false, 1),
            (false, Some(RefusalReason::LineageRetired), 3, true, 2),
        ],
        "the highest admitted revision is 3, and only the entry step and the retiring step journal two events"
    );
    assert_eq!(
        fold.universe,
        universe_of(&[]),
        "retirement removes the instrument the lineage held, so the universe ends empty"
    );
    let lineage = fold
        .lineages
        .lineage(&lineage_id("th-20"))
        .expect("the fold records the lineage it walked");
    assert_eq!(
        (lineage.revisions, lineage.admitted, lineage.retired),
        (3, 4, true),
        "`revisions` is the highest admitted revision and `admitted` counts the four admissions"
    );
    assert_eq!(
        fold.lineages.holder_of(&lineage_id("th-20")),
        Some(&asset(INSTRUMENT_5)),
        "a retired lineage keeps its holder entry: the map records what it held, not what the universe holds"
    );
}

/// The retiring step's second event is the removal, with the size counted after it.
#[test]
fn mc_n17_the_retiring_step_journals_the_removal_with_the_size_after() {
    let f = FoldScenario::new(
        1,
        vec![
            thesis("th-50", INSTRUMENT_5),
            revision("th-51", "th-50", 1, "th-50", INSTRUMENT_5),
            revision("th-52", "th-50", 2, "th-51", INSTRUMENT_5),
        ],
    );

    let fold = fold_theses(&f.input()).expect("the fold decides");
    let last = fold.steps.last().expect("the fold walked three theses");
    let ResearchEvent::UniverseChanged(change) = &last.journal[1] else {
        panic!("a retirement journals the thesis entry, then the removal");
    };
    assert_eq!(
        (
            change.change,
            change.reason,
            change.universe_size_after,
            change.thesis_id.clone()
        ),
        (
            UniverseChange::Removed,
            UniverseChangeReason::LineageRetired,
            0,
            thesis_id("th-52")
        ),
        "MC-N24: the removal names the refused thesis and the size after the removal"
    );
}

/// MC-N24: the cap-one base, where revision 2 retires and removes.
#[test]
fn mc_n24_retirement_removes_the_instrument_it_holds() {
    let f = FoldScenario::new(
        1,
        vec![
            thesis("th-50", INSTRUMENT_5),
            revision("th-51", "th-50", 1, "th-50", INSTRUMENT_5),
            revision("th-52", "th-50", 2, "th-51", INSTRUMENT_5),
        ],
    );

    let fold = fold_theses(&f.input()).expect("the fold decides");
    assert_eq!(
        fold.universe,
        universe_of(&[]),
        "the platform has failed on the idea `max_revisions_per_lineage` times, so the position gets no path back"
    );
    let lineage = fold
        .lineages
        .lineage(&lineage_id("th-50"))
        .expect("the fold records the lineage");
    assert_eq!(
        (lineage.revisions, lineage.admitted, lineage.retired),
        (1, 2, true),
        "the cap-one lineage admitted revisions 0 and 1 before retiring"
    );
}

/// MC-N27: the over-cap revision also has a forbidden direction, so check 1 decides and retirement
/// never happens. Retirement follows the journaled reason, never the revision number.
#[test]
fn mc_n27_an_over_cap_revision_an_earlier_check_refuses_retires_nothing() {
    let mut f = FoldScenario::new(
        1,
        vec![
            thesis("th-60", INSTRUMENT_5),
            revision("th-61", "th-60", 1, "th-60", INSTRUMENT_5),
            revision("th-62", "th-60", 2, "th-61", INSTRUMENT_5),
        ],
    );
    if let Some(last) = f.proposals.last_mut() {
        last.thesis.direction = Direction::Other;
    }

    let fold = fold_theses(&f.input()).expect("the fold decides");
    let last = fold.steps.last().expect("the fold walked three theses");
    assert_eq!(
        (last.decision.reason(), last.lineage_retired),
        (Some(RefusalReason::DirectionNotAllowed), false),
        "check 1 is lower-numbered than check 16, so the thesis never reached the cap"
    );
    assert_eq!(
        fold.universe,
        universe_of(&[INSTRUMENT_5]),
        "nothing was removed, because nothing retired"
    );
    let lineage = fold
        .lineages
        .lineage(&lineage_id("th-60"))
        .expect("the fold records the lineage");
    assert_eq!(
        (lineage.revisions, lineage.admitted, lineage.retired),
        (1, 2, false),
        "the lineage is still live at revision 1"
    );
    assert_eq!(
        last.journal.len(),
        1,
        "an ignored output journals its entry and no universe change"
    );
}

/// MC-N28: another lineage renews the same instrument before the first one retires, so the retirement
/// has nothing to remove.
#[test]
fn mc_n28_retirement_never_removes_an_instrument_another_lineage_now_holds() {
    let f = FoldScenario::new(
        1,
        vec![
            thesis("th-70", INSTRUMENT_5),
            revision("th-71", "th-70", 1, "th-70", INSTRUMENT_5),
            thesis("th-80", INSTRUMENT_5),
            revision("th-72", "th-70", 2, "th-71", INSTRUMENT_5),
        ],
    );

    let fold = fold_theses(&f.input()).expect("the fold decides");
    let third = fold.steps.get(2).expect("the fold walked four theses");
    assert_eq!(
        third.decision,
        AdmissionDecision::Admitted {
            change: AdmissionChange::Renewed
        },
        "a different lineage's thesis for an active instrument is a renewal"
    );
    let last = fold.steps.last().expect("the fold walked four theses");
    assert_eq!(
        (
            last.decision.reason(),
            last.lineage_retired,
            last.journal.len()
        ),
        (Some(RefusalReason::LineageRetired), true, 1),
        "the lineage retires but removes nothing, so its step journals the entry alone"
    );
    assert_eq!(
        fold.universe,
        universe_of(&[INSTRUMENT_5]),
        "th-80 holds instrument 5 now, so th-70's retirement leaves it alone"
    );
    assert_eq!(
        fold.lineages.holder_of(&lineage_id("th-70")),
        None,
        "th-70 lost its holder entry when th-80's thesis was admitted"
    );
    assert_eq!(
        fold.lineages.holder_of(&lineage_id("th-80")),
        Some(&asset(INSTRUMENT_5)),
        "the holder moved to the lineage whose thesis is current"
    );
    assert_eq!(
        fold.lineages.held_by(&asset(INSTRUMENT_5)),
        Some(&lineage_id("th-80")),
        "the reverse lookup agrees, so no two lineages can claim one instrument"
    );
}

/// MC-N18: two admissions in one lineage, and no step ever carries a score forward (MI-18).
#[test]
fn mc_n18_a_revision_never_carries_its_predecessor_score_forward() {
    let f = FoldScenario::new(
        3,
        vec![
            thesis("th-30", INSTRUMENT_5),
            revision("th-31", "th-30", 1, "th-30", INSTRUMENT_5),
        ],
    );

    let fold = fold_theses(&f.input()).expect("the fold decides");
    assert!(
        fold.steps.iter().all(|s| !s.score_carried_forward()),
        "DEC-111 item 2: a revision starts with no track record, and no API here can give it one"
    );
    let lineage = fold
        .lineages
        .lineage(&lineage_id("th-30"))
        .expect("the fold records the lineage");
    assert_eq!(
        (lineage.revisions, lineage.admitted, lineage.retired),
        (1, 2, false),
        "MC-N18's lineage ends live at revision 1 with two admissions"
    );
}

/// MC-N19: a revision with no predecessor is ignored, and the lineage still appears in the fold's
/// state at zero — the step ran, so the lineage is known, but nothing was admitted.
#[test]
fn mc_n19_a_revision_without_a_predecessor_id_is_ignored_and_admits_nothing() {
    let mut f = FoldScenario::new(
        3,
        vec![revision("th-40", "th-40", 1, "th-39", INSTRUMENT_5)],
    );
    if let Some(first) = f.proposals.first_mut() {
        first.thesis.predecessor_thesis_id = None;
    }

    let fold = fold_theses(&f.input()).expect("the fold decides");
    let only = fold.steps.first().expect("the fold walked one thesis");
    assert_eq!(
        (only.decision.reason(), only.decision.ignored()),
        (Some(RefusalReason::RevisionWithoutPredecessor), true),
        "check 3 requires a predecessor exactly when revision > 0"
    );
    let lineage = fold
        .lineages
        .lineage(&lineage_id("th-40"))
        .expect("the step ran, so the lineage is known");
    assert_eq!(
        (lineage.revisions, lineage.admitted, lineage.retired),
        (0, 0, false),
        "an ignored thesis leaves its lineage at zero rather than crediting the revision"
    );
    assert_eq!(
        fold.lineages.holder_of(&lineage_id("th-40")),
        None,
        "nothing was admitted, so the lineage holds nothing"
    );
    assert_eq!(fold.universe, universe_of(&[]), "a refusal admits nothing");
}

/// A lineage that retires twice in one fold removes once: `retired` is already true the second time.
#[test]
fn a_lineage_retires_and_removes_only_once() {
    let f = FoldScenario::new(
        1,
        vec![
            thesis("th-90", INSTRUMENT_5),
            revision("th-91", "th-90", 2, "th-90", INSTRUMENT_5),
            revision("th-92", "th-90", 3, "th-91", INSTRUMENT_5),
        ],
    );

    let fold = fold_theses(&f.input()).expect("the fold decides");
    let removals = fold
        .steps
        .iter()
        .flat_map(|s| s.journal.iter())
        .filter(|e| {
            matches!(
                e,
                ResearchEvent::UniverseChanged(c) if c.change == UniverseChange::Removed
            )
        })
        .count();
    assert_eq!(
        removals, 1,
        "the second over-cap revision finds the lineage already retired, so a stale holder cannot remove twice"
    );
}

/// A fold naming one thesis twice is an input error, not a silent double admission.
#[test]
fn a_fold_that_names_one_thesis_twice_is_an_error() {
    let f = FoldScenario::new(
        3,
        vec![thesis("th-20", INSTRUMENT_5), thesis("th-20", INSTRUMENT_5)],
    );

    let e = fold_theses(&f.input()).expect_err("a duplicate thesis id cannot be folded");
    assert_eq!(
        e.code(),
        "duplicate_thesis_id",
        "a replay must not be able to credit one thesis twice"
    );
}
