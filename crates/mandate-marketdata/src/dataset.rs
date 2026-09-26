//! Parquet partitions (DEC-89): one UTC day of one dataset per file, with every decimal column a
//! `Decimal128(38, s)` at the scale [`decimal_scales`] names.

use std::path::PathBuf;

use crate::model::Kind;
use crate::number::NumberError;
use crate::timestamp::TimestampError;

mod partition;

pub use partition::{encode, read, schema};

/// Scale of trade prices (trading domain spec §2.1: at most 9 decimal places).
pub const PRICE_SCALE: u8 = 9;
/// Scale of trade sizes (trading domain spec §2.1: at most 9 decimal places).
pub const SIZE_SCALE: u8 = 9;
/// Scale of every bar column: bars are vendor aggregates that the spec does not bound, and
/// BTC/USD bars arrive with ten fractional digits in prices and VWAP (DEC-89).
pub const BAR_SCALE: u8 = 18;

#[derive(Debug, thiserror::Error)]
pub enum DatasetError {
    #[error("column `{column}` row {row}: {source}")]
    Number {
        column: &'static str,
        row: usize,
        source: NumberError,
    },
    #[error("row {row} time: {source}")]
    Time { row: usize, source: TimestampError },
    #[error("parquet: {0}")]
    Parquet(String),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl DatasetError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Number { .. } => "number",
            Self::Time { .. } => "time",
            Self::Parquet(_) => "parquet",
            Self::Io { .. } => "io",
        }
    }
}

/// The decimal columns of a kind and their scales.
pub fn decimal_scales(kind: Kind) -> &'static [(&'static str, u8)] {
    match kind {
        Kind::Bars(_) => &[
            ("open", BAR_SCALE),
            ("high", BAR_SCALE),
            ("low", BAR_SCALE),
            ("close", BAR_SCALE),
            ("volume", BAR_SCALE),
            ("vwap", BAR_SCALE),
        ],
        Kind::Trades => &[("price", PRICE_SCALE), ("size", SIZE_SCALE)],
    }
}
