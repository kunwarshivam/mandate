//! `inspect` over hand-built datasets written by the real store (backlog E2-2): coverage, exact
//! statistics, gaps with exact timestamps and their missing slots classified by market session
//! (E2-4), raw and split-adjusted prices, duplicates, and partitions that cannot be trusted.

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use common::{Scratch, dataset, day};
use mandate_canon::{DecStr, Digest};
use mandate_marketdata::actions::{CORPORATE_ACTIONS, RecordedActions, write_actions};
use mandate_marketdata::dataset::{DatasetError, MANIFEST, Store};
use mandate_marketdata::inspect::{
    ActionsReport, AdjustedPrices, ClassifiedGap, Coverage, Duplicate, Gap, GapClass, InspectError,
    Problem, Stretch, Values, classify, gaps, inspect,
};
use mandate_marketdata::model::{
    AssetClass, Bar, CashDividend, CorporateActions, DatasetId, DayRange, Feed, Kind, OtherAction,
    Records, Split, Symbol, TimeUnit, Timeframe, Trade, split_ratio,
};
use mandate_marketdata::venue::{Venue, VenueState};
use mandate_time::{Date, UtcNanos};
use proptest::prelude::*;

use GapClass::{NoTrade, SessionClosure, TrueGap, Unclassified};

fn at(date: &str, hour: u8) -> UtcNanos {
    UtcNanos::parse(&format!("{date}T{hour:02}:00:00.000000000Z")).unwrap()
}

fn time(s: &str) -> UtcNanos {
    UtcNanos::parse(s).unwrap()
}

fn dec(s: &str) -> DecStr {
    DecStr::parse(s).unwrap()
}

fn range(first: &str, last: &str) -> DayRange {
    DayRange::new(day(first), day(last)).unwrap()
}

fn bar(start: UtcNanos, low: &str, high: &str, volume: &str, trade_count: u64) -> Bar {
    Bar {
        start,
        open: dec(low),
        high: dec(high),
        low: dec(low),
        close: dec(high),
        volume: dec(volume),
        vwap: dec(low),
        trade_count,
    }
}

fn hourly(date: &str, hours: impl IntoIterator<Item = u8>) -> Records {
    Records::Bars(
        hours
            .into_iter()
            .map(|h| bar(at(date, h), "100", "101", "10", 1))
            .collect(),
    )
}

fn trade(time: UtcNanos, price: &str, size: &str, trade_id: u64, exchange: &str) -> Trade {
    Trade {
        time,
        price: dec(price),
        size: dec(size),
        trade_id,
        exchange: Some(exchange.to_owned()),
        conditions: Some(vec!["@".to_owned()]),
        tape: Some("C".to_owned()),
        taker_side: None,
    }
}

fn one_hour() -> Kind {
    Kind::Bars("1Hour".parse::<Timeframe>().unwrap())
}

fn btc_hourly() -> DatasetId {
    dataset(AssetClass::Crypto, Feed::CryptoUs, one_hour(), "BTC/USD")
}

fn spy_hourly() -> DatasetId {
    dataset(AssetClass::UsEquity, Feed::Sip, one_hour(), "SPY")
}

fn spy_trades() -> DatasetId {
    dataset(AssetClass::UsEquity, Feed::Iex, Kind::Trades, "SPY")
}

fn stored(scratch: &Scratch, id: &DatasetId, days: Vec<(&str, Records)>) -> PathBuf {
    let store = Store::new(scratch.path());
    for (date, records) in days {
        store.put_day(id, day(date), &records).unwrap();
    }
    store.dataset_dir(id)
}

fn stretch(class: GapClass, first: UtcNanos, last: UtcNanos, slots: u64) -> Stretch {
    Stretch {
        class,
        first,
        last,
        slots,
    }
}

fn classified(previous: UtcNanos, next: UtcNanos, stretches: Vec<Stretch>) -> ClassifiedGap {
    ClassifiedGap {
        gap: Gap { previous, next },
        stretches,
    }
}

#[test]
fn a_clean_dataset_has_no_gaps_duplicates_or_problems() {
    let scratch = Scratch::new("inspect-clean");
    let id = btc_hourly();
    let dir = stored(
        &scratch,
        &id,
        vec![
            ("2026-09-21", hourly("2026-09-21", 0..24)),
            ("2026-09-22", hourly("2026-09-22", 0..24)),
        ],
    );
    let found = inspect(&dir).unwrap();
    assert_eq!(found.dataset, id);
    assert_eq!(
        found.coverage,
        Coverage {
            span: Some(range("2026-09-21", "2026-09-22")),
            listed: 2,
            empty: vec![],
            closed: 0,
            missing: vec![],
        }
    );
    let stats = found.stats.unwrap();
    assert_eq!(
        (stats.rows, stats.first, stats.last),
        (48, at("2026-09-21", 0), at("2026-09-22", 23))
    );
    assert_eq!(found.gaps, []);
    assert_eq!(found.duplicates, []);
    assert_eq!(found.problems, []);
}

