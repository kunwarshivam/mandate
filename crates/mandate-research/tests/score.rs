//! E17-8's forward-paper evaluator: hand-calculated scoring cases, every expected figure an
//! exact decimal recomputable from DEC-281's rules with integer arithmetic alone.
//!
//! The aggregate scenario is five Long theses over `[t(100), t(1000)]` with no round-trip cost,
//! every instrument closing `100` at its entry edge, so the net returns are
//! `[0.25, 0.15, 0.05, 0.05, 0.0]`: the mean is `0.1`, the sample variance
//! `(5 × 0.09 − 0.25) ÷ 20 = 0.01`, so σ is exactly `0.1`, and with the registered `z = 1.645`
//! the margin is `root_ceiling(0.1645² ÷ 5) = root_ceiling(0.00541205) = 0.07356663646` (the
//! smallest 12-digit value whose square reaches it: `0.073566636459² = 0.005412049999890668…`
//! does not), so the bound is `0.1 − 0.07356663646 = 0.02643336354`. A floored root at either
//! place — the outer root here, σ's root in the non-square-variance case below — moves a figure
//! the tests pin exactly, and a division by anything but the count moves every figure.
//!
//! Every test is pending until its story lands (DEC-77), and every one fails on the stubs
//! because the stubs return an error where the case expects a figure or a refusal it does not
//! make.

use std::cell::Cell;
use std::collections::BTreeMap;

use mandate_num::{NumError, Price, Ratio, Rounding, SignedQty};
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};

use mandate_research::score::{
    CloseSeries, ClosedThesis, EvaluationDecision, EvaluationInput, EvaluationWindow,
    ObservedClose, UnscoreableReason, basket_return, buy_and_hold, entry_close, evaluate,
    exit_close,
};
use mandate_research::{AssetId, Direction, LineageId, ResearchError, ThesisId};
use mandate_time::UtcNanos;

/// An instant at a whole second past the epoch.
fn t(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("a fixture instant is in range")
}

/// A close price.
fn p(text: &str) -> Price {
    Price::parse(text).expect("a fixture price parses")
}

/// An exact decimal figure.
fn r(text: &str) -> Ratio {
    Ratio::parse(text).expect("a fixture ratio parses")
}

/// One observed close at a whole second.
fn close(secs: i64, price: &str) -> ObservedClose {
    ObservedClose {
        at: t(secs),
        price: p(price),
    }
}

/// A close series from `(second, price)` entries, in order.
fn series(id: &str, entries: &[(i64, &str)]) -> CloseSeries {
    let closes = entries
        .iter()
        .map(|(secs, price)| close(*secs, price))
        .collect();
    CloseSeries::new(asset(id), closes).expect("a fixture series is well formed")
}

/// An instrument id, in the tests' own words.
fn asset(id: &str) -> AssetId {
    AssetId::new(id).expect("a fixture asset id is not empty")
}

/// One Long closed thesis from revision 0, over `[as_of, horizon]`, with its modeled cost.
fn thesis(id: &str, instrument: &str, as_of: i64, horizon: i64, cost: &str) -> ClosedThesis {
    ClosedThesis {
        thesis: ThesisId::new(id).expect("a fixture thesis id is not empty"),
        lineage: LineageId::new(id).expect("a fixture lineage id is not empty"),
        revision: 0,
        instrument: asset(instrument),
        direction: Direction::Long,
        as_of: t(as_of),
        horizon_end: t(horizon),
        round_trip_cost: r(cost),
    }
}

/// The registered decision over `[t(0), t(10_000)]`, the minimum the case names, and
/// DEC-122's `z` for 95%.
fn decision(minimum_scoreable: u32) -> EvaluationDecision {
    EvaluationDecision {
        window: EvaluationWindow {
            from: t(0),
            to: t(10_000),
        },
        minimum_scoreable,
        z: r("1.645"),
    }
}

/// The five-thesis aggregate scenario: nets `[0.25, 0.15, 0.05, 0.05, 0.0]` against a flat
/// two-member basket, and an index whose exit close the case names (`400` flat, `420`
/// for a 0.05 baseline that one more thesis beats and one fewer does not).
fn five_theses(
    index_exit: &str,
) -> (
    Vec<ClosedThesis>,
    BTreeMap<AssetId, CloseSeries>,
    Vec<CloseSeries>,
    CloseSeries,
) {
    let exits = ["125", "115", "105", "105", "100"];
    let mut theses = Vec::with_capacity(exits.len());
    let mut instruments = BTreeMap::new();
    for (index, exit) in exits.iter().enumerate() {
        let number = index + 1;
        theses.push(thesis(
            &format!("th-{number}"),
            &format!("asset-{number}"),
            100,
            1_000,
            "0",
        ));
        let id = format!("asset-{number}");
        instruments.insert(asset(&id), series(&id, &[(101, "100"), (999, exit)]));
    }
    let basket = vec![
        series("basket-1", &[(101, "100"), (999, "100")]),
        series("basket-2", &[(101, "200"), (999, "200")]),
    ];
    let index_series = series("index", &[(101, "400"), (999, index_exit)]);
    (theses, instruments, basket, index_series)
}

/// One evaluation input over the scenario pieces.
fn input<'a>(
    decision: &'a EvaluationDecision,
    theses: &'a [ClosedThesis],
    instruments: &'a BTreeMap<AssetId, CloseSeries>,
    basket: &'a [CloseSeries],
    index: &'a CloseSeries,
) -> EvaluationInput<'a> {
    EvaluationInput {
        decision,
        theses,
        instruments,
        basket,
        index,
    }
}

