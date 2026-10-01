//! E17-8's forward-paper evaluator: hand-calculated scoring cases, every expected figure an
//! exact decimal recomputable from DEC-281's rules with integer arithmetic alone.
//!
//! The aggregate scenario is five Long theses over `[t(100), t(1000)]` with no round-trip cost,
//! every instrument closing `100` at its entry edge, so the net returns are
//! `[0.25, 0.15, 0.05, 0.05, 0.0]`: the mean is `0.1`, the sample variance
//! `(5 × 0.09 − 0.25) ÷ 20 = 0.01`, so σ is exactly `0.1`, and with the registered `z = 1.645`
//! the margin is `root_ceiling(0.1645² ÷ 5) = root_ceiling(0.00541205) = 0.073567` (the
//! smallest 12-digit value whose square reaches it: `0.073566² = 0.005411956356` does not), so
//! the bound is `0.1 − 0.073567 = 0.026433`. A floored root would report `0.073566` and a
//! division by anything but the count moves every figure.
//!
//! Every test is pending until its story lands (DEC-77), and every one fails on the stubs
//! because the stubs return an error where the case expects a figure or a refusal it does not
//! make.

use std::collections::BTreeMap;

use mandate_num::{Price, Ratio};
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
#[ignore = "pending E17-8"]
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
#[ignore = "pending E17-8"]
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
#[ignore = "pending E17-8"]
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
#[ignore = "pending E17-8"]
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
#[ignore = "pending E17-8"]
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
#[ignore = "pending E17-8"]
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
#[ignore = "pending E17-8"]
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
#[ignore = "pending E17-8"]
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
#[ignore = "pending E17-8"]
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
#[ignore = "pending E17-8"]
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
#[ignore = "pending E17-8"]
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
        &[scored, no_entry, no_exit],
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
        "both edge failures are named, in thesis order"
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
#[ignore = "pending E17-8"]
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
#[ignore = "pending E17-8"]
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
#[ignore = "pending E17-8"]
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
#[ignore = "pending E17-8"]
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
        r("0.073567"),
        "root_ceiling(0.1645² ÷ 5) = root_ceiling(0.00541205): the smallest 12-digit value whose square reaches it — a floored root would read 0.073566"
    );
    assert_eq!(card.lower_bound_basket, r("0.026433"));
    assert_eq!(card.excess_sum_index, r("0.5"));
    assert_eq!(card.mean_excess_index, r("0.1"));
    assert_eq!(card.sample_variance_index, Some(r("0.01")));
    assert_eq!(card.margin_index, r("0.073567"));
    assert_eq!(card.lower_bound_index, r("0.026433"));
    assert!(card.passed, "both bounds are above zero");
    let first = card.theses.first().expect("the first thesis was scored");
    assert_eq!(first.net_return, r("0.25"));
    assert_eq!(first.excess_over_basket, r("0.25"));
}

#[test]
#[ignore = "pending E17-8"]
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
#[ignore = "pending E17-8"]
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
    assert_eq!(card.margin_index, r("0.073567"));
    assert_eq!(card.lower_bound_index, r("-0.023567"), "0.05 − 0.073567");
    assert_eq!(
        card.lower_bound_basket,
        r("0.026433"),
        "the basket bound still passes"
    );
    assert!(
        !card.passed,
        "DEC-122's threshold is against each baseline: one negative bound fails the evaluation"
    );
}

#[test]
#[ignore = "pending E17-8"]
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
#[ignore = "pending E17-8"]
fn the_scorecard_is_independent_of_the_input_order() {
    let (theses, instruments, basket, index) = five_theses("400");
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
        "the report is keyed by thesis, not by input order (ES-21's replay equality)"
    );
    let ids: Vec<&str> = sorted
        .theses
        .iter()
        .map(|row| row.thesis.as_str())
        .collect();
    assert_eq!(ids, ["th-1", "th-2", "th-3", "th-4", "th-5"]);
}