#[test]
fn gaps_inside_a_day_and_across_the_partition_boundary_name_the_bars_on_either_side() {
    let scratch = Scratch::new("inspect-gaps");
    let id = btc_hourly();
    let dir = stored(
        &scratch,
        &id,
        vec![
            (
                "2026-09-21",
                hourly("2026-09-21", (0..22).filter(|h| *h != 5)),
            ),
            ("2026-09-22", hourly("2026-09-22", 2..24)),
        ],
    );
    assert_eq!(
        inspect(&dir).unwrap().gaps,
        [
            classified(
                at("2026-09-21", 4),
                at("2026-09-21", 6),
                vec![stretch(
                    NoTrade,
                    at("2026-09-21", 5),
                    at("2026-09-21", 5),
                    1
                )],
            ),
            classified(
                at("2026-09-21", 21),
                at("2026-09-22", 2),
                vec![stretch(
                    NoTrade,
                    at("2026-09-21", 22),
                    at("2026-09-22", 1),
                    4
                )],
            ),
        ]
    );
}

#[test]
fn a_gap_across_the_equity_overnight_session_is_a_session_closure() {
    let scratch = Scratch::new("inspect-session");
    let id = spy_hourly();
    let dir = stored(
        &scratch,
        &id,
        vec![
            ("2026-09-24", hourly("2026-09-24", 8..24)),
            ("2026-09-25", hourly("2026-09-25", 8..24)),
        ],
    );
    assert_eq!(
        inspect(&dir).unwrap().gaps,
        [classified(
            at("2026-09-24", 23),
            at("2026-09-25", 8),
            vec![stretch(
                SessionClosure,
                at("2026-09-25", 0),
                at("2026-09-25", 7),
                8
            )],
        )]
    );
}

#[test]
fn gap_classes_have_stable_names() {
    assert_eq!(
        [SessionClosure, NoTrade, TrueGap, Unclassified].map(GapClass::as_str),
        ["session closure", "no trade", "true gap", "unclassified"]
    );
}

fn iex_hourly() -> DatasetId {
    dataset(AssetClass::UsEquity, Feed::Iex, one_hour(), "SPY")
}

#[test]
fn iex_gaps_split_into_no_trade_closures_true_gaps_and_the_unclassified_early_close_evening() {
    let scratch = Scratch::new("inspect-iex-thanksgiving");
    let none = || Records::empty(one_hour());
    let dir = stored(
        &scratch,
        &iex_hourly(),
        vec![
            (
                "2026-11-24",
                hourly("2026-11-24", (13..22).filter(|h| *h != 15)),
            ),
            ("2026-11-26", none()),
            ("2026-11-27", hourly("2026-11-27", 13..18)),
            ("2026-11-28", none()),
            ("2026-11-30", hourly("2026-11-30", 13..22)),
        ],
    );
    let found = inspect(&dir).unwrap();
    assert_eq!(
        found.gaps,
        [
            classified(
                at("2026-11-24", 14),
                at("2026-11-24", 16),
                vec![stretch(
                    NoTrade,
                    at("2026-11-24", 15),
                    at("2026-11-24", 15),
                    1
                )],
            ),
            classified(
                at("2026-11-24", 21),
                at("2026-11-27", 13),
                vec![
                    stretch(
                        SessionClosure,
                        at("2026-11-24", 22),
                        at("2026-11-25", 12),
                        15
                    ),
                    stretch(TrueGap, at("2026-11-25", 13), at("2026-11-25", 21), 9),
                    stretch(
                        SessionClosure,
                        at("2026-11-25", 22),
                        at("2026-11-27", 12),
                        39
                    ),
                ],
            ),
            classified(
                at("2026-11-27", 17),
                at("2026-11-30", 13),
                vec![
                    stretch(Unclassified, at("2026-11-27", 18), at("2026-11-27", 21), 4),
                    stretch(
                        SessionClosure,
                        at("2026-11-27", 22),
                        at("2026-11-30", 12),
                        63
                    ),
                ],
            ),
        ],
        "Wednesday was never fetched, Thanksgiving is closed, Friday closes early, and Sunday was never fetched but is closed"
    );
    assert_eq!(
        (found.coverage.empty, found.coverage.closed),
        (vec![], 2),
        "Thanksgiving and Saturday are listed without records while IEX is closed"
    );
}

#[test]
fn the_open_hours_of_a_day_whose_partition_cannot_be_trusted_are_true_gaps() {
    let scratch = Scratch::new("inspect-sip-altered");
    let dir = stored(
        &scratch,
        &spy_hourly(),
        vec![
            ("2026-09-21", hourly("2026-09-21", 8..24)),
            ("2026-09-22", hourly("2026-09-22", 8..24)),
            ("2026-09-23", hourly("2026-09-23", 8..24)),
        ],
    );
    fs::write(dir.join("2026-09-22.parquet"), b"altered").unwrap();
    let found = inspect(&dir).unwrap();
    assert_eq!(
        found.problems,
        [Problem::Altered {
            day: day("2026-09-22")
        }]
    );
    assert_eq!(
        found.gaps,
        [classified(
            at("2026-09-21", 23),
            at("2026-09-23", 8),
            vec![
                stretch(SessionClosure, at("2026-09-22", 0), at("2026-09-22", 7), 8),
                stretch(TrueGap, at("2026-09-22", 8), at("2026-09-22", 23), 16),
                stretch(SessionClosure, at("2026-09-23", 0), at("2026-09-23", 7), 8),
            ],
        )]
    );
}

