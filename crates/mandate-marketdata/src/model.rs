//! What a download names and returns: the dataset identity (asset class, feed, kind, symbol), the
//! day range, and the vendor's bar and trade records (trading domain spec §4.1). The records live
//! here with [`DecStr`] numbers until `mandate-domain` exists (DEC-89).

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use mandate_canon::DecStr;
use mandate_time::{Date, TimeError, UtcNanos};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("unknown asset class `{0}` (us-equity, crypto)")]
    AssetClass(String),
    #[error("unknown feed `{0}` (sip, iex, crypto-us)")]
    Feed(String),
    #[error("timeframe `{0}` is not 1-59Min, 1-23Hour, or 1Day")]
    Timeframe(String),
    #[error("symbol `{0}` is not an equity symbol (A-Z, 0-9, `.`) or a crypto pair (BASE/QUOTE)")]
    Symbol(String),
    #[error("feed {feed} does not serve {asset_class}")]
    FeedMismatch { asset_class: AssetClass, feed: Feed },
    #[error("symbol {symbol} is not a {asset_class} symbol")]
    SymbolMismatch {
        asset_class: AssetClass,
        symbol: Symbol,
    },
    #[error("the range ends ({last}) before it starts ({first})")]
    EmptyRange { first: Date, last: Date },
    #[error("day arithmetic failed: {0}")]
    Day(TimeError),
}

impl ModelError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::AssetClass(_) => "asset_class",
            Self::Feed(_) => "feed",
            Self::Timeframe(_) => "timeframe",
            Self::Symbol(_) => "symbol",
            Self::FeedMismatch { .. } => "feed_mismatch",
            Self::SymbolMismatch { .. } => "symbol_mismatch",
            Self::EmptyRange { .. } => "empty_range",
            Self::Day(_) => "day",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AssetClass {
    UsEquity,
    Crypto,
}

impl AssetClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UsEquity => "us-equity",
            Self::Crypto => "crypto",
        }
    }
}

impl fmt::Display for AssetClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AssetClass {
    type Err = ModelError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "us-equity" => Ok(Self::UsEquity),
            "crypto" => Ok(Self::Crypto),
            other => Err(ModelError::AssetClass(other.to_owned())),
        }
    }
}

/// The data profile a dataset comes from (trading domain spec §4.2): consolidated `sip` or `iex`
/// for stocks and ETFs, and Alpaca's US crypto location for crypto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Feed {
    Sip,
    Iex,
    CryptoUs,
}

impl Feed {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sip => "sip",
            Self::Iex => "iex",
            Self::CryptoUs => "crypto-us",
        }
    }

    pub fn serves(self, asset_class: AssetClass) -> bool {
        match self {
            Self::Sip | Self::Iex => asset_class == AssetClass::UsEquity,
            Self::CryptoUs => asset_class == AssetClass::Crypto,
        }
    }
}

impl fmt::Display for Feed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Feed {
    type Err = ModelError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "sip" => Ok(Self::Sip),
            "iex" => Ok(Self::Iex),
            "crypto-us" => Ok(Self::CryptoUs),
            other => Err(ModelError::Feed(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TimeUnit {
    Minute,
    Hour,
    Day,
}

/// A bar interval in Alpaca's notation (`1Min`, `15Min`, `1Hour`, `1Day`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timeframe {
    amount: u8,
    unit: TimeUnit,
}

impl Timeframe {
    pub fn new(amount: u8, unit: TimeUnit) -> Result<Self, ModelError> {
        let max = match unit {
            TimeUnit::Minute => 59,
            TimeUnit::Hour => 23,
            TimeUnit::Day => 1,
        };
        if amount == 0 || amount > max {
            let text = Self { amount, unit }.to_string();
            return Err(ModelError::Timeframe(text));
        }
        Ok(Self { amount, unit })
    }

    pub fn amount(self) -> u8 {
        self.amount
    }

    pub fn unit(self) -> TimeUnit {
        self.unit
    }
}

impl fmt::Display for Timeframe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let unit = match self.unit {
            TimeUnit::Minute => "Min",
            TimeUnit::Hour => "Hour",
            TimeUnit::Day => "Day",
        };
        write!(f, "{}{unit}", self.amount)
    }
}

impl FromStr for Timeframe {
    type Err = ModelError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || ModelError::Timeframe(s.to_owned());
        let digits = s.bytes().take_while(u8::is_ascii_digit).count();
        let (amount, unit) = s.split_at_checked(digits).ok_or_else(invalid)?;
        if amount.starts_with('0') {
            return Err(invalid());
        }
        let amount: u8 = amount.parse().map_err(|_| invalid())?;
        let unit = match unit {
            "Min" => TimeUnit::Minute,
            "Hour" => TimeUnit::Hour,
            "Day" => TimeUnit::Day,
            _ => return Err(invalid()),
        };
        Self::new(amount, unit).map_err(|_| invalid())
    }
}

