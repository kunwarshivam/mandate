//! The account state and the fold over [`Input`]s.

use std::collections::{BTreeMap, BTreeSet};

use mandate_num::{CostBasis, NumError, Price, SignedQty, Usd};
use mandate_time::{Date, UtcNanos};

use crate::{AccountingError, Config, Fee, FeeFamily, Input, InstrumentId};

/// Signed quantity Q and signed cost basis B; average cost is derived, never stored (spec §8.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    qty: SignedQty,
    basis: CostBasis,
}

impl Position {
    pub const FLAT: Self = Self {
        qty: SignedQty::ZERO,
        basis: CostBasis::ZERO,
    };

    pub fn new(_qty: SignedQty, _basis: CostBasis) -> Result<Self, AccountingError> {
        Err(AccountingError::InvalidPosition)
    }

    pub fn qty(self) -> SignedQty {
        self.qty
    }

    pub fn basis(self) -> CostBasis {
        self.basis
    }
}

/// What an input did, for the journal record that carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    Fill {
        /// Equities only; crypto has no trade date (spec §2.2).
        trade_date: Option<Date>,
        /// Equity sells only; other fills settle at once.
        settles_on: Option<Date>,
        /// Gross quantity, less the asset fee on crypto buys; negative for sells.
        received: SignedQty,
        realized_gross: Usd,
        fees: Vec<Fee>,
    },
    Mark,
    FeesCharged {
        accrued: Usd,
        charged: Usd,
    },
    SettlementPosted {
        amount: Usd,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub account: Account,
    pub record: Record,
}

/// One broker account's ledger (spec §7.1, §8). Equality is structural, so two folds of the same
/// inputs compare equal exactly when every stored value matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    positions: BTreeMap<InstrumentId, Position>,
    marks: BTreeMap<InstrumentId, Price>,
    last_fill_prices: BTreeMap<InstrumentId, Price>,
    settled: Usd,
    unsettled: BTreeMap<Date, Usd>,
    accrued: BTreeMap<(FeeFamily, Date), Usd>,
    charged: Usd,
    asset_fees: Usd,
    realized_gross: Usd,
    applied_fills: BTreeSet<String>,
    taf_by_order: BTreeMap<String, Usd>,
}

impl Account {
    /// An account holding `settled` cash and `positions` (without marks until one arrives).
    pub fn opening(
        _settled: Usd,
        _positions: impl IntoIterator<Item = (InstrumentId, Position)>,
    ) -> Self {
        Self {
            positions: BTreeMap::new(),
            marks: BTreeMap::new(),
            last_fill_prices: BTreeMap::new(),
            settled: Usd::ZERO,
            unsettled: BTreeMap::new(),
            accrued: BTreeMap::new(),
            charged: Usd::ZERO,
            asset_fees: Usd::ZERO,
            realized_gross: Usd::ZERO,
            applied_fills: BTreeSet::new(),
            taf_by_order: BTreeMap::new(),
        }
    }

    /// Folds one input. On error the account is unchanged: the caller still holds `self`.
    pub fn apply(&self, _input: &Input, _config: &Config) -> Result<Applied, AccountingError> {
        Err(AccountingError::Num(NumError::Overflow))
    }

    /// Settlement dates whose `SettlementPosted` (00:00 ET) is due at or before `at`, in order.
    pub fn settlements_due(&self, _at: UtcNanos) -> Result<Vec<Date>, AccountingError> {
        Err(AccountingError::Num(NumError::Overflow))
    }

    pub fn position(&self, _instrument: &InstrumentId) -> Position {
        Position::FLAT
    }

    pub fn positions(&self) -> impl Iterator<Item = (&InstrumentId, Position)> {
        core::iter::empty()
    }

    /// The reporting mark: the latest `MarkUpdated`, else the last fill price (spec §8.2).
    pub fn mark(&self, _instrument: &InstrumentId) -> Option<Price> {
        None
    }

    pub fn settled(&self) -> Usd {
        Usd::ZERO
    }

    pub fn unsettled(&self) -> impl Iterator<Item = (Date, Usd)> {
        core::iter::empty()
    }

    /// settled + Σ unsettled (I4).
    pub fn cash_total(&self) -> Result<Usd, AccountingError> {
        Err(AccountingError::Num(NumError::Overflow))
    }

    pub fn fees_accrued(&self) -> Result<Usd, AccountingError> {
        Err(AccountingError::Num(NumError::Overflow))
    }

    pub fn fees_charged(&self) -> Usd {
        Usd::ZERO
    }

    /// USD value of asset-denominated fees (crypto buys), never accrued.
    pub fn asset_fees(&self) -> Usd {
        Usd::ZERO
    }

    /// charged + accrued + USD value of asset fees.
    pub fn fees_total(&self) -> Result<Usd, AccountingError> {
        Err(AccountingError::Num(NumError::Overflow))
    }

    pub fn realized_gross(&self) -> Usd {
        Usd::ZERO
    }

    /// Gross realized − fees (spec §8.1).
    pub fn realized_net(&self) -> Result<Usd, AccountingError> {
        Err(AccountingError::Num(NumError::Overflow))
    }

    /// Σ Q × mark.
    pub fn market_value(&self) -> Result<Usd, AccountingError> {
        Err(AccountingError::Num(NumError::Overflow))
    }

    /// Σ (Q × mark − B).
    pub fn unrealized(&self) -> Result<Usd, AccountingError> {
        Err(AccountingError::Num(NumError::Overflow))
    }

    /// settled + Σ unsettled − accrued fees + Σ market value (spec §8.2; receivables and payables
    /// arrive with corporate actions).
    pub fn equity(&self) -> Result<Usd, AccountingError> {
        Err(AccountingError::Num(NumError::Overflow))
    }
}
