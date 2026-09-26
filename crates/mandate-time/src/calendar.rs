//! Trading and settlement calendars, equity trade dates, and America/New_York wall-clock
//! conversions ([trading-domain spec §2.2, §8.4](../../../docs/specs/trading-domain.md#22-time-and-calendars)).

use std::collections::BTreeSet;

use jiff::Timestamp;
use jiff::civil;
use jiff::tz::TimeZone;

use crate::{Date, TimeError, UtcNanos};

const NEW_YORK: &str = "America/New_York";
/// Equity fills at or after 20:00 ET belong to the next trading day.
const TRADE_DATE_CUTOFF_HOUR: u8 = 20;

fn new_york() -> Result<TimeZone, TimeError> {
    TimeZone::get(NEW_YORK).map_err(|_| TimeError::TimeZone)
}

fn to_civil(date: Date) -> Result<civil::Date, TimeError> {
    let year = i16::try_from(date.year()).map_err(|_| TimeError::OutOfRange)?;
    let month = i8::try_from(date.month()).map_err(|_| TimeError::OutOfRange)?;
    let day = i8::try_from(date.day()).map_err(|_| TimeError::OutOfRange)?;
    civil::Date::new(year, month, day).map_err(|_| TimeError::OutOfRange)
}

fn from_civil(date: civil::Date) -> Result<Date, TimeError> {
    let year = u16::try_from(date.year()).map_err(|_| TimeError::OutOfRange)?;
    let month = u8::try_from(date.month()).map_err(|_| TimeError::OutOfRange)?;
    let day = u8::try_from(date.day()).map_err(|_| TimeError::OutOfRange)?;
    Date::new(year, month, day)
}

/// The America/New_York calendar date and hour of an instant.
pub fn new_york_date_and_hour(at: UtcNanos) -> Result<(Date, u8), TimeError> {
    let nanos = i32::try_from(at.nanos()).map_err(|_| TimeError::OutOfRange)?;
    let zoned = Timestamp::new(at.secs(), nanos)
        .map_err(|_| TimeError::OutOfRange)?
        .to_zoned(new_york()?);
    let hour = u8::try_from(zoned.hour()).map_err(|_| TimeError::OutOfRange)?;
    Ok((from_civil(zoned.date())?, hour))
}

/// 00:00 America/New_York on `date`, when `SettlementPosted` and `DividendPaid` fall due (spec §8.3).
pub fn new_york_midnight(date: Date) -> Result<UtcNanos, TimeError> {
    let timestamp = to_civil(date)?
        .to_zoned(new_york()?)
        .map_err(|_| TimeError::OutOfRange)?
        .timestamp();
    let nanos = u32::try_from(timestamp.subsec_nanosecond()).map_err(|_| TimeError::OutOfRange)?;
    UtcNanos::from_parts(timestamp.as_second(), nanos)
}

/// A versioned trading calendar over a validity range, under the rule "weekdays not listed as
/// holidays are trading days". Settlement days are trading days that are not settlement (Federal
/// Reserve) holidays. Every question about a date outside the range is an error, never a guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradingCalendar {
    valid_from: Date,
    valid_to: Date,
    trading_holidays: BTreeSet<Date>,
    settlement_holidays: BTreeSet<Date>,
}

impl TradingCalendar {
    pub fn new(
        valid_from: Date,
        valid_to: Date,
        trading_holidays: impl IntoIterator<Item = Date>,
        settlement_holidays: impl IntoIterator<Item = Date>,
    ) -> Result<Self, TimeError> {
        let trading_holidays: BTreeSet<Date> = trading_holidays.into_iter().collect();
        let settlement_holidays: BTreeSet<Date> = settlement_holidays.into_iter().collect();
        let in_range = |d: &Date| (valid_from..=valid_to).contains(d);
        if valid_from > valid_to
            || !trading_holidays.iter().all(in_range)
            || !settlement_holidays.iter().all(in_range)
        {
            return Err(TimeError::InvalidCalendar);
        }
        Ok(Self {
            valid_from,
            valid_to,
            trading_holidays,
            settlement_holidays,
        })
    }

    fn check(&self, date: Date) -> Result<Date, TimeError> {
        if (self.valid_from..=self.valid_to).contains(&date) {
            Ok(date)
        } else {
            Err(TimeError::OutsideCalendar)
        }
    }

    pub fn is_trading_day(&self, date: Date) -> Result<bool, TimeError> {
        let date = self.check(date)?;
        Ok(!date.is_weekend() && !self.trading_holidays.contains(&date))
    }

    pub fn is_settlement_day(&self, date: Date) -> Result<bool, TimeError> {
        Ok(self.is_trading_day(date)? && !self.settlement_holidays.contains(&date))
    }

    fn first_on_or_after(
        &self,
        from: Date,
        wanted: impl Fn(&Self, Date) -> Result<bool, TimeError>,
    ) -> Result<Date, TimeError> {
        let mut date = from;
        while !wanted(self, self.check(date)?)? {
            date = date.next()?;
        }
        Ok(date)
    }

    /// The equity trade date of an execution: its New York date, moved to the next day from 20:00
    /// ET, then to the first trading day on or after that (spec §2.2).
    pub fn equity_trade_date(&self, executed_at: UtcNanos) -> Result<Date, TimeError> {
        let (date, hour) = new_york_date_and_hour(executed_at)?;
        let date = if hour >= TRADE_DATE_CUTOFF_HOUR {
            date.next()?
        } else {
            date
        };
        self.first_on_or_after(date, Self::is_trading_day)
    }

    /// T+1 on the settlement calendar: the first settlement day after the trade date (spec §8.4).
    pub fn settlement_date(&self, trade_date: Date) -> Result<Date, TimeError> {
        self.check(trade_date)?;
        self.first_on_or_after(trade_date.next()?, Self::is_settlement_day)
    }
}
