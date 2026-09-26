//! `inspect` over hand-built datasets written by the real store (backlog E2-2): coverage, exact
//! statistics, gaps with exact timestamps, duplicates, and partitions that cannot be trusted.

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use common::{Scratch, dataset, day};
use mandate_canon::{DecStr, Digest};
use mandate_marketdata::dataset::{DatasetError, MANIFEST, Store};
use mandate_marketdata::inspect::{
    Coverage, Duplicate, Gap, InspectError, Problem, Values, gaps, inspect,
};
use mandate_marketdata::model::{
    AssetClass, Bar, DatasetId, DayRange, Feed, Kind, Records, TimeUnit, Timeframe, Trade,
};
use mandate_time::UtcNanos;
use proptest::prelude::*;

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
            Gap {
                previous: at("2026-09-21", 4),
                next: at("2026-09-21", 6),
            },
            Gap {
                previous: at("2026-09-21", 21),
                next: at("2026-09-22", 2),
            },
        ]
    );
}

#[test]
fn a_gap_across_the_equity_overnight_session_is_reported_unclassified() {
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
        [Gap {
            previous: at("2026-09-24", 23),
            next: at("2026-09-25", 8),
        }]
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
            ("2026-09-26", Records::empty(Kind::Trades)),
            ("2026-09-27", Records::empty(Kind::Trades)),
        ],
    );
    let found = inspect(&dir).unwrap();
    assert_eq!(
        found.coverage,
        Coverage {
            span: Some(range("2026-09-26", "2026-09-27")),
            listed: 2,
            empty: vec![range("2026-09-26", "2026-09-27")],
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
    let coverage = inspect(&dir).unwrap().coverage;
    assert_eq!(
        coverage,
        Coverage {
            span: Some(range("2026-09-21", "2026-09-28")),
            listed: 4,
            empty: vec![
                range("2026-09-24", "2026-09-24"),
                range("2026-09-28", "2026-09-28")
            ],
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