#[test]
fn a_series_refuses_unordered_or_empty_closes() {
    let ordered = series("asset-a", &[(100, "10"), (200, "11")]);
    assert_eq!(
        ordered,
        series("asset-a", &[(100, "10"), (200, "11")]),
        "an ordered series builds"
    );
    assert!(matches!(
        CloseSeries::new(asset("asset-a"), Vec::new()),
        Err(ResearchError::EmptyCloses)
    ));
    assert!(
        matches!(
            CloseSeries::new(asset("asset-a"), vec![close(200, "11"), close(200, "11")],),
            Err(ResearchError::ClosesOutOfOrder)
        ),
        "a repeated instant is ambiguous for the boundary rules"
    );
    assert!(matches!(
        CloseSeries::new(asset("asset-a"), vec![close(300, "12"), close(200, "11")],),
        Err(ResearchError::ClosesOutOfOrder)
    ));
}

#[test]
fn the_entry_close_is_the_first_strictly_after_as_of() {
    let series = series("asset-a", &[(100, "10"), (200, "11"), (300, "12")]);
    assert_eq!(
        entry_close(&series, t(150)).expect("a close after 150 exists"),
        p("11")
    );
    assert_eq!(
        entry_close(&series, t(200)).expect("a close after 200 exists"),
        p("12"),
        "a close at the same instant is the model's own, not a price it could act on"
    );
    assert_eq!(
        entry_close(&series, t(299)).expect("a close after 299 exists"),
        p("12")
    );
}

#[test]
fn the_exit_close_is_the_last_at_or_before_the_horizon() {
    let series = series("asset-a", &[(100, "10"), (200, "11"), (300, "12")]);
    assert_eq!(
        exit_close(&series, t(200)).expect("a close at or before 200 exists"),
        p("11"),
        "the close at the horizon itself is the exit"
    );
    assert_eq!(
        exit_close(&series, t(250)).expect("a close at or before 250 exists"),
        p("11"),
        "a close past the horizon is never read"
    );
    assert_eq!(
        exit_close(&series, t(100_000)).expect("a close at or before 100000 exists"),
        p("12")
    );
}

#[test]
fn a_window_edge_without_a_close_on_its_side_is_an_error() {
    let series = series("asset-a", &[(100, "10"), (200, "11")]);
    assert!(
        matches!(
            entry_close(&series, t(200)),
            Err(ResearchError::NoCloseAfter)
        ),
        "no close strictly after the last one: the entry edge has no price"
    );
    assert!(
        matches!(
            exit_close(&series, t(99)),
            Err(ResearchError::NoCloseOnOrBefore)
        ),
        "no close at or before the first one: the exit edge has no price"
    );
}

#[test]
fn buy_and_hold_rounds_once_at_twelve_places() {
    let falling = series("asset-a", &[(101, "3"), (999, "1")]);
    assert_eq!(
        buy_and_hold(&falling, t(100), t(1_000)).expect("both edges have closes"),
        r("-0.666666666667"),
        "(1 − 3) ÷ 3 at 12 places half_even: the 13th digit is 6, so it rounds up"
    );
    let rising = series("asset-b", &[(101, "10"), (999, "11")]);
    assert_eq!(
        buy_and_hold(&rising, t(100), t(1_000)).expect("both edges have closes"),
        r("0.1")
    );
}

#[test]
fn the_basket_is_the_equal_weighted_mean_of_its_members() {
    let first = series("basket-1", &[(101, "10"), (999, "11")]);
    let second = series("basket-2", &[(101, "100"), (999, "98")]);
    assert_eq!(
        basket_return(&[first, second], t(100), t(1_000)).expect("both members score"),
        r("0.04"),
        "(0.1 + (−0.02)) ÷ 2: each member's own window return, then the mean"
    );
}

#[test]
fn a_long_thesis_scores_its_window_net_of_the_round_trip_cost() {
    let cost = thesis("th-1", "asset-a", 100, 1_000, "0.01");
    let instruments = BTreeMap::from([(
        asset("asset-a"),
        series("asset-a", &[(101, "100"), (999, "110")]),
    )]);
    let basket = vec![series("basket-1", &[(101, "100"), (999, "100")])];
    let index = series("index", &[(101, "400"), (999, "400")]);
    let registered = decision(1);
    let card = evaluate(&input(&registered, &[cost], &instruments, &basket, &index))
        .expect("one scoreable thesis meets the minimum");
    let row = card.theses.first().expect("the thesis was scored");
    assert_eq!(row.entry, p("100"), "the first close strictly after as_of");
    assert_eq!(
        row.exit,
        p("110"),
        "the last close at or before the horizon"
    );
    assert_eq!(
        row.net_return,
        r("0.09"),
        "the window return 0.1 less the round-trip cost 0.01"
    );
    assert_eq!(card.scoreable_count, 1);
    assert_eq!(card.mean_excess_basket, r("0.09"));
}

#[test]
fn excess_subtracts_each_baseline_over_the_same_window() {
    let one = thesis("th-1", "asset-a", 100, 1_000, "0.01");
    let instruments = BTreeMap::from([(
        asset("asset-a"),
        series("asset-a", &[(101, "100"), (999, "110")]),
    )]);
    let basket = vec![series("basket-1", &[(101, "100"), (999, "100")])];
    let index = series("index", &[(101, "400"), (999, "408")]);
    let registered = decision(1);
    let card = evaluate(&input(&registered, &[one], &instruments, &basket, &index))
        .expect("one scoreable thesis meets the minimum");
    let row = card.theses.first().expect("the thesis was scored");
    assert_eq!(
        row.excess_over_basket,
        r("0.09"),
        "the flat basket's return is 0: the excess is the net return itself"
    );
    assert_eq!(
        row.excess_over_index,
        r("0.07"),
        "thesis minus baseline: 0.09 − 0.02, never the other way round"
    );
}

