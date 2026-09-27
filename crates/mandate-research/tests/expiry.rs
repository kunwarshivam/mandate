//! Hand-calculated removals: the horizon, invalidation before it, and a retired lineage
//! (MC-N20 to MC-N22), plus the boundary and ordering rules those three cases fix.

mod common;

use common::{
    EXPIRES_AT, INSTRUMENT_2, INSTRUMENT_5, asset, at, entry, lineage_id, thesis_id, universe_of,
};
use mandate_research::{
    InstrumentRestriction, Lineage, LineageState, ResearchEvent, UniverseChange,
    UniverseChangeReason, expire_theses,
};

/// A lineage state in which one lineage has retired.
fn retired(lineage: &str) -> LineageState {
    LineageState::from_parts(
        [(
            lineage_id(lineage),
            Lineage {
                revisions: 3,
                admitted: 4,
                retired: true,
            },
        )]
        .into_iter()
        .collect(),
        [].into_iter().collect(),
    )
}

/// MC-N20: `now` equals `expires_at`, so the comparison is inclusive and the instrument goes.
#[test]
#[ignore = "pending E17-3"]
fn mc_n20_a_thesis_at_its_horizon_removes_its_instrument() {
    let entries = [entry(INSTRUMENT_5, "th-1", "th-1", EXPIRES_AT, false)];

    let e = expire_theses(at(EXPIRES_AT), &entries, &LineageState::default())
        .expect("the removal is decided");
    assert_eq!(
        e.universe,
        universe_of(&[]),
        "DEC-118: at its horizon a thesis is not renewed, and its instrument becomes removed"
    );
    assert_eq!(e.removed, vec![asset(INSTRUMENT_5)]);
    assert_eq!(
        e.instrument_restrictions.get(&asset(INSTRUMENT_5)),
        Some(&InstrumentRestriction::RemovedInstrument),
        "a removed instrument is exits-only in that instrument (§2.3)"
    );
    let ResearchEvent::UniverseChanged(change) = &e.journal[0] else {
        panic!("a removal journals UniverseChanged");
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
            UniverseChangeReason::ThesisExpired,
            0,
            thesis_id("th-1")
        ),
        "the reason is thesis_expired and the size is counted after the removal"
    );
}

/// One nanosecond before the horizon the thesis is current, which is what makes the comparison's
/// direction visible.
#[test]
#[ignore = "pending E17-3"]
fn the_horizon_removes_at_exactly_the_horizon_and_not_before() {
    let entries = [entry(INSTRUMENT_5, "th-1", "th-1", EXPIRES_AT, false)];
    let just_before = at("2026-09-23T13:59:59.999999999Z");

    let e = expire_theses(just_before, &entries, &LineageState::default())
        .expect("the removal is decided");
    assert_eq!(
        e.universe,
        universe_of(&[INSTRUMENT_5]),
        "a thesis is current from admission until expires_at, so one nanosecond earlier it stays"
    );
    assert!(e.removed.is_empty(), "nothing has ended yet");
    assert!(
        e.journal.is_empty(),
        "nothing changed, so nothing is journaled"
    );
}

/// MC-N21: twenty hours of horizon left, but the invalidation condition holds, so it goes at once.
#[test]
#[ignore = "pending E17-3"]
fn mc_n21_an_invalidated_thesis_removes_at_once_before_its_horizon() {
    let entries = [entry(INSTRUMENT_5, "th-1", "th-1", EXPIRES_AT, true)];

    let e = expire_theses(
        at("2026-09-22T18:00:00.000000000Z"),
        &entries,
        &LineageState::default(),
    )
    .expect("the removal is decided");
    let ResearchEvent::UniverseChanged(change) = &e.journal[0] else {
        panic!("a removal journals UniverseChanged");
    };
    assert_eq!(
        change.reason,
        UniverseChangeReason::ThesisInvalidated,
        "§8.6: when an invalidation condition holds the instrument becomes removed, whatever the horizon"
    );
    assert_eq!(e.universe, universe_of(&[]));
}

/// MC-N22: two unexpired theses, one of whose lineages retired. Retirement is read from the folded
/// lineage state, never from a per-entry flag.
#[test]
#[ignore = "pending E17-9"]
fn mc_n22_a_retired_lineage_removes_its_instrument_and_an_unexpired_thesis_stays() {
    let entries = [
        entry(INSTRUMENT_5, "th-1", "th-1", EXPIRES_AT, false),
        entry(INSTRUMENT_2, "th-2", "th-2", EXPIRES_AT, false),
    ];

    let e = expire_theses(
        at("2026-09-22T18:00:00.000000000Z"),
        &entries,
        &retired("th-1"),
    )
    .expect("the removal is decided");
    assert_eq!(
        e.universe,
        universe_of(&[INSTRUMENT_2]),
        "only the retired lineage's instrument goes"
    );
    assert_eq!(e.removed, vec![asset(INSTRUMENT_5)]);
    let ResearchEvent::UniverseChanged(change) = &e.journal[0] else {
        panic!("a removal journals UniverseChanged");
    };
    assert_eq!(
        (change.reason, change.universe_size_after),
        (UniverseChangeReason::LineageRetired, 1),
        "one instrument stays, so the size after the removal is 1"
    );
    assert_eq!(
        e.instrument_restrictions.len(),
        1,
        "MI-19: removal restricts that instrument only, never the agent and never a sibling"
    );
}

