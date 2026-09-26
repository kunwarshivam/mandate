//! The account state and the fold over [`Input`]s.

use core::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use mandate_num::{CostBasis, NumError, Price, Rounding, SignedQty, Usd};
use mandate_time::{Date, UtcNanos, new_york_midnight};

use crate::{
    AccountingError, AssetClass, Config, EquityFees, Execution, Fee, FeeFamily, FeeKind, Input,
    InstrumentId, Liquidity, Side, TafCapBasis,
};

/// Cost-basis reduction: `round(B × |q| ÷ |Q|, 12, half_even)` (spec §8.1).
const BASIS_REDUCTION_SCALE: u32 = 12;
/// The equities daily charge: `round(total, 2, ceiling)` (spec §6.2).
const CHARGE_SCALE: u32 = 2;
/// A crypto sell's USD fee: `round(x, 2, half_up)` (spec §6.3).
const CRYPTO_USD_FEE_SCALE: u32 = 2;

fn sum(values: impl IntoIterator<Item = Usd>) -> Result<Usd, NumError> {
    values.into_iter().try_fold(Usd::ZERO, Usd::checked_add)
}

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

    pub fn new(qty: SignedQty, basis: CostBasis) -> Result<Self, AccountingError> {
        if qty.is_zero() && !basis.is_zero() {
            Err(AccountingError::InvalidPosition)
        } else {
            Ok(Self { qty, basis })
        }
    }

    pub fn qty(self) -> SignedQty {
        self.qty
    }

    pub fn basis(self) -> CostBasis {
        self.basis
    }

    /// Applies a fill of signed quantity `q` at `price`; returns the new position and the gross
    /// realized P&L. A fill that crosses zero is split into a close and an open.
    fn apply(self, q: SignedQty, price: Price) -> Result<(Self, Usd), AccountingError> {
        if q.is_zero() {
            return Ok((self, Usd::ZERO));
        }
        if self.qty.is_zero() || self.qty.is_negative() == q.is_negative() {
            let opened = Self {
                qty: self.qty.checked_add(q)?,
                basis: self.basis.checked_add(q.value_at(price)?)?,
            };
            return Ok((opened, Usd::ZERO));
        }
        let (held, traded) = (self.qty.abs(), q.abs());
        match traded.cmp(&held) {
            Ordering::Less => {
                let removed =
                    self.basis
                        .portion(traded, held, BASIS_REDUCTION_SCALE, Rounding::HalfEven)?;
                let realized = q.value_at(price)?.negated().checked_sub(removed.to_usd())?;
                let reduced = Self {
                    qty: self.qty.checked_add(q)?,
                    basis: self.basis.checked_sub(removed)?,
                };
                Ok((reduced, realized))
            }
            Ordering::Equal => {
                let realized = q
                    .value_at(price)?
                    .negated()
                    .checked_sub(self.basis.to_usd())?;
                Ok((Self::FLAT, realized))
            }
            Ordering::Greater => {
                let (flat, realized) = self.apply(self.qty.negated(), price)?;
                let (opened, _) = flat.apply(q.checked_add(self.qty)?, price)?;
                Ok((opened, realized))
            }
        }
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
        settled: Usd,
        positions: impl IntoIterator<Item = (InstrumentId, Position)>,
    ) -> Self {
        Self {
            positions: positions
                .into_iter()
                .filter(|(_, p)| !p.qty.is_zero())
                .collect(),
            marks: BTreeMap::new(),
            last_fill_prices: BTreeMap::new(),
            settled,
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
    pub fn apply(&self, input: &Input, config: &Config) -> Result<Applied, AccountingError> {
        let mut next = self.clone();
        let record = match input {
            Input::Fill(execution) => next.fill(execution, config)?,
            Input::Mark { instrument, price } => {
                next.marks.insert(instrument.clone(), *price);
                Record::Mark
            }
            Input::FeesCharged { family, day } => next.charge(*family, *day)?,
            Input::SettlementPosted { date } => next.settle(*date)?,
        };
        Ok(Applied {
            account: next,
            record,
        })
    }

    fn fill(&mut self, e: &Execution, config: &Config) -> Result<Record, AccountingError> {
        if self.applied_fills.contains(&e.fill_id) {
            return Err(AccountingError::DuplicateFill(e.fill_id.clone()));
        }
        if e.qty_gross.is_zero() {
            return Err(AccountingError::ZeroQuantity);
        }
        let notional = e.qty_gross.notional(e.price)?;
        let mut received = e.qty_gross;
        let mut trade_date = None;
        let mut settles_on = None;
        let fees = match e.asset_class {
            AssetClass::UsEquity => {
                let date = config.calendar.equity_trade_date(e.executed_at)?;
                trade_date = Some(date);
                match e.side {
                    Side::Buy => self.settled = self.settled.checked_sub(notional)?,
                    Side::Sell => {
                        let settles = config.calendar.settlement_date(date)?;
                        let bucket = self.unsettled.get(&settles).copied().unwrap_or(Usd::ZERO);
                        self.unsettled
                            .insert(settles, bucket.checked_add(notional)?);
                        settles_on = Some(settles);
                    }
                }
                let fees = self.equity_fees(e, notional, &config.equities)?;
                self.accrue(FeeFamily::Equities, date, &fees)?;
                fees
            }
            AssetClass::Crypto => {
                let bps = match e.liquidity {
                    Some(Liquidity::Maker) => config.crypto.maker,
                    Some(Liquidity::Taker) => config.crypto.taker,
                    None => return Err(AccountingError::MissingLiquidity),
                };
                match e.side {
                    Side::Buy => {
                        let fee_qty = e.qty_gross.times_bps(bps, Rounding::HalfUp)?;
                        received = e.qty_gross.checked_sub(fee_qty)?;
                        let usd = fee_qty.notional(e.price)?;
                        self.asset_fees = self.asset_fees.checked_add(usd)?;
                        self.settled = self.settled.checked_sub(notional)?;
                        vec![Fee {
                            kind: FeeKind::CryptoAsset,
                            usd,
                            asset_qty: Some(fee_qty),
                        }]
                    }
                    Side::Sell => {
                        let usd =
                            notional.times_bps(bps, CRYPTO_USD_FEE_SCALE, Rounding::HalfUp)?;
                        self.settled = self.settled.checked_add(notional)?;
                        let fees = vec![Fee {
                            kind: FeeKind::CryptoUsd,
                            usd,
                            asset_qty: None,
                        }];
                        self.accrue(FeeFamily::Crypto, e.executed_at.date(), &fees)?;
                        fees
                    }
                }
            }
        };
        let signed = match e.side {
            Side::Buy => SignedQty::from(received),
            Side::Sell => SignedQty::from(received).negated(),
        };
        let (position, realized) = self.position(&e.instrument).apply(signed, e.price)?;
        if position.qty.is_zero() {
            self.positions.remove(&e.instrument);
        } else {
            self.positions.insert(e.instrument.clone(), position);
        }
        self.realized_gross = self.realized_gross.checked_add(realized)?;
        self.last_fill_prices.insert(e.instrument.clone(), e.price);
        self.applied_fills.insert(e.fill_id.clone());
        Ok(Record::Fill {
            trade_date,
            settles_on,
            received: signed,
            realized_gross: realized,
            fees,
        })
    }

    fn equity_fees(
        &mut self,
        e: &Execution,
        notional: Usd,
        schedule: &EquityFees,
    ) -> Result<Vec<Fee>, AccountingError> {
        let usd_fee = |kind, usd| Fee {
            kind,
            usd,
            asset_qty: None,
        };
        let cat = usd_fee(
            FeeKind::Cat,
            e.qty_gross.times_per_share(schedule.cat_per_share)?,
        );
        match e.side {
            Side::Buy => Ok(vec![cat]),
            Side::Sell => {
                let sec = notional.times_rate(schedule.sec_rate)?;
                let uncapped = e.qty_gross.times_per_share(schedule.taf_per_share)?;
                let taf = match (schedule.taf_cap_basis, &e.client_order_id) {
                    (TafCapBasis::PerOrder, Some(order)) => {
                        let so_far = self.taf_by_order.get(order).copied().unwrap_or(Usd::ZERO);
                        let room = schedule.taf_cap.checked_sub(so_far)?.max(Usd::ZERO);
                        let taf = uncapped.min(room);
                        self.taf_by_order
                            .insert(order.clone(), so_far.checked_add(taf)?);
                        taf
                    }
                    (TafCapBasis::PerOrder, None) | (TafCapBasis::PerExecution, _) => {
                        uncapped.min(schedule.taf_cap)
                    }
                };
                Ok(vec![
                    usd_fee(FeeKind::Sec, sec),
                    usd_fee(FeeKind::Taf, taf),
                    cat,
                ])
            }
        }
    }

    fn accrue(&mut self, family: FeeFamily, day: Date, fees: &[Fee]) -> Result<(), NumError> {
        let so_far = self
            .accrued
            .get(&(family, day))
            .copied()
            .unwrap_or(Usd::ZERO);
        let total = so_far.checked_add(sum(fees.iter().map(|f| f.usd))?)?;
        self.accrued.insert((family, day), total);
        Ok(())
    }

    fn charge(&mut self, family: FeeFamily, day: Date) -> Result<Record, AccountingError> {
        let accrued = self.accrued.remove(&(family, day)).unwrap_or(Usd::ZERO);
        let charged = match family {
            FeeFamily::Equities => accrued.round(CHARGE_SCALE, Rounding::Ceiling)?,
            FeeFamily::Crypto => accrued,
        };
        self.settled = self.settled.checked_sub(charged)?;
        self.charged = self.charged.checked_add(charged)?;
        Ok(Record::FeesCharged { accrued, charged })
    }

    fn settle(&mut self, date: Date) -> Result<Record, AccountingError> {
        let (due, later): (BTreeMap<Date, Usd>, BTreeMap<Date, Usd>) =
            core::mem::take(&mut self.unsettled)
                .into_iter()
                .partition(|(d, _)| *d <= date);
        self.unsettled = later;
        let amount = sum(due.into_values())?;
        self.settled = self.settled.checked_add(amount)?;
        Ok(Record::SettlementPosted { amount })
    }

    /// Settlement dates whose `SettlementPosted` (00:00 ET) is due at or before `at`, in order.
    pub fn settlements_due(&self, at: UtcNanos) -> Result<Vec<Date>, AccountingError> {
        let mut due = Vec::new();
        for date in self.unsettled.keys() {
            if new_york_midnight(*date)? <= at {
                due.push(*date);
            }
        }
        Ok(due)
    }

    pub fn position(&self, instrument: &InstrumentId) -> Position {
        self.positions
            .get(instrument)
            .copied()
            .unwrap_or(Position::FLAT)
    }

    pub fn positions(&self) -> impl Iterator<Item = (&InstrumentId, Position)> {
        self.positions.iter().map(|(id, p)| (id, *p))
    }

    /// The reporting mark: the latest `MarkUpdated`, else the last fill price (spec §8.2).
    pub fn mark(&self, instrument: &InstrumentId) -> Option<Price> {
        self.marks
            .get(instrument)
            .or_else(|| self.last_fill_prices.get(instrument))
            .copied()
    }

    pub fn settled(&self) -> Usd {
        self.settled
    }

    pub fn unsettled(&self) -> impl Iterator<Item = (Date, Usd)> {
        self.unsettled.iter().map(|(d, a)| (*d, *a))
    }

    /// settled + Σ unsettled (I4).
    pub fn cash_total(&self) -> Result<Usd, AccountingError> {
        Ok(self
            .settled
            .checked_add(sum(self.unsettled.values().copied())?)?)
    }

    pub fn fees_accrued(&self) -> Result<Usd, AccountingError> {
        Ok(sum(self.accrued.values().copied())?)
    }

    pub fn fees_charged(&self) -> Usd {
        self.charged
    }

    /// USD value of asset-denominated fees (crypto buys), never accrued.
    pub fn asset_fees(&self) -> Usd {
        self.asset_fees
    }

    /// charged + accrued + USD value of asset fees.
    pub fn fees_total(&self) -> Result<Usd, AccountingError> {
        Ok(sum([self.charged, self.fees_accrued()?, self.asset_fees])?)
    }

    pub fn realized_gross(&self) -> Usd {
        self.realized_gross
    }

    /// Gross realized − fees (spec §8.1).
    pub fn realized_net(&self) -> Result<Usd, AccountingError> {
        Ok(self.realized_gross.checked_sub(self.fees_total()?)?)
    }

    fn marked(&self) -> Result<Vec<(Position, Price)>, AccountingError> {
        self.positions
            .iter()
            .map(|(id, p)| {
                self.mark(id)
                    .map(|mark| (*p, mark))
                    .ok_or_else(|| AccountingError::NoMark(id.clone()))
            })
            .collect()
    }

    /// Σ Q × mark.
    pub fn market_value(&self) -> Result<Usd, AccountingError> {
        let values = self
            .marked()?
            .into_iter()
            .map(|(p, mark)| p.qty.value_at(mark))
            .collect::<Result<Vec<Usd>, NumError>>()?;
        Ok(sum(values)?)
    }

    /// Σ (Q × mark − B).
    pub fn unrealized(&self) -> Result<Usd, AccountingError> {
        let values = self
            .marked()?
            .into_iter()
            .map(|(p, mark)| p.qty.value_at(mark)?.checked_sub(p.basis.to_usd()))
            .collect::<Result<Vec<Usd>, NumError>>()?;
        Ok(sum(values)?)
    }

    /// settled + Σ unsettled − accrued fees + Σ market value (spec §8.2; receivables and payables
    /// arrive with corporate actions).
    pub fn equity(&self) -> Result<Usd, AccountingError> {
        Ok(self
            .cash_total()?
            .checked_sub(self.fees_accrued()?)?
            .checked_add(self.market_value()?)?)
    }
}