fn minute(s: &str) -> UtcNanos {
    time(&format!("{s}:00.000000000Z"))
}

fn dates(list: &[&str]) -> BTreeSet<Date> {
    list.iter().map(|d| day(d)).collect()
}

fn gap(previous: &str, next: &str) -> Gap {
    Gap {
        previous: minute(previous),
        next: minute(next),
    }
}

#[test]
fn iex_minutes_from_the_early_close_on_are_unclassified() {
    let iex = Venue::of(Feed::Iex).unwrap();
    let one_minute: Timeframe = "1Min".parse().unwrap();
    assert_eq!(
        classify(
            one_minute,
            gap("2026-11-27T17:58", "2026-11-27T18:02"),
            &iex,
            &dates(&["2026-11-27"])
        )
        .unwrap(),
        [
            stretch(
                NoTrade,
                minute("2026-11-27T17:59"),
                minute("2026-11-27T17:59"),
                1
            ),
            stretch(
                Unclassified,
                minute("2026-11-27T18:00"),
                minute("2026-11-27T18:01"),
                2
            ),
        ]
    );
}

#[test]
fn sip_slots_after_its_recorded_hours_end_are_unclassified() {
    let sip = Venue::of(Feed::Sip).unwrap();
    assert_eq!(
        classify(
            "1Hour".parse().unwrap(),
            gap("2026-12-05T00:00", "2026-12-07T15:00"),
            &sip,
            &BTreeSet::new()
        )
        .unwrap(),
        [
            stretch(
                SessionClosure,
                minute("2026-12-05T01:00"),
                minute("2026-12-06T04:00"),
                28
            ),
            stretch(
                Unclassified,
                minute("2026-12-06T05:00"),
                minute("2026-12-07T14:00"),
                34
            ),
        ]
    );
}

#[test]
fn a_skipped_day_is_a_closure_when_the_market_is_closed_and_a_true_gap_when_it_was_not_fetched() {
    let daily: Timeframe = "1Day".parse().unwrap();
    let sip = Venue::of(Feed::Sip).unwrap();
    assert_eq!(
        classify(
            daily,
            gap("2026-09-04T04:00", "2026-09-10T04:00"),
            &sip,
            &dates(&["2026-09-08"])
        )
        .unwrap(),
        [
            stretch(
                SessionClosure,
                minute("2026-09-05T04:00"),
                minute("2026-09-07T04:00"),
                3
            ),
            stretch(
                NoTrade,
                minute("2026-09-08T04:00"),
                minute("2026-09-08T04:00"),
                1
            ),
            stretch(
                TrueGap,
                minute("2026-09-09T04:00"),
                minute("2026-09-09T04:00"),
                1
            ),
        ],
        "Saturday, Sunday, and Labor Day are closed"
    );
    assert_eq!(
        classify(
            daily,
            gap("2026-10-30T04:00", "2026-11-02T05:00"),
            &sip,
            &BTreeSet::new()
        )
        .unwrap(),
        [stretch(
            SessionClosure,
            minute("2026-10-31T04:00"),
            minute("2026-11-01T04:00"),
            2
        )],
        "the day of the next bar is not a slot, even an hour before it starts"
    );
    assert_eq!(
        classify(
            daily,
            gap("2026-09-04T00:00", "2026-09-07T00:00"),
            &Venue::continuous(),
            &dates(&["2026-09-05"])
        )
        .unwrap(),
        [
            stretch(
                NoTrade,
                minute("2026-09-05T00:00"),
                minute("2026-09-05T00:00"),
                1
            ),
            stretch(
                TrueGap,
                minute("2026-09-06T00:00"),
                minute("2026-09-06T00:00"),
                1
            ),
        ],
        "crypto never closes"
    );
    assert_eq!(
        classify(
            daily,
            gap("2017-12-29T05:00", "2018-01-03T05:00"),
            &sip,
            &dates(&["2017-12-30", "2017-12-31", "2018-01-01", "2018-01-02"])
        )
        .unwrap(),
        [
            stretch(
                Unclassified,
                minute("2017-12-30T05:00"),
                minute("2017-12-31T05:00"),
                2
            ),
            stretch(
                SessionClosure,
                minute("2018-01-01T05:00"),
                minute("2018-01-01T05:00"),
                1
            ),
            stretch(
                NoTrade,
                minute("2018-01-02T05:00"),
                minute("2018-01-02T05:00"),
                1
            ),
        ],
        "days before the calendar starts are unclassified"
    );
}

