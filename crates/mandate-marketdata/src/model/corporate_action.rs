//! Corporate actions as the broker announces them (trading domain spec §4.5, §8.5): forward and
//! reverse splits and cash dividends as typed records, and every other action kept by kind so none
//! is silently dropped. Bars stay raw (DEC-89); split-adjusted bars are derived point in time, with
//! exact integer arithmetic that rounds once or fails, never through a float.

use mandate_canon::DecStr;
use mandate_time::{Date, TimeError, UtcNanos, new_york_midnight};

use crate::model::{Bar, Symbol};
use crate::number::{NumberError, from_units, to_units};

/// Fractional digits of an adjusted price: the mark rule of spec §8.5 and §2.1.
pub const ADJUSTED_PRICE_SCALE: u8 = 12;

/// Seconds from 20:00 ET to the next midnight ET. No US daylight-saving change falls between them
/// (changes happen at 02:00), so the offset is the same on every date.
const EVENING_TO_MIDNIGHT_SECS: i64 = 4 * 3_600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AdjustmentError {
    #[error("a split ratio needs positive share counts, got {new}:{old}")]
    ZeroRatio { new: u64, old: u64 },
    #[error("the cumulative split ratio does not fit 128 bits")]
    RatioOverflow,
    #[error("the adjusted value does not fit 38 digits")]
    Overflow,
    #[error("{0}")]
    Number(NumberError),
    #[error("{0}")]
    Time(TimeError),
}

impl AdjustmentError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::ZeroRatio { .. } => "zero_ratio",
            Self::RatioOverflow => "ratio_overflow",
            Self::Overflow => "overflow",
            Self::Number(_) => "number",
            Self::Time(_) => "time",
        }
    }
}

/// A split of `old` shares into `new` (spec §8.5: integer ratio new:old), in lowest terms. The
/// product of several splits is again a ratio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SplitRatio {
    new: u128,
    old: u128,
}

impl SplitRatio {
    pub const ONE: Self = Self { new: 1, old: 1 };

    pub fn new(new: u64, old: u64) -> Result<Self, AdjustmentError> {
        if new == 0 || old == 0 {
            return Err(AdjustmentError::ZeroRatio { new, old });
        }
        Ok(Self::reduced(u128::from(new), u128::from(old)))
    }

    pub fn new_shares(self) -> u128 {
        self.new
    }

    pub fn old_shares(self) -> u128 {
        self.old
    }

    /// This split followed by `next`.
    pub fn then(self, next: Self) -> Result<Self, AdjustmentError> {
        let new = self.new.checked_mul(next.new);
        let old = self.old.checked_mul(next.old);
        new.zip(old)
            .map(|(new, old)| Self::reduced(new, old))
            .ok_or(AdjustmentError::RatioOverflow)
    }

    /// A pre-split price in post-split terms: round(price × old ÷ new, 12, half_even).
    pub fn adjust_price(self, price: &DecStr) -> Result<DecStr, AdjustmentError> {
        scaled(price, self.old, self.new, ADJUSTED_PRICE_SCALE)
    }

    /// A pre-split share quantity in post-split terms: round(quantity × new ÷ old, scale,
    /// half_even).
    pub fn adjust_quantity(self, quantity: &DecStr, scale: u8) -> Result<DecStr, AdjustmentError> {
        scaled(quantity, self.new, self.old, scale)
    }

