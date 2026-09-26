//! Corporate actions as the broker announces them (trading domain spec §4.5, §8.5): forward and
//! reverse splits and cash dividends as typed records, and every other action kept by kind so none
//! is silently dropped. Bars stay raw (DEC-89); split-adjusted bars are derived point in time. The
//! split ratio and the §8.5 mark rule are `mandate_num`'s (DEC-91), so market data and the
//! accounting fold adjust a price the same way; volumes use exact integer arithmetic that rounds
//! once or fails, never through a float.

use mandate_canon::{DecError, DecStr};
use mandate_num::{MarkPrice, NumError, Rounding, SplitRatio};
use mandate_time::{Date, TimeError, UtcNanos, new_york_midnight};

use crate::model::{Bar, DayRange, Symbol};
use crate::number::{NumberError, from_units, to_units};

/// Fractional digits of an adjusted price: the mark rule of spec §8.5 and §2.1.
pub const ADJUSTED_PRICE_SCALE: u8 = 12;

/// Seconds from 20:00 ET to the next midnight ET. No US daylight-saving change falls between them
/// (changes happen at 02:00), so the offset is the same on every date.
const EVENING_TO_MIDNIGHT_SECS: i64 = 4 * 3_600;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdjustmentError {
    #[error("split ratio {new}:{old}: {source}")]
    Ratio {
        new: u64,
        old: u64,
        source: NumError,
    },
    #[error("the cumulative split ratio does not fit 64-bit terms")]
    RatioOverflow,
    #[error("price {price}: {source}")]
    Price { price: DecStr, source: NumError },
    #[error("the adjusted price {0} is not a decimal: {1}")]
    AdjustedPrice(String, DecError),
    #[error("the adjusted value does not fit 38 digits")]
    Overflow,
    #[error("{0}")]
    Number(NumberError),
    #[error("{0}")]
    Time(TimeError),
}

impl AdjustmentError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Ratio { .. } => "ratio",
            Self::RatioOverflow => "ratio_overflow",
            Self::Price { .. } => "price",
            Self::AdjustedPrice(..) => "adjusted_price",
            Self::Overflow => "overflow",
            Self::Number(_) => "number",
            Self::Time(_) => "time",
        }
    }
}

/// The split of `old` shares into `new`, as `mandate_num` checks it (both positive).
pub fn split_ratio(new: u64, old: u64) -> Result<SplitRatio, AdjustmentError> {
    SplitRatio::new(new, old).map_err(|source| AdjustmentError::Ratio { new, old, source })
}

