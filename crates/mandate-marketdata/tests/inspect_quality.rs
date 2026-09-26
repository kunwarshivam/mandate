//! `inspect`'s data-quality findings from the M1 rehearsal (claim #116): records stamped while the
//! venue is closed, zero-volume bars, single-trade bars whose prices differ, bars outside their own
//! range, listed days without records on closed days, the IEX evening after an early close, and
//! trade size totals that count restated official closes again. The SPY, JPM, and BTC/USD records
//! are Alpaca's, recorded from the market-data host on 2026-09-26 and copied field for field.

mod common;

use std::collections::BTreeSet;

use common::{Scratch, dataset, day};
use mandate_canon::DecStr;
use mandate_marketdata::dataset::Store;
use mandate_marketdata::inspect::{
    Coverage, EXAMPLES, Gap, GapClass, Occurrences, Problem, Quality, Stretch, Values, classify,
    inspect,
};
use mandate_marketdata::model::{
    AssetClass, Bar, DatasetId, DayRange, Feed, Kind, Records, Timeframe, Trade,
};
use mandate_marketdata::venue::Venue;
use mandate_time::{Date, UtcNanos};

/// An RFC 3339 time as Alpaca sends it, with up to nine fractional digits.
fn time(s: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(s).unwrap()
}

fn dec(s: &str) -> DecStr {
    DecStr::parse(s).unwrap()
}

fn range(first: &str, last: &str) -> DayRange {
    DayRange::new(day(first), day(last)).unwrap()
}

/// A bar from its vendor fields in Alpaca's order: t, o, h, l, c, v, vw, n.
fn bar(t: &str, [o, h, l, c, v, vw]: [&str; 6], n: u64) -> Bar {
    Bar {
        start: time(t),
        open: dec(o),
        high: dec(h),
        low: dec(l),
        close: dec(c),
        volume: dec(v),
        vwap: dec(vw),
        trade_count: n,
    }
}

fn bars(timeframe: &str, symbol: &str, feed: Feed) -> DatasetId {
    let asset_class = match feed {
        Feed::Sip | Feed::Iex => AssetClass::UsEquity,
        Feed::CryptoUs => AssetClass::Crypto,
    };
    dataset(
        asset_class,
        feed,
        Kind::Bars(timeframe.parse::<Timeframe>().unwrap()),
        symbol,
    )
}

fn stored(scratch: &Scratch, id: &DatasetId, days: Vec<(&str, Records)>) -> std::path::PathBuf {
    let store = Store::new(scratch.path());
    for (date, records) in days {
        store.put_day(id, day(date), &records).unwrap();
    }
    store.dataset_dir(id)
}

fn occurrences(count: u64, first: &[&str]) -> Occurrences {
    Occurrences {
        count,
        first: first.iter().map(|t| time(t)).collect(),
    }
}

fn minute(s: &str) -> UtcNanos {
    time(&format!("{s}:00Z"))
}

#[test]
fn the_iex_evening_after_the_2024_11_29_early_close_is_a_session_closure_from_17_00() {
    let clean: BTreeSet<Date> = ["2024-11-29", "2024-11-30", "2024-12-01", "2024-12-02"]
        .iter()
        .map(|d| day(d))
        .collect();
    let stretches = classify(
        "1Min".parse().unwrap(),
        Gap {
            previous: minute("2024-11-29T18:12"),
            next: minute("2024-12-02T13:00"),
        },
        &Venue::of(Feed::Iex).unwrap(),
        &clean,
    )
    .unwrap();
    assert_eq!(
        stretches,
        [
            Stretch {
                class: GapClass::Unclassified,
                first: minute("2024-11-29T18:13"),
                last: minute("2024-11-29T21:59"),
                slots: 227,
            },
            Stretch {
                class: GapClass::SessionClosure,
                first: minute("2024-11-29T22:00"),
                last: minute("2024-12-02T12:59"),
                slots: 3_780,
            },
        ],
        "SPY's last IEX bar of the early close starts at 13:12 ET; from 17:00 ET both IEX and the calendar are closed, so the 180 slots from 22:00Z to 00:59Z are closures"
    );
}

fn spy_sip_minutes() -> DatasetId {
    bars("1Min", "SPY", Feed::Sip)
}

const SPY_0327_CLOSE: [&str; 6] = ["519.71", "519.71", "519.71", "519.71", "100", "519.71"];