#[test]
fn a_non_long_thesis_is_unscoreable_and_never_counts() {
    let long = thesis("th-1", "asset-a", 100, 1_000, "0");
    let mut other = thesis("th-2", "asset-b", 100, 1_000, "0");
    other.direction = Direction::Other;
    let instruments = BTreeMap::from([
        (
            asset("asset-a"),
            series("asset-a", &[(101, "100"), (999, "120")]),
        ),
        (
            asset("asset-b"),
            series("asset-b", &[(101, "100"), (999, "100")]),
        ),
    ]);
    let basket = vec![series("basket-1", &[(101, "100"), (999, "100")])];
    let index = series("index", &[(101, "400"), (999, "400")]);
    let registered = decision(1);
    let card = evaluate(&input(
        &registered,
        &[long, other],
        &instruments,
        &basket,
        &index,
    ))
    .expect("one scoreable thesis meets the minimum of one");
    assert_eq!(card.scoreable_count, 1, "only the Long thesis scores");
    assert_eq!(
        card.unscoreable,
        vec![mandate_research::score::Unscoreable {
            thesis: ThesisId::new("th-2").expect("a fixture id"),
            reason: UnscoreableReason::NotScoreableDirection,
        }],
        "the non-Long thesis is named with its reason, never silently dropped"
    );
    assert_eq!(
        card.mean_excess_basket,
        r("0.2"),
        "the mean is over the scoreable alone: a non-Long thesis is not a quiet zero"
    );
}

#[test]
fn a_thesis_without_a_series_is_unscoreable_and_never_counts() {
    let scored = thesis("th-1", "asset-a", 100, 1_000, "0");
    let missing = thesis("th-2", "asset-b", 100, 1_000, "0");
    let instruments = BTreeMap::from([(
        asset("asset-a"),
        series("asset-a", &[(101, "100"), (999, "120")]),
    )]);
    let basket = vec![series("basket-1", &[(101, "100"), (999, "100")])];
    let index = series("index", &[(101, "400"), (999, "400")]);
    let registered = decision(1);
    let card = evaluate(&input(
        &registered,
        &[scored, missing],
        &instruments,
        &basket,
        &index,
    ))
    .expect("one scoreable thesis meets the minimum of one");
    assert_eq!(card.scoreable_count, 1);
    assert_eq!(
        card.unscoreable,
        vec![mandate_research::score::Unscoreable {
            thesis: ThesisId::new("th-2").expect("a fixture id"),
            reason: UnscoreableReason::NoSeries,
        }]
    );
    assert_eq!(card.mean_excess_basket, r("0.2"));
}

#[test]
fn a_thesis_with_no_close_on_an_edge_is_unscoreable_and_never_counts() {
    let scored = thesis("th-1", "asset-a", 100, 1_000, "0");
    let no_entry = thesis("th-2", "asset-b", 500, 1_000, "0");
    let no_exit = thesis("th-3", "asset-c", 100, 1_000, "0");
    let instruments = BTreeMap::from([
        (
            asset("asset-a"),
            series("asset-a", &[(101, "100"), (999, "120")]),
        ),
        (
            asset("asset-b"),
            series("asset-b", &[(101, "100"), (499, "100")]),
        ),
        (
            asset("asset-c"),
            series("asset-c", &[(1_001, "100"), (1_100, "100")]),
        ),
    ]);
    let basket = vec![series("basket-1", &[(101, "100"), (999, "100")])];
    let index = series("index", &[(101, "400"), (999, "400")]);
    let registered = decision(1);
    let card = evaluate(&input(
        &registered,
        &[no_exit, no_entry, scored],
        &instruments,
        &basket,
        &index,
    ))
    .expect("one scoreable thesis meets the minimum of one");
    assert_eq!(card.scoreable_count, 1);
    let named: Vec<&ThesisId> = card.unscoreable.iter().map(|each| &each.thesis).collect();
    assert_eq!(
        named,
        [
            &ThesisId::new("th-2").expect("a fixture id"),
            &ThesisId::new("th-3").expect("a fixture id")
        ],
        "both edge failures are named, in thesis order even though the input supplied them reversed — the report never depends on input order"
    );
    assert!(
        card.unscoreable
            .iter()
            .any(|each| each.reason == UnscoreableReason::NoEntryClose)
    );
    assert!(
        card.unscoreable
            .iter()
            .any(|each| each.reason == UnscoreableReason::NoExitClose)
    );
    assert_eq!(card.mean_excess_basket, r("0.2"));
}

#[test]
fn the_window_refuses_an_early_run() {
    let (theses, instruments, basket, index) = five_theses("400");
    let registered = decision(6);
    assert!(
        matches!(
            evaluate(&input(&registered, &theses, &instruments, &basket, &index)),
            Err(ResearchError::WindowNotClosed)
        ),
        "five scoreable theses against a minimum of six: no report before the window closes"
    );
}