#[test]
fn bars_sharing_a_start_are_one_duplicate_whatever_the_vendor_order() {
    let scratch = Scratch::new("inspect-duplicate-bars");
    let d = "2026-09-24";
    let twelve = bar(at(d, 12), "100", "101", "10", 1);
    let twelve_revised = bar(at(d, 12), "100", "101", "11", 1);
    let eleven = bar(at(d, 11), "100", "101", "10", 1);
    let vendor_order = vec![
        twelve.clone(),
        bar(at(d, 10), "100", "101", "10", 1),
        eleven.clone(),
        twelve_revised,
        eleven,
        twelve,
    ];
    let expected = [
        Duplicate {
            time: at(d, 11),
            trade_id: None,
            count: 2,
            identical: true,
        },
        Duplicate {
            time: at(d, 12),
            trade_id: None,
            count: 3,
            identical: false,
        },
    ];
    let mut sorted = vendor_order.clone();
    sorted.sort_by_key(|b| b.start);
    for (label, bars) in [("vendor", vendor_order), ("sorted", sorted)] {
        let id = btc_hourly();
        let dir = stored(&scratch, &id, vec![(d, Records::Bars(bars))]);
        let found = inspect(&dir).unwrap();
        assert_eq!(found.duplicates, expected, "{label}");
        assert_eq!(found.gaps, [], "{label}: repeated starts are not gaps");
        assert_eq!(
            found.stats.unwrap().rows,
            6,
            "{label}: statistics count raw rows"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn trades_are_duplicates_only_when_time_id_exchange_and_tape_all_match() {
    let scratch = Scratch::new("inspect-duplicate-trades");
    let id = spy_trades();
    let (t1, t2, t3) = (
        time("2026-09-24T14:00:00.000000001Z"),
        time("2026-09-24T14:00:00.000000002Z"),
        time("2026-09-24T14:00:00.000000003Z"),
    );
    let trades = vec![
        trade(t1, "81.19", "100", 1, "V"),
        trade(t1, "81.19", "100", 2, "V"),
        trade(t2, "81.19", "100", 3, "V"),
        trade(t2, "81.19", "100", 3, "V"),
        trade(t3, "81.19", "100", 4, "V"),
        trade(t3, "81.19", "200", 4, "V"),
        trade(t3, "81.19", "100", 4, "P"),
    ];
    let dir = stored(&scratch, &id, vec![("2026-09-24", Records::Trades(trades))]);
    let found = inspect(&dir).unwrap();
    assert_eq!(
        found.duplicates,
        [
            Duplicate {
                time: t2,
                trade_id: Some(3),
                count: 2,
                identical: true,
            },
            Duplicate {
                time: t3,
                trade_id: Some(4),
                count: 2,
                identical: false,
            },
        ]
    );
    assert_eq!(found.gaps, [], "trades have no fixed interval");
}

#[test]
fn statistics_are_exact_decimals_compared_by_value_not_text() {
    let scratch = Scratch::new("inspect-stats");
    let d = "2026-09-24";
    let id = btc_hourly();
    let bars = vec![
        bar(at(d, 1), "10", "10.5", "0.000000000000000001", 1),
        bar(at(d, 2), "9.5", "100", "123456789.5", 2),
        bar(at(d, 3), "10.25", "99.9", "0.5", 3),
    ];
    let dir = stored(&scratch, &id, vec![(d, Records::Bars(bars))]);
    let stats = inspect(&dir).unwrap().stats.unwrap();
    assert_eq!(
        stats.values,
        Values::Bars {
            low: dec("9.5"),
            high: dec("100"),
            volume: dec("123456790.000000000000000001"),
            trade_count: 6,
        }
    );
    assert_eq!((stats.first, stats.last), (at(d, 1), at(d, 3)));

    let id = spy_trades();
    let trades = vec![
        trade(at(d, 14), "81.19", "0.5", 1, "V"),
        trade(at(d, 15), "81.2", "100", 2, "V"),
        trade(at(d, 13), "9.000000001", "0.000000001", 3, "V"),
    ];
    let dir = stored(&scratch, &id, vec![(d, Records::Trades(trades))]);
    let stats = inspect(&dir).unwrap().stats.unwrap();
    assert_eq!(
        stats.values,
        Values::Trades {
            low: dec("9.000000001"),
            high: dec("81.2"),
            size: dec("100.500000001"),
        }
    );
    assert_eq!(
        (stats.rows, stats.first, stats.last),
        (3, at(d, 13), at(d, 15)),
        "first and last are by time, not vendor order"
    );
}

#[test]
fn an_empty_dataset_lists_its_empty_days_and_has_no_statistics() {
    let scratch = Scratch::new("inspect-empty");
    let id = spy_trades();
    let dir = stored(
        &scratch,
        &id,
        vec![
            ("2026-09-24", Records::empty(Kind::Trades)),
            ("2026-09-25", Records::empty(Kind::Trades)),
        ],
    );
    let found = inspect(&dir).unwrap();
    assert_eq!(
        found.coverage,
        Coverage {
            span: Some(range("2026-09-24", "2026-09-25")),
            listed: 2,
            empty: vec![range("2026-09-24", "2026-09-25")],
            closed: 0,
            missing: vec![],
        }
    );
    assert_eq!(found.stats, None);
    assert_eq!(
        (found.gaps, found.duplicates, found.problems),
        (vec![], vec![], vec![])
    );
}

#[test]
fn days_never_fetched_inside_the_span_are_missing_runs() {
    let scratch = Scratch::new("inspect-missing");
    let id = btc_hourly();
    let none = || Records::empty(one_hour());
    let dir = stored(
        &scratch,
        &id,
        vec![
            ("2026-09-21", hourly("2026-09-21", 0..24)),
            ("2026-09-24", none()),
            ("2026-09-25", hourly("2026-09-25", 0..24)),
            ("2026-09-28", none()),
        ],
    );
    let found = inspect(&dir).unwrap();
    assert_eq!(
        found.gaps,
        [classified(
            at("2026-09-21", 23),
            at("2026-09-25", 0),
            vec![
                stretch(TrueGap, at("2026-09-22", 0), at("2026-09-23", 23), 48),
                stretch(NoTrade, at("2026-09-24", 0), at("2026-09-24", 23), 24),
            ],
        )],
        "crypto never closes: days never fetched are true gaps, an empty fetched day no trade"
    );
    let coverage = found.coverage;
    assert_eq!(
        coverage,
        Coverage {
            span: Some(range("2026-09-21", "2026-09-28")),
            listed: 4,
            empty: vec![
                range("2026-09-24", "2026-09-24"),
                range("2026-09-28", "2026-09-28")
            ],
            closed: 0,
            missing: vec![
                range("2026-09-22", "2026-09-23"),
                range("2026-09-26", "2026-09-27")
            ],
        }
    );
}

#[test]
fn an_altered_partition_is_a_problem_and_left_out_of_the_statistics() {
    let scratch = Scratch::new("inspect-altered");
    let id = btc_hourly();
    let dir = stored(
        &scratch,
        &id,
        vec![
            ("2026-09-21", hourly("2026-09-21", 0..24)),
            ("2026-09-22", hourly("2026-09-22", 0..24)),
        ],
    );
    fs::write(dir.join("2026-09-22.parquet"), b"not parquet").unwrap();
    let found = inspect(&dir).unwrap();
    assert_eq!(
        found.problems,
        [Problem::Altered {
            day: day("2026-09-22")
        }]
    );
    let stats = found.stats.unwrap();
    assert_eq!((stats.rows, stats.last), (24, at("2026-09-21", 23)));
}

#[test]
fn a_missing_partition_and_files_the_manifest_does_not_list_are_problems() {
    let scratch = Scratch::new("inspect-missing-file");
    let id = btc_hourly();
    let dir = stored(
        &scratch,
        &id,
        vec![
            ("2026-09-21", hourly("2026-09-21", 0..24)),
            ("2026-09-22", hourly("2026-09-22", 0..24)),
            ("2026-09-23", Records::empty(one_hour())),
        ],
    );
    let copy = fs::read(dir.join("2026-09-22.parquet")).unwrap();
    fs::remove_file(dir.join("2026-09-21.parquet")).unwrap();
    fs::write(dir.join("2026-09-30.parquet"), &copy).unwrap();
    fs::write(dir.join("2026-09-23.parquet"), &copy).unwrap();
    fs::write(dir.join("notes.txt"), b"not a partition").unwrap();
    assert_eq!(
        inspect(&dir).unwrap().problems,
        [
            Problem::Missing {
                day: day("2026-09-21")
            },
            Problem::Unlisted {
                file: "2026-09-23.parquet".to_owned(),
            },
            Problem::Unlisted {
                file: "2026-09-30.parquet".to_owned(),
            },
        ]
    );
}

/// Rewrites the manifest's only file entry so it records `bytes` and, optionally, `rows`.
fn relist(dir: &Path, old: &[u8], bytes: &[u8], rows: Option<(u64, u64)>) {
    let path = dir.join(MANIFEST);
    let mut text = fs::read_to_string(&path).unwrap();
    text = text
        .replace(
            &format!("\"bytes\":{}", old.len()),
            &format!("\"bytes\":{}", bytes.len()),
        )
        .replace(&Digest::of(old).to_hex(), &Digest::of(bytes).to_hex());
    if let Some((from, to)) = rows {
        text = text.replace(&format!("\"rows\":{from}"), &format!("\"rows\":{to}"));
    }
    fs::write(path, text).unwrap();
}

#[test]
fn a_partition_the_manifest_vouches_for_but_that_does_not_decode_is_unreadable() {
    let scratch = Scratch::new("inspect-unreadable");
    let id = btc_hourly();
    let dir = stored(
        &scratch,
        &id,
        vec![("2026-09-24", hourly("2026-09-24", 0..3))],
    );
    let partition = dir.join("2026-09-24.parquet");
    let old = fs::read(&partition).unwrap();
    let garbage = b"PAR1 truncated PAR1".to_vec();
    fs::write(&partition, &garbage).unwrap();
    relist(&dir, &old, &garbage, None);
    let found = inspect(&dir).unwrap();
    assert!(
        matches!(
            found.problems.as_slice(),
            [Problem::Unreadable { day: d, .. }] if *d == day("2026-09-24")
        ),
        "{:?}",
        found.problems
    );
    assert_eq!(found.stats, None);
}

#[test]
fn a_row_count_that_disagrees_with_the_manifest_is_a_problem() {
    let scratch = Scratch::new("inspect-rows");
    let id = btc_hourly();
    let dir = stored(
        &scratch,
        &id,
        vec![("2026-09-24", hourly("2026-09-24", 0..3))],
    );
    let bytes = fs::read(dir.join("2026-09-24.parquet")).unwrap();
    relist(&dir, &bytes, &bytes, Some((3, 4)));
    assert_eq!(
        inspect(&dir).unwrap().problems,
        [Problem::RowCount {
            day: day("2026-09-24"),
            listed: 4,
            read: 3,
        }]
    );
}

#[test]
fn records_outside_their_partition_day_are_a_problem() {
    let scratch = Scratch::new("inspect-outside");
    let id = btc_hourly();
    let bars = Records::Bars(vec![
        bar(at("2026-09-24", 23), "100", "101", "10", 1),
        bar(at("2026-09-25", 0), "100", "101", "10", 1),
    ]);
    let dir = stored(&scratch, &id, vec![("2026-09-24", bars)]);
    let found = inspect(&dir).unwrap();
    assert_eq!(
        found.problems,
        [Problem::OutsideDay {
            day: day("2026-09-24"),
            records: 1,
        }]
    );
    assert_eq!(found.stats, None);
}

#[test]
fn a_directory_without_a_valid_manifest_is_an_error() {
    let scratch = Scratch::new("inspect-no-manifest");
    let err = inspect(scratch.path()).unwrap_err();
    assert!(
        matches!(err, InspectError::Dataset(DatasetError::Io { .. })),
        "{err}"
    );
    fs::write(scratch.path().join(MANIFEST), b"{\"format\":\"other\"}").unwrap();
    let err = inspect(scratch.path()).unwrap_err();
    assert_eq!(err.code(), "manifest", "{err}");
}

#[test]
fn daily_bars_have_a_gap_only_when_a_calendar_day_is_skipped() {
    let daily: Timeframe = "1Day".parse().unwrap();
    let starts = [
        time("2026-10-29T04:00:00.000000000Z"),
        time("2026-10-30T04:00:00.000000000Z"),
        time("2026-11-02T05:00:00.000000000Z"),
        time("2026-11-03T05:00:00.000000000Z"),
    ];
    assert_eq!(
        gaps(daily, &starts).unwrap(),
        [Gap {
            previous: starts[1],
            next: starts[2],
        }],
        "a weekend across the end of daylight saving time"
    );
    let crypto = [
        time("2026-10-31T04:00:00.000000000Z"),
        time("2026-11-01T04:00:00.000000000Z"),
        time("2026-11-02T05:00:00.000000000Z"),
    ];
    assert_eq!(
        gaps(daily, &crypto).unwrap(),
        [],
        "25 hours apart is the next day"
    );
    let spring = [
        time("2026-03-06T05:00:00.000000000Z"),
        time("2026-03-07T05:00:00.000000000Z"),
        time("2026-03-09T04:00:00.000000000Z"),
    ];
    assert_eq!(
        gaps(daily, &spring).unwrap(),
        [Gap {
            previous: spring[1],
            next: spring[2],
        }],
        "47 hours apart skips a day"
    );
}

fn timeframes() -> impl Strategy<Value = Timeframe> {
    prop_oneof![
        (1u8..=59).prop_map(|n| Timeframe::new(n, TimeUnit::Minute).unwrap()),
        (1u8..=23).prop_map(|n| Timeframe::new(n, TimeUnit::Hour).unwrap()),
    ]
}

fn step_secs(timeframe: Timeframe) -> i64 {
    let unit = match timeframe.unit() {
        TimeUnit::Minute => 60,
        TimeUnit::Hour => 3_600,
        TimeUnit::Day => 86_400,
    };
    i64::from(timeframe.amount()) * unit
}

proptest! {
    #[test]
    fn gaps_are_the_runs_of_empty_slots_on_the_bar_grid(
        timeframe in timeframes(),
        slots in proptest::collection::vec(0i64..400, 1..80).prop_shuffle(),
    ) {
        let base = time("2026-09-21T00:00:00.000000000Z").secs();
        let step = step_secs(timeframe);
        let start = |slot: i64| UtcNanos::from_parts(base + slot * step, 0).unwrap();
        let present: BTreeSet<i64> = slots.iter().copied().collect();
        let (lo, hi) = (*present.first().unwrap(), *present.last().unwrap());
        let mut expected = Vec::new();
        let mut last_present = lo;
        for slot in lo..=hi {
            if present.contains(&slot) {
                if slot > last_present + 1 {
                    expected.push(Gap { previous: start(last_present), next: start(slot) });
                }
                last_present = slot;
            }
        }
        let starts: Vec<UtcNanos> = slots.iter().map(|s| start(*s)).collect();
        prop_assert_eq!(gaps(timeframe, &starts).unwrap(), expected);
    }
}

fn feeds() -> impl Strategy<Value = Feed> {
    prop_oneof![Just(Feed::Sip), Just(Feed::Iex), Just(Feed::CryptoUs)]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn stretches_are_the_runs_of_missing_slots_each_classed_by_the_venue_and_the_fetch(
        timeframe in timeframes(),
        feed in feeds(),
        start_minute in 0_i64..(364 * 1_440),
        missing in 1_i64..400,
        clean_days in any::<u16>(),
    ) {
        let venue = Venue::of(feed).unwrap();
        let step = step_secs(timeframe);
        let previous = UtcNanos::from_parts(time("2026-01-01T00:00:00.000000000Z").secs() + start_minute * 60, 0).unwrap();
        let slot = |k: i64| UtcNanos::from_parts(previous.secs() + k * step, 0).unwrap();
        let next = slot(missing + 1);
        let first_day = previous.date();
        let offset = |date: Date| {
            let mut d = first_day;
            let mut n = 0_u32;
            while d < date {
                d = d.next().unwrap();
                n += 1;
            }
            n
        };
        let mut clean = BTreeSet::new();
        let mut date = first_day;
        while date <= next.date() {
            if clean_days & (1 << (offset(date) % 16)) != 0 {
                clean.insert(date);
            }
            date = date.next().unwrap();
        }
        let mut expected: Vec<Stretch> = Vec::new();
        for k in 1..=missing {
            let at = slot(k);
            let class = match venue.state_at(at).unwrap() {
                VenueState::Closed => SessionClosure,
                VenueState::Unclassified => Unclassified,
                VenueState::Open if clean.contains(&at.date()) => NoTrade,
                VenueState::Open => TrueGap,
            };
            match expected.last_mut() {
                Some(run) if run.class == class => {
                    run.last = at;
                    run.slots += 1;
                }
                _ => expected.push(stretch(class, at, at, 1)),
            }
        }
        prop_assert_eq!(classify(timeframe, Gap { previous, next }, &venue, &clean).unwrap(), expected);
    }
}

fn nvda_daily() -> DatasetId {
    dataset(
        AssetClass::UsEquity,
        Feed::Sip,
        Kind::Bars("1Day".parse().unwrap()),
        "NVDA",
    )
}

fn nvda_split_actions(range: DayRange, split_ex_date: &str) -> RecordedActions {
    RecordedActions {
        range,
        actions: CorporateActions {
            symbol: Symbol::parse("NVDA").unwrap(),
            splits: vec![Split {
                id: "split".to_owned(),
                ex_date: day(split_ex_date),
                ratio: split_ratio(10, 1).unwrap(),
            }],
            cash_dividends: vec![CashDividend {
                id: "dividend".to_owned(),
                ex_date: day("2024-06-10"),
                record_date: Some(day("2024-06-11")),
                payable_date: Some(day("2024-06-28")),
                rate: dec("0.01"),
                special: false,
                foreign: false,
            }],
            other: vec![OtherAction {
                id: "other".to_owned(),
                kind: "name_change".to_owned(),
                ex_date: None,
                process_date: day("2024-06-08"),
            }],
        },
    }
}

fn nvda_around_the_split(scratch: &Scratch) -> PathBuf {
    let daily = |date: &str, low: &str, high: &str| {
        Records::Bars(vec![bar(
            time(&format!("{date}T04:00:00.000000000Z")),
            low,
            high,
            "1000",
            10,
        )])
    };
    let none = || Records::empty(Kind::Bars("1Day".parse().unwrap()));
    stored(
        scratch,
        &nvda_daily(),
        vec![
            ("2024-06-07", daily("2024-06-07", "1190", "1210")),
            ("2024-06-08", none()),
            ("2024-06-09", none()),
            ("2024-06-10", daily("2024-06-10", "125", "130")),
        ],
    )
}

#[test]
fn a_stock_dataset_reports_raw_and_split_adjusted_prices_as_of_its_last_day() {
    let scratch = Scratch::new("inspect-split");
    let dir = nvda_around_the_split(&scratch);
    let recorded = nvda_split_actions(range("2024-06-07", "2024-06-10"), "2024-06-10");
    write_actions(&dir, &recorded).unwrap();
    let found = inspect(&dir).unwrap();
    let stats = found.stats.unwrap();
    assert_eq!(
        stats.values,
        Values::Bars {
            low: dec("125"),
            high: dec("1210"),
            volume: dec("2000"),
            trade_count: 20,
        },
        "raw prices, as stored"
    );
    assert_eq!(
        stats.adjusted,
        Some(AdjustedPrices {
            low: dec("119"),
            high: dec("130"),
        }),
        "the Friday bar in post-split terms"
    );
    assert_eq!(
        found.corporate_actions,
        ActionsReport::Applied {
            recorded,
            as_of: day("2024-06-10"),
        }
    );
    assert_eq!(found.problems, []);
}

#[test]
fn a_split_after_the_last_day_is_not_yet_known_and_adjusts_nothing() {
    let scratch = Scratch::new("inspect-split-later");
    let dir = nvda_around_the_split(&scratch);
    let recorded = nvda_split_actions(range("2024-06-01", "2024-06-30"), "2024-06-11");
    write_actions(&dir, &recorded).unwrap();
    let found = inspect(&dir).unwrap();
    assert_eq!(
        found.stats.unwrap().adjusted,
        Some(AdjustedPrices {
            low: dec("125"),
            high: dec("1210"),
        })
    );
    assert_eq!(
        found.corporate_actions,
        ActionsReport::Applied {
            recorded,
            as_of: day("2024-06-10"),
        }
    );
}

#[test]
fn actions_that_do_not_cover_the_span_give_no_adjusted_prices() {
    let scratch = Scratch::new("inspect-split-partial");
    let dir = nvda_around_the_split(&scratch);
    for recorded in [
        nvda_split_actions(range("2024-06-08", "2024-06-10"), "2024-06-10"),
        nvda_split_actions(range("2024-06-07", "2024-06-09"), "2024-06-09"),
    ] {
        write_actions(&dir, &recorded).unwrap();
        let found = inspect(&dir).unwrap();
        assert_eq!(found.stats.unwrap().adjusted, None);
        assert_eq!(found.corporate_actions, ActionsReport::Incomplete(recorded));
    }
}

#[test]
fn a_stock_dataset_stored_without_actions_says_so_and_crypto_has_none() {
    let scratch = Scratch::new("inspect-no-actions");
    let dir = nvda_around_the_split(&scratch);
    let found = inspect(&dir).unwrap();
    assert_eq!(found.corporate_actions, ActionsReport::NotRecorded);
    assert_eq!(found.stats.unwrap().adjusted, None);

    let crypto = stored(
        &scratch,
        &btc_hourly(),
        vec![("2026-09-24", hourly("2026-09-24", 0..2))],
    );
    let found = inspect(&crypto).unwrap();
    assert_eq!(found.corporate_actions, ActionsReport::NotApplicable);
    assert_eq!(found.stats.unwrap().adjusted, None);
}

#[test]
fn actions_of_another_symbol_or_a_malformed_file_are_errors() {
    let scratch = Scratch::new("inspect-foreign-actions");
    let dir = nvda_around_the_split(&scratch);
    let mut foreign = nvda_split_actions(range("2024-06-07", "2024-06-10"), "2024-06-10");
    foreign.actions.symbol = Symbol::parse("AAPL").unwrap();
    write_actions(&dir, &foreign).unwrap();
    let err = inspect(&dir).unwrap_err();
    assert!(matches!(err, InspectError::Actions(_)), "{err}");
    assert_eq!(err.code(), "actions_file");
    fs::write(dir.join(CORPORATE_ACTIONS), b"{}").unwrap();
    assert_eq!(inspect(&dir).unwrap_err().code(), "actions_file");
}

#[test]
fn trades_before_the_split_takes_effect_are_adjusted_and_the_overnight_session_into_the_ex_date_is_not()
 {
    let scratch = Scratch::new("inspect-split-trades");
    let id = dataset(AssetClass::UsEquity, Feed::Iex, Kind::Trades, "NVDA");
    let one = |at: &str, price: &str, size: &str, trade_id: u64| {
        Records::Trades(vec![trade(time(at), price, size, trade_id, "V")])
    };
    let dir = stored(
        &scratch,
        &id,
        vec![
            (
                "2024-06-07",
                one("2024-06-07T19:59:59.999999999Z", "1200", "10", 1),
            ),
            ("2024-06-08", Records::empty(Kind::Trades)),
            (
                "2024-06-09",
                one("2024-06-09T23:59:59.999999999Z", "1190", "2", 2),
            ),
            (
                "2024-06-10",
                Records::Trades(vec![
                    trade(time("2024-06-10T00:00:00.000000000Z"), "120.5", "3", 3, "V"),
                    trade(time("2024-06-10T13:30:00.000000000Z"), "121", "5", 4, "V"),
                ]),
            ),
        ],
    );
    write_actions(
        &dir,
        &nvda_split_actions(range("2024-06-07", "2024-06-10"), "2024-06-10"),
    )
    .unwrap();
    let stats = inspect(&dir).unwrap().stats.unwrap();
    assert_eq!(
        stats.values,
        Values::Trades {
            low: dec("120.5"),
            high: dec("1200"),
            size: dec("20"),
        }
    );
    assert_eq!(
        stats.adjusted,
        Some(AdjustedPrices {
            low: dec("119"),
            high: dec("121"),
        }),
        "until 20:00 ET the evening before the ex-date, prices are pre-split"
    );
}
