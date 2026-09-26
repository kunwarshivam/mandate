//! The account state and the fold over [`Input`]s.

use core::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use mandate_num::{CostBasis, MarkPrice, NumError, Price, Rounding, SignedQty, Usd};
use mandate_time::{Date, UtcNanos, new_york_midnight};

use crate::{
    AccountingError, AssetClass, CashDividend, Config, CorporateAction, EquityFees, Execution, Fee,
    FeeFamily, FeeKind, Input, InstrumentId, Liquidity, Side, Split, TafCapBasis,
};

/// Cost-basis reduction: `round(B × |q| ÷ |Q|, 12, half_even)` (spec §8.1).
const BASIS_REDUCTION_SCALE: u32 = 12;
/// The equities daily charge: `round(total, 2, ceiling)` (spec §6.2).
const CHARGE_SCALE: u32 = 2;
/// A crypto sell's USD fee: `round(x, 2, half_up)` (spec §6.3).
const CRYPTO_USD_FEE_SCALE: u32 = 2;
/// An adjusted mark: `round(mark × old ÷ new, 12, half_even)` (spec §8.5).
const MARK_SCALE: u32 = 12;
/// Dividends and cash in lieu are cents, `half_even` (spec §8.5, DEC-93).
const CASH_SCALE: u32 = 2;

fn sum(values: impl IntoIterator<Item = Usd>) -> Result<Usd, NumError> {
    values.into_iter().try_fold(Usd::ZERO, Usd::checked_add)
}

