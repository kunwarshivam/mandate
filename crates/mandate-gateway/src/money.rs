//! Fixed-point USD for the meter (inference spec §7.1, §7.2): whole micro-dollars, never floating
//! point. Six fractional digits is the metering record's precision; a token price is held per
//! million tokens, so a price of $0.15 per million tokens is exact.

use core::fmt;

use mandate_canon::Digest;

/// Micro-dollars in one dollar.
const MICROS: u64 = 1_000_000;
/// Tokens in the unit a [`TokenPrice`] is quoted for.
const PER: u128 = 1_000_000;
/// Fractional digits of a [`Usd6`].
const PLACES: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MoneyError {
    #[error("not a non-negative decimal with at most six fractional digits")]
    NotUsd6,
    #[error("the amount does not fit")]
    Overflow,
}

/// A non-negative USD amount in whole micro-dollars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Usd6(u64);

impl Usd6 {
    pub const ZERO: Self = Self(0);

    pub fn from_micros(micros: u64) -> Self {
        Self(micros)
    }

    pub fn micros(self) -> u64 {
        self.0
    }

    /// Whole dollars.
    pub fn dollars(whole: u64) -> Result<Self, MoneyError> {
        whole
            .checked_mul(MICROS)
            .map(Self)
            .ok_or(MoneyError::Overflow)
    }

    /// Plain decimal text, `0` to six fractional digits (`50`, `0.0117`); no sign, no exponent.
    pub fn parse(text: &str) -> Result<Self, MoneyError> {
        let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        if !digits(whole) || (text.contains('.') && !digits(fraction)) || fraction.len() > PLACES {
            return Err(MoneyError::NotUsd6);
        }
        let whole: u64 = whole.parse().map_err(|_| MoneyError::Overflow)?;
        let padded = format!("{fraction:0<PLACES$}");
        let fraction: u64 = padded.parse().map_err(|_| MoneyError::NotUsd6)?;
        whole
            .checked_mul(MICROS)
            .and_then(|w| w.checked_add(fraction))
            .map(Self)
            .ok_or(MoneyError::Overflow)
    }

    pub fn checked_add(self, other: Self) -> Result<Self, MoneyError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(MoneyError::Overflow)
    }

    /// `self − other`, or zero when `other` is larger.
    pub fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

/// The journal's canonical decimal (journal spec §4.6): no trailing fractional zeros, `0` for zero.
impl fmt::Display for Usd6 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0.checked_div(MICROS).ok_or(fmt::Error)?;
        let fraction = self.0.checked_rem(MICROS).ok_or(fmt::Error)?;
        if fraction == 0 {
            return write!(f, "{whole}");
        }
        let digits = format!("{fraction:0>PLACES$}");
        write!(f, "{whole}.{}", digits.trim_end_matches('0'))
    }
}

/// USD per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TokenPrice(Usd6);

impl TokenPrice {
    pub fn per_million(price: Usd6) -> Self {
        Self(price)
    }

    pub fn amount(self) -> Usd6 {
        self.0
    }
}

/// Tokens a provider reported for one call. `input` counts every prompt token, `cached` the
/// part of them served from the provider's prompt cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cached: u64,
}

/// One endpoint's prices (inference spec §7.2), versioned by the digest of their configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceTable {
    input: TokenPrice,
    output: TokenPrice,
    cached: TokenPrice,
    version: Digest,
}

impl PriceTable {
    /// A cached token never costs more than a fresh one, so the maximum cost of §7.2, which prices
    /// every prompt token as fresh, bounds every call.
    pub fn new(
        input: TokenPrice,
        output: TokenPrice,
        cached: TokenPrice,
        version: Digest,
    ) -> Result<Self, PriceError> {
        if cached > input {
            return Err(PriceError::CachedAboveInput);
        }
        Ok(Self {
            input,
            output,
            cached,
            version,
        })
    }

    pub fn version(&self) -> Digest {
        self.version
    }

    /// Inference spec §7.2's maximum: every prompt byte as one fresh input token (a byte-level
    /// tokenizer never makes more tokens than bytes) and `max_output_tokens` output tokens,
    /// rounded up to the micro-dollar.
    pub fn max_cost(&self, prompt_bytes: u64, max_output_tokens: u32) -> Result<Usd6, MoneyError> {
        let sum = priced(prompt_bytes, self.input)?
            .checked_add(priced(u64::from(max_output_tokens), self.output)?)
            .ok_or(MoneyError::Overflow)?;
        micros_of(sum, true)
    }

    /// The reported tokens at these prices, rounded half up to the micro-dollar (inference spec
    /// §7.1). Cached tokens beyond the input count are priced as fresh input.
    pub fn cost(&self, usage: Usage) -> Result<Usd6, MoneyError> {
        let cached = usage.cached.min(usage.input);
        let fresh = usage.input.saturating_sub(cached);
        let sum = priced(fresh, self.input)?
            .checked_add(priced(cached, self.cached)?)
            .and_then(|s| s.checked_add(priced(usage.output, self.output).ok()?))
            .ok_or(MoneyError::Overflow)?;
        micros_of(sum, false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PriceError {
    #[error("a cached token is priced above a fresh one")]
    CachedAboveInput,
}

/// `tokens × price`, in micro-dollars per million tokens.
fn priced(tokens: u64, price: TokenPrice) -> Result<u128, MoneyError> {
    u128::from(tokens)
        .checked_mul(u128::from(price.0.micros()))
        .ok_or(MoneyError::Overflow)
}

/// Whole micro-dollars of `sum` (micro-dollars per million tokens): rounded up, or half up.
fn micros_of(sum: u128, up: bool) -> Result<Usd6, MoneyError> {
    let half = PER.checked_div(2).ok_or(MoneyError::Overflow)?;
    let bump = if up { PER.saturating_sub(1) } else { half };
    let micros = sum
        .checked_add(bump)
        .and_then(|s| s.checked_div(PER))
        .ok_or(MoneyError::Overflow)?;
    u64::try_from(micros)
        .map(Usd6)
        .map_err(|_| MoneyError::Overflow)
}