#[test]
fn unscoreable_theses_never_count_toward_the_minimum() {
    let scored = thesis("th-1", "asset-a", 100, 1_000, "0");
    let missing = thesis("th-2", "asset-b", 100, 1_000, "0");
    let instruments = BTreeMap::from([(
        asset("asset-a"),
        series("asset-a", &[(101, "100"), (999, "120")]),
    )]);
    let basket = vec![series("basket-1", &[(101, "100"), (999, "100")])];
    let index = series("index", &[(101, "400"), (999, "400")]);
    let strict = decision(2);
    assert!(
        matches!(
            evaluate(&input(
                &strict,
                &[scored.clone(), missing],
                &instruments,
                &basket,
                &index
            )),
            Err(ResearchError::WindowNotClosed)
        ),
        "one scoreable and one unscoreable thesis against a minimum of two: the gap does not close the window"
    );
    let lenient = decision(1);
    let card = evaluate(&input(&lenient, &[scored], &instruments, &basket, &index))
        .expect("one scoreable thesis meets the minimum of one");
    assert_eq!(card.scoreable_count, 1);
}

#[test]
fn a_thesis_outside_the_registered_window_is_refused() {
    let (mut theses, instruments, basket, index) = five_theses("400");
    theses.push(thesis("th-6", "asset-6", 100, 20_000, "0"));
    let registered = decision(5);
    assert!(
        matches!(
            evaluate(&input(&registered, &theses, &instruments, &basket, &index)),
            Err(ResearchError::ThesisOutsideWindow)
        ),
        "a horizon closing at t(20000) is outside the registered window to t(10000): refused outright, even though five scoreable theses would meet the minimum"
    );
}

#[test]
fn the_lower_bound_is_the_mean_less_the_ceilinged_margin() {
    let (theses, instruments, basket, index) = five_theses("400");
    let registered = decision(2);
    let card = evaluate(&input(&registered, &theses, &instruments, &basket, &index))
        .expect("five scoreable theses meet the minimum");
    assert_eq!(card.scoreable_count, 5);
    assert_eq!(card.excess_sum_basket, r("0.5"));
    assert_eq!(card.mean_excess_basket, r("0.1"));
    assert_eq!(
        card.sample_variance_basket,
        Some(r("0.01")),
        "(5 × 0.09 − 0.25) ÷ 20"
    );
    assert_eq!(
        card.margin_basket,
        r("0.07356663646"),
        "root_ceiling(0.1645² ÷ 5) = root_ceiling(0.00541205): the smallest 12-digit value whose square reaches it — 0.073566636459² = 0.005412049999890668… does not, so a floored root reads 0.073566636459"
    );
    assert_eq!(card.lower_bound_basket, r("0.02643336354"));
    assert_eq!(card.excess_sum_index, r("0.5"));
    assert_eq!(card.mean_excess_index, r("0.1"));
    assert_eq!(card.sample_variance_index, Some(r("0.01")));
    assert_eq!(card.margin_index, r("0.07356663646"));
    assert_eq!(card.lower_bound_index, r("0.02643336354"));
    assert!(card.passed, "both bounds are above zero");
    let first = card.theses.first().expect("the first thesis was scored");
    assert_eq!(first.net_return, r("0.25"));
    assert_eq!(first.excess_over_basket, r("0.25"));
}

#[test]
fn a_single_thesis_has_no_dispersion_and_the_bound_is_the_mean() {
    let one = thesis("th-1", "asset-a", 100, 1_000, "0");
    let instruments = BTreeMap::from([(
        asset("asset-a"),
        series("asset-a", &[(101, "100"), (999, "110")]),
    )]);
    let basket = vec![series("basket-1", &[(101, "100"), (999, "100")])];
    let index = series("index", &[(101, "400"), (999, "400")]);
    let registered = decision(1);
    let card = evaluate(&input(&registered, &[one], &instruments, &basket, &index))
        .expect("one scoreable thesis meets the minimum");
    assert_eq!(card.sample_variance_basket, None, "below two theses");
    assert_eq!(card.margin_basket, r("0"), "no dispersion, no margin");
    assert_eq!(
        card.lower_bound_basket,
        r("0.1"),
        "the bound is the mean itself"
    );
    assert!(card.passed);
}

#[test]
fn pass_requires_the_bound_above_zero_against_both_baselines() {
    let (theses, instruments, basket, index) = five_theses("420");
    let registered = decision(2);
    let card = evaluate(&input(&registered, &theses, &instruments, &basket, &index))
        .expect("five scoreable theses meet the minimum");
    assert_eq!(
        card.mean_excess_index,
        r("0.05"),
        "the index returned 0.05 over the window: [0.2, 0.1, 0, 0, −0.05] averages 0.05"
    );
    assert_eq!(card.sample_variance_index, Some(r("0.01")));
    assert_eq!(card.margin_index, r("0.07356663646"));
    assert_eq!(
        card.lower_bound_index,
        r("-0.02356663646"),
        "0.05 − 0.07356663646"
    );
    assert_eq!(
        card.lower_bound_basket,
        r("0.02643336354"),
        "the basket bound still passes"
    );
    assert!(
        !card.passed,
        "DEC-122's threshold is against each baseline: one negative bound fails the evaluation"
    );
}

#[test]
fn every_scored_thesis_shows_its_lineage_s_revision_count() {
    let mut revised = thesis("th-2", "asset-2", 100, 1_000, "0");
    revised.revision = 2;
    revised.lineage = LineageId::new("th-1").expect("a fixture lineage id");
    let (mut theses, instruments, basket, index) = five_theses("400");
    theses[1] = revised;
    let registered = decision(2);
    let card = evaluate(&input(&registered, &theses, &instruments, &basket, &index))
        .expect("five scoreable theses meet the minimum");
    let row = card
        .theses
        .iter()
        .find(|row| row.thesis == ThesisId::new("th-2").expect("a fixture id"))
        .expect("the revised thesis was scored");
    assert_eq!(
        row.revision, 2,
        "MI-18: every report shows the revision count"
    );
    assert_eq!(
        row.lineage,
        LineageId::new("th-1").expect("a fixture lineage id")
    );
    assert_eq!(
        row.net_return,
        r("0.15"),
        "a revision scores like any other thesis: no predecessor's score exists to carry"
    );
    assert_eq!(card.scoreable_count, 5);
}