/// `first` followed by `then`, in lowest terms.
pub fn compose(first: SplitRatio, then: SplitRatio) -> Result<SplitRatio, AdjustmentError> {
    let new = u128::from(first.new_shares()) * u128::from(then.new_shares());
    let old = u128::from(first.old_shares()) * u128::from(then.old_shares());
    let divisor = gcd(new, old);
    let term = |n: u128| u64::try_from(n / divisor).map_err(|_| AdjustmentError::RatioOverflow);
    split_ratio(term(new)?, term(old)?)
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn is_unit(ratio: SplitRatio) -> bool {
    ratio.new_shares() == ratio.old_shares()
}

/// A pre-split price in post-split terms: `mandate_num`'s round(price × old ÷ new, 12,
/// half_even). The price must be a positive mark of at most 12 places, and so must the result.
pub fn adjust_price(ratio: SplitRatio, price: &DecStr) -> Result<DecStr, AdjustmentError> {
    let failed = |source| AdjustmentError::Price {
        price: price.clone(),
        source,
    };
    let mark = MarkPrice::parse(price.as_str()).map_err(failed)?;
    let adjusted = ratio
        .mark(mark, u32::from(ADJUSTED_PRICE_SCALE), Rounding::HalfEven)
        .map_err(failed)?
        .to_string();
    DecStr::parse(&adjusted).map_err(|e| AdjustmentError::AdjustedPrice(adjusted, e))
}

/// A pre-split share quantity in post-split terms: round(quantity × new ÷ old, scale, half_even).
pub fn adjust_quantity(
    ratio: SplitRatio,
    quantity: &DecStr,
    scale: u8,
) -> Result<DecStr, AdjustmentError> {
    let digits = quantity
        .as_str()
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    let from = u8::try_from(digits).map_err(|_| AdjustmentError::Overflow)?;
    let units = to_units(quantity, from).map_err(AdjustmentError::Number)?;
    let numerator = units
        .checked_mul(i128::from(ratio.new_shares()))
        .and_then(|n| n.checked_mul(power_of_ten(scale.saturating_sub(from))?))
        .ok_or(AdjustmentError::Overflow)?;
    let denominator = power_of_ten(from.saturating_sub(scale))
        .and_then(|p| i128::from(ratio.old_shares()).checked_mul(p))
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
    if away {
        quotient.checked_add(numerator.signum())
    } else {
        Some(quotient)
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
    pub ex_date: Option<Date>,
    pub process_date: Date,
}

impl OtherAction {
    /// The date the action is dated by: its ex-date, or its process date when it has none.
    pub fn date(&self) -> Date {
        self.ex_date.unwrap_or(self.process_date)
    }
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

    /// Only the actions dated in `range`: splits and cash dividends by ex-date, every other action
    /// per [`OtherAction::date`].
    pub fn dated_in(mut self, range: DayRange) -> Self {
        let inside = |date: Date| range.first() <= date && date <= range.last();
        self.splits.retain(|s| inside(s.ex_date));
        self.cash_dividends.retain(|d| inside(d.ex_date));
        self.other.retain(|o| inside(o.date()));
        self
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
        let mut ratio = split_ratio(1, 1)?;
        for split in &self.splits {
            if split.ex_date <= as_of && at < split.effective_at()? {
                ratio = compose(ratio, split.ratio)?;
            }
        }
        Ok(ratio)
    }

    /// Split adjustment as of `as_of` for many prices, each split's effective instant computed
    /// once.
    pub fn price_adjuster(&self, as_of: Date) -> Result<PriceAdjuster, AdjustmentError> {
        let splits = self
            .splits
            .iter()
            .filter(|split| split.ex_date <= as_of)
            .map(|split| Ok((split.effective_at()?, split.ratio)))
            .collect::<Result<_, AdjustmentError>>()?;
        Ok(PriceAdjuster { splits })
    }

    /// `bar` split-adjusted as of `as_of`: prices per [`adjust_price`] and volume per
    /// [`adjust_quantity`] at `volume_scale`, each by the product of the splits that apply and so
    /// rounded once, and the trade count unchanged. A bar no split applies to comes back unchanged.
    pub fn adjust_bar(
        &self,
        bar: &Bar,
        as_of: Date,
        volume_scale: u8,
    ) -> Result<Bar, AdjustmentError> {
        let ratio = self.split_ratio_after(bar.start, as_of)?;
        if is_unit(ratio) {
            return Ok(bar.clone());
        }
        Ok(Bar {
            start: bar.start,
            open: adjust_price(ratio, &bar.open)?,
            high: adjust_price(ratio, &bar.high)?,
            low: adjust_price(ratio, &bar.low)?,
            close: adjust_price(ratio, &bar.close)?,
            volume: adjust_quantity(ratio, &bar.volume, volume_scale)?,
            vwap: adjust_price(ratio, &bar.vwap)?,
            trade_count: bar.trade_count,
        })
    }
}

/// The splits known as of one date ([`CorporateActions::price_adjuster`]), each with the instant
/// it takes effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceAdjuster {
    splits: Vec<(UtcNanos, SplitRatio)>,
}

impl PriceAdjuster {
    /// `price`, observed at `at`, in the terms of the adjuster's date: as
    /// [`CorporateActions::adjust_bar`] adjusts a bar's prices, and unchanged when no split
    /// applies.
    pub fn adjust(&self, price: &DecStr, at: UtcNanos) -> Result<DecStr, AdjustmentError> {
        let mut ratio = split_ratio(1, 1)?;
        for &(effective, split) in &self.splits {
            if at < effective {
                ratio = compose(ratio, split)?;
            }
        }
        if is_unit(ratio) {
            return Ok(price.clone());
        }
        adjust_price(ratio, price)
    }
}
