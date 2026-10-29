//! An instrument's last trusted daily close (§3.11 item 11, §4.1,
//! [DEC-2152](../../../docs/project/decisions/DEC-2152.md)): the bars the workspace's data process
//! stores, read from the root the process is given in `MANDATE_BARS`, through `mandate-bars`' one
//! trust rule (DEC-2385) at the API's clock. It is the close of the last completed regular
//! session, never a bid, a quote or a current price; when no trusted bar can be served it says
//! why, and never serves a stub.

use std::path::Path;

use mandate_bars::generations::{daily_dir_at, is_generational};
use mandate_bars::trusted_daily_bars;
use mandate_marketdata::dataset::MANIFEST;
use mandate_time::UtcNanos;

/// The one asset class the stored bars and their trust rule cover: the equity calendar's.
const COVERED_CLASS: &str = "us_equity";

/// Where the bars came from: a generations root (the data process's `alpaca-iex` store on
/// staging), or any other root (the local cell's recording).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    AlpacaIex,
    Recorded,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AlpacaIex => "alpaca-iex",
            Self::Recorded => "recorded",
        }
    }
}

/// Why no trusted bar is served (`read-models/instrument-price`'s `reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreported {
    /// No bars root, or one that does not exist.
    NoStore,
    /// An asset class other than `us_equity`.
    NotCovered,
    /// No dataset manifest where the root and the clock name the symbol's daily bars.
    NoBars,
    /// Anything `mandate-bars` refuses or cannot read.
    Untrusted,
}

impl Unreported {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoStore => "no_store",
            Self::NotCovered => "not_covered",
            Self::NoBars => "no_bars",
            Self::Untrusted => "untrusted",
        }
    }
}

/// The last trusted daily bar's close and start, as stored, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Close {
    pub close: String,
    pub bar_start: UtcNanos,
    pub source: Source,
}

/// The last trusted daily bar of `symbol`, an instrument of `asset_class`, under `root` at `now`;
/// else why none is served, tried in the spec's order: the store, the asset class, the dataset's
/// manifest, then the trust rule.
pub fn last_close(
    root: Option<&Path>,
    symbol: &str,
    asset_class: Option<&str>,
    now: UtcNanos,
) -> Result<Close, Unreported> {
    let root = root
        .filter(|root| root.is_dir())
        .ok_or(Unreported::NoStore)?;
    if asset_class != Some(COVERED_CLASS) {
        return Err(Unreported::NotCovered);
    }
    let dir = daily_dir_at(root, symbol, now).map_err(|_| Unreported::Untrusted)?;
    if !dir.join(MANIFEST).is_file() {
        return Err(Unreported::NoBars);
    }
    let bars = trusted_daily_bars(&dir, symbol, now).map_err(|_| Unreported::Untrusted)?;
    let last = bars.last().ok_or(Unreported::Untrusted)?;
    Ok(Close {
        close: last.close.as_str().to_owned(),
        bar_start: last.start,
        source: if is_generational(root) {
            Source::AlpacaIex
        } else {
            Source::Recorded
        },
    })
}
