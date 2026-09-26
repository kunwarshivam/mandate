//! The report: both metric blocks, the digests of what produced them, and the canonical bytes whose
//! SHA-256 is a run's identity (FR-4.5, DEC-127 item 15).

use crate::metrics::Metrics;
use crate::{BacktestError, RunConfig};
use mandate_canon::{Digest, Value};
use mandate_num::Ratio;
use mandate_sim::SimBar;

/// The sign of a Sharpe's excess mean, reported beside the squared figure because squaring loses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sign {
    Negative,
    Zero,
    Positive,
}

impl Sign {
    /// The canonical spelling in a report: `negative`, `zero`, or `positive`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Negative => "negative",
            Self::Zero => "zero",
            Self::Positive => "positive",
        }
    }
}

/// Why a block's dispersion or Sharpe figures are absent (DEC-127 item 7). `ZeroVariance` covers a
/// positive exact variance that rounds to zero at 12 places, not only a genuinely flat series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbsentStatistics {
    FewerThanTwoPeriods,
    ZeroVariance,
}

impl AbsentStatistics {
    /// The canonical spelling in a report.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FewerThanTwoPeriods => "fewer_than_two_periods",
            Self::ZeroVariance => "zero_variance",
        }
    }
}

/// The digests that say which snapshot and which configuration a report came from (FR-4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputDigests {
    pub bars: Digest,
    pub config: Digest,
}

impl InputDigests {
    /// The digest of the bars' canonical form and of the configuration's, each over
    /// `mandate_canon::to_canonical` of a value holding every field as canonical text.
    pub fn of(bars: &[SimBar], config: &RunConfig) -> Result<Self, BacktestError> {
        let _ = (bars, config);
        Err(BacktestError::Unimplemented)
    }
}

/// One run's report: the strategy's figures, the buy-and-hold benchmark's, and the exact difference
/// of their total returns. `report_version` rises whenever a definition changes, as the fold's
/// `fold_version` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub report_version: u32,
    pub inputs: InputDigests,
    pub strategy: Metrics,
    pub benchmark: Metrics,
    pub excess_total_return: Ratio,
}

/// The version this crate writes. A change to any figure's definition raises it, so a report always
/// says which rules produced it.
pub const REPORT_VERSION: u32 = 1;

impl Report {
    /// The report as canonical JSON: decimals as the canonical text their `Display` produces, counts
    /// and versions as integers, dates as `YYYY-MM-DD`, the sign and the absence reason as their
    /// strings, and an absent figure as `null`. Keys sort themselves, because a canonical object is a
    /// `BTreeMap`.
    pub fn canonical(&self) -> Result<Value, BacktestError> {
        Err(BacktestError::Unimplemented)
    }

    /// `to_canonical` of [`Report::canonical`]: the bytes two runs of the same inputs must match
    /// byte for byte (the story's acceptance criterion).
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, BacktestError> {
        Err(BacktestError::Unimplemented)
    }

    /// The SHA-256 of those bytes: a run's identity, and what a `BacktestRunRecorded` event will
    /// carry once a runner journals it (journal spec §12).
    pub fn digest(&self) -> Result<Digest, BacktestError> {
        Err(BacktestError::Unimplemented)
    }
}
