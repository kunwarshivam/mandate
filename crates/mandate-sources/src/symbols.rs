//! Factual tagging tables (data-plane spec §4.3, DEC-433 item 10): a vendor's ticker resolved to an
//! `asset_id` as of the item's knowledge time, and a filer's CIK mapped to tickers through a
//! versioned table. Nothing here reads text.

use mandate_domain::AssetId;
use mandate_time::UtcNanos;

use crate::SourceError;

/// A ticker as a vendor lists it in a structured field: 1 to 12 characters of `A-Z`, `0-9`, `.`,
/// `/`, and `-`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Symbol(String);

impl Symbol {
    pub fn parse(text: &str) -> Result<Self, SourceError> {
        let _ = text;
        Err(SourceError::Unimplemented)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An SEC Central Index Key: 1 to 10 digits, held as its number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cik(u64);

impl Cik {
    /// Accepts 1 to 10 ASCII digits, leading zeros allowed, and nothing else.
    pub fn parse(text: &str) -> Result<Self, SourceError> {
        let _ = text;
        Err(SourceError::Unimplemented)
    }

    pub fn get(self) -> u64 {
        self.0
    }

    /// The ten-digit, zero-padded form EDGAR's submissions endpoint names.
    pub fn padded(self) -> String {
        format!("{:010}", self.0)
    }
}

/// When a mapping holds: from `from`, inclusive, until `until`, exclusive, or for good.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Validity {
    pub from: UtcNanos,
    pub until: Option<UtcNanos>,
}

impl Validity {
    pub fn contains(self, at: UtcNanos) -> bool {
        let _ = at;
        false
    }

    fn overlaps(self, other: Self) -> bool {
        let _ = other;
        false
    }
}

/// Ticker to `asset_id`, versioned, so a reused ticker never attaches an old item to a new company.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolTable {
    version: u32,
    rows: Vec<(Symbol, AssetId, Validity)>,
}

impl SymbolTable {
    /// Refuses two rows for one ticker whose validities overlap, and a validity that ends at or
    /// before it starts.
    pub fn new(version: u32, rows: Vec<(Symbol, AssetId, Validity)>) -> Result<Self, SourceError> {
        let _ = Validity::overlaps;
        let _ = rows;
        Ok(Self {
            version,
            rows: Vec::new(),
        })
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// The instrument `symbol` named at `at`, if any.
    pub fn resolve(&self, symbol: &Symbol, at: UtcNanos) -> Option<&AssetId> {
        let _ = (symbol, at);
        None
    }
}

/// Filer CIK to tickers, versioned (data-plane spec §4.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CikTable {
    version: u32,
    rows: Vec<(Cik, Symbol, Validity)>,
}

impl CikTable {
    /// Refuses a validity that ends at or before it starts, and the same CIK and ticker twice with
    /// overlapping validities.
    pub fn new(version: u32, rows: Vec<(Cik, Symbol, Validity)>) -> Result<Self, SourceError> {
        let _ = rows;
        Ok(Self {
            version,
            rows: Vec::new(),
        })
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// The tickers `cik` filed under at `at`, in order.
    pub fn symbols(&self, cik: Cik, at: UtcNanos) -> Vec<&Symbol> {
        let _ = (cik, at);
        Vec::new()
    }
}
