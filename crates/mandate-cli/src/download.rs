//! `mandate download`: validate the request, plan one dataset per symbol, and store every UTC day
//! of each, printing one line per partition and a total.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::{Args, ValueEnum};
use mandate_marketdata::client::{Client, Pause, Transport};
use mandate_marketdata::dataset::{Outcome, Status, Store};
use mandate_marketdata::download::{describe, download};
use mandate_marketdata::model::{AssetClass, DatasetId, DayRange, Feed, Kind, Symbol, Timeframe};
use mandate_time::Date;
use serde::Deserialize;

/// Bars or trades.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum KindArg {
    Bars,
    Trades,
}

#[derive(Debug, Args)]
pub struct DownloadArgs {
    /// Comma-separated symbols, e.g. `SPY,QQQ` or `BTC/USD`.
    #[arg(
        long,
        value_delimiter = ',',
        required_unless_present = "basket",
        conflicts_with = "basket"
    )]
    pub symbols: Vec<String>,
    /// A basket file whose table for `--asset-class` lists the symbols.
    #[arg(long)]
    pub basket: Option<PathBuf>,
    /// `us-equity` (stocks and ETFs) or `crypto`.
    #[arg(long)]
    pub asset_class: AssetClass,
    #[arg(long, value_enum)]
    pub kind: KindArg,
    /// Bar width for `--kind bars`: 1-59Min, 1-23Hour, or 1Day.
    #[arg(long)]
    pub timeframe: Option<Timeframe>,
    /// `sip` or `iex` for stocks, required; crypto uses `crypto-us`.
    #[arg(long)]
    pub feed: Option<Feed>,
    /// First UTC day, inclusive (YYYY-MM-DD).
    #[arg(long)]
    pub start: Date,
    /// Last UTC day, inclusive; it must be before today's UTC date.
    #[arg(long)]
    pub end: Date,
    /// Output root; datasets go under `<out>/alpaca/<feed>/<kind>/<symbol>`.
    #[arg(long)]
    pub out: PathBuf,
}

/// What a download will fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub datasets: Vec<DatasetId>,
    pub days: DayRange,
    pub out: PathBuf,
}

/// Counts over a whole run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub datasets: u64,
    pub days: u64,
    pub rows: u64,
    pub bytes: u64,
    pub written: u64,
    pub unchanged: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Basket {
    #[serde(rename = "us-equity")]
    us_equity: Option<Group>,
    crypto: Option<Group>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Group {
    symbols: Vec<String>,
}

fn basket_symbols(path: &Path, asset_class: AssetClass) -> anyhow::Result<Vec<String>> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let basket: Basket =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let group = match asset_class {
        AssetClass::UsEquity => basket.us_equity,
        AssetClass::Crypto => basket.crypto,
    };
    let group =
        group.with_context(|| format!("{} has no [{asset_class}] table", path.display()))?;
    Ok(group.symbols)
}

fn feed(args: &DownloadArgs) -> anyhow::Result<Feed> {
    let feed = match (args.asset_class, args.feed) {
        (_, Some(feed)) => feed,
        (AssetClass::Crypto, None) => Feed::CryptoUs,
        (AssetClass::UsEquity, None) => {
            bail!("us-equity needs --feed sip (backtests) or --feed iex (the paper profile)")
        }
    };
    if !feed.serves(args.asset_class) {
        bail!("feed {feed} does not serve {}", args.asset_class);
    }
    Ok(feed)
}

fn kind(args: &DownloadArgs) -> anyhow::Result<Kind> {
    match (args.kind, args.timeframe) {
        (KindArg::Bars, Some(timeframe)) => Ok(Kind::Bars(timeframe)),
        (KindArg::Bars, None) => bail!("--kind bars needs --timeframe (e.g. 1Min)"),
        (KindArg::Trades, Some(_)) => bail!("--timeframe applies only to --kind bars"),
        (KindArg::Trades, None) => Ok(Kind::Trades),
    }
}

/// Validates `args` against `today`, the current UTC date, and reads the basket if one is named.
pub fn plan(args: &DownloadArgs, today: Date) -> anyhow::Result<Plan> {
    let feed = feed(args)?;
    let kind = kind(args)?;
    let days = DayRange::new(args.start, args.end)?;
    if args.end >= today {
        bail!(
            "--end {} is not a complete UTC day yet; the last complete day is before {today}",
            args.end
        );
    }
    let names = match &args.basket {
        Some(path) => basket_symbols(path, args.asset_class)?,
        None => args.symbols.clone(),
    };
    if names.is_empty() {
        bail!("no symbols to download");
    }
    let mut seen = BTreeSet::new();
    let mut datasets = Vec::with_capacity(names.len());
    for name in names {
        let symbol = Symbol::parse(&name)?;
        if !seen.insert(symbol.clone()) {
            bail!("symbol {symbol} is listed twice");
        }
        datasets.push(DatasetId::new(args.asset_class, feed, kind, symbol)?);
    }
    Ok(Plan {
        datasets,
        days,
        out: args.out.clone(),
    })
}

fn line(dataset: &DatasetId, outcome: &Outcome) -> String {
    let file = match outcome.file {
        Some((bytes, digest)) => format!("{bytes} bytes sha256 {}", digest.to_hex()),
        None => "no file".to_owned(),
    };
    let status = match outcome.status {
        Status::Written => "written",
        Status::Unchanged => "unchanged",
    };
    format!(
        "{} {} {} rows {file} {status}",
        describe(dataset),
        outcome.day,
        outcome.rows
    )
}

impl Totals {
    fn add(&mut self, outcome: &Outcome) {
        self.days = self.days.saturating_add(1);
        self.rows = self.rows.saturating_add(outcome.rows);
        if let Some((bytes, _)) = outcome.file {
            self.bytes = self.bytes.saturating_add(bytes);
        }
        match outcome.status {
            Status::Written => self.written = self.written.saturating_add(1),
            Status::Unchanged => self.unchanged = self.unchanged.saturating_add(1),
        }
    }
}

/// Downloads every dataset of `plan`, writing one line per partition and a total to `report`.
pub async fn run<T: Transport, P: Pause>(
    plan: &Plan,
    client: &Client<T, P>,
    report: &mut impl Write,
) -> anyhow::Result<Totals> {
    let store = Store::new(&plan.out);
    let mut totals = Totals::default();
    for dataset in &plan.datasets {
        let mut lines = Ok(());
        download(client, &store, dataset, plan.days, |outcome| {
            totals.add(outcome);
            if lines.is_ok() {
                lines = writeln!(report, "{}", line(dataset, outcome));
            }
        })
        .await?;
        lines.context("writing the report")?;
        totals.datasets = totals.datasets.saturating_add(1);
    }
    writeln!(
        report,
        "total: {} datasets, {} days, {} rows, {} bytes, {} written, {} unchanged",
        totals.datasets, totals.days, totals.rows, totals.bytes, totals.written, totals.unchanged
    )
    .context("writing the report")?;
    Ok(totals)
}
