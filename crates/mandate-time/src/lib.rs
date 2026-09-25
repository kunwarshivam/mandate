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

const MIN_YEAR: u16 = 1970;
const MAX_YEAR: u16 = 9999;
const SECS_PER_DAY: i64 = 86_400;
/// 9999-12-31T23:59:59Z.
const MAX_SECS: i64 = 253_402_300_799;
const MAX_NANOS: u32 = 999_999_999;

/// A calendar date, 1970-01-01 to 9999-12-31.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    pub fn new(year: u16, month: u8, day: u8) -> Result<Self, TimeError> {
        if !(MIN_YEAR..=MAX_YEAR).contains(&year) {
            return Err(TimeError::OutOfRange);
        }
        if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
            return Err(TimeError::InvalidDate);
        }
        Ok(Self { year, month, day })
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
    pub fn parse(s: &str) -> Result<Self, TimeError> {
        if !matches_template(s.as_bytes(), b"dddd-dd-dd") {
            return Err(TimeError::Syntax);
        }
        Self::new(field(s, 0, 4)?, field(s, 5, 7)?, field(s, 8, 10)?)
    }

    fn days_since_epoch(self) -> Option<i64> {
        let (m, d) = (i64::from(self.month), i64::from(self.day));
        let y = i64::from(self.year).checked_sub(i64::from(self.month <= 2))?;
        let era = y.checked_div(400)?;
        let yoe = y.checked_rem(400)?;
        let mp = if m > 2 {
            m.checked_sub(3)?
        } else {
            m.checked_add(9)?
        };
        let doy = mp
            .checked_mul(153)?
            .checked_add(2)?
            .checked_div(5)?
            .checked_add(d)?
            .checked_sub(1)?;
        let doe = yoe
            .checked_mul(365)?
            .checked_add(yoe.checked_div(4)?)?
            .checked_sub(yoe.checked_div(100)?)?
            .checked_add(doy)?;
        era.checked_mul(146_097)?
            .checked_add(doe)?
            .checked_sub(719_468)
    }

    fn from_days_since_epoch(days: i64) -> Option<Self> {
        let z = days.checked_add(719_468)?;
        let era = z.checked_div(146_097)?;
        let doe = z.checked_rem(146_097)?;
        let yoe = doe
            .checked_sub(doe.checked_div(1_460)?)?
            .checked_add(doe.checked_div(36_524)?)?
            .checked_sub(doe.checked_div(146_096)?)?
            .checked_div(365)?;
        let doy = doe.checked_sub(
            yoe.checked_mul(365)?
                .checked_add(yoe.checked_div(4)?)?
                .checked_sub(yoe.checked_div(100)?)?,
        )?;
        let mp = doy.checked_mul(5)?.checked_add(2)?.checked_div(153)?;
        let d = doy
            .checked_sub(mp.checked_mul(153)?.checked_add(2)?.checked_div(5)?)?
            .checked_add(1)?;
        let m = if mp < 10 {
            mp.checked_add(3)?
        } else {
            mp.checked_sub(9)?
        };
        let y = yoe
            .checked_add(era.checked_mul(400)?)?
            .checked_add(i64::from(m <= 2))?;
        Self::new(
            u16::try_from(y).ok()?,
            u8::try_from(m).ok()?,
            u8::try_from(d).ok()?,
        )
        .ok()
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
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
    pub fn from_parts(secs: i64, nanos: u32) -> Result<Self, TimeError> {
        if !(0..=MAX_SECS).contains(&secs) || nanos > MAX_NANOS {
            return Err(TimeError::OutOfRange);
        }
        Ok(Self { secs, nanos })
    }

    pub fn secs(self) -> i64 {
        self.secs
    }

    pub fn nanos(self) -> u32 {
        self.nanos
    }

    pub fn date(self) -> Date {
        // from_parts bounds secs, so the conversion cannot fail; EPOCH's date is the fallback.
        self.secs
            .checked_div(SECS_PER_DAY)
            .and_then(Date::from_days_since_epoch)
            .unwrap_or(Date {
                year: MIN_YEAR,
                month: 1,
                day: 1,
            })
    }

    /// Parses exactly `YYYY-MM-DDTHH:MM:SS.nnnnnnnnnZ`.
    pub fn parse(s: &str) -> Result<Self, TimeError> {
        if !matches_template(s.as_bytes(), b"dddd-dd-ddTdd:dd:dd.dddddddddZ") {
            return Err(TimeError::Syntax);
        }
        let date = Date::new(field(s, 0, 4)?, field(s, 5, 7)?, field(s, 8, 10)?)?;
        let (h, m, sec): (i64, i64, i64) =
            (field(s, 11, 13)?, field(s, 14, 16)?, field(s, 17, 19)?);
        if h > 23 || m > 59 || sec > 59 {
            return Err(TimeError::InvalidDate);
        }
        let secs = date
            .days_since_epoch()
            .and_then(|d| d.checked_mul(SECS_PER_DAY))
            .and_then(|s| s.checked_add(h.checked_mul(3_600)?))
            .and_then(|s| s.checked_add(m.checked_mul(60)?))
            .and_then(|s| s.checked_add(sec))
            .ok_or(TimeError::OutOfRange)?;
        Self::from_parts(secs, field(s, 20, 29)?)
    }
}

impl fmt::Display for UtcNanos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let of_day = self.secs.checked_rem(SECS_PER_DAY).ok_or(fmt::Error)?;
        let h = of_day.checked_div(3_600).ok_or(fmt::Error)?;
        let m = of_day
            .checked_rem(3_600)
            .and_then(|r| r.checked_div(60))
            .ok_or(fmt::Error)?;
        let s = of_day.checked_rem(60).ok_or(fmt::Error)?;
        write!(f, "{}T{h:02}:{m:02}:{s:02}.{:09}Z", self.date(), self.nanos)
    }
}

impl FromStr for UtcNanos {
    type Err = TimeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// `template` uses `d` for an ASCII digit; every other byte must match exactly.
fn matches_template(s: &[u8], template: &[u8]) -> bool {
    s.len() == template.len()
        && s.iter().zip(template).all(|(c, t)| {
            if *t == b'd' {
                c.is_ascii_digit()
            } else {
                c == t
            }
        })
}

fn field<T: FromStr>(s: &str, from: usize, to: usize) -> Result<T, TimeError> {
    s.get(from..to)
        .and_then(|digits| digits.parse().ok())
        .ok_or(TimeError::Syntax)
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn is_leap(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}
