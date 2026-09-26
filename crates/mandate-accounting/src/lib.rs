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
//! The account fold ([trading-domain spec §6, §8](../../../docs/specs/trading-domain.md#8-accounting)):
//! positions and signed cost basis, cash buckets and settlement, fee accrual and daily charges,
//! marks, realized and unrealized P&L, and equity.
//!
//! [`Account::apply`] is pure: it reads only the account, one [`Input`], and the pinned [`Config`],
//! and returns the next account without touching the previous one. Inputs mirror the journal's
//! account-stream events in `seq` order (spec §12): `FillApplied` and `LateFillApplied` become
//! [`Input::Fill`], `MarkUpdated` [`Input::Mark`], `FeesCharged` [`Input::FeesCharged`],
//! `SettlementPosted` [`Input::SettlementPosted`], `CorporateActionApplied`
//! [`Input::CorporateAction`], `DividendPaid` [`Input::DividendPaid`], and `CashInLieuPosted`
//! [`Input::CashInLieuPosted`]. Fees are computed here from the pinned fee configuration and
//! returned in the [`Record`] so the executor can journal them with the fill.

mod account;

pub use account::{Account, Applied, Position, Receivable, ReceivableKind, Record};

use mandate_num::{
    Bps, FeeCap, FeePerShare, FeeRate, NumError, Price, Qty, ShareIncrement, SplitRatio, Usd,
};
use mandate_time::{Date, TimeError, TradingCalendar, UtcNanos};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AccountingError {
    #[error("fill {0} was already applied")]
    DuplicateFill(String),
    #[error("fill quantity must be positive")]
    ZeroQuantity,
    #[error("crypto fills need maker or taker liquidity")]
    MissingLiquidity,
    #[error("no mark or fill price for {0}")]
    NoMark(InstrumentId),
    #[error(
        "the cost basis sign must follow the quantity sign, and a flat position has zero basis"
    )]
    InvalidPosition,
    #[error("the instrument ID is empty")]
    EmptyInstrumentId,
    #[error("a dividend's pay date is before its ex-date")]
    PayDateBeforeExDate,
    #[error("a corporate action of this type and ex-date was already applied to {0}")]
    DuplicateCorporateAction(InstrumentId),
    #[error(
        "a corporate action in {0} has an ex-date on or before the trade date of an applied fill, or before the ex-date of an applied action"
    )]
    CorporateActionOutOfOrder(InstrumentId),
    #[error("fill {0} has a trade date before the ex-date of a corporate action already applied")]
    FillBeforeCorporateAction(String),
    #[error("no dividend on {0} with that ex-date is outstanding")]
    NoDividendDue(InstrumentId),
    #[error("no outstanding cash in lieu on {0} equals the posted amount")]
    CashInLieuMismatch(InstrumentId),
    #[error(transparent)]
    Num(#[from] NumError),
    #[error(transparent)]
    Time(#[from] TimeError),
}

impl AccountingError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::DuplicateFill(_) => "duplicate_fill",
            Self::ZeroQuantity => "zero_quantity",
            Self::MissingLiquidity => "missing_liquidity",
            Self::NoMark(_) => "no_mark",
            Self::InvalidPosition => "invalid_position",
            Self::EmptyInstrumentId => "empty_instrument_id",
            Self::PayDateBeforeExDate
            | Self::DuplicateCorporateAction(_)
            | Self::CorporateActionOutOfOrder(_)
            | Self::FillBeforeCorporateAction(_)
            | Self::NoDividendDue(_)
            | Self::CashInLieuMismatch(_) => "",
            Self::Num(e) => e.code(),
            Self::Time(e) => e.code(),
        }
    }
}

/// The broker's asset ID (spec §2.3).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InstrumentId(String);