#[test]
fn the_scorecard_is_independent_of_the_input_order() {
    let (theses, instruments, _flat_basket, index) = five_theses("400");
    let basket = vec![
        series("basket-1", &[(101, "100"), (999, "110")]),
        series("basket-2", &[(101, "200"), (999, "196")]),
    ];
    let registered = decision(2);
    let sorted = evaluate(&input(&registered, &theses, &instruments, &basket, &index))
        .expect("five scoreable theses meet the minimum");
    let mut scrambled = theses.clone();
    scrambled.reverse();
    let mut basket_swapped = basket.clone();
    basket_swapped.reverse();
    let other = evaluate(&input(
        &registered,
        &scrambled,
        &instruments,
        &basket_swapped,
        &index,
    ))
    .expect("the same theses meet the minimum");
    assert_eq!(
        sorted, other,
        "the report is keyed by thesis, not by input order (ES-21's replay equality), and the equal-weighted basket mean does not care which member came first"
    );
    let ids: Vec<&str> = sorted
        .theses
        .iter()
        .map(|row| row.thesis.as_str())
        .collect();
    assert_eq!(ids, ["th-1", "th-2", "th-3", "th-4", "th-5"]);
}

#[test]
fn the_sigma_root_is_ceilinged_so_the_margin_is_never_understated() {
    let rising = thesis("th-1", "asset-a", 100, 1_000, "0");
    let milder = thesis("th-2", "asset-b", 100, 1_000, "0");
    let instruments = BTreeMap::from([
        (
            asset("asset-a"),
            series("asset-a", &[(101, "100"), (999, "120")]),
        ),
        (
            asset("asset-b"),
            series("asset-b", &[(101, "100"), (999, "110")]),
        ),
    ]);
    let basket = vec![series("basket-1", &[(101, "100"), (999, "100")])];
    let index = series("index", &[(101, "400"), (999, "400")]);
    let registered = EvaluationDecision {
        window: EvaluationWindow {
            from: t(0),
            to: t(10_000),
        },
        minimum_scoreable: 1,
        z: r("10"),
    };
    let card = evaluate(&input(
        &registered,
        &[rising, milder],
        &instruments,
        &basket,
        &index,
    ))
    .expect("two scoreable theses meet the minimum");
    assert_eq!(card.scoreable_count, 2);
    assert_eq!(card.mean_excess_basket, r("0.15"));
    assert_eq!(
        card.sample_variance_basket,
        Some(r("0.005")),
        "(2 × 0.05 − 0.09) ÷ 2, not a perfect square: √0.005 = 0.0707106781186547…"
    );
    assert_eq!(
        card.margin_basket,
        r("0.500000000002"),
        "σ is the 12-place ceiling root 0.070710678119 (the floor is 0.070710678118); with z = 10 the step survives every rounding, and a floored σ lands the margin on 0.499999999995 instead"
    );
    assert_eq!(
        card.lower_bound_basket,
        r("-0.350000000002"),
        "0.15 − 0.500000000002"
    );
    assert_eq!(card.margin_index, r("0.500000000002"));
    assert_eq!(card.lower_bound_index, r("-0.350000000002"));
    assert!(!card.passed);
}

#[test]
fn the_window_edges_are_inclusive() {
    let at_from = thesis("th-1", "asset-a", 100, 1_000, "0");
    let at_to = thesis("th-2", "asset-b", 100, 2_000, "0");
    let before_from = thesis("th-3", "asset-c", 100, 999, "0");
    let after_to = thesis("th-4", "asset-d", 100, 2_001, "0");
    let instruments = BTreeMap::from([
        (
            asset("asset-a"),
            series("asset-a", &[(101, "100"), (1_000, "110")]),
        ),
        (
            asset("asset-b"),
            series("asset-b", &[(101, "100"), (2_000, "120")]),
        ),
        (
            asset("asset-c"),
            series("asset-c", &[(101, "100"), (999, "100")]),
        ),
        (
            asset("asset-d"),
            series("asset-d", &[(101, "100"), (2_100, "100")]),
        ),
    ]);
    let basket = vec![series("basket-1", &[(101, "100"), (999, "100")])];
    let index = series("index", &[(101, "400"), (999, "400")]);
    let registered = EvaluationDecision {
        window: EvaluationWindow {
            from: t(1_000),
            to: t(2_000),
        },
        minimum_scoreable: 1,
        z: r("1.645"),
    };
    let from_card = evaluate(&input(
        &registered,
        std::slice::from_ref(&at_from),
        &instruments,
        &basket,
        &index,
    ))
    .expect("a horizon closing exactly at `from` is the first scored instant");
    assert_eq!(from_card.scoreable_count, 1);
    assert_eq!(
        from_card
            .theses
            .first()
            .expect("the thesis was scored")
            .net_return,
        r("0.1")
    );
    let to_card = evaluate(&input(&registered, &[at_to], &instruments, &basket, &index))
        .expect("a horizon closing exactly at `to` is inside the window: `to` is inclusive");
    assert_eq!(to_card.scoreable_count, 1);
    assert_eq!(
        to_card
            .theses
            .first()
            .expect("the thesis was scored")
            .net_return,
        r("0.2")
    );
    assert!(
        matches!(
            evaluate(&input(
                &registered,
                &[at_from.clone(), before_from],
                &instruments,
                &basket,
                &index,
            )),
            Err(ResearchError::ThesisOutsideWindow)
        ),
        "a horizon closing at t(999) is before `from` t(1000): refused, not silently scored"
    );
    assert!(
        matches!(
            evaluate(&input(
                &registered,
                &[at_from.clone(), after_to],
                &instruments,
                &basket,
                &index,
            )),
            Err(ResearchError::ThesisOutsideWindow)
        ),
        "a horizon closing at t(2001) is past `to` t(2000): refused, not silently scored"
    );
}