#[test]
fn spy_sip_bars_starting_at_the_20_00_close_are_records_while_the_venue_is_closed() {
    let scratch = Scratch::new("quality-spy-close");
    let dir = stored(
        &scratch,
        &spy_sip_minutes(),
        vec![
            (
                "2024-03-26",
                Records::Bars(vec![
                    bar(
                        "2024-03-26T23:58:00Z",
                        ["519.69", "519.71", "519.69", "519.71", "5087", "519.708988"],
                        21,
                    ),
                    bar(
                        "2024-03-26T23:59:00Z",
                        ["519.71", "519.72", "519.69", "519.7", "12439", "519.706774"],
                        61,
                    ),
                ]),
            ),
            (
                "2024-03-27",
                Records::Bars(vec![
                    bar("2024-03-27T00:00:00Z", SPY_0327_CLOSE, 1),
                    bar(
                        "2024-03-27T08:00:00Z",
                        ["520.32", "520.32", "520.32", "520.32", "393", "520.32"],
                        40,
                    ),
                ]),
            ),
            (
                "2024-05-01",
                Records::Bars(vec![bar(
                    "2024-05-01T00:00:00Z",
                    ["501.3", "501.3", "501.3", "501.3", "336", "501.3"],
                    3,
                )]),
            ),
            (
                "2024-06-15",
                Records::Bars(vec![bar(
                    "2024-06-15T00:00:00Z",
                    ["542.85", "542.85", "542.85", "542.85", "200", "542.85"],
                    1,
                )]),
            ),
            (
                "2024-06-21",
                Records::Bars(vec![bar(
                    "2024-06-21T00:00:00Z",
                    ["547.17", "547.17", "547.17", "547.17", "1000", "547.17"],
                    10,
                )]),
            ),
        ],
    );
    let found = inspect(&dir).unwrap();
    assert_eq!(
        found.quality,
        Quality {
            closed_period: occurrences(
                4,
                &[
                    "2024-03-27T00:00:00Z",
                    "2024-05-01T00:00:00Z",
                    "2024-06-15T00:00:00Z",
                    "2024-06-21T00:00:00Z",
                ]
            ),
            zero_volume: Occurrences::default(),
            single_trade_spread: Occurrences::default(),
        },
        "20:00 ET is the SIP close, so a bar starting then starts while the venue is closed; single-trade bars at one price are consistent"
    );
    assert_eq!(found.problems, [], "a warning is not a problem");
    assert_eq!(found.stats.unwrap().rows, 7);
}

fn stock_trade(t: &str, price: &str, size: &str, trade_id: u64, conditions: [&str; 2]) -> Trade {
    Trade {
        time: time(t),
        price: dec(price),
        size: dec(size),
        trade_id,
        exchange: Some("N".to_owned()),
        conditions: Some(conditions.iter().map(|c| (*c).to_owned()).collect()),
        tape: Some("A".to_owned()),
        taker_side: None,
    }
}

#[test]
fn jpm_trades_after_the_17_00_early_close_are_closed_period_records_and_restated_closes_count_again()
 {
    let scratch = Scratch::new("quality-jpm-close");
    let id = dataset(AssetClass::UsEquity, Feed::Sip, Kind::Trades, "JPM");
    let trades = vec![
        stock_trade(
            "2024-11-29T18:00:00.000488704Z",
            "250.15",
            "100",
            52_983_558_718_795,
            [" ", " "],
        ),
        stock_trade(
            "2024-11-29T18:00:47.351096832Z",
            "249.72",
            "1351385",
            52_983_558_719_562,
            [" ", "6"],
        ),
        stock_trade(
            "2024-11-29T18:00:47.407026944Z",
            "249.72",
            "1351385",
            52_983_558_719_563,
            [" ", "M"],
        ),
        stock_trade(
            "2024-11-29T22:00:00.002009088Z",
            "249.72",
            "0",
            52_983_558_721_350,
            [" ", "9"],
        ),
        stock_trade(
            "2024-11-29T22:00:00.002021376Z",
            "249.72",
            "1351385",
            52_983_558_721_351,
            [" ", "M"],
        ),
    ];
    let dir = stored(&scratch, &id, vec![("2024-11-29", Records::Trades(trades))]);
    let found = inspect(&dir).unwrap();
    assert_eq!(
        found.quality.closed_period,
        occurrences(
            2,
            &[
                "2024-11-29T22:00:00.002009088Z",
                "2024-11-29T22:00:00.002021376Z"
            ]
        ),
        "after the 13:00 early close, SIP trades until the calendar's 17:00 after-hours end"
    );
    assert_eq!(
        found.stats.unwrap().values,
        Values::Trades {
            low: dec("249.72"),
            high: dec("250.15"),
            size: dec("4054255"),
        },
        "NYSE's 1,351,385-share closing cross (condition 6) is reported again as the official close (M) at 13:00:47 and at 17:00, and every stored trade counts"
    );
    assert_eq!(found.problems, []);
}