/// Whether `basis` is on the side of zero that `qty` is, with zero allowed on either side and
/// required when flat. A separate function from `Position::new` so the mutation gate reaches it:
/// cargo-mutants never mutates a function named `new`.
fn basis_follows_qty(qty: SignedQty, basis: CostBasis) -> bool {
    match qty.cmp(&SignedQty::ZERO) {
        Ordering::Less => basis <= CostBasis::ZERO,
        Ordering::Equal => basis.is_zero(),
        Ordering::Greater => basis >= CostBasis::ZERO,
    }
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

    /// A long's basis is ≥ 0, a short's ≤ 0, and a flat position's 0 (DEC-86); any other pair is
    /// `invalid_position`.
    pub fn new(qty: SignedQty, basis: CostBasis) -> Result<Self, AccountingError> {
        if basis_follows_qty(qty, basis) {
            Ok(Self { qty, basis })
        } else {
            Err(AccountingError::InvalidPosition)
        }
    }

    pub fn qty(self) -> SignedQty {
        self.qty
    }

    pub fn basis(self) -> CostBasis {
        self.basis
    }

    /// Applies a fill of signed quantity `q` at `price`; returns the new position and the gross
    /// realized P&L. A fill that crosses zero is split into a close and an open. A reduction removes
    /// at most the basis held, so the basis never crosses zero (DEC-86).
    fn apply(self, q: SignedQty, price: Price) -> Result<(Self, Usd), AccountingError> {
        if q.is_zero() {
            return Ok((self, Usd::ZERO));
        }
        if self.qty.is_zero() || self.qty.is_negative() == q.is_negative() {
            let opened = Self::new(
                self.qty.checked_add(q)?,
                self.basis.checked_add(q.value_at(price)?)?,
            )?;
            return Ok((opened, Usd::ZERO));
        }
        let (held, traded) = (self.qty.abs(), q.abs());
        match traded.cmp(&held) {
            Ordering::Less => {
                let rounded =
                    self.basis
                        .portion(traded, held, BASIS_REDUCTION_SCALE, Rounding::HalfEven)?;
                let removed = if self.qty.is_negative() {
                    rounded.max(self.basis)
                } else {
                    rounded.min(self.basis)
                };
                let realized = q.value_at(price)?.negated().checked_sub(removed.to_usd())?;
                let reduced =
                    Self::new(self.qty.checked_add(q)?, self.basis.checked_sub(removed)?)?;
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
    Split {
        before: SignedQty,
        after: SignedQty,
        /// The basis removed with the residual; the whole basis when no share remains.
        residual_basis: CostBasis,
        /// Signed like the position: receivable for a long, payable for a short.
        cash_in_lieu: Usd,
        /// cash in lieu − residual basis.
        realized_gross: Usd,
    },
    CashDividend {
        /// The position held when the dividend was applied.
        entitlement: SignedQty,
        /// Signed like the entitlement: receivable for a long, payable for a short.
        amount: Usd,
    },
    DividendPaid {
        amount: Usd,
    },
    CashInLieuPosted {
        amount: Usd,
    },
}

/// Cash a corporate action made due and the broker has not yet posted. `amount` is signed: positive
/// is owed to the account, negative is owed by it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receivable {
    pub instrument: InstrumentId,
    pub ex_date: Date,
    pub kind: ReceivableKind,
    pub amount: Usd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReceivableKind {
    /// Settled by `DividendPaid` at 00:00 ET on `pay_date`.
    Dividend { pay_date: Date },
    /// Settled by the broker's `CashInLieuPosted`.
    CashInLieu,
}

/// Which corporate actions of one instrument and ex-date have been applied: one of each at most.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ActionKind {
    Split,
    CashDividend,
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
    marks: BTreeMap<InstrumentId, MarkPrice>,
    last_fill_prices: BTreeMap<InstrumentId, MarkPrice>,
    settled: Usd,
    unsettled: BTreeMap<Date, Usd>,
    accrued: BTreeMap<(FeeFamily, Date), Usd>,
    charged: Usd,
    asset_fees: Usd,
    realized_gross: Usd,
    applied_fills: BTreeSet<String>,
    taf_by_order: BTreeMap<String, Usd>,
    income: Usd,
    receivables: BTreeMap<(InstrumentId, Date, ReceivableKind), Usd>,
    last_trade_dates: BTreeMap<InstrumentId, Date>,
    last_ex_dates: BTreeMap<InstrumentId, Date>,
    applied_actions: BTreeSet<(InstrumentId, Date, ActionKind)>,
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
            income: Usd::ZERO,
            receivables: BTreeMap::new(),
            last_trade_dates: BTreeMap::new(),
            last_ex_dates: BTreeMap::new(),
            applied_actions: BTreeSet::new(),
        }
    }

    /// Folds one input. On error the account is unchanged: the caller still holds `self`.
    pub fn apply(&self, input: &Input, config: &Config) -> Result<Applied, AccountingError> {
        let mut next = self.clone();
        let record = match input {
            Input::Fill(execution) => next.fill(execution, config)?,
            Input::Mark { instrument, price } => {
                next.marks
                    .insert(instrument.clone(), MarkPrice::from(*price));
                Record::Mark
            }
            Input::FeesCharged { family, day } => next.charge(*family, *day)?,
            Input::SettlementPosted { date } => next.settle(*date)?,
            Input::CorporateAction(CorporateAction::Split(split)) => next.split(split)?,
            Input::CorporateAction(CorporateAction::CashDividend(dividend)) => {
                next.dividend(dividend)?
            }
            Input::DividendPaid {
                instrument,
                ex_date,
            } => next.dividend_paid(instrument, *ex_date)?,
            Input::CashInLieuPosted { instrument, amount } => {
                next.cash_in_lieu_posted(instrument, *amount)?
            }
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
                if self
                    .last_ex_dates
                    .get(&e.instrument)
                    .is_some_and(|ex| date < *ex)
                {
                    return Err(AccountingError::FillBeforeCorporateAction(
                        e.fill_id.clone(),
                    ));
                }
                let latest = self
                    .last_trade_dates
                    .get(&e.instrument)
                    .map_or(date, |d| date.max(*d));
                self.last_trade_dates.insert(e.instrument.clone(), latest);
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
        self.last_fill_prices
            .insert(e.instrument.clone(), MarkPrice::from(e.price));
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
                let charged = e
                    .client_order_id
                    .as_ref()
                    .and_then(|o| self.taf_by_order.get(o))
                    .copied()
                    .unwrap_or(Usd::ZERO);
                let cap = schedule.taf_cap.to_usd();
                let room = match (schedule.taf_cap_basis, &e.client_order_id) {
                    (TafCapBasis::PerOrder, Some(_)) => cap.checked_sub(charged)?.max(Usd::ZERO),
                    (TafCapBasis::PerOrder, None) | (TafCapBasis::PerExecution, _) => cap,
                };
                let taf = uncapped.min(room);
                if let Some(order) = &e.client_order_id {
                    self.taf_by_order
                        .insert(order.clone(), charged.checked_add(taf)?);
                }
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

    /// Admits one action of `kind` on `instrument` for `ex_date`: at most once, after every fill
    /// traded before the ex-date and before any fill traded on or after it (spec §8.5, DEC-95).
    fn admit(
        &mut self,
        instrument: &InstrumentId,
        ex_date: Date,
        kind: ActionKind,
    ) -> Result<(), AccountingError> {
        let key = (instrument.clone(), ex_date, kind);
        if self.applied_actions.contains(&key) {
            return Err(AccountingError::DuplicateCorporateAction(
                instrument.clone(),
            ));
        }
        let traded_since = self
            .last_trade_dates
            .get(instrument)
            .is_some_and(|d| ex_date <= *d);
        let after_later_action = self
            .last_ex_dates
            .get(instrument)
            .is_some_and(|d| ex_date < *d);
        if traded_since || after_later_action {
            return Err(AccountingError::CorporateActionOutOfOrder(
                instrument.clone(),
            ));
        }
        self.applied_actions.insert(key);
        self.last_ex_dates.insert(instrument.clone(), ex_date);
        Ok(())
    }

    /// Spec §8.5 with DEC-92: while a share remains, the residual's basis is removed by the
    /// formula; when none remains, the whole basis is.
    fn split(&mut self, s: &Split) -> Result<Record, AccountingError> {
        self.admit(&s.instrument, s.ex_date, ActionKind::Split)?;
        for marks in [&mut self.marks, &mut self.last_fill_prices] {
            if let Some(mark) = marks.get_mut(&s.instrument) {
                *mark = s.ratio.mark(*mark, MARK_SCALE, Rounding::HalfEven)?;
            }
        }
        let held = self.position(&s.instrument);
        let split = s.ratio.split(held.qty, s.increment)?;
        let residual_basis = if split.after().is_zero() {
            held.basis
        } else {
            split.residual_basis(held.basis, BASIS_REDUCTION_SCALE, Rounding::HalfEven)?
        };
        let cash_in_lieu = match s.cash_in_lieu_price {
            Some(price) => split.cash_in_lieu(price, CASH_SCALE, Rounding::HalfEven)?,
            None => Usd::ZERO,
        };
        let position = Position::new(split.after(), held.basis.checked_sub(residual_basis)?)?;
        if position.qty.is_zero() {
            self.positions.remove(&s.instrument);
        } else {
            self.positions.insert(s.instrument.clone(), position);
        }
        let realized = cash_in_lieu.checked_sub(residual_basis.to_usd())?;
        self.realized_gross = self.realized_gross.checked_add(realized)?;
        if !cash_in_lieu.is_zero() {
            self.receivables.insert(
                (s.instrument.clone(), s.ex_date, ReceivableKind::CashInLieu),
                cash_in_lieu,
            );
        }
        Ok(Record::Split {
            before: held.qty,
            after: position.qty,
            residual_basis,
            cash_in_lieu,
            realized_gross: realized,
        })
    }

    fn dividend(&mut self, c: &CashDividend) -> Result<Record, AccountingError> {
        self.admit(c.instrument(), c.ex_date(), ActionKind::CashDividend)?;
        let entitlement = self.position(c.instrument()).qty;
        let amount = entitlement
            .value_at(c.per_share())?
            .round(CASH_SCALE, Rounding::HalfEven)?;
        self.income = self.income.checked_add(amount)?;
        if !amount.is_zero() {
            let kind = ReceivableKind::Dividend {
                pay_date: c.pay_date(),
            };
            self.receivables
                .insert((c.instrument().clone(), c.ex_date(), kind), amount);
        }
        Ok(Record::CashDividend {
            entitlement,
            amount,
        })
    }

    fn dividend_paid(
        &mut self,
        instrument: &InstrumentId,
        ex_date: Date,
    ) -> Result<Record, AccountingError> {
        let (key, owed) = self
            .receivables
            .iter()
            .find(|((i, ex, kind), _)| {
                i == instrument && *ex == ex_date && matches!(kind, ReceivableKind::Dividend { .. })
            })
            .map(|(key, owed)| (key.clone(), *owed))
            .ok_or_else(|| AccountingError::NoDividendDue(instrument.clone()))?;
        self.settle_receivable(&key, owed)
            .map(|amount| Record::DividendPaid { amount })
    }

    /// Settles the earliest outstanding cash in lieu on `instrument` of exactly `amount` (DEC-93).
    fn cash_in_lieu_posted(
        &mut self,
        instrument: &InstrumentId,
        amount: Usd,
    ) -> Result<Record, AccountingError> {
        let key = self
            .receivables
            .iter()
            .find(|((i, _, kind), owed)| {
                i == instrument && *kind == ReceivableKind::CashInLieu && **owed == amount
            })
            .map(|(key, _)| key.clone())
            .ok_or_else(|| AccountingError::CashInLieuMismatch(instrument.clone()))?;
        self.settle_receivable(&key, amount)
            .map(|amount| Record::CashInLieuPosted { amount })
    }

    fn settle_receivable(
        &mut self,
        key: &(InstrumentId, Date, ReceivableKind),
        amount: Usd,
    ) -> Result<Usd, AccountingError> {
        self.settled = self.settled.checked_add(amount)?;
        self.receivables.remove(key);
        Ok(amount)
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

    /// Every `SettlementPosted` and `DividendPaid` whose time (00:00 ET of its date) is at or
    /// before `at`, by date; on one date, the settlement first, then dividends by instrument and
    /// ex-date.
    pub fn due(&self, at: UtcNanos) -> Result<Vec<Input>, AccountingError> {
        let mut due: Vec<(Date, bool, Input)> = self
            .settlements_due(at)?
            .into_iter()
            .map(|date| (date, false, Input::SettlementPosted { date }))
            .collect();
        for (instrument, ex_date, kind) in self.receivables.keys() {
            if let ReceivableKind::Dividend { pay_date } = kind
                && new_york_midnight(*pay_date)? <= at
            {
                due.push((
                    *pay_date,
                    true,
                    Input::DividendPaid {
                        instrument: instrument.clone(),
                        ex_date: *ex_date,
                    },
                ));
            }
        }
        due.sort_by_key(|(date, dividend, _)| (*date, *dividend));
        Ok(due.into_iter().map(|(_, _, input)| input).collect())
    }

    /// Outstanding receivables and payables, by instrument, ex-date, and kind.
    pub fn receivables(&self) -> Vec<Receivable> {
        self.receivables
            .iter()
            .map(|((instrument, ex_date, kind), amount)| Receivable {
                instrument: instrument.clone(),
                ex_date: *ex_date,
                kind: *kind,
                amount: *amount,
            })
            .collect()
    }

    /// Σ receivables − Σ payables.
    pub fn net_receivables(&self) -> Result<Usd, AccountingError> {
        Ok(sum(self.receivables.values().copied())?)
    }

    /// Dividends earned (paid on shorts count negative), from the ex-date.
    pub fn income(&self) -> Usd {
        self.income
    }

    /// Gross realized + unrealized + income − fees (spec §8.2).
    pub fn total_pnl(&self) -> Result<Usd, AccountingError> {
        Ok(self
            .realized_gross
            .checked_add(self.unrealized()?)?
            .checked_add(self.income)?
            .checked_sub(self.fees_total()?)?)
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

    /// The reporting mark: the latest `MarkUpdated`, else the last fill price (spec §8.2), each
    /// adjusted by every split since.
    pub fn mark(&self, instrument: &InstrumentId) -> Option<MarkPrice> {
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

    fn marked(&self) -> Result<Vec<(Position, MarkPrice)>, AccountingError> {
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
            .map(|(p, mark)| p.qty.value_at_mark(mark))
            .collect::<Result<Vec<Usd>, NumError>>()?;
        Ok(sum(values)?)
    }

    /// Σ (Q × mark − B).
    pub fn unrealized(&self) -> Result<Usd, AccountingError> {
        let values = self
            .marked()?
            .into_iter()
            .map(|(p, mark)| p.qty.value_at_mark(mark)?.checked_sub(p.basis.to_usd()))
            .collect::<Result<Vec<Usd>, NumError>>()?;
        Ok(sum(values)?)
    }

    /// settled + Σ unsettled + Σ receivables − Σ payables − accrued fees + Σ market value
    /// (spec §8.2).
    pub fn equity(&self) -> Result<Usd, AccountingError> {
        Ok(self
            .cash_total()?
            .checked_add(self.net_receivables()?)?
            .checked_sub(self.fees_accrued()?)?
            .checked_add(self.market_value()?)?)
    }
}
