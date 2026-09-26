//! `mandate download`: argument validation, the download plan, and a replayed run (backlog E2-1).
//! No test here opens a connection.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clap::Parser;
use mandate_cli::download::{self, Plan, Totals};
use mandate_cli::{Cli, Command};
use mandate_marketdata::client::{Client, Pause, Response, Transport, TransportError};
use mandate_marketdata::model::{AssetClass, Feed, Kind};
use mandate_time::Date;

fn day(s: &str) -> Date {
    Date::parse(s).unwrap()
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn parse(args: &[&str]) -> Result<download::DownloadArgs, clap::Error> {
    let argv = ["mandate", "download"].iter().chain(args).copied();
    Cli::try_parse_from(argv).map(|cli| match cli.command {
        Command::Download(args) => args,
        Command::Inspect(_) => panic!("`download` parsed as `inspect`"),
    })
}

fn plan(args: &[&str]) -> anyhow::Result<Plan> {
    download::plan(&parse(args)?, day("2026-09-26"))
}

fn err(args: &[&str]) -> String {
    format!("{:#}", plan(args).unwrap_err())
}

#[test]
fn the_research_basket_plans_one_dataset_per_symbol() {
    let basket = workspace().join("config/research-basket.toml");
    let basket = basket.to_str().unwrap();
    let p = plan(&[
        "--basket",
        basket,
        "--asset-class",
        "us-equity",
        "--kind",
        "bars",
        "--timeframe",
        "1Min",
        "--feed",
        "sip",
        "--start",
        "2026-09-21",
        "--end",
        "2026-09-25",
        "--out",
        "data",
    ])
    .unwrap();
    let symbols: Vec<&str> = p.datasets.iter().map(|d| d.symbol().as_str()).collect();
    assert_eq!(
        symbols,
        [
            "SPY", "QQQ", "IWM", "TLT", "GLD", "AAPL", "MSFT", "NVDA", "AMZN", "JPM"
        ]
    );
    assert!(p.datasets.iter().all(|d| d.feed() == Feed::Sip
        && d.asset_class() == AssetClass::UsEquity
        && d.kind().dir_name() == "bars-1Min"));
    assert_eq!(p.days.days().unwrap().len(), 5);
    assert_eq!(p.out, PathBuf::from("data"));

    let p = plan(&[
        "--basket",
        basket,
        "--asset-class",
        "crypto",
        "--kind",
        "trades",
        "--start",
        "2026-09-24",
        "--end",
        "2026-09-24",
        "--out",
        "data",
    ])
    .unwrap();
    assert_eq!(p.datasets.len(), 1);
    assert_eq!(p.datasets[0].symbol().as_str(), "BTC/USD");
    assert_eq!(p.datasets[0].feed(), Feed::CryptoUs);
    assert_eq!(p.datasets[0].kind(), Kind::Trades);
}

const STOCK_TRADES: [&str; 10] = [
    "--symbols",
    "SPY,QQQ",
    "--asset-class",
    "us-equity",
    "--kind",
    "trades",
    "--start",
    "2026-09-24",
    "--end",
    "2026-09-24",
];

fn with(extra: &[&'static str]) -> Vec<&'static str> {
    STOCK_TRADES
        .iter()
        .chain(["--out", "data"].iter())
        .chain(extra)
        .copied()
        .collect()
}

#[test]
fn stocks_need_an_explicit_feed_and_crypto_takes_only_its_own() {
    assert!(err(&with(&[])).contains("--feed"));
    assert_eq!(plan(&with(&["--feed", "iex"])).unwrap().datasets.len(), 2);
    assert!(err(&with(&["--feed", "crypto-us"])).contains("crypto-us"));
    let crypto = [
        "--symbols",
        "BTC/USD",
        "--asset-class",
        "crypto",
        "--kind",
        "trades",
        "--start",
        "2026-09-24",
        "--end",
        "2026-09-24",
        "--out",
        "data",
    ];
    let sip: Vec<&str> = crypto.iter().chain(&["--feed", "sip"]).copied().collect();
    assert!(err(&sip).contains("sip"));
    assert_eq!(
        plan(&crypto).unwrap().datasets[0].feed(),
        Feed::CryptoUs,
        "crypto defaults to its only feed"
    );
}

#[test]
fn a_timeframe_goes_with_bars_and_only_bars() {
    assert!(err(&with(&["--feed", "iex", "--timeframe", "1Min"])).contains("--timeframe"));
    let bars: Vec<&str> = with(&["--feed", "iex"])
        .into_iter()
        .map(|a| if a == "trades" { "bars" } else { a })
        .collect();
    assert!(err(&bars).contains("--timeframe"));
    let bad: Vec<&str> = bars
        .iter()
        .chain(&["--timeframe", "60Min"])
        .copied()
        .collect();
    assert!(err(&bad).contains("60Min"));
}

#[test]
fn only_complete_past_utc_days_are_downloaded() {
    let range = |start: &'static str, end: &'static str| {
        let mut args = with(&["--feed", "iex"]);
        args[7] = start;
        args[9] = end;
        args
    };
    assert!(plan(&range("2026-09-24", "2026-09-25")).is_ok());
    assert!(err(&range("2026-09-24", "2026-09-26")).contains("2026-09-26"));
    assert!(err(&range("2026-09-25", "2026-09-24")).contains("2026-09-24"));
    assert!(parse(&range("2026-09-24", "2026-9-25")).is_err());
}

#[test]
fn symbols_are_validated_and_listed_once() {
    let mut args = with(&["--feed", "iex"]);
    args[1] = "spy";
    assert!(err(&args).contains("spy"));
    args[1] = "SPY,SPY";
    assert!(err(&args).contains("SPY"));
    args[1] = "BTC/USD";
    assert!(err(&args).contains("BTC/USD"));
}

#[test]
fn symbols_come_from_exactly_one_source() {
    let mut neither = with(&["--feed", "iex"]);
    neither.drain(0..2);
    assert!(parse(&neither).is_err());
    let both: Vec<&str> = with(&["--feed", "iex", "--basket", "b.toml"]);
    assert!(parse(&both).is_err());
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("mandate-cli-test-{label}-{}", std::process::id()));
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

#[test]
fn a_basket_without_the_asset_class_or_with_unknown_keys_is_refused() {
    let scratch = Scratch::new("basket");
    let run = |text: &str| {
        let path = scratch.0.join("basket.toml");
        fs::write(&path, text).unwrap();
        let args = [
            "--basket",
            path.to_str().unwrap(),
            "--asset-class",
            "crypto",
            "--kind",
            "trades",
            "--start",
            "2026-09-24",
            "--end",
            "2026-09-24",
            "--out",
            "data",
        ];
        plan(&args).map(|p| p.datasets.len())
    };
    assert_eq!(run("[crypto]\nsymbols = [\"BTC/USD\"]\n").unwrap(), 1);
    assert!(run("[us-equity]\nsymbols = [\"SPY\"]\n").is_err());
    assert!(run("[crypto]\nsymbols = [\"BTC/USD\"]\nextra = 1\n").is_err());
    assert!(run("[crypto]\nsymbols = []\n").is_err());
}

#[derive(Clone, Default)]
struct Replay(Arc<Mutex<Vec<Vec<u8>>>>);

impl Transport for Replay {
    async fn get(&self, _path_and_query: &str) -> Result<Response, TransportError> {
        let mut bodies = self.0.lock().unwrap();
        if bodies.is_empty() {
            return Err(TransportError::Request);
        }
        Ok(Response {
            status: 200,
            body: bodies.remove(0),
        })
    }
}

struct NoPause;

impl Pause for NoPause {
    async fn pause(&self, _duration: std::time::Duration) {}
}

fn recorded(name: &str) -> Vec<u8> {
    fs::read(
        workspace()
            .join("crates/mandate-marketdata/tests/fixtures/alpaca")
            .join(name)
            .join("page-1.json"),
    )
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn a_run_reports_each_partition_and_a_rerun_reports_no_change() {
    let scratch = Scratch::new("run");
    let out = scratch.0.to_str().unwrap().to_owned();
    let args = [
        "--symbols",
        "SPY",
        "--asset-class",
        "us-equity",
        "--kind",
        "bars",
        "--timeframe",
        "1Hour",
        "--feed",
        "sip",
        "--start",
        "2026-09-23",
        "--end",
        "2026-09-24",
        "--out",
        &out,
    ];
    let p = plan(&args).unwrap();
    let bodies = || {
        vec![
            recorded("stock-bars-sip-spy-1hour-2026-09-23"),
            recorded("stock-bars-sip-spy-1hour-2026-09-24"),
            br#"{"corporate_actions":{"forward_splits":[{"id":"spy-split","symbol":"SPY","cusip":"78462F103","new_rate":2,"old_rate":1,"process_date":"2026-09-24","ex_date":"2026-09-24","record_date":"2026-09-23","payable_date":"2026-09-23"}]},"next_page_token":null}"#.to_vec(),
        ]
    };

    let mut report = Vec::new();
    let client = Client::new(Replay(Arc::new(Mutex::new(bodies()))), NoPause);
    let first = download::run(&p, &client, &mut report).await.unwrap();
    assert_eq!(
        first,
        Totals {
            datasets: 1,
            days: 2,
            rows: 32,
            bytes: first.bytes,
            written: 2,
            unchanged: 0,
        }
    );
    assert!(first.bytes > 0);
    let report = String::from_utf8(report).unwrap();
    let lines: Vec<&str> = report.lines().collect();
    assert_eq!(lines.len(), 4, "{report}");
    assert!(lines[0].starts_with("SPY bars-1Hour (sip) 2026-09-23 16 rows "));
    assert!(lines[0].ends_with(" written"));
    assert!(lines[0].contains(" sha256 "));
    assert_eq!(
        lines[2],
        "SPY bars-1Hour (sip) corporate actions 2026-09-23 to 2026-09-24: 1 splits, 0 cash dividends, 0 other written"
    );
    assert!(lines[3].starts_with("total: 1 datasets, 2 days, 32 rows"));
    let stored = fs::read_to_string(
        scratch
            .0
            .join("alpaca/sip/bars-1Hour/SPY/corporate-actions.json"),
    )
    .unwrap();
    assert!(stored.contains(r#""id":"spy-split""#), "{stored}");

    let mut report = Vec::new();
    let client = Client::new(Replay(Arc::new(Mutex::new(bodies()))), NoPause);
    let second = download::run(&p, &client, &mut report).await.unwrap();
    assert_eq!((second.written, second.unchanged), (0, 2));
    assert_eq!(second.bytes, first.bytes);
    let report = String::from_utf8(report).unwrap();
    assert!(
        report.lines().take(3).all(|l| l.ends_with(" unchanged")),
        "{report}"
    );
}