#[test]
fn each_row_excess_uses_its_own_window() {
    let earlier = thesis("th-1", "asset-a", 100, 1_000, "0");
    let later = thesis("th-2", "asset-b", 2_000, 3_000, "0");
    let instruments = BTreeMap::from([
        (
            asset("asset-a"),
            series("asset-a", &[(101, "100"), (999, "110")]),
        ),
        (
            asset("asset-b"),
            series("asset-b", &[(2_001, "100"), (2_999, "100")]),
        ),
    ]);
    let basket = vec![series(
        "basket-1",
        &[(101, "100"), (999, "100"), (2_001, "100"), (2_999, "102")],
    )];
    let index = series(
        "index",
        &[(101, "400"), (999, "408"), (2_001, "500"), (2_999, "505")],
    );
    let registered = decision(1);
    let card = evaluate(&input(
        &registered,
        &[earlier, later],
        &instruments,
        &basket,
        &index,
    ))
    .expect("two scoreable theses meet the minimum");
    assert_eq!(card.scoreable_count, 2);
    let earlier_row = card.theses.first().expect("the earlier thesis was scored");
    assert_eq!(earlier_row.excess_over_basket, r("0.1"), "0.1 − 0");
    assert_eq!(earlier_row.excess_over_index, r("0.08"), "0.1 − 0.02");
    let later_row = card.theses.last().expect("the later thesis was scored");
    assert_eq!(
        later_row.excess_over_basket,
        r("-0.02"),
        "the basket returned 0.02 over the later thesis's own window [t(2000), t(3000)], not over the first thesis's: 0 − 0.02"
    );
    assert_eq!(
        later_row.excess_over_index,
        r("-0.01"),
        "the index returned 0.01 over the later thesis's own window: 0 − 0.01"
    );
    assert_eq!(card.mean_excess_basket, r("0.04"));
    assert_eq!(card.mean_excess_index, r("0.035"));
}

#[test]
fn the_report_recomputes_from_its_own_fields() {
    let (theses, instruments, basket, index) = five_theses("400");
    let registered = decision(2);
    let card = evaluate(&input(&registered, &theses, &instruments, &basket, &index))
        .expect("five scoreable theses meet the minimum");
    assert_eq!(
        card.z,
        r("1.645"),
        "the report echoes the decision's z, so the bound recomputes from the report alone"
    );
    let one = SignedQty::parse("1").expect("a unit quantity parses");
    for row in &card.theses {
        let entry_usd = one.value_at(row.entry).expect("an entry price values");
        let exit_usd = one.value_at(row.exit).expect("an exit price values");
        let gross = exit_usd
            .checked_sub(entry_usd)
            .expect("the prices subtract")
            .ratio_to(
                entry_usd,
                mandate_research::score::REPORT_SCALE,
                Rounding::HalfEven,
            )
            .expect("the window return rounds");
        assert_eq!(
            row.net_return,
            gross
                .checked_sub(row.round_trip_cost)
                .expect("the cost subtracts"),
            "each row's net return recomputes from its own entry, exit, and echoed round-trip cost"
        );
    }
    let excesses_basket: Vec<Ratio> = card
        .theses
        .iter()
        .map(|row| row.excess_over_basket)
        .collect();
    let excesses_index: Vec<Ratio> = card
        .theses
        .iter()
        .map(|row| row.excess_over_index)
        .collect();
    assert_eq!(
        card.excess_sum_basket,
        Ratio::sum(&excesses_basket).expect("the excesses add")
    );
    assert_eq!(
        card.mean_excess_basket,
        Ratio::mean(&excesses_basket).expect("the mean divides")
    );
    assert_eq!(
        card.mean_excess_index,
        Ratio::mean(&excesses_index).expect("the mean divides")
    );
    let sigma = card
        .sample_variance_basket
        .expect("five theses have dispersion")
        .root_ceiling()
        .expect("the sigma root ceils");
    let count = Ratio::parse("5").expect("the count parses");
    let margin = Ratio::squared_quotient(
        card.z.checked_mul(sigma).expect("the product is exact"),
        count,
    )
    .expect("the quotient rounds")
    .root_ceiling()
    .expect("the margin root ceils");
    assert_eq!(
        card.margin_basket, margin,
        "the margin recomputes from the report's own z, variance, and count"
    );
    assert_eq!(
        card.lower_bound_basket,
        card.mean_excess_basket
            .checked_sub(card.margin_basket)
            .expect("the bound subtracts")
    );
    assert_eq!(
        card.lower_bound_index,
        card.mean_excess_index
            .checked_sub(card.margin_index)
            .expect("the bound subtracts")
    );
}

#[test]
fn an_empty_basket_is_an_error() {
    let refusal = basket_return(&[], t(100), t(1_000))
        .expect_err("an empty basket refuses, never a quiet zero");
    assert!(
        matches!(refusal, ResearchError::Num(_)),
        "an empty basket's equal-weighted mean is undefined: the Num error {refusal:?}"
    );
}