#[test]
fn closed_period_records_keep_their_earliest_distinct_times_in_any_vendor_order() {
    let scratch = Scratch::new("quality-examples");
    let id = dataset(AssetClass::UsEquity, Feed::Iex, Kind::Trades, "SPY");
    let saturday = |second: u8, trade_id: u64| {
        stock_trade(
            &format!("2026-09-26T15:00:{second:02}.000000000Z"),
            "660",
            "1",
            trade_id,
            [" ", "@"],
        )
    };
    let friday = stock_trade("2026-09-25T15:00:00.000000000Z", "660", "1", 1, [" ", "@"]);
    let dir = stored(
        &scratch,
        &id,
        vec![
            ("2026-09-25", Records::Trades(vec![friday])),
            (
                "2026-11-27",
                Records::Trades(vec![stock_trade(
                    "2026-11-27T18:30:00Z",
                    "660",
                    "1",
                    9,
                    [" ", "@"],
                )]),
            ),
            (
                "2026-09-26",
                Records::Trades(vec![
                    saturday(9, 2),
                    saturday(3, 3),
                    saturday(7, 4),
                    saturday(3, 5),
                    saturday(1, 6),
                    saturday(8, 7),
                    saturday(5, 8),
                ]),
            ),
        ],
    );
    let found = inspect(&dir).unwrap();
    assert_eq!(EXAMPLES, 5);
    assert_eq!(
        found.quality.closed_period,
        occurrences(
            7,
            &[
                "2026-09-26T15:00:01Z",
                "2026-09-26T15:00:03Z",
                "2026-09-26T15:00:05Z",
                "2026-09-26T15:00:07Z",
                "2026-09-26T15:00:08Z",
            ]
        ),
        "every Saturday trade counts, a repeated time is one example, and 13:30 ET after IEX's early close is unclassified, not closed"
    );
}

#[test]
fn records_of_a_partition_with_a_problem_are_not_warnings() {
    let scratch = Scratch::new("quality-problem-partition");
    let dir = stored(
        &scratch,
        &spy_sip_minutes(),
        vec![(
            "2024-03-27",
            Records::Bars(vec![
                bar("2024-03-27T00:00:00Z", SPY_0327_CLOSE, 1),
                bar(
                    "2024-03-27T08:00:00Z",
                    ["520.33", "520.32", "520.32", "520.32", "0", "520.32"],
                    1,
                ),
            ]),
        )],
    );
    let found = inspect(&dir).unwrap();
    assert_eq!(
        found.problems,
        [Problem::InconsistentBars {
            day: day("2024-03-27"),
            bars: 1,
        }]
    );
    assert_eq!(
        found.quality,
        Quality::default(),
        "neither the bar at the close nor the zero-volume bar is counted"
    );
}

#[test]
fn zero_volume_is_judged_by_volume_and_a_single_trade_bar_by_its_low_and_high() {
    let scratch = Scratch::new("quality-synthetic");
    let hour = |h: u8, [o, hi, l, c]: [&str; 4], v: &str, n: u64| {
        bar(&format!("2026-09-24T{h:02}:00:00Z"), [o, hi, l, c, v, o], n)
    };
    let dir = stored(
        &scratch,
        &bars("1Hour", "BTC/USD", Feed::CryptoUs),
        vec![(
            "2026-09-24",
            Records::Bars(vec![
                hour(0, ["10", "10", "10", "10"], "0", 3),
                hour(1, ["10", "10", "10", "10"], "0.000000000000000001", 0),
                hour(2, ["10", "11", "9", "10"], "1", 1),
                hour(3, ["10", "11", "9", "11"], "1", 2),
            ]),
        )],
    );
    assert_eq!(
        inspect(&dir).unwrap().quality,
        Quality {
            closed_period: Occurrences::default(),
            zero_volume: occurrences(1, &["2026-09-24T00:00:00Z"]),
            single_trade_spread: occurrences(1, &["2026-09-24T02:00:00Z"]),
        },
        "a bar of three trades can report no volume, and one trade opening and closing at 10 can still span 9 to 11"
    );
}

