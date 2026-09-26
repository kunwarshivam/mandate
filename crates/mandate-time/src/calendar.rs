//! Trading and settlement calendars, equity trade dates, and America/New_York wall-clock
//! conversions ([trading-domain spec §2.2, §8.4](../../../docs/specs/trading-domain.md#22-time-and-calendars)).

use std::collections::BTreeSet;

use crate::{Date, TimeError, UtcNanos};

/// The America/New_York calendar date and hour of an instant.
pub fn new_york_date_and_hour(_at: UtcNanos) -> Result<(Date, u8), TimeError> {
    Err(TimeError::TimeZone)
}

/// 00:00 America/New_York on `date`, when `SettlementPosted` and `DividendPaid` fall due (spec §8.3).
pub fn new_york_midnight(_date: Date) -> Result<UtcNanos, TimeError> {
    Err(TimeError::TimeZone)
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
        _valid_from: Date,
        _valid_to: Date,
        _trading_holidays: impl IntoIterator<Item = Date>,
        _settlement_holidays: impl IntoIterator<Item = Date>,
    ) -> Result<Self, TimeError> {
        Err(TimeError::InvalidCalendar)
    }

    pub fn is_trading_day(&self, _date: Date) -> Result<bool, TimeError> {
        Err(TimeError::OutsideCalendar)
    }

    pub fn is_settlement_day(&self, _date: Date) -> Result<bool, TimeError> {
        Err(TimeError::OutsideCalendar)
    }

    /// The equity trade date of an execution: its New York date, moved to the next day from 20:00
    /// ET, then to the first trading day on or after that (spec §2.2).
    pub fn equity_trade_date(&self, _executed_at: UtcNanos) -> Result<Date, TimeError> {
        Err(TimeError::OutsideCalendar)
    }

    /// T+1 on the settlement calendar: the first settlement day after the trade date (spec §8.4).
    pub fn settlement_date(&self, _trade_date: Date) -> Result<Date, TimeError> {
        Err(TimeError::OutsideCalendar)
    }
}