/// The reason order, shown where all three conditions hold at once: invalidated wins over a retired
/// lineage, which wins over an expired horizon.
#[test]
#[ignore = "pending E17-9"]
fn the_removal_reason_is_the_first_that_holds() {
    let entries = [entry(INSTRUMENT_5, "th-1", "th-1", EXPIRES_AT, true)];

    let e =
        expire_theses(at(EXPIRES_AT), &entries, &retired("th-1")).expect("the removal is decided");
    let ResearchEvent::UniverseChanged(change) = &e.journal[0] else {
        panic!("a removal journals UniverseChanged");
    };
    assert_eq!(
        change.reason,
        UniverseChangeReason::ThesisInvalidated,
        "invalidation is the first reason §8.6 lists, so it is the one journaled"
    );
}

/// A retired lineage outranks an expired horizon, which the invalidated case alone would not show.
#[test]
#[ignore = "pending E17-9"]
fn a_retired_lineage_outranks_an_expired_horizon() {
    let entries = [entry(INSTRUMENT_5, "th-1", "th-1", EXPIRES_AT, false)];

    let e =
        expire_theses(at(EXPIRES_AT), &entries, &retired("th-1")).expect("the removal is decided");
    let ResearchEvent::UniverseChanged(change) = &e.journal[0] else {
        panic!("a removal journals UniverseChanged");
    };
    assert_eq!(
        change.reason,
        UniverseChangeReason::LineageRetired,
        "the lineage has no path back, which is the stronger statement about the position"
    );
}

/// Several removals at once count down: each event's size is the universe after that removal.
#[test]
#[ignore = "pending E17-3"]
fn several_removals_count_the_universe_down() {
    let entries = [
        entry(INSTRUMENT_2, "th-2", "th-2", EXPIRES_AT, false),
        entry(INSTRUMENT_5, "th-1", "th-1", EXPIRES_AT, false),
    ];

    let e = expire_theses(at(EXPIRES_AT), &entries, &LineageState::default())
        .expect("the removal is decided");
    let sizes: Vec<usize> = e
        .journal
        .iter()
        .filter_map(|event| match event {
            ResearchEvent::UniverseChanged(c) => Some(c.universe_size_after),
            _ => None,
        })
        .collect();
    assert_eq!(
        sizes,
        vec![1, 0],
        "two instruments leave a two-instrument universe, so the sizes after are 1 then 0"
    );
    assert_eq!(
        e.removed,
        vec![asset(INSTRUMENT_2), asset(INSTRUMENT_5)],
        "the walk is ordered by instrument id, so the same inputs always give the same events"
    );
}

/// The walk's order does not depend on the order the entries arrive in (ES-21).
#[test]
#[ignore = "pending E17-3"]
fn the_removal_order_does_not_depend_on_the_input_order() {
    let forwards = [
        entry(INSTRUMENT_2, "th-2", "th-2", EXPIRES_AT, false),
        entry(INSTRUMENT_5, "th-1", "th-1", EXPIRES_AT, false),
    ];
    let backwards = [
        entry(INSTRUMENT_5, "th-1", "th-1", EXPIRES_AT, false),
        entry(INSTRUMENT_2, "th-2", "th-2", EXPIRES_AT, false),
    ];

    let a = expire_theses(at(EXPIRES_AT), &forwards, &LineageState::default())
        .expect("the removal is decided");
    let b = expire_theses(at(EXPIRES_AT), &backwards, &LineageState::default())
        .expect("the removal is decided");
    assert_eq!(
        a, b,
        "a replay reproduces the events whatever order the fold handed the entries over in"
    );
}

/// An entry list naming one instrument twice is an input error: MI-15 forbids a duplicate.
#[test]
#[ignore = "pending E17-3"]
fn an_entry_list_holding_one_instrument_twice_is_an_error() {
    let entries = [
        entry(INSTRUMENT_5, "th-1", "th-1", EXPIRES_AT, false),
        entry(INSTRUMENT_5, "th-2", "th-2", EXPIRES_AT, false),
    ];

    let e = expire_theses(at(EXPIRES_AT), &entries, &LineageState::default())
        .expect_err("a duplicate instrument cannot be expired");
    assert_eq!(
        e.code(),
        "duplicate_instrument",
        "MI-15: the working universe holds no duplicate, so neither may its entry list"
    );
}
