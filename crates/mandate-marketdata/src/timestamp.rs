//! Vendor timestamps and UTC days. Alpaca writes RFC 3339 in UTC with zero to nine fractional
//! digits; they become [`UtcNanos`] exactly. Parquet stores nanoseconds since the epoch in an
//! `i64`, so conversion checks the range (DEC-87).

use mandate_time::{Date, TimeError, UtcNanos};

const NANO_DIGITS: usize = 9;
const SECONDS_LEN: usize = "YYYY-MM-DDTHH:MM:SS".len();
const SECS_PER_DAY: i64 = 86_400;
const NANOS_PER_SEC: i64 = 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TimestampError {
    #[error("not an RFC 3339 UTC timestamp (YYYY-MM-DDTHH:MM:SS[.f{{1,9}}]Z)")]
    Syntax,
    #[error("invalid time: {0}")]
    Time(TimeError),
    #[error("outside the range of nanoseconds since 1970 in a signed 64-bit integer")]
    Overflow,
}

impl TimestampError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Time(_) => "time",
            Self::Overflow => "overflow",
        }
    }
}

/// Parses `YYYY-MM-DDTHH:MM:SS[.f]Z` with one to nine fractional digits.
pub fn parse_rfc3339_utc(raw: &str) -> Result<UtcNanos, TimestampError> {
    let body = raw.strip_suffix('Z').ok_or(TimestampError::Syntax)?;
    let (seconds, fraction) = match body.split_once('.') {
        Some((seconds, fraction)) => (seconds, fraction),
        None => (body, ""),
    };
    let fraction_ok = !body.contains('.')
        || (!fraction.is_empty()
            && fraction.len() <= NANO_DIGITS
            && fraction.bytes().all(|b| b.is_ascii_digit()));
    if seconds.len() != SECONDS_LEN || !fraction_ok {
        return Err(TimestampError::Syntax);
    }
    let canonical = format!("{seconds}.{fraction:0<NANO_DIGITS$}Z");
    UtcNanos::parse(&canonical).map_err(TimestampError::Time)
}

/// Midnight UTC at the start of `day`.
pub fn day_start(day: Date) -> Result<UtcNanos, TimestampError> {
    UtcNanos::parse(&format!("{day}T00:00:00.000000000Z")).map_err(TimestampError::Time)
}

/// The calendar day after `day`.
pub fn next_day(day: Date) -> Result<Date, TimestampError> {
    let secs = day_start(day)?
        .secs()
        .checked_add(SECS_PER_DAY)
        .ok_or(TimestampError::Overflow)?;
    UtcNanos::from_parts(secs, 0)
        .map(UtcNanos::date)
        .map_err(|_| TimestampError::Overflow)
}

/// Nanoseconds since 1970-01-01T00:00:00Z.
pub fn to_unix_nanos(time: UtcNanos) -> Result<i64, TimestampError> {
    time.secs()
        .checked_mul(NANOS_PER_SEC)
        .and_then(|n| n.checked_add(i64::from(time.nanos())))
        .ok_or(TimestampError::Overflow)
}

/// The inverse of [`to_unix_nanos`].
pub fn from_unix_nanos(nanos: i64) -> Result<UtcNanos, TimestampError> {
    if nanos < 0 {
        return Err(TimestampError::Overflow);
    }
    let secs = nanos
        .checked_div(NANOS_PER_SEC)
        .ok_or(TimestampError::Overflow)?;
    let sub = nanos
        .checked_rem(NANOS_PER_SEC)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(TimestampError::Overflow)?;
    UtcNanos::from_parts(secs, sub).map_err(TimestampError::Time)
}