/// One generated thesis: whether it is scoreable (an unscoreable one names an instrument with no
/// series) and the second its horizon closes at, inside the registered window `[t(0),
/// t(10_000)]` or past its end.
#[derive(Debug)]
struct PlannedThesis {
    scoreable: bool,
    horizon_end: i64,
}

/// One generated scenario: how many theses there are (`0..=3`), each one's scoreability and
/// horizon, and the registered minimum `0..=3`. Every close and price comes from the same small
/// fixture family the hand cases use, so every window return stays inside the ±100% the
/// aggregate's sums of squares are bounded by (DEC-282 item 6), and the basket and the index are
/// always well formed.
#[derive(Debug)]
struct EmptySetPlan {
    theses: Vec<PlannedThesis>,
    minimum: u32,
}

/// A horizon inside the registered window three times in four, past its end once in four, so a
/// thesis outside the window meets every scoreable count and every minimum (#435 review minor 2).
fn planned_horizon() -> impl Strategy<Value = i64> {
    prop_oneof![3 => Just(1_000_i64), 1 => Just(12_000_i64)]
}

/// The scenario strategy: nothing else varies, so a failure names the boundary it crossed.
fn empty_set_plan() -> impl Strategy<Value = EmptySetPlan> {
    (
        proptest::collection::vec(
            (any::<bool>(), planned_horizon()).prop_map(|(scoreable, horizon_end)| PlannedThesis {
                scoreable,
                horizon_end,
            }),
            0..=3,
        ),
        0u32..4,
    )
        .prop_map(|(theses, minimum)| EmptySetPlan { theses, minimum })
}

/// How many generated scenarios reached each of the property's four expectations.
#[derive(Debug, Default)]
struct BranchCounts {
    outside: Cell<u32>,
    below_minimum: Cell<u32>,
    empty_at_zero: Cell<u32>,
    reported: Cell<u32>,
}

impl BranchCounts {
    /// Counts one scenario that reached `branch`.
    fn hit(branch: &Cell<u32>) {
        branch.set(branch.get().saturating_add(1));
    }
}

/// No generated scenario — an empty input, an all-unscoreable one, or a partly scoreable one,
/// against a registered minimum from zero to three — ever reaches the aggregate's division by
/// zero (#410 review minor 3, DEC-282 item 9): a scoreable set the count refusal does not
/// answer — an empty one at a minimum of zero — refuses with its own arm (DEC-335), an empty
/// one below a positive minimum keeps the window refusal as #410's round-1 pins freeze it, a
/// non-empty one below the minimum keeps it too, a thesis outside the registered window is
/// refused before any of that (DEC-282 item 3), and a set at its minimum reports with the
/// count an oracle counts for itself. Every one of the four expectations is reached by at least
/// one generated scenario, so none of them reads as coverage it does not give. A plain function
/// over a `TestRunner`, because a pending test must not be one a macro generates.
#[test]
fn no_scoreable_set_reaches_division_by_zero() {
    let counts = BranchCounts::default();
    let mut runner = TestRunner::new(ProptestConfig::with_cases(256));
    let outcome = runner.run(&empty_set_plan(), |plan| {
        let theses: Vec<ClosedThesis> = plan
            .theses
            .iter()
            .enumerate()
            .map(|(index, planned)| {
                let number = index + 1;
                let id = format!("th-{number}");
                let instrument = if planned.scoreable {
                    format!("asset-{number}")
                } else {
                    format!("missing-{number}")
                };
                thesis(&id, &instrument, 100, planned.horizon_end, "0")
            })
            .collect();
        let instruments: BTreeMap<AssetId, CloseSeries> = plan
            .theses
            .iter()
            .enumerate()
            .filter(|(_, planned)| planned.scoreable)
            .map(|(index, _)| {
                let number = index + 1;
                let id = format!("asset-{number}");
                (asset(&id), series(&id, &[(101, "100"), (999, "120")]))
            })
            .collect();
        let basket = vec![series("basket-1", &[(101, "100"), (999, "100")])];
        let index = series("index", &[(101, "400"), (999, "400")]);
        let registered = decision(plan.minimum);
        let outcome = evaluate(&input(&registered, &theses, &instruments, &basket, &index));
        if let Err(refused) = &outcome {
            prop_assert!(
                !matches!(refused, ResearchError::Num(NumError::DivisionByZero)),
                "the aggregate's division by zero is unreachable, got {refused:?}"
            );
        }
        let outside = theses.iter().any(|thesis| {
            thesis.horizon_end < registered.window.from || thesis.horizon_end > registered.window.to
        });
        let scoreable = theses
            .iter()
            .filter(|thesis| {
                thesis.direction == Direction::Long && instruments.contains_key(&thesis.instrument)
            })
            .count();
        let scoreable = u32::try_from(scoreable).unwrap_or(u32::MAX);
        if outside {
            BranchCounts::hit(&counts.outside);
            prop_assert!(
                matches!(outcome, Err(ResearchError::ThesisOutsideWindow)),
                "a thesis outside the registered window is refused first, got {:?}",
                outcome.as_ref().err()
            );
        } else if scoreable < plan.minimum {
            BranchCounts::hit(&counts.below_minimum);
            prop_assert!(
                matches!(outcome, Err(ResearchError::WindowNotClosed)),
                "a scoreable set below the minimum keeps the window refusal, got {:?}",
                outcome.as_ref().err()
            );
        } else if scoreable == 0 {
            BranchCounts::hit(&counts.empty_at_zero);
            prop_assert!(
                matches!(outcome, Err(ResearchError::EmptyScoreableSet)),
                "an empty scoreable set the count refusal does not answer - a minimum of zero - \
                 refuses with its own arm, got {:?}",
                outcome.as_ref().err()
            );
        } else {
            BranchCounts::hit(&counts.reported);
            let card = match outcome {
                Ok(card) => card,
                Err(refused) => {
                    return Err(TestCaseError::fail(format!(
                        "a scoreable set at its minimum reports, not {refused:?}"
                    )));
                }
            };
            prop_assert_eq!(
                card.scoreable_count,
                scoreable,
                "the report's count is the oracle's own"
            );
            prop_assert_eq!(
                card.theses.len().saturating_add(card.unscoreable.len()),
                theses.len(),
                "every thesis is either scored or named"
            );
        }
        Ok(())
    });
    outcome.expect("no scoreable set reaches the division by zero");
    for (branch, reached) in [
        ("a thesis outside the window", &counts.outside),
        ("a scoreable set below the minimum", &counts.below_minimum),
        (
            "an empty scoreable set at a minimum of zero",
            &counts.empty_at_zero,
        ),
        ("a scoreable set at its minimum", &counts.reported),
    ] {
        assert!(
            reached.get() > 0,
            "the generator reaches {branch} at least once, counts {counts:?}"
        );
    }
}

