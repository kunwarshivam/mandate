//! `mandate inspect`: the text report, the problem count behind the exit status, and argument
//! parsing (backlog E2-2), with gaps classified by market session and raw and split-adjusted
//! prices (E2-4), and each side, the spread, and the classes of a quotes dataset (E2-3, DEC-116).

use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use mandate_canon::DecStr;
use mandate_cli::inspect::{self, InspectArgs};
use mandate_cli::{Cli, Command};
use mandate_marketdata::actions::RecordedActions;
use mandate_marketdata::dataset::Store;
use mandate_marketdata::inspect::{
    ActionsReport, AdjustedPrices, ClassifiedGap, Coverage, Duplicate, Extent, Gap, GapClass,
    Inspection, Problem, Spread, Stats, Stretch, Values,
};
use mandate_marketdata::model::{
    AssetClass, Bar, CashDividend, CorporateActions, DatasetId, DayRange, Feed, Kind, OtherAction,
    Records, Split, Symbol, Trade, split_ratio,
};
use mandate_time::{Date, UtcNanos};

fn day(s: &str) -> Date {
    Date::parse(s).unwrap()
}

fn time(s: &str) -> UtcNanos {
    UtcNanos::parse(s).unwrap()
}

fn dec(s: &str) -> DecStr {
    DecStr::parse(s).unwrap()
}

fn id(asset_class: AssetClass, feed: Feed, kind: Kind, symbol: &str) -> DatasetId {
    DatasetId::new(asset_class, feed, kind, Symbol::parse(symbol).unwrap()).unwrap()
}

fn btc_hourly() -> DatasetId {
    id(
        AssetClass::Crypto,
        Feed::CryptoUs,
        Kind::Bars("1Hour".parse().unwrap()),
        "BTC/USD",
    )
}

fn spy_trades() -> DatasetId {
    id(AssetClass::UsEquity, Feed::Iex, Kind::Trades, "SPY")
}

fn bar(start: &str, low: &str, high: &str, volume: &str, trade_count: u64) -> Bar {
    Bar {
        start: time(start),
        open: dec(low),
        high: dec(high),
        low: dec(low),
        close: dec(high),
        volume: dec(volume),
        vwap: dec(low),
        trade_count,
    }
}

