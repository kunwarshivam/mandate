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
        let allowed = |b: u8| b.is_ascii_uppercase() || b.is_ascii_digit() || b"./-".contains(&b);
        if !(1..=12).contains(&text.len()) || !text.bytes().all(allowed) {
            return Err(SourceError::BadField);
        }
        Ok(Self(text.to_owned()))
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
        if !(1..=10).contains(&text.len()) || !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(SourceError::BadField);
        }
        text.parse().map(Self).map_err(|_| SourceError::BadField)
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
        self.from <= at && self.until.is_none_or(|until| at < until)
    }

    fn is_empty(self) -> bool {
        self.until.is_some_and(|until| until <= self.from)
    }

    fn overlaps(self, other: Self) -> bool {
        self.until.is_none_or(|until| other.from < until)
            && other.until.is_none_or(|until| self.from < until)
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
        check_rows(&rows, |(symbol, _, validity)| (symbol.clone(), *validity))?;
        Ok(Self { version, rows })
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// The instrument `symbol` named at `at`, if any.
    pub fn resolve(&self, symbol: &Symbol, at: UtcNanos) -> Option<&AssetId> {
        self.rows
            .iter()
            .find(|(s, _, validity)| s == symbol && validity.contains(at))
            .map(|(_, asset, _)| asset)
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
        check_rows(&rows, |(cik, symbol, validity)| {
            ((*cik, symbol.clone()), *validity)
        })?;
        Ok(Self { version, rows })
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// The tickers `cik` filed under at `at`, in order.
    pub fn symbols(&self, cik: Cik, at: UtcNanos) -> Vec<&Symbol> {
        self.rows
            .iter()
            .filter(|(c, _, validity)| *c == cik && validity.contains(at))
            .map(|(_, symbol, _)| symbol)
            .collect()
    }
}

/// Refuses an empty validity, and two rows with one key whose validities overlap.
fn check_rows<R, K: PartialEq>(
    rows: &[R],
    key: impl Fn(&R) -> (K, Validity),
) -> Result<(), SourceError> {
    for (i, row) in rows.iter().enumerate() {
        let (k, validity) = key(row);
        if validity.is_empty() {
            return Err(SourceError::BadField);
        }
        let clash = rows.iter().skip(i.saturating_add(1)).any(|other| {
            let (other_key, other_validity) = key(other);
            other_key == k && validity.overlaps(other_validity)
        });
        if clash {
            return Err(SourceError::BadField);
        }
    }
    Ok(())
}