#[test]
fn a_daily_bar_is_a_closed_period_record_only_when_the_market_is_closed_all_day() {
    let scratch = Scratch::new("quality-daily");
    let daily = |date: &str| {
        Records::Bars(vec![bar(
            &format!("{date}T05:00:00Z"),
            ["660", "661", "659", "660.5", "1000", "660.2"],
            10,
        )])
    };
    let dir = stored(
        &scratch,
        &bars("1Day", "SPY", Feed::Sip),
        vec![
            ("2026-11-25", daily("2026-11-25")),
            ("2026-11-26", daily("2026-11-26")),
            ("2026-11-27", daily("2026-11-27")),
        ],
    );
    assert_eq!(
        inspect(&dir).unwrap().quality.closed_period,
        occurrences(1, &["2026-11-26T05:00:00Z"]),
        "a daily bar starts at midnight ET, when even a trading day is closed; only Thanksgiving is"
    );
}

/// The first ten BTC/USD one-minute bars of 2026-09-24.
fn btc_bars() -> Vec<Bar> {
    let zero =
        |t: &str, o: &str, h: &str, l: &str, c: &str, vw: &str| bar(t, [o, h, l, c, "0", vw], 0);
    vec![
        zero(
            "2026-09-24T00:00:00Z",
            "84391.985",
            "84443.91",
            "84391.985",
            "84443.91",
            "84417.9475",
        ),
        bar(
            "2026-09-24T00:01:00Z",
            [
                "84432.274",
                "84469.8",
                "84392.3",
                "84428.9685",
                "0.004651",
                "84431.8763825994",
            ],
            4,
        ),
        zero(
            "2026-09-24T00:02:00Z",
            "84436.072",
            "84436.072",
            "84430.2115",
            "84430.2115",
            "84433.14175",
        ),
        zero(
            "2026-09-24T00:03:00Z",
            "84427.9595",
            "84447.925",
            "84427.9595",
            "84447.925",
            "84437.94225",
        ),
        zero(
            "2026-09-24T00:04:00Z",
            "84457.6965",
            "84457.958",
            "84433.669",
            "84457.958",
            "84445.8135",
        ),
        zero(
            "2026-09-24T00:05:00Z",
            "84461.6",
            "84476.9445",
            "84447.445",
            "84447.445",
            "84462.19475",
        ),
        bar(
            "2026-09-24T00:06:00Z",
            [
                "84451.457",
                "84490.51",
                "84451.457",
                "84475.61",
                "0.000804",
                "84490.51",
            ],
            1,
        ),
        zero(
            "2026-09-24T00:07:00Z",
            "84476.083",
            "84476.083",
            "84440.745",
            "84445.2415",
            "84458.414",
        ),
        zero(
            "2026-09-24T00:08:00Z",
            "84443.625",
            "84443.625",
            "84409.5385",
            "84409.5385",
            "84426.58175",
        ),
        bar(
            "2026-09-24T00:09:00Z",
            [
                "84415.054",
                "84415.054",
                "84370.6",
                "84388.6315",
                "0.00017775",
                "84387.73",
            ],
            1,
        ),
    ]
}

#[test]
fn zero_volume_bars_and_single_trade_bars_whose_prices_differ_are_warnings_not_problems() {
    let scratch = Scratch::new("quality-btc");
    let dir = stored(
        &scratch,
        &bars("1Min", "BTC/USD", Feed::CryptoUs),
        vec![("2026-09-24", Records::Bars(btc_bars()))],
    );
    let found = inspect(&dir).unwrap();
    assert_eq!(
        found.quality,
        Quality {
            closed_period: Occurrences::default(),
            zero_volume: occurrences(
                7,
                &[
                    "2026-09-24T00:00:00Z",
                    "2026-09-24T00:02:00Z",
                    "2026-09-24T00:03:00Z",
                    "2026-09-24T00:04:00Z",
                    "2026-09-24T00:05:00Z",
                ]
            ),
            single_trade_spread: occurrences(2, &["2026-09-24T00:06:00Z", "2026-09-24T00:09:00Z"]),
        },
        "crypto never closes; seven bars traded nothing, and two of one trade span several prices"
    );
    assert_eq!(found.problems, []);
    assert_eq!(found.stats.unwrap().rows, 10);
}