/// The empty scoreable set refuses with its own arm and the code ES-09 registers for it,
/// wherever the count refusal does not answer it: no theses at all, or every thesis
/// unscoreable, against a registered minimum of zero — the minimum an empty set is not below,
/// where the run used to reach the aggregate's mean of nothing and refuse a nameless
/// `Num(DivisionByZero)` (#410 review minor 3, DEC-282 item 9, DEC-335). Below a positive
/// minimum the count refusal keeps the empty set, as #410's round-1 pins freeze it; a thesis
/// outside the registered window is refused before any of that (DEC-282 item 3).
#[test]
fn an_empty_scoreable_set_refuses_with_its_own_code_at_a_minimum_of_zero() {
    let instruments = BTreeMap::from([(
        asset("asset-a"),
        series("asset-a", &[(101, "100"), (999, "120")]),
    )]);
    let basket = vec![series("basket-1", &[(101, "100"), (999, "100")])];
    let index = series("index", &[(101, "400"), (999, "400")]);
    let registered = decision(0);
    let refused = evaluate(&input(&registered, &[], &instruments, &basket, &index))
        .expect_err("an empty scoreable set against a minimum of zero refuses, named");
    assert!(
        matches!(refused, ResearchError::EmptyScoreableSet),
        "an empty scoreable set refuses with its own arm, got {refused:?}"
    );
    assert_eq!(refused.code(), "empty_scoreable_set");
    let missing = thesis("th-1", "asset-b", 100, 1_000, "0");
    let refused = evaluate(&input(
        &registered,
        &[missing],
        &instruments,
        &basket,
        &index,
    ))
    .expect_err("every thesis unscoreable leaves the scoreable set empty");
    assert!(
        matches!(refused, ResearchError::EmptyScoreableSet),
        "an all-unscoreable input refuses with the empty-set arm, got {refused:?}"
    );
    for minimum in [1, 3] {
        let strict = decision(minimum);
        assert!(
            matches!(
                evaluate(&input(&strict, &[], &instruments, &basket, &index)),
                Err(ResearchError::WindowNotClosed)
            ),
            "an empty scoreable set below a positive minimum keeps the count refusal"
        );
    }
    let outside = thesis("th-1", "asset-a", 100, 20_000, "0");
    assert!(
        matches!(
            evaluate(&input(
                &registered,
                &[outside],
                &instruments,
                &basket,
                &index
            )),
            Err(ResearchError::ThesisOutsideWindow)
        ),
        "a thesis outside the registered window is refused before the empty scoreable set"
    );
}

/// A non-empty scoreable set is unchanged: at its minimum it reports with one scored row and
/// nothing named, below it the refusal is still the window's count refusal and never the new
/// arm — and the empty set at the minimum where it used to reach the aggregate, zero, refuses
/// with its own arm instead, which is the boundary between the two refusals pinned from both
/// sides (DEC-336).
#[test]
fn a_non_empty_scoreable_set_is_unchanged_and_an_empty_one_never_reaches_the_aggregate() {
    let scored = thesis("th-1", "asset-a", 100, 1_000, "0");
    let instruments = BTreeMap::from([(
        asset("asset-a"),
        series("asset-a", &[(101, "100"), (999, "120")]),
    )]);
    let basket = vec![series("basket-1", &[(101, "100"), (999, "100")])];
    let index = series("index", &[(101, "400"), (999, "400")]);
    let registered = decision(0);
    let card = evaluate(&input(
        &registered,
        std::slice::from_ref(&scored),
        &instruments,
        &basket,
        &index,
    ))
    .expect("one scoreable thesis meets the minimum of zero and reports as before");
    assert_eq!(card.scoreable_count, 1);
    assert!(card.unscoreable.is_empty());
    assert_eq!(card.theses.len(), 1);
    let strict = decision(2);
    assert!(
        matches!(
            evaluate(&input(&strict, &[scored], &instruments, &basket, &index)),
            Err(ResearchError::WindowNotClosed)
        ),
        "one scoreable thesis against a minimum of two is still the window refusal"
    );
    let refused = evaluate(&input(&registered, &[], &instruments, &basket, &index))
        .expect_err("an empty scoreable set at the minimum where it used to reach the aggregate");
    assert!(
        matches!(refused, ResearchError::EmptyScoreableSet),
        "an empty scoreable set at a minimum of zero takes its own arm, got {refused:?}"
    );
}