/// What a dataset holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Bars(Timeframe),
    Trades,
}

impl Kind {
    /// The dataset directory name: `bars-<timeframe>` or `trades`.
    pub fn dir_name(self) -> String {
        match self {
            Self::Bars(timeframe) => format!("bars-{timeframe}"),
            Self::Trades => "trades".to_owned(),
        }
    }
}

/// A ticker (`SPY`, `BRK.B`) or a crypto pair (`BTC/USD`), upper case, as Alpaca names it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Symbol(String);

const MAX_SYMBOL_PART: usize = 15;

impl Symbol {
    pub fn parse(s: &str) -> Result<Self, ModelError> {
        let part_ok = |p: &str, dots: bool| {
            !p.is_empty()
                && p.len() <= MAX_SYMBOL_PART
                && p.bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || (dots && b == b'.'))
                && !p.starts_with('.')
                && !p.ends_with('.')
        };
        let valid = match s.split_once('/') {
            Some((base, quote)) => part_ok(base, false) && part_ok(quote, false),
            None => part_ok(s, true),
        };
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(ModelError::Symbol(s.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_pair(&self) -> bool {
        self.0.contains('/')
    }

    /// The directory name: the symbol with `/` written as `-` (`BTC-USD`).
    pub fn slug(&self) -> String {
        self.0.replace('/', "-")
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One dataset: a symbol's bars at one timeframe, or its trades, from one feed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DatasetId {
    asset_class: AssetClass,
    feed: Feed,
    kind: Kind,
    symbol: Symbol,
}

impl DatasetId {
    pub fn new(
        asset_class: AssetClass,
        feed: Feed,
        kind: Kind,
        symbol: Symbol,
    ) -> Result<Self, ModelError> {
        if !feed.serves(asset_class) {
            return Err(ModelError::FeedMismatch { asset_class, feed });
        }
        if symbol.is_pair() != (asset_class == AssetClass::Crypto) {
            return Err(ModelError::SymbolMismatch {
                asset_class,
                symbol,
            });
        }
        Ok(Self {
            asset_class,
            feed,
            kind,
            symbol,
        })
    }

    pub fn asset_class(&self) -> AssetClass {
        self.asset_class
    }

    pub fn feed(&self) -> Feed {
        self.feed
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// The dataset's directory below the output root:
    /// `alpaca/<feed>/<bars-<timeframe>|trades>/<symbol slug>`.
    pub fn relative_dir(&self) -> PathBuf {
        ["alpaca", self.feed.as_str()]
            .iter()
            .collect::<PathBuf>()
            .join(self.kind.dir_name())
            .join(self.symbol.slug())
    }
}

/// Whole UTC days, both ends inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayRange {
    first: Date,
    last: Date,
}

impl DayRange {
    pub fn new(first: Date, last: Date) -> Result<Self, ModelError> {
        if last < first {
            return Err(ModelError::EmptyRange { first, last });
        }
        Ok(Self { first, last })
    }

    pub fn first(self) -> Date {
        self.first
    }

    pub fn last(self) -> Date {
        self.last
    }

    /// Every day from `first` to `last`, in order.
    pub fn days(self) -> Result<Vec<Date>, ModelError> {
        let mut days = vec![self.first];
        let mut day = self.first;
        while day < self.last {
            day = day.next().map_err(ModelError::Day)?;
            days.push(day);
        }
        Ok(days)
    }
}

/// A bar (trading domain spec §4.1) as the vendor sent it; `start` is the left edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    pub start: UtcNanos,
    pub open: DecStr,
    pub high: DecStr,
    pub low: DecStr,
    pub close: DecStr,
    pub volume: DecStr,
    pub vwap: DecStr,
    pub trade_count: u64,
}

/// A trade (trading domain spec §4.1) as the vendor sent it. Stock trades carry `exchange`,
/// `conditions`, and `tape`; crypto trades carry `taker_side`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trade {
    pub time: UtcNanos,
    pub price: DecStr,
    pub size: DecStr,
    pub trade_id: u64,
    pub exchange: Option<String>,
    pub conditions: Option<Vec<String>>,
    pub tape: Option<String>,
    pub taker_side: Option<String>,
}

/// One day of one dataset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Records {
    Bars(Vec<Bar>),
    Trades(Vec<Trade>),
}

impl Records {
    pub fn empty(kind: Kind) -> Self {
        match kind {
            Kind::Bars(_) => Self::Bars(Vec::new()),
            Kind::Trades => Self::Trades(Vec::new()),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Bars(bars) => bars.len(),
            Self::Trades(trades) => trades.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Each record's timestamp, in order.
    pub fn times(&self) -> Vec<UtcNanos> {
        match self {
            Self::Bars(bars) => bars.iter().map(|b| b.start).collect(),
            Self::Trades(trades) => trades.iter().map(|t| t.time).collect(),
        }
    }
}