    fn reduced(new: u128, old: u128) -> Self {
        let divisor = gcd(new, old);
        Self {
            new: new / divisor,
            old: old / divisor,
        }
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// round(value × mul ÷ div, scale, half_even), exactly: one division of integers, rounded once.
fn scaled(value: &DecStr, mul: u128, div: u128, scale: u8) -> Result<DecStr, AdjustmentError> {
    let digits = value
        .as_str()
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    let from = u8::try_from(digits).map_err(|_| AdjustmentError::Overflow)?;
    let units = to_units(value, from).map_err(AdjustmentError::Number)?;
    let mul = i128::try_from(mul).map_err(|_| AdjustmentError::Overflow)?;
    let div = i128::try_from(div).map_err(|_| AdjustmentError::Overflow)?;
    let numerator = units
        .checked_mul(mul)
        .and_then(|n| n.checked_mul(power_of_ten(scale.saturating_sub(from))?))
        .ok_or(AdjustmentError::Overflow)?;
    let denominator = power_of_ten(from.saturating_sub(scale))
        .and_then(|p| div.checked_mul(p))
        .ok_or(AdjustmentError::Overflow)?;
    let rounded = divide_half_even(numerator, denominator).ok_or(AdjustmentError::Overflow)?;
    from_units(rounded, scale).map_err(AdjustmentError::Number)
}

fn power_of_ten(exponent: u8) -> Option<i128> {
    10_i128.checked_pow(u32::from(exponent))
}

/// `numerator ÷ denominator` rounded half to even, for a positive denominator.
fn divide_half_even(numerator: i128, denominator: i128) -> Option<i128> {
    let quotient = numerator.checked_div(denominator)?;
    let remainder = numerator.checked_rem(denominator)?.unsigned_abs();
    let rest = denominator.unsigned_abs().checked_sub(remainder)?;
    let away = remainder > rest || (remainder == rest && quotient % 2 != 0);
    if !away {
        return Some(quotient);
    }
    if numerator < 0 {
        quotient.checked_sub(1)
    } else {
        quotient.checked_add(1)
    }
}

/// A forward or reverse split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split {
    pub id: String,
    pub ex_date: Date,
    pub ratio: SplitRatio,
}

impl Split {
    /// 20:00 ET on the calendar day before the ex-date, when the overnight session into the ex-date
    /// opens and trades post-split. Spec §8.5 applies a split at 20:00 ET on the last trading day
    /// before the ex-date; the two differ only by closed days, which have no bars.
    pub fn effective_at(&self) -> Result<UtcNanos, AdjustmentError> {
        let midnight = new_york_midnight(self.ex_date).map_err(AdjustmentError::Time)?;
        let secs = midnight
            .secs()
            .checked_sub(EVENING_TO_MIDNIGHT_SECS)
            .ok_or(AdjustmentError::Time(TimeError::OutOfRange))?;
        UtcNanos::from_parts(secs, midnight.nanos()).map_err(AdjustmentError::Time)
    }
}

/// A cash dividend of `rate` dollars per share. Recorded, never applied to prices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CashDividend {
    pub id: String,
    pub ex_date: Date,
    pub record_date: Option<Date>,
    pub payable_date: Option<Date>,
    pub rate: DecStr,
    pub special: bool,
    pub foreign: bool,
}

/// An action of a kind this crate does not interpret (spin-off, merger, stock dividend, name
/// change, and the rest), kept so a report can say history around it is not adjusted (spec §8.5:
/// anything else is out of scope and must be surfaced).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtherAction {
    pub id: String,
    pub kind: String,
    pub process_date: Date,
}

/// The corporate actions the broker reports for one symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorporateActions {
    pub symbol: Symbol,
    pub splits: Vec<Split>,
    pub cash_dividends: Vec<CashDividend>,
    pub other: Vec<OtherAction>,
}

impl CorporateActions {
    pub fn none(symbol: Symbol) -> Self {
        Self {
            symbol,
            splits: Vec::new(),
            cash_dividends: Vec::new(),
            other: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.splits.is_empty() && self.cash_dividends.is_empty() && self.other.is_empty()
    }

    /// Every action's ID, in the order split, cash dividend, other.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        let splits = self.splits.iter().map(|s| s.id.as_str());
        let dividends = self.cash_dividends.iter().map(|d| d.id.as_str());
        let other = self.other.iter().map(|o| o.id.as_str());
        splits.chain(dividends).chain(other)
    }

    /// The product of every split known as of `as_of` (ex-date ≤ `as_of`, spec §4.5) that took
    /// effect after `at`: what a price observed at `at` needs to be comparable with `as_of`.
    pub fn split_ratio_after(
        &self,
        at: UtcNanos,
        as_of: Date,
    ) -> Result<SplitRatio, AdjustmentError> {
        let mut ratio = SplitRatio::ONE;
        for split in &self.splits {
            if split.ex_date <= as_of && at < split.effective_at()? {
                ratio = ratio.then(split.ratio)?;
            }
        }
        Ok(ratio)
    }

    /// `bar` split-adjusted as of `as_of`: prices per [`SplitRatio::adjust_price`], volume per
    /// [`SplitRatio::adjust_quantity`] at `volume_scale`, and the trade count unchanged. A bar no
    /// split applies to comes back unchanged.
    pub fn adjust_bar(
        &self,
        bar: &Bar,
        as_of: Date,
        volume_scale: u8,
    ) -> Result<Bar, AdjustmentError> {
        let ratio = self.split_ratio_after(bar.start, as_of)?;
        if ratio == SplitRatio::ONE {
            return Ok(bar.clone());
        }
        Ok(Bar {
            start: bar.start,
            open: ratio.adjust_price(&bar.open)?,
            high: ratio.adjust_price(&bar.high)?,
            low: ratio.adjust_price(&bar.low)?,
            close: ratio.adjust_price(&bar.close)?,
            volume: ratio.adjust_quantity(&bar.volume, volume_scale)?,
            vwap: ratio.adjust_price(&bar.vwap)?,
            trade_count: bar.trade_count,
        })
    }
}