impl InstrumentId {
    pub fn new(id: &str) -> Result<Self, AccountingError> {
        if id.is_empty() {
            Err(AccountingError::EmptyInstrumentId)
        } else {
            Ok(Self(id.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for InstrumentId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetClass {
    UsEquity,
    Crypto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liquidity {
    Maker,
    Taker,
}

/// Fees accrue and are charged per family and day: equities per trade date, charged at 20:00 ET;
/// crypto per UTC day, charged at 00:00 UTC (spec §6.2, §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FeeFamily {
    Equities,
    Crypto,
}

/// How the TAF cap in force for a fill applies (DEC-87). `PerExecution` caps the fill's TAF.
/// `PerOrder` caps it at max(0, cap − TAF already charged on the fill's client order, in either
/// mode); a fill without a client order ID is its own order. Charged TAF is never refunded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TafCapBasis {
    PerExecution,
    PerOrder,
}

/// US equities regulatory fees (spec §6.2). Commission is zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquityFees {
    pub sec_rate: FeeRate,
    pub taf_per_share: FeePerShare,
    pub taf_cap: FeeCap,
    pub taf_cap_basis: TafCapBasis,
    pub cat_per_share: FeePerShare,
}

/// Alpaca crypto fees for the account's volume tier (spec §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CryptoFees {
    pub maker: Bps,
    pub taker: Bps,
}

/// The configuration the journal pins by content hash (spec §12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub equities: EquityFees,
    pub crypto: CryptoFees,
    pub calendar: TradingCalendar,
}

/// An execution as the broker reported it (spec §6.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Execution {
    pub fill_id: String,
    /// Absent for external fills: each is its own order.
    pub client_order_id: Option<String>,
    pub instrument: InstrumentId,
    pub asset_class: AssetClass,
    pub side: Side,
    pub qty_gross: Qty,
    pub price: Price,
    pub liquidity: Option<Liquidity>,
    pub executed_at: UtcNanos,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Fill(Execution),
    Mark {
        instrument: InstrumentId,
        price: Price,
    },
    FeesCharged {
        family: FeeFamily,
        day: Date,
    },
    /// Every unsettled bucket dated on or before `date` becomes settled.
    SettlementPosted {
        date: Date,
    },
    /// Applied to the position held when it is folded: at 20:00 ET on the last trading day before
    /// the ex-date, after every fill traded before the ex-date and before any fill traded on or
    /// after it (spec §8.5).
    CorporateAction(CorporateAction),
    /// The dividend's receivable or payable becomes settled cash (00:00 ET on the pay date).
    DividendPaid {
        instrument: InstrumentId,
        ex_date: Date,
    },
    /// The broker's cash in lieu posting: the earliest outstanding cash in lieu on `instrument`
    /// whose amount is exactly `amount` becomes settled cash.
    CashInLieuPosted {
        instrument: InstrumentId,
        amount: Usd,
    },
}

/// A corporate action the fold applies (spec §8.5). Every other type is out of scope and never
/// reaches the fold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorporateAction {
    Split(Split),
    CashDividend(CashDividend),
}

/// A split `ratio.new:ratio.old`. Q' = Q × new ÷ old truncated toward zero to `increment` (the
/// instrument snapshot's `fractionable`); the residual's basis is removed; marks are divided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split {
    pub instrument: InstrumentId,
    pub ex_date: Date,
    pub ratio: SplitRatio,
    pub increment: ShareIncrement,
    /// The broker's price per post-split share for the residual; `None` when none is posted, and
    /// then no cash in lieu is due.
    pub cash_in_lieu_price: Option<Price>,
}

/// A cash dividend of `per_share` USD, positive, with at most 9 places. The pay date is never
/// before the ex-date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CashDividend {
    instrument: InstrumentId,
    ex_date: Date,
    pay_date: Date,
    per_share: Price,
}

impl CashDividend {
    /// `pay_date_before_ex_date` if `pay_date < ex_date`.
    pub fn new(
        instrument: InstrumentId,
        ex_date: Date,
        pay_date: Date,
        per_share: Price,
    ) -> Result<Self, AccountingError> {
        let _ = (instrument, ex_date, pay_date, per_share);
        Err(AccountingError::PayDateBeforeExDate)
    }

    pub fn instrument(&self) -> &InstrumentId {
        &self.instrument
    }

    pub fn ex_date(&self) -> Date {
        self.ex_date
    }

    pub fn pay_date(&self) -> Date {
        self.pay_date
    }

    pub fn per_share(&self) -> Price {
        self.per_share
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeeKind {
    Sec,
    Taf,
    Cat,
    /// Taken from the received crypto; valued at the fill price and never accrued.
    CryptoAsset,
    CryptoUsd,
}

/// One fee on a fill. `asset_qty` is the quantity withheld for [`FeeKind::CryptoAsset`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fee {
    pub kind: FeeKind,
    pub usd: Usd,
    pub asset_qty: Option<Qty>,
}
