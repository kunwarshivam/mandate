//! `mandate inspect`: the text report, the problem count behind the exit status, and argument
//! parsing (backlog E2-2).

use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use mandate_canon::DecStr;
use mandate_cli::inspect::{self, InspectArgs};
use mandate_cli::{Cli, Command};
use mandate_marketdata::dataset::Store;
use mandate_marketdata::inspect::{Coverage, Duplicate, Inspection, Problem, Stats, Values};
use mandate_marketdata::model::{
    AssetClass, Bar, DatasetId, DayRange, Feed, Kind, Records, Symbol, Trade,
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
gaps: 1 (the starts of the bars on either side; not yet classified by market session)
  2026-09-21T01:00:00.000000000Z to 2026-09-24T02:00:00.000000000Z
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

#[test]
fn every_problem_and_trade_duplicate_has_a_line() {
    let range = |a: &str, b: &str| DayRange::new(day(a), day(b)).unwrap();
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
        }),
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
gaps: 0
duplicates: 0
problems: 0
"
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