#[test]
fn bars_whose_open_or_close_lies_outside_their_range_make_their_partition_a_problem() {
    let scratch = Scratch::new("quality-range");
    let hourly = |date: &str, hour: u8, [o, h, l, c]: [&str; 4], v: &str| {
        bar(&format!("{date}T{hour:02}:00:00Z"), [o, h, l, c, v, o], 1)
    };
    let consistent = |date: &str| {
        Records::Bars(
            (0..24)
                .map(|hour| match hour {
                    0 => hourly(date, hour, ["11", "11", "10", "10"], "5"),
                    _ => hourly(date, hour, ["10", "10", "10", "10"], "5"),
                })
                .collect(),
        )
    };
    let dir = stored(
        &scratch,
        &bars("1Hour", "BTC/USD", Feed::CryptoUs),
        vec![
            ("2026-09-21", consistent("2026-09-21")),
            (
                "2026-09-22",
                Records::Bars(vec![
                    hourly("2026-09-22", 0, ["9", "11", "10", "10"], "0"),
                    hourly("2026-09-22", 1, ["12", "11", "10", "10"], "0"),
                    hourly("2026-09-22", 2, ["10", "11", "10", "9"], "0"),
                    hourly("2026-09-22", 3, ["10", "11", "10", "12"], "0"),
                    hourly("2026-09-22", 4, ["10", "11", "10", "11"], "0"),
                ]),
            ),
            ("2026-09-23", consistent("2026-09-23")),
        ],
    );
    let found = inspect(&dir).unwrap();
    assert_eq!(
        found.problems,
        [Problem::InconsistentBars {
            day: day("2026-09-22"),
            bars: 4,
        }],
        "an open below the low, an open above the high, a close below the low, and a close above the high; a bar at its range's ends is consistent"
    );
    assert_eq!(
        found.stats.unwrap().rows,
        48,
        "the problem partition is left out"
    );
    assert_eq!(
        found.quality,
        Quality {
            closed_period: Occurrences::default(),
            zero_volume: Occurrences::default(),
            single_trade_spread: occurrences(2, &["2026-09-21T00:00:00Z", "2026-09-23T00:00:00Z"]),
        },
        "the problem partition's zero-volume bars are not counted"
    );
    let [gap] = found.gaps.as_slice() else {
        panic!("{:?}", found.gaps)
    };
    assert_eq!(
        gap.stretches,
        [Stretch {
            class: GapClass::TrueGap,
            first: time("2026-09-22T00:00:00Z"),
            last: time("2026-09-22T23:00:00Z"),
            slots: 24,
        }],
        "the untrusted day's hours are true gaps"
    );
}

#[test]
fn listed_days_without_records_while_the_venue_is_closed_are_counted_not_listed() {
    let scratch = Scratch::new("quality-closed-days");
    let id = bars("1Day", "SPY", Feed::Sip);
    let none = || Records::empty(Kind::Bars("1Day".parse().unwrap()));
    let daily = |date: &str| {
        Records::Bars(vec![bar(
            &format!("{date}T05:00:00Z"),
            ["660", "661", "659", "660.5", "1000", "660.2"],
            10,
        )])
    };
    let dir = stored(
        &scratch,
        &id,
        vec![
            ("2026-11-20", daily("2026-11-20")),
            ("2026-11-21", none()),
            ("2026-11-22", none()),
            ("2026-11-23", daily("2026-11-23")),
            ("2026-11-24", none()),
            ("2026-11-26", none()),
            ("2026-11-27", daily("2026-11-27")),
            ("2026-11-28", none()),
            ("2026-11-29", none()),
            ("2026-11-30", daily("2026-11-30")),
            ("2026-12-04", daily("2026-12-04")),
            ("2026-12-05", none()),
            ("2026-12-06", none()),
        ],
    );
    assert_eq!(
        inspect(&dir).unwrap().coverage,
        Coverage {
            span: Some(range("2026-11-20", "2026-12-06")),
            listed: 13,
            empty: vec![
                range("2026-11-24", "2026-11-24"),
                range("2026-12-06", "2026-12-06")
            ],
            closed: 6,
            missing: vec![
                range("2026-11-25", "2026-11-25"),
                range("2026-12-01", "2026-12-03")
            ],
        },
        "two weekends, Thanksgiving, and Saturday 2026-12-05 are closed; a trading day without bars and a day after SIP's recorded hours stay listed; days never fetched stay missing"
    );

    let crypto = stored(
        &scratch,
        &bars("1Day", "BTC/USD", Feed::CryptoUs),
        vec![
            ("2026-09-25", daily("2026-09-25")),
            (
                "2026-09-26",
                Records::empty(Kind::Bars("1Day".parse().unwrap())),
            ),
        ],
    );
    let coverage = inspect(&crypto).unwrap().coverage;
    assert_eq!(
        (coverage.empty, coverage.closed),
        (vec![range("2026-09-26", "2026-09-26")], 0),
        "crypto never closes"
    );
}