fn trade(at: &str, trade_id: u64) -> Trade {
    Trade {
        time: time(at),
        price: dec("81.19"),
        size: dec("100"),
        trade_id,
        exchange: Some("V".to_owned()),
        conditions: None,
        tape: Some("C".to_owned()),
        taker_side: None,
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "mandate-cli-inspect-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn stored(root: &Path, dataset: &DatasetId, days: Vec<(&str, Records)>) -> PathBuf {
    let store = Store::new(root);
    for (date, records) in days {
        store.put_day(dataset, day(date), &records).unwrap();
    }
    store.dataset_dir(dataset)
}

fn bars_with_a_gap_and_a_duplicate(root: &Path) -> PathBuf {
    let repeated = bar("2026-09-24T02:00:00.000000000Z", "10.25", "99.9", "0.5", 3);
    stored(
        root,
        &btc_hourly(),
        vec![
            (
                "2026-09-21",
                Records::Bars(vec![
                    bar(
                        "2026-09-21T00:00:00.000000000Z",
                        "10",
                        "10.5",
                        "0.000000000000000001",
                        1,
                    ),
                    bar(
                        "2026-09-21T01:00:00.000000000Z",
                        "9.5",
                        "100",
                        "123456789.5",
                        2,
                    ),
                ]),
            ),
            ("2026-09-22", Records::Bars(vec![])),
            (
                "2026-09-24",
                Records::Bars(vec![repeated.clone(), repeated]),
            ),
        ],
    )
}

const BARS_REPORT: &str = "\
BTC/USD bars-1Hour (crypto-us)
coverage: 2026-09-21 to 2026-09-24, 3 days listed
  empty: 2026-09-22
  missing: 2026-09-23
rows: 4, first 2026-09-21T00:00:00.000000000Z, last 2026-09-24T02:00:00.000000000Z
bars: low 9.5, high 100, volume 123456790.500000000000000001, trade count 9
corporate actions: not applicable to crypto
gaps: 1, missing bar slots: 0 session closure, 48 no trade, 24 true gap, 0 unclassified
  2026-09-21T01:00:00.000000000Z to 2026-09-24T02:00:00.000000000Z
    no trade: 46 slots, 2026-09-21T02:00:00.000000000Z to 2026-09-22T23:00:00.000000000Z
    true gap: 24 slots, 2026-09-23T00:00:00.000000000Z to 2026-09-23T23:00:00.000000000Z
    no trade: 2 slots, 2026-09-24T00:00:00.000000000Z to 2026-09-24T01:00:00.000000000Z
duplicates: 1
  2026-09-24T02:00:00.000000000Z x2 identical
problems: 0
";

#[test]
fn the_report_gives_coverage_exact_statistics_gaps_and_duplicates() {
    let scratch = Scratch::new("report");
    let dir = bars_with_a_gap_and_a_duplicate(&scratch.0);
    let found = mandate_marketdata::inspect::inspect(&dir).unwrap();
    assert_eq!(inspect::render(&found), BARS_REPORT);
}

fn range(a: &str, b: &str) -> DayRange {
    DayRange::new(day(a), day(b)).unwrap()
}

fn spy_actions(recorded: &str, through: &str) -> RecordedActions {
    let split = |id: &str, ex_date: &str, new: u64, old: u64| Split {
        id: id.to_owned(),
        ex_date: day(ex_date),
        ratio: split_ratio(new, old).unwrap(),
    };
    RecordedActions {
        range: range(recorded, through),
        actions: CorporateActions {
            symbol: Symbol::parse("SPY").unwrap(),
            splits: vec![
                split("forward", "2026-09-22", 4, 1),
                split("later", "2026-10-05", 1, 2),
            ],
            cash_dividends: vec![
                CashDividend {
                    id: "plain".to_owned(),
                    ex_date: day("2026-09-23"),
                    record_date: None,
                    payable_date: None,
                    rate: dec("1.83"),
                    special: false,
                    foreign: false,
                },
                CashDividend {
                    id: "extra".to_owned(),
                    ex_date: day("2026-09-24"),
                    record_date: None,
                    payable_date: None,
                    rate: dec("0.5"),
                    special: true,
                    foreign: true,
                },
            ],
            other: vec![
                OtherAction {
                    id: "spin".to_owned(),
                    kind: "spin_off".to_owned(),
                    ex_date: Some(day("2026-09-25")),
                    process_date: day("2026-09-26"),
                },
                OtherAction {
                    id: "name".to_owned(),
                    kind: "name_change".to_owned(),
                    ex_date: None,
                    process_date: day("2026-09-27"),
                },
            ],
        },
    }
}

#[test]
fn every_problem_and_trade_duplicate_has_a_line() {
    let found = Inspection {
        dataset: spy_trades(),
        coverage: Coverage {
            span: Some(range("2026-09-19", "2026-09-30")),
            listed: 9,
            empty: vec![
                range("2026-09-19", "2026-09-20"),
                range("2026-09-27", "2026-09-27"),
            ],
            missing: vec![range("2026-09-28", "2026-09-29")],
        },
        stats: Some(Stats {
            rows: 3,
            first: time("2026-09-21T13:30:00.000000001Z"),
            last: time("2026-09-30T19:59:59.999999999Z"),
            values: Values::Trades {
                low: dec("81.19"),
                high: dec("81.2"),
                size: dec("300.5"),
            },
            adjusted: Some(AdjustedPrices {
                low: dec("20.3"),
                high: dec("81.2"),
            }),
        }),
        corporate_actions: ActionsReport::Applied {
            recorded: spy_actions("2026-09-01", "2026-10-31"),
            as_of: day("2026-09-30"),
        },
        gaps: vec![],
        duplicates: vec![Duplicate {
            time: time("2026-09-21T13:30:00.000000001Z"),
            trade_id: Some(7),
            count: 2,
            identical: false,
        }],
        problems: vec![
            Problem::Missing {
                day: day("2026-09-21"),
            },
            Problem::Altered {
                day: day("2026-09-22"),
            },
            Problem::Unreadable {
                day: day("2026-09-23"),
                reason: "parquet: bad footer".to_owned(),
            },
            Problem::RowCount {
                day: day("2026-09-24"),
                listed: 4,
                read: 3,
            },
            Problem::OutsideDay {
                day: day("2026-09-25"),
                records: 2,
            },
            Problem::Unlisted {
                file: "2026-10-01.parquet".to_owned(),
            },
        ],
    };
    assert_eq!(
        inspect::render(&found),
        "\
SPY trades (iex)
coverage: 2026-09-19 to 2026-09-30, 9 days listed
  empty: 2026-09-19 to 2026-09-20, 2026-09-27
  missing: 2026-09-28 to 2026-09-29
rows: 3, first 2026-09-21T13:30:00.000000001Z, last 2026-09-30T19:59:59.999999999Z
trades: low 81.19, high 81.2, size 300.5
split-adjusted as of 2026-09-30: low 20.3, high 81.2
corporate actions: recorded for 2026-09-01 to 2026-10-31, applied as of 2026-09-30
  split 4:1, ex-date 2026-09-22 (forward): applied
  split 1:2, ex-date 2026-10-05 (later): not applied, after 2026-09-30
  cash dividend 1.83 per share, ex-date 2026-09-23 (plain): not applied, cash
  cash dividend 0.5 per share, special, foreign, ex-date 2026-09-24 (extra): not applied, cash
  spin_off, ex-date 2026-09-25 (spin): not applied, not interpreted
  name_change, process date 2026-09-27 (name): not applied, not interpreted
gaps: not applicable to trades
duplicates: 1
  2026-09-21T13:30:00.000000001Z trade 7 x2 differing
problems: 6
  2026-09-21.parquet: missing
  2026-09-22.parquet: size or SHA-256 differs from the manifest
  2026-09-23.parquet: unreadable: parquet: bad footer
  2026-09-24.parquet: 3 rows, the manifest lists 4
  2026-09-25.parquet: 2 records outside 2026-09-25
  2026-10-01.parquet: not in the manifest
"
    );
}

#[test]
fn a_dataset_without_days_or_rows_says_so() {
    let found = Inspection {
        dataset: btc_hourly(),
        coverage: Coverage {
            span: None,
            listed: 0,
            empty: vec![],
            missing: vec![],
        },
        stats: None,
        corporate_actions: ActionsReport::NotApplicable,
        gaps: vec![],
        duplicates: vec![],
        problems: vec![],
    };
    assert_eq!(
        inspect::render(&found),
        "\
BTC/USD bars-1Hour (crypto-us)
coverage: no days listed
rows: 0
corporate actions: not applicable to crypto
gaps: 0
duplicates: 0
problems: 0
"
    );
}

fn spy_iex_hourly() -> DatasetId {
    id(
        AssetClass::UsEquity,
        Feed::Iex,
        Kind::Bars("1Hour".parse().unwrap()),
        "SPY",
    )
}

fn stretch(class: GapClass, first: &str, last: &str, slots: u64) -> Stretch {
    Stretch {
        class,
        first: time(first),
        last: time(last),
        slots,
    }
}

#[test]
fn each_gap_lists_its_missing_slots_by_class_with_a_total_per_class() {
    let found = Inspection {
        dataset: spy_iex_hourly(),
        coverage: Coverage {
            span: Some(range("2026-11-24", "2026-11-30")),
            listed: 5,
            empty: vec![],
            missing: vec![],
        },
        stats: None,
        corporate_actions: ActionsReport::NotRecorded,
        gaps: vec![
            ClassifiedGap {
                gap: Gap {
                    previous: time("2026-11-24T14:00:00.000000000Z"),
                    next: time("2026-11-24T16:00:00.000000000Z"),
                },
                stretches: vec![stretch(
                    GapClass::NoTrade,
                    "2026-11-24T15:00:00.000000000Z",
                    "2026-11-24T15:00:00.000000000Z",
                    1,
                )],
            },
            ClassifiedGap {
                gap: Gap {
                    previous: time("2026-11-24T21:00:00.000000000Z"),
                    next: time("2026-11-28T13:00:00.000000000Z"),
                },
                stretches: vec![
                    stretch(
                        GapClass::SessionClosure,
                        "2026-11-24T22:00:00.000000000Z",
                        "2026-11-25T12:00:00.000000000Z",
                        15,
                    ),
                    stretch(
                        GapClass::TrueGap,
                        "2026-11-25T13:00:00.000000000Z",
                        "2026-11-25T21:00:00.000000000Z",
                        9,
                    ),
                    stretch(
                        GapClass::Unclassified,
                        "2026-11-27T18:00:00.000000000Z",
                        "2026-11-28T00:00:00.000000000Z",
                        7,
                    ),
                ],
            },
        ],
        duplicates: vec![],
        problems: vec![],
    };
    assert_eq!(
        inspect::render(&found),
        "\
SPY bars-1Hour (iex)
coverage: 2026-11-24 to 2026-11-30, 5 days listed
rows: 0
corporate actions: not recorded with this dataset; no split-adjusted prices
gaps: 2, missing bar slots: 15 session closure, 1 no trade, 9 true gap, 7 unclassified
  2026-11-24T14:00:00.000000000Z to 2026-11-24T16:00:00.000000000Z
    no trade: 1 slot, 2026-11-24T15:00:00.000000000Z
  2026-11-24T21:00:00.000000000Z to 2026-11-28T13:00:00.000000000Z
    session closure: 15 slots, 2026-11-24T22:00:00.000000000Z to 2026-11-25T12:00:00.000000000Z
    true gap: 9 slots, 2026-11-25T13:00:00.000000000Z to 2026-11-25T21:00:00.000000000Z
    unclassified: 7 slots, 2026-11-27T18:00:00.000000000Z to 2026-11-28T00:00:00.000000000Z
duplicates: 0
problems: 0
"
    );
}

#[test]
fn actions_that_do_not_cover_the_span_are_reported_and_left_unapplied() {
    let found = Inspection {
        dataset: spy_trades(),
        coverage: Coverage {
            span: Some(range("2026-09-21", "2026-09-30")),
            listed: 10,
            empty: vec![],
            missing: vec![],
        },
        stats: None,
        corporate_actions: ActionsReport::Incomplete(spy_actions("2026-09-22", "2026-09-30")),
        gaps: vec![],
        duplicates: vec![],
        problems: vec![],
    };
    let report = inspect::render(&found);
    assert!(
        report.contains(
            "\ncorporate actions: recorded for 2026-09-22 to 2026-09-30, which does not cover the span; 6 actions, none applied\ngaps:"
        ),
        "{report}"
    );
}

fn parse(args: &[&str]) -> Result<InspectArgs, clap::Error> {
    let argv = ["mandate", "inspect"].iter().chain(args).copied();
    Cli::try_parse_from(argv).map(|cli| match cli.command {
        Command::Inspect(args) => args,
        Command::Download(_) => panic!("`inspect` parsed as `download`"),
    })
}

#[test]
fn a_run_reports_every_dataset_in_order_and_counts_the_problems() {
    let scratch = Scratch::new("run");
    let bars = bars_with_a_gap_and_a_duplicate(&scratch.0);
    let trades = stored(
        &scratch.0,
        &spy_trades(),
        vec![
            (
                "2026-09-24",
                Records::Trades(vec![trade("2026-09-24T14:00:00.000000000Z", 1)]),
            ),
            (
                "2026-09-25",
                Records::Trades(vec![trade("2026-09-25T14:00:00.000000000Z", 2)]),
            ),
        ],
    );
    fs::write(trades.join("2026-09-25.parquet"), b"altered").unwrap();
    let args = parse(&[trades.to_str().unwrap(), bars.to_str().unwrap()]).unwrap();
    let mut report = Vec::new();
    let problems = inspect::run(&args, &mut report).unwrap();
    assert_eq!(problems, 1);
    let expected = format!(
        "\
SPY trades (iex)
coverage: 2026-09-24 to 2026-09-25, 2 days listed
rows: 1, first 2026-09-24T14:00:00.000000000Z, last 2026-09-24T14:00:00.000000000Z
trades: low 81.19, high 81.19, size 100
corporate actions: not recorded with this dataset; no split-adjusted prices
gaps: not applicable to trades
duplicates: 0
problems: 1
  2026-09-25.parquet: size or SHA-256 differs from the manifest

{BARS_REPORT}
total: 2 datasets, 1 problems
"
    );
    assert_eq!(String::from_utf8(report).unwrap(), expected);
}

#[test]
fn a_directory_without_a_manifest_fails_the_run_and_names_the_path() {
    let scratch = Scratch::new("no-manifest");
    let path = scratch.0.to_str().unwrap();
    let args = parse(&[path]).unwrap();
    let err = inspect::run(&args, &mut Vec::new()).unwrap_err();
    assert!(format!("{err:#}").contains(path), "{err:#}");
}

#[test]
fn inspect_takes_one_or_more_dataset_directories() {
    assert!(parse(&[]).is_err());
    assert_eq!(
        parse(&["a", "b/c"]).unwrap().datasets,
        [PathBuf::from("a"), PathBuf::from("b/c")]
    );
}

fn cphc_quotes() -> DatasetId {
    id(AssetClass::UsEquity, Feed::Sip, Kind::Quotes, "CPHC")
}

fn quote_inspection(values: Values, corporate_actions: ActionsReport) -> Inspection {
    Inspection {
        dataset: cphc_quotes(),
        coverage: Coverage {
            span: Some(range("2026-09-23", "2026-09-24")),
            listed: 2,
            empty: vec![range("2026-09-23", "2026-09-23")],
            missing: vec![],
        },
        stats: Some(Stats {
            rows: 4,
            first: time("2026-09-24T14:00:00.000000000Z"),
            last: time("2026-09-24T14:00:03.000000000Z"),
            values,
            adjusted: match corporate_actions {
                ActionsReport::Applied { .. } => Some(AdjustedPrices {
                    low: dec("2.975"),
                    high: dec("3.35"),
                }),
                _ => None,
            },
        }),
        corporate_actions,
        gaps: vec![],
        duplicates: vec![],
        problems: vec![],
    }
}

#[test]
fn the_quotes_report_gives_each_side_the_spread_and_the_counts() {
    let found = quote_inspection(
        Values::Quotes {
            bid: Some(Extent {
                low: dec("11.9"),
                high: dec("12.52"),
                rows: 4,
            }),
            ask: Some(Extent {
                low: dec("12.51"),
                high: dec("13.4"),
                rows: 3,
            }),
            spread: Some(Spread {
                narrowest: dec("-0.01"),
                widest: dec("0.01"),
                total: dec("0"),
                rows: 3,
            }),
            locked: 1,
            crossed: 1,
            one_sided: 1,
            unquoted: 0,
        },
        ActionsReport::NotRecorded,
    );
    assert_eq!(
        inspect::render(&found),
        "\
CPHC quotes (sip)
coverage: 2026-09-23 to 2026-09-24, 2 days listed
  empty: 2026-09-23
rows: 4, first 2026-09-24T14:00:00.000000000Z, last 2026-09-24T14:00:03.000000000Z
quotes: 3 two-sided (1 locked, 1 crossed), 1 one-sided, 0 with neither side quoted
  bid: low 11.9, high 12.52 over 4 rows
  ask: low 12.51, high 13.4 over 3 rows
  spread: narrowest -0.01, widest 0.01, total 0 over 3 rows
corporate actions: not recorded with this dataset; no split-adjusted prices
gaps: not applicable to quotes
duplicates: 0
problems: 0
"
    );
}

#[test]
fn a_quotes_report_says_which_side_no_row_quotes() {
    let actions = ActionsReport::Applied {
        recorded: RecordedActions {
            range: range("2026-09-23", "2026-09-24"),
            actions: CorporateActions {
                symbol: Symbol::parse("CPHC").unwrap(),
                splits: vec![Split {
                    id: "cphc-split".to_owned(),
                    ex_date: day("2026-09-24"),
                    ratio: split_ratio(4, 1).unwrap(),
                }],
                cash_dividends: vec![],
                other: vec![],
            },
        },
        as_of: day("2026-09-24"),
    };
    let found = quote_inspection(
        Values::Quotes {
            bid: None,
            ask: Some(Extent {
                low: dec("11.9"),
                high: dec("13.4"),
                rows: 3,
            }),
            spread: None,
            locked: 0,
            crossed: 0,
            one_sided: 3,
            unquoted: 1,
        },
        actions,
    );
    assert_eq!(
        inspect::render(&found),
        "\
CPHC quotes (sip)
coverage: 2026-09-23 to 2026-09-24, 2 days listed
  empty: 2026-09-23
rows: 4, first 2026-09-24T14:00:00.000000000Z, last 2026-09-24T14:00:03.000000000Z
quotes: 0 two-sided (0 locked, 0 crossed), 3 one-sided, 1 with neither side quoted
  bid: no row quotes it
  ask: low 11.9, high 13.4 over 3 rows
  spread: no row quotes both sides
split-adjusted as of 2026-09-24: low 2.975, high 3.35
corporate actions: recorded for 2026-09-23 to 2026-09-24, applied as of 2026-09-24
  split 4:1, ex-date 2026-09-24 (cphc-split): applied
gaps: not applicable to quotes
duplicates: 0
problems: 0
"
    );
}
