#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! UTC timestamps with nanosecond precision, and calendar dates, in the text forms fixed by the
//! [journal spec §4.7](../../../docs/specs/journal.md#4-canonical-serialization):
//! `YYYY-MM-DDTHH:MM:SS.nnnnnnnnnZ` (years 1970–9999, seconds 00–59, leap seconds smeared) and
//! `YYYY-MM-DD`. Core code never reads a clock (ADR-0001 ES-05): values arrive as inputs.
//!
//! API stubs for the E5-1 tests PR (DEC-77); the implementation PR replaces the bodies.

use core::fmt;
use core::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TimeError {
    #[error("not in the form YYYY-MM-DDTHH:MM:SS.nnnnnnnnnZ or YYYY-MM-DD")]
    Syntax,
    #[error("no such calendar date or time of day")]
    InvalidDate,
    #[error("outside 1970-01-01T00:00:00Z to 9999-12-31T23:59:59.999999999Z")]
    OutOfRange,
}

impl TimeError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::InvalidDate => "invalid_date",
            Self::OutOfRange => "out_of_range",
        }
    }
}

/// A calendar date, 1970-01-01 to 9999-12-31.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    pub fn new(_year: u16, _month: u8, _day: u8) -> Result<Self, TimeError> {
        Err(TimeError::Syntax)
    }

    pub fn year(self) -> u16 {
        self.year
    }

    pub fn month(self) -> u8 {
        self.month
    }

    pub fn day(self) -> u8 {
        self.day
    }

    /// Parses exactly `YYYY-MM-DD`.
    pub fn parse(_s: &str) -> Result<Self, TimeError> {
        Err(TimeError::Syntax)
    }
}

impl fmt::Display for Date {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

impl FromStr for Date {
    type Err = TimeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// A UTC instant with nanosecond precision, 1970-01-01T00:00:00Z to
/// 9999-12-31T23:59:59.999999999Z. Ordering is chronological.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcNanos {
    secs: i64,
    nanos: u32,
}

impl UtcNanos {
    pub const EPOCH: Self = Self { secs: 0, nanos: 0 };

    /// `secs` counts seconds since 1970-01-01T00:00:00Z without leap seconds.
    pub fn from_parts(_secs: i64, _nanos: u32) -> Result<Self, TimeError> {
        Err(TimeError::OutOfRange)
    }

    pub fn secs(self) -> i64 {
        self.secs
    }

    pub fn nanos(self) -> u32 {
        self.nanos
    }

    pub fn date(self) -> Date {
        Date {
            year: 1970,
            month: 1,
            day: 1,
        }
    }

    /// Parses exactly `YYYY-MM-DDTHH:MM:SS.nnnnnnnnnZ`.
    pub fn parse(_s: &str) -> Result<Self, TimeError> {
        Err(TimeError::Syntax)
    }
}

impl fmt::Display for UtcNanos {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

impl FromStr for UtcNanos {
    type Err = TimeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}
