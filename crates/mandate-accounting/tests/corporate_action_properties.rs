//! Corporate-action invariants as properties (trading-domain spec §8.5, §8.6; DEC-91 to DEC-95),
//! over random sequences of equity fills (whole and fractional, reducing, closing, and crossing
//! zero), marks, splits (forward and reverse, to whole or fractional shares, with and without cash
//! in lieu), cash dividends on longs and shorts, clock advances that pay dividends and settle
//! sales, cash in lieu postings that match or do not, and deliveries the fold must reject.
//! Opening positions include dust: a few 10⁻⁹ shares and a basis of a few 10⁻¹⁸ USD.
//!
//! The oracle is a separate ledger on `i128` integers: quantities in 10⁻⁹, basis in 10⁻¹⁸, marks
//! in 10⁻¹², money in 10⁻²¹. It applies the spec's formulas with its own floor-based rounding and
//! truncating division, and its own weekday and settlement arithmetic (a fixed −4 h offset, valid
//! for the generated window), and every reported value is compared with it after every event.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{d, id, no_fees, round_trip, usd};
use mandate_accounting::{
    Account, AccountingError, AssetClass, CashDividend, Config, CorporateAction, Execution, Input,
    Position, Receivable, ReceivableKind, Record, Side, Split,
};
use mandate_num::{CostBasis, Price, Qty, ShareIncrement, SignedQty, SplitRatio, Usd};
use mandate_time::{Date, UtcNanos};
use proptest::collection::vec;
use proptest::prelude::*;

const QTY: i128 = 1_000_000_000;
/// Money units (10⁻²¹) per basis unit (10⁻¹⁸).
const BASIS_TO_MONEY: i128 = 1_000;
/// Money units per cent.
const CENT: i128 = 10_000_000_000_000_000_000;
/// Mark units (10⁻¹²) per cent.
const MARK_CENT: i128 = 10_000_000_000;
/// Basis units per 10⁻¹² USD, the grid of a removed basis.
const BASIS_GRID: i128 = 1_000_000;
/// 2026-09-21T00:00:00-04:00, a Monday; the generated window ends before daylight-saving time does.
const LOCAL_BASE: i64 = 1_789_963_200;
/// 2026-10-12, a settlement holiday, as a day index from 2026-09-21.
const BANK_HOLIDAY: i64 = 21;
/// 2026-11-01, when the −4 h offset stops holding.
const WINDOW_END: i64 = 41;
const INSTRUMENTS: [&str; 2] = ["AAA", "BBB"];
const MAX_SPLITS: usize = 3;
const SETTLED: i128 = 1_000_000 * 100 * CENT;

/// `n ÷ d` rounded half-even to an integer, for a divisor of either sign.
fn floor_round(n: i128, d: i128) -> i128 {
    let (n, d) = if d < 0 { (-n, -d) } else { (n, d) };
    assert!(d > 0);
    let q = n.div_euclid(d);
    let r = n.rem_euclid(d);
    if 2 * r > d || (2 * r == d && q % 2 != 0) {
        q + 1
    } else {
        q
    }
}

/// Reads canonical decimal text as an integer in units of 10^−scale.
fn units(text: &str, scale: u32) -> i128 {
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (int, frac) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    assert!(
        frac.len() <= scale as usize,
        "{text} has more than {scale} places"
    );
    let value: i128 = format!("{int}{frac:0<width$}", width = scale as usize)
        .parse()
        .unwrap();
    if negative { -value } else { value }
}

/// Canonical decimal text of `units` × 10^−scale.
fn decimal(units: i128, scale: u32) -> String {
    let unit = 10i128.pow(scale);
    let magnitude = units.unsigned_abs();
    let unit_abs = unit.unsigned_abs();
    let frac = format!("{:0width$}", magnitude % unit_abs, width = scale as usize);
    let frac = frac.trim_end_matches('0');
    let sign = if units < 0 { "-" } else { "" };
    if frac.is_empty() {
        format!("{sign}{}", magnitude / unit_abs)
    } else {
        format!("{sign}{}.{frac}", magnitude / unit_abs)
    }
}

fn money(v: Usd) -> i128 {
    units(&v.to_string(), 21)
}

fn qty_units(q: SignedQty) -> i128 {
    units(&q.to_string(), 9)
}

fn basis_units(b: CostBasis) -> i128 {
    units(&b.to_string(), 18)
}

fn mark_units(account: &Account, instrument: usize) -> Option<i128> {
    account
        .mark(&id(INSTRUMENTS[instrument]))
        .map(|m| units(&m.to_string(), 12))
}

fn price(cents: i128) -> Price {
    Price::parse(&decimal(cents, 2)).unwrap()
}

fn is_weekday(day: i64) -> bool {
    day.rem_euclid(7) < 5
}

fn next_weekday(day: i64) -> i64 {
    let mut next = day + 1;
    while !is_weekday(next) {
        next += 1;
    }
    next
}

fn previous_weekday(day: i64) -> i64 {
    let mut previous = day - 1;
    while !is_weekday(previous) {
        previous -= 1;
    }
    previous
}

fn settlement_day(trade_day: i64) -> i64 {
    let mut day = next_weekday(trade_day);
    while day == BANK_HOLIDAY {
        day = next_weekday(day);
    }
    day
}

fn day_date(day: i64) -> Date {
    let mut date = d("2026-09-21");
    for _ in 0..day {
        date = date.next().unwrap();
    }
    date
}

fn day_index(date: Date) -> i64 {
    (0..=WINDOW_END)
        .find(|day| day_date(*day) == date)
        .expect("date inside the window")
}

fn instrument_index(id: &mandate_accounting::InstrumentId) -> usize {
    INSTRUMENTS
        .iter()
        .position(|name| *name == id.as_str())
        .unwrap()
}

fn midnight(day: i64) -> UtcNanos {
    assert!(
        day < WINDOW_END,
        "day {day} is past the fixed-offset window"
    );
    UtcNanos::from_parts(LOCAL_BASE + day * 86_400, 0).unwrap()
}

fn morning(day: i64) -> UtcNanos {
    assert!(
        day < WINDOW_END,
        "day {day} is past the fixed-offset window"
    );
    UtcNanos::from_parts(LOCAL_BASE + day * 86_400 + 10 * 3_600, 0).unwrap()
}

#[derive(Debug, Clone)]
enum Event {
    Fill {
        instrument: usize,
        buy: bool,
        qty: i128,
        cents: i128,
    },
    Mark {
        instrument: usize,
        cents: i128,
    },
    /// Cash in lieu prices are often a few odd cents, and 1:2 whole-share splits common, so that
    /// half a share lands on a half-cent tie.
    Split {
        instrument: usize,
        new: u64,
        old: u64,
        whole: bool,
        cil_cents: Option<i128>,
    },
    /// `per_share` in 10⁻⁴ USD, often a multiple of 0.005 so that amounts land on half-cent ties;
    /// the pay date is `pay_offset` weekdays after the ex-date.
    Dividend {
        instrument: usize,
        per_share: i128,
        pay_offset: u8,
    },
    /// The next weekday's 00:00 ET: every settlement and dividend payment due is folded.
    Advance,
    /// Posts an outstanding cash in lieu amount, or one cent more.
    PostCashInLieu {
        pick: usize,
        exact: bool,
    },
    /// A second delivery of an applied corporate action.
    Redeliver {
        pick: usize,
    },
    /// A fill traded the weekday before the instrument's latest applied ex-date.
    LateFill {
        instrument: usize,
    },
    /// A split whose ex-date is the trade date of the instrument's latest applied fill.
    StaleAction {
        instrument: usize,
    },
}

fn event() -> impl Strategy<Value = Event> {
    prop_oneof![
        6 => (
            0usize..2,
            any::<bool>(),
            prop_oneof![3 => (1i128..=50).prop_map(|s| s * QTY), 1 => 1i128..=50 * QTY, 1 => 1i128..=10],
            1i128..=10_000,
        )
            .prop_map(|(instrument, buy, qty, cents)| Event::Fill { instrument, buy, qty, cents }),
        2 => (0usize..2, 1i128..=10_000).prop_map(|(instrument, cents)| Event::Mark { instrument, cents }),
        4 => (0usize..2, 1u64..=4, 1u64..=4, any::<bool>(), proptest::option::of(prop_oneof![1i128..=10_000, (0i128..50).prop_map(|k| 2 * k + 1)]))
            .prop_map(|(instrument, new, old, whole, cil_cents)| Event::Split { instrument, new, old, whole, cil_cents }),
        2 => (0usize..2, (0i128..50).prop_map(|k| 2 * k + 1))
            .prop_map(|(instrument, cents)| Event::Split { instrument, new: 1, old: 2, whole: true, cil_cents: Some(cents) }),
        3 => (0usize..2, prop_oneof![1i128..=10_000, (1i128..=200).prop_map(|k| 50 * k)], 0u8..=5)
            .prop_map(|(instrument, per_share, pay_offset)| Event::Dividend { instrument, per_share, pay_offset }),
        3 => Just(Event::Advance),
        2 => (any::<usize>(), any::<bool>()).prop_map(|(pick, exact)| Event::PostCashInLieu { pick, exact }),
        1 => any::<usize>().prop_map(|pick| Event::Redeliver { pick }),
        1 => (0usize..2).prop_map(|instrument| Event::LateFill { instrument }),
        1 => (0usize..2).prop_map(|instrument| Event::StaleAction { instrument }),
    ]
}

/// An opening position (10⁻⁹ shares, 10⁻¹⁸ USD of basis) and its mark in cents.
#[derive(Debug, Clone, Copy)]
struct Opening {
    qty: i128,
    basis: i128,
    cents: i128,
}

fn opening() -> impl Strategy<Value = Option<Opening>> {
    proptest::option::of(
        (
            prop_oneof![
                1 => 1i128..=10,
                3 => (1i128..=100).prop_map(|s| s * QTY),
                1 => 1i128..=100 * QTY
            ],
            prop_oneof![0i128..=10_000_000, 0i128..=10_000_000_000_000_000_000_000],
            any::<bool>(),
            1i128..=10_000,
        )
            .prop_map(|(qty, basis, short, cents)| {
                let sign = if short { -1 } else { 1 };
                Opening {
                    qty: sign * qty,
                    basis: sign * basis,
                    cents,
                }
            }),
    )
}

#[derive(Debug, Clone)]
struct Scenario {
    openings: [Option<Opening>; 2],
    events: Vec<Event>,
}

fn scenario() -> impl Strategy<Value = Scenario> {
    ((opening(), opening()), vec(event(), 1..24)).prop_map(|((a, b), events)| Scenario {
        openings: [a, b],
        events,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Due {
    Settlement,
    Dividend { instrument: usize, ex: i64 },
}

/// The oracle ledger. Cash in money units, basis in basis units, marks in mark units.
#[derive(Debug, Clone, Default)]
struct Oracle {
    positions: BTreeMap<usize, (i128, i128)>,
    marks: BTreeMap<usize, i128>,
    last_fill: BTreeMap<usize, i128>,
    settled: i128,
    unsettled: BTreeMap<i64, i128>,
    realized: i128,
    income: i128,
    /// (instrument, ex-date) → (pay date, amount).
    dividends: BTreeMap<(usize, i64), (i64, i128)>,
    /// (instrument, ex-date) → amount.
    cash_in_lieu: BTreeMap<(usize, i64), i128>,
    last_trade: BTreeMap<usize, i64>,
    last_ex: BTreeMap<usize, i64>,
    /// (instrument, ex-date, is a split).
    applied: BTreeSet<(usize, i64, bool)>,
    splits: usize,
}

impl Oracle {
    fn mark(&self, i: usize) -> Option<i128> {
        self.marks
            .get(&i)
            .or_else(|| self.last_fill.get(&i))
            .copied()
    }

    fn market_value(&self) -> i128 {
        self.positions
            .iter()
            .map(|(i, (q, _))| q * self.mark(*i).unwrap())
            .sum()
    }

    fn unrealized(&self) -> i128 {
        self.positions
            .iter()
            .map(|(i, (q, b))| q * self.mark(*i).unwrap() - b * BASIS_TO_MONEY)
            .sum()
    }

    fn receivables(&self) -> Vec<Receivable> {
        let mut all: Vec<((usize, i64, u8), Receivable)> = Vec::new();
        for ((i, ex), (pay, amount)) in &self.dividends {
            all.push((
                (*i, *ex, 0),
                Receivable {
                    instrument: id(INSTRUMENTS[*i]),
                    ex_date: day_date(*ex),
                    kind: ReceivableKind::Dividend {
                        pay_date: day_date(*pay),
                    },
                    amount: usd(&decimal(*amount, 21)),
                },
            ));
        }
        for ((i, ex), amount) in &self.cash_in_lieu {
            all.push((
                (*i, *ex, 1),
                Receivable {
                    instrument: id(INSTRUMENTS[*i]),
                    ex_date: day_date(*ex),
                    kind: ReceivableKind::CashInLieu,
                    amount: usd(&decimal(*amount, 21)),
                },
            ));
        }
        all.sort_by_key(|a| a.0);
        all.into_iter().map(|(_, r)| r).collect()
    }

    fn net_receivables(&self) -> i128 {
        self.dividends.values().map(|(_, a)| a).sum::<i128>()
            + self.cash_in_lieu.values().sum::<i128>()
    }

    fn equity(&self) -> i128 {
        self.settled
            + self.unsettled.values().sum::<i128>()
            + self.net_receivables()
            + self.market_value()
    }

    /// §8.1 with DEC-86's limit, crossings split into a close and an open.
    fn trade(&mut self, i: usize, q: i128, cents: i128) {
        let (held, basis) = self.positions.get(&i).copied().unwrap_or((0, 0));
        let cost = q * cents * 10_000_000;
        let proceeds = -q * cents * 10_000_000_000;
        let (new_q, new_b, realized) = if held == 0 || (held > 0) == (q > 0) {
            (held + q, basis + cost, 0)
        } else if q.abs() < held.abs() {
            let rounded = floor_round(basis * q.abs(), held.abs() * BASIS_GRID) * BASIS_GRID;
            let removed = if held > 0 {
                rounded.min(basis)
            } else {
                rounded.max(basis)
            };
            (
                held + q,
                basis - removed,
                proceeds - removed * BASIS_TO_MONEY,
            )
        } else {
            let realized = held * cents * 10_000_000_000 - basis * BASIS_TO_MONEY;
            let rest = q + held;
            (rest, rest * cents * 10_000_000, realized)
        };
        self.realized += realized;
        if new_q == 0 {
            self.positions.remove(&i);
        } else {
            self.positions.insert(i, (new_q, new_b));
        }
    }

    fn fill(
        &mut self,
        i: usize,
        fill_id: &str,
        q: i128,
        cents: i128,
        trade_day: i64,
    ) -> Result<Option<Record>, AccountingError> {
        if self.last_ex.get(&i).is_some_and(|ex| trade_day < *ex) {
            return Err(AccountingError::FillBeforeCorporateAction(fill_id.into()));
        }
        let notional = q.abs() * cents * 10_000_000_000;
        if q > 0 {
            self.settled -= notional;
        } else {
            *self.unsettled.entry(settlement_day(trade_day)).or_default() += notional;
        }
        self.trade(i, q, cents);
        self.last_fill.insert(i, cents * MARK_CENT);
        let latest = self.last_trade.entry(i).or_insert(trade_day);
        *latest = (*latest).max(trade_day);
        Ok(None)
    }

    fn admit(&self, i: usize, ex: i64, split: bool) -> Result<(), AccountingError> {
        let instrument = id(INSTRUMENTS[i]);
        if self.applied.contains(&(i, ex, split)) {
            return Err(AccountingError::DuplicateCorporateAction(instrument));
        }
        if self.last_trade.get(&i).is_some_and(|t| ex <= *t)
            || self.last_ex.get(&i).is_some_and(|e| ex < *e)
        {
            return Err(AccountingError::CorporateActionOutOfOrder(instrument));
        }
        Ok(())
    }

    fn split(
        &mut self,
        i: usize,
        ex: i64,
        (new, old): (i128, i128),
        whole: bool,
        cil_cents: Option<i128>,
    ) -> Result<Option<Record>, AccountingError> {
        self.admit(i, ex, true)?;
        let mut next = self.clone();
        for m in [next.marks.get_mut(&i), next.last_fill.get_mut(&i)]
            .into_iter()
            .flatten()
        {
            *m = floor_round(*m * old, new);
            if *m <= 0 {
                return Err(AccountingError::Num(mandate_num::NumError::NotPositive));
            }
        }
        let (q, b) = next.positions.get(&i).copied().unwrap_or((0, 0));
        let scaled = q * new;
        let increment = if whole { QTY } else { 1 };
        let after = scaled / (old * increment) * increment;
        let residual = scaled - after * old;
        let removed = if after == 0 {
            b
        } else {
            floor_round(b * residual, scaled * BASIS_GRID) * BASIS_GRID
        };
        let cil = cil_cents.map_or(0, |p| floor_round(residual * p, old * QTY)) * CENT;
        let realized = cil - removed * BASIS_TO_MONEY;
        let basis = b - removed;
        assert_basis_follows(after, basis);
        if after == 0 {
            next.positions.remove(&i);
        } else {
            next.positions.insert(i, (after, basis));
        }
        next.realized += realized;
        if cil != 0 {
            next.cash_in_lieu.insert((i, ex), cil);
        }
        next.applied.insert((i, ex, true));
        next.last_ex.insert(i, ex);
        next.splits += 1;
        *self = next;
        Ok(Some(Record::Split {
            before: SignedQty::parse(&decimal(q, 9)).unwrap(),
            after: SignedQty::parse(&decimal(after, 9)).unwrap(),
            residual_basis: CostBasis::parse(&decimal(removed, 18)).unwrap(),
            cash_in_lieu: usd(&decimal(cil, 21)),
            realized_gross: usd(&decimal(realized, 21)),
        }))
    }

    fn dividend(
        &mut self,
        i: usize,
        ex: i64,
        pay: i64,
        per_share: i128,
    ) -> Result<Option<Record>, AccountingError> {
        self.admit(i, ex, false)?;
        let q = self.positions.get(&i).map_or(0, |(q, _)| *q);
        let amount = floor_round(q * per_share, 100_000_000_000) * CENT;
        self.income += amount;
        if amount != 0 {
            self.dividends.insert((i, ex), (pay, amount));
        }
        self.applied.insert((i, ex, false));
        self.last_ex.insert(i, ex);
        Ok(Some(Record::CashDividend {
            entitlement: SignedQty::parse(&decimal(q, 9)).unwrap(),
            amount: usd(&decimal(amount, 21)),
        }))
    }

    /// Everything due at 00:00 ET of `today`, by date, the settlement first on each date.
    fn due(&self, today: i64) -> Vec<(i64, Due)> {
        let mut due: Vec<(i64, Due)> = self
            .unsettled
            .keys()
            .filter(|day| **day <= today)
            .map(|day| (*day, Due::Settlement))
            .chain(
                self.dividends
                    .iter()
                    .filter(|(_, (pay, _))| *pay <= today)
                    .map(|((i, ex), (pay, _))| {
                        (
                            *pay,
                            Due::Dividend {
                                instrument: *i,
                                ex: *ex,
                            },
                        )
                    }),
            )
            .collect();
        due.sort();
        due
    }

    fn settle(&mut self, date: i64) -> Result<Option<Record>, AccountingError> {
        let days: Vec<i64> = self
            .unsettled
            .keys()
            .copied()
            .filter(|day| *day <= date)
            .collect();
        let mut amount = 0;
        for day in days {
            amount += self.unsettled.remove(&day).unwrap();
        }
        self.settled += amount;
        Ok(Some(Record::SettlementPosted {
            amount: usd(&decimal(amount, 21)),
        }))
    }

    fn pay(&mut self, i: usize, ex: i64) -> Result<Option<Record>, AccountingError> {
        let Some((_, amount)) = self.dividends.remove(&(i, ex)) else {
            return Err(AccountingError::NoDividendDue(id(INSTRUMENTS[i])));
        };
        self.settled += amount;
        Ok(Some(Record::DividendPaid {
            amount: usd(&decimal(amount, 21)),
        }))
    }

    fn post(&mut self, i: usize, amount: i128) -> Result<Option<Record>, AccountingError> {
        let Some(key) = self
            .cash_in_lieu
            .iter()
            .find(|((instrument, _), a)| *instrument == i && **a == amount)
            .map(|(key, _)| *key)
        else {
            return Err(AccountingError::CashInLieuMismatch(id(INSTRUMENTS[i])));
        };
        self.cash_in_lieu.remove(&key);
        self.settled += amount;
        Ok(Some(Record::CashInLieuPosted {
            amount: usd(&decimal(amount, 21)),
        }))
    }
}

fn assert_basis_follows(qty: i128, basis: i128) {
    assert!(
        (qty > 0 && basis >= 0) || (qty < 0 && basis <= 0) || (qty == 0 && basis == 0),
        "the oracle's own split left qty {qty} with basis {basis}"
    );
}

struct Step {
    input: Input,
    before: Account,
    after: Account,
    result: Result<Record, AccountingError>,
}

struct Run {
    opening: Account,
    steps: Vec<Step>,
    oracle: Oracle,
}

fn split_input(i: usize, ex: i64, new: u64, old: u64, whole: bool, cil: Option<i128>) -> Input {
    Input::CorporateAction(CorporateAction::Split(Split {
        instrument: id(INSTRUMENTS[i]),
        ex_date: day_date(ex),
        ratio: SplitRatio::new(new, old).unwrap(),
        increment: if whole {
            ShareIncrement::Whole
        } else {
            ShareIncrement::Fractional
        },
        cash_in_lieu_price: cil.map(price),
    }))
}

fn fill_input(fill_id: &str, i: usize, q: i128, cents: i128, day: i64) -> Input {
    Input::Fill(Execution {
        fill_id: fill_id.into(),
        client_order_id: None,
        instrument: id(INSTRUMENTS[i]),
        asset_class: AssetClass::UsEquity,
        side: if q > 0 { Side::Buy } else { Side::Sell },
        qty_gross: Qty::parse(&decimal(q.abs(), 9)).unwrap(),
        price: price(cents),
        liquidity: None,
        executed_at: morning(day),
    })
}

/// Drives the library through a scenario and then drains it: the clock advances until every
/// dividend is paid and every sale settled, and each outstanding cash in lieu is posted. With
/// `check_oracle`, every result and every reported value is compared with the oracle after every
/// event. Invariant properties run without it so each stands on its own.
fn run(scenario: &Scenario, check_oracle: bool) -> Result<Run, TestCaseError> {
    let config = no_fees();
    let mut oracle = Oracle {
        settled: SETTLED,
        ..Oracle::default()
    };
    let mut positions = Vec::new();
    let mut marks = Vec::new();
    for (i, opening) in scenario.openings.iter().enumerate() {
        if let Some(o) = opening {
            positions.push((
                id(INSTRUMENTS[i]),
                Position::new(
                    SignedQty::parse(&decimal(o.qty, 9)).unwrap(),
                    CostBasis::parse(&decimal(o.basis, 18)).unwrap(),
                )
                .unwrap(),
            ));
            oracle.positions.insert(i, (o.qty, o.basis));
            oracle.marks.insert(i, o.cents * MARK_CENT);
            marks.push(Input::Mark {
                instrument: id(INSTRUMENTS[i]),
                price: price(o.cents),
            });
        }
    }
    let opening = marks.iter().fold(
        Account::opening(usd(&decimal(SETTLED, 21)), positions),
        |account, mark| account.apply(mark, &config).unwrap().account,
    );
    let mut account = opening.clone();
    let mut work: Vec<(Input, Result<Option<Record>, AccountingError>)> = Vec::new();
    let mut steps = Vec::new();
    let mut applied_actions: Vec<Input> = Vec::new();
    let mut today = 0i64;

    let fold = |account: &mut Account,
                oracle: &Oracle,
                work: Vec<(Input, Result<Option<Record>, AccountingError>)>,
                steps: &mut Vec<Step>|
     -> Result<(), TestCaseError> {
        for (input, expected) in work {
            let before = account.clone();
            let result = account.apply(&input, &config).map(|applied| {
                *account = applied.account;
                applied.record
            });
            match (&expected, &result) {
                (Ok(Some(want)), Ok(got)) if check_oracle => {
                    prop_assert_eq!(want, got, "{:?}", input);
                }
                (Ok(_), Ok(_)) => {}
                (Err(want), Err(got)) => {
                    prop_assert_eq!(want, got, "{:?}", input);
                    prop_assert_eq!(&before, &*account, "rejected {:?}", input);
                }
                _ => prop_assert!(
                    false,
                    "{:?}: expected {:?}, got {:?}",
                    input,
                    expected,
                    result
                ),
            }
            steps.push(Step {
                input,
                before,
                after: account.clone(),
                result,
            });
        }
        if check_oracle {
            compare(account, oracle)?;
        }
        Ok(())
    };

    if check_oracle {
        compare(&account, &oracle)?;
    }

    for (n, event) in scenario.events.iter().enumerate() {
        match *event {
            Event::Fill {
                instrument,
                buy,
                qty,
                cents,
            } => {
                let q = if buy { qty } else { -qty };
                let fill_id = format!("f{n}");
                let expected = oracle.fill(instrument, &fill_id, q, cents, today);
                work.push((fill_input(&fill_id, instrument, q, cents, today), expected));
            }
            Event::Mark { instrument, cents } => {
                oracle.marks.insert(instrument, cents * MARK_CENT);
                work.push((
                    Input::Mark {
                        instrument: id(INSTRUMENTS[instrument]),
                        price: price(cents),
                    },
                    Ok(None),
                ));
            }
            Event::Split {
                instrument,
                new,
                old,
                whole,
                cil_cents,
            } => {
                if oracle.splits < MAX_SPLITS {
                    let ex = next_weekday(today);
                    let expected = oracle.split(
                        instrument,
                        ex,
                        (i128::from(new), i128::from(old)),
                        whole,
                        cil_cents,
                    );
                    let input = split_input(instrument, ex, new, old, whole, cil_cents);
                    if expected.is_ok() {
                        today = ex;
                        applied_actions.push(input.clone());
                    }
                    work.push((input, expected));
                }
            }
            Event::Dividend {
                instrument,
                per_share,
                pay_offset,
            } => {
                let ex = next_weekday(today);
                let mut pay = ex;
                for _ in 0..pay_offset {
                    pay = next_weekday(pay);
                }
                let expected = oracle.dividend(instrument, ex, pay, per_share);
                let input = Input::CorporateAction(CorporateAction::CashDividend(
                    CashDividend::new(
                        id(INSTRUMENTS[instrument]),
                        day_date(ex),
                        day_date(pay),
                        Price::parse(&decimal(per_share, 4)).unwrap(),
                    )
                    .unwrap(),
                ));
                if expected.is_ok() {
                    today = ex;
                    applied_actions.push(input.clone());
                }
                work.push((input, expected));
            }
            Event::Advance => {
                today = next_weekday(today);
                advance(&account, &mut oracle, today, check_oracle, &mut work)?;
            }
            Event::PostCashInLieu { pick, exact } => {
                let outstanding: Vec<((usize, i64), i128)> =
                    oracle.cash_in_lieu.iter().map(|(k, a)| (*k, *a)).collect();
                if !outstanding.is_empty() {
                    let ((i, _), amount) = outstanding[pick % outstanding.len()];
                    let amount = if exact { amount } else { amount + CENT };
                    let expected = oracle.post(i, amount);
                    work.push((
                        Input::CashInLieuPosted {
                            instrument: id(INSTRUMENTS[i]),
                            amount: usd(&decimal(amount, 21)),
                        },
                        expected,
                    ));
                }
            }
            Event::Redeliver { pick } => {
                if !applied_actions.is_empty() {
                    let input = applied_actions[pick % applied_actions.len()].clone();
                    let Input::CorporateAction(action) = &input else {
                        unreachable!("only corporate actions are kept")
                    };
                    let expected = match action {
                        CorporateAction::Split(s) => Err(
                            AccountingError::DuplicateCorporateAction(s.instrument.clone()),
                        ),
                        CorporateAction::CashDividend(c) => Err(
                            AccountingError::DuplicateCorporateAction(c.instrument().clone()),
                        ),
                    };
                    work.push((input, expected));
                }
            }
            Event::LateFill { instrument } => {
                if let Some(ex) = oracle.last_ex.get(&instrument).copied() {
                    let fill_id = format!("late{n}");
                    let day = previous_weekday(ex);
                    let expected = oracle.fill(instrument, &fill_id, QTY, 100, day);
                    work.push((fill_input(&fill_id, instrument, QTY, 100, day), expected));
                }
            }
            Event::StaleAction { instrument } => {
                if let Some(ex) = oracle.last_trade.get(&instrument).copied() {
                    let expected = oracle.split(instrument, ex, (2, 1), false, None);
                    prop_assert!(expected.is_err(), "a stale split was admitted");
                    work.push((split_input(instrument, ex, 2, 1, false, None), expected));
                }
            }
        }
        fold(
            &mut account,
            &oracle,
            core::mem::take(&mut work),
            &mut steps,
        )?;
    }

    while !oracle.dividends.is_empty() || !oracle.unsettled.is_empty() {
        today = next_weekday(today);
        advance(&account, &mut oracle, today, check_oracle, &mut work)?;
        fold(
            &mut account,
            &oracle,
            core::mem::take(&mut work),
            &mut steps,
        )?;
    }
    let outstanding: Vec<((usize, i64), i128)> =
        oracle.cash_in_lieu.iter().map(|(k, a)| (*k, *a)).collect();
    for ((i, _), amount) in outstanding {
        let expected = oracle.post(i, amount);
        work.push((
            Input::CashInLieuPosted {
                instrument: id(INSTRUMENTS[i]),
                amount: usd(&decimal(amount, 21)),
            },
            expected,
        ));
        fold(
            &mut account,
            &oracle,
            core::mem::take(&mut work),
            &mut steps,
        )?;
    }
    Ok(Run {
        opening,
        steps,
        oracle,
    })
}

/// Queues everything `account.due` reports at 00:00 ET of `today`, after checking it against the
/// oracle's own list.
fn advance(
    account: &Account,
    oracle: &mut Oracle,
    today: i64,
    check_oracle: bool,
    work: &mut Vec<(Input, Result<Option<Record>, AccountingError>)>,
) -> Result<(), TestCaseError> {
    let reported = account.due(midnight(today)).unwrap();
    let due = oracle.due(today);
    let expected: Vec<Input> = due
        .iter()
        .map(|(date, due)| match due {
            Due::Settlement => Input::SettlementPosted {
                date: day_date(*date),
            },
            Due::Dividend { instrument, ex } => Input::DividendPaid {
                instrument: id(INSTRUMENTS[*instrument]),
                ex_date: day_date(*ex),
            },
        })
        .collect();
    if check_oracle {
        prop_assert_eq!(&reported, &expected, "due on day {}", today);
    }
    for input in reported {
        let result = match &input {
            Input::SettlementPosted { date } => oracle.settle(day_index(*date)),
            Input::DividendPaid {
                instrument,
                ex_date,
            } => oracle.pay(instrument_index(instrument), day_index(*ex_date)),
            other => {
                prop_assert!(false, "due reported {:?}", other);
                unreachable!()
            }
        };
        work.push((input, result));
    }
    Ok(())
}

fn compare(account: &Account, oracle: &Oracle) -> Result<(), TestCaseError> {
    for (i, name) in INSTRUMENTS.iter().enumerate() {
        let p = account.position(&id(name));
        let (q, b) = oracle.positions.get(&i).copied().unwrap_or((0, 0));
        prop_assert_eq!(qty_units(p.qty()), q, "{} qty", name);
        prop_assert_eq!(basis_units(p.basis()), b, "{} basis", name);
        prop_assert_eq!(mark_units(account, i), oracle.mark(i), "{} mark", name);
    }
    prop_assert_eq!(money(account.settled()), oracle.settled, "settled");
    let unsettled: Vec<(Date, i128)> = account.unsettled().map(|(d, a)| (d, money(a))).collect();
    let expected: Vec<(Date, i128)> = oracle
        .unsettled
        .iter()
        .map(|(day, a)| (day_date(*day), *a))
        .collect();
    prop_assert_eq!(unsettled, expected, "unsettled");
    prop_assert_eq!(account.receivables(), oracle.receivables(), "receivables");
    prop_assert_eq!(
        money(account.net_receivables().unwrap()),
        oracle.net_receivables(),
        "net receivables"
    );
    prop_assert_eq!(money(account.income()), oracle.income, "income");
    prop_assert_eq!(money(account.realized_gross()), oracle.realized, "realized");
    prop_assert_eq!(
        money(account.market_value().unwrap()),
        oracle.market_value(),
        "market value"
    );
    prop_assert_eq!(
        money(account.unrealized().unwrap()),
        oracle.unrealized(),
        "unrealized"
    );
    prop_assert_eq!(money(account.equity().unwrap()), oracle.equity(), "equity");
    prop_assert_eq!(
        money(account.total_pnl().unwrap()),
        oracle.realized + oracle.unrealized() + oracle.income,
        "total P&L"
    );
    Ok(())
}

fn split_of(input: &Input) -> Option<&Split> {
    match input {
        Input::CorporateAction(CorporateAction::Split(s)) => Some(s),
        _ => None,
    }
}

proptest! {
    #[test]
    #[ignore = "pending E3-2"]
    fn every_reported_value_matches_the_oracle_after_every_event(s in scenario()) {
        run(&s, true)?;
    }

    /// I1 with income: Δequity = Δrealized + Δunrealized + Δincome − Δfees, exactly, for every
    /// event including splits, dividends, payments, and postings.
    #[test]
    #[ignore = "pending E3-2"]
    fn i1_conservation_with_income_holds_for_every_event(s in scenario()) {
        for step in run(&s, false)?.steps {
            let (b, a) = (&step.before, &step.after);
            let delta = |f: &dyn Fn(&Account) -> Usd| money(f(a)) - money(f(b));
            let equity = delta(&|x| x.equity().unwrap());
            let realized = delta(&|x| x.realized_gross());
            let unrealized = delta(&|x| x.unrealized().unwrap());
            let income = delta(&|x| x.income());
            let fees = delta(&|x| x.fees_total().unwrap());
            prop_assert_eq!(equity, realized + unrealized + income - fees, "{:?}", step.input);
        }
    }

    /// I3 as corrected by DEC-94: ΔMV + f × mark' = Q_raw × (mark' − mark × old ÷ new), so
    /// |ΔMV + f × mark'| ≤ |Q_raw| × 5 × 10⁻¹³, and it is 0 when mark × old ÷ new terminates within
    /// 12 places. Multiplied through by `old`, in 10⁻²¹ USD: old × ΔMV + r × mark' =
    /// Q × (new × mark' − old × mark), with r = Q × new − Q' × old.
    #[test]
    #[ignore = "pending E3-2"]
    fn i3_a_split_moves_market_value_only_by_the_mark_rounding(s in scenario()) {
        for step in run(&s, false)?.steps {
            let (Some(split), Ok(Record::Split { before, after, .. })) = (split_of(&step.input), &step.result) else { continue };
            let i = instrument_index(&split.instrument);
            let (new, old) = (i128::from(split.ratio.new_shares()), i128::from(split.ratio.old_shares()));
            let (q, q_after) = (qty_units(*before), qty_units(*after));
            let (Some(mark), Some(adjusted)) = (mark_units(&step.before, i), mark_units(&step.after, i)) else { continue };
            let delta_mv = money(step.after.market_value().unwrap()) - money(step.before.market_value().unwrap());
            let residual = q * new - q_after * old;
            let lhs = old * delta_mv + residual * adjusted;
            prop_assert_eq!(lhs, q * (new * adjusted - old * mark));
            prop_assert!(2 * lhs.abs() <= q.abs() * new, "{} exceeds |Q_raw| × 5 × 10⁻¹³", lhs);
            if (mark * old) % new == 0 {
                prop_assert_eq!(lhs, 0);
            }
        }
    }

    /// §8.5 and DEC-92: B' = B − R with R = round(B × (Q·new − Q'·old) ÷ (Q·new), 12, half_even)
    /// while a share remains and R = B when none does, so R never exceeds the basis held and B'
    /// keeps the sign of Q'; realized changes by cash in lieu − R, and cash in lieu is
    /// round(f × price, 2, half_even) (DEC-93).
    #[test]
    #[ignore = "pending E3-2"]
    fn a_split_removes_the_residuals_basis_and_never_more_than_the_basis_held(s in scenario()) {
        for step in run(&s, false)?.steps {
            let (Some(split), Ok(Record::Split { before, after, residual_basis, cash_in_lieu, realized_gross })) = (split_of(&step.input), &step.result) else { continue };
            let instrument = &split.instrument;
            let (new, old) = (i128::from(split.ratio.new_shares()), i128::from(split.ratio.old_shares()));
            let (q, q_after) = (qty_units(*before), qty_units(*after));
            let b = basis_units(step.before.position(instrument).basis());
            let b_after = basis_units(step.after.position(instrument).basis());
            let r = basis_units(*residual_basis);
            let residual = q * new - q_after * old;
            if q_after == 0 {
                prop_assert_eq!(r, b);
            } else {
                prop_assert_eq!(r, floor_round(b * residual, q * new * BASIS_GRID) * BASIS_GRID);
            }
            prop_assert!(r.abs() <= b.abs() && (r == 0 || (r > 0) == (b > 0)));
            prop_assert_eq!(b_after, b - r);
            prop_assert!((q_after > 0 && b_after >= 0) || (q_after < 0 && b_after <= 0) || (q_after == 0 && b_after == 0));
            let cil = split.cash_in_lieu_price.map_or(0, |p| {
                floor_round(residual * units(&p.to_string(), 2), old * QTY) * CENT
            });
            prop_assert_eq!(money(*cash_in_lieu), cil);
            prop_assert_eq!(money(*realized_gross), cil - r * BASIS_TO_MONEY);
            prop_assert_eq!(
                money(step.after.realized_gross()) - money(step.before.realized_gross()),
                cil - r * BASIS_TO_MONEY
            );
        }
    }

    /// §8.5: every stored mark is replaced by round(mark × old ÷ new, 12, half_even) on each
    /// split, whether it came from a mark or a fill and whether a position is held; marks of other
    /// instruments are untouched.
    #[test]
    #[ignore = "pending E3-2"]
    fn every_stored_mark_is_replaced_by_the_adjusted_mark(s in scenario()) {
        for step in run(&s, false)?.steps {
            let (Some(split), Ok(_)) = (split_of(&step.input), &step.result) else { continue };
            let target = instrument_index(&split.instrument);
            let (new, old) = (i128::from(split.ratio.new_shares()), i128::from(split.ratio.old_shares()));
            for i in 0..INSTRUMENTS.len() {
                let before = mark_units(&step.before, i);
                let expected = if i == target { before.map(|m| floor_round(m * old, new)) } else { before };
                prop_assert_eq!(mark_units(&step.after, i), expected);
            }
        }
    }

    /// I5 with splits: Q is the fold of received fill quantities, split truncations, and residual
    /// removals; each split's Q' is Q × new ÷ old truncated toward zero to the share increment.
    #[test]
    #[ignore = "pending E3-2"]
    fn i5_quantity_is_the_fold_of_fills_and_split_truncations(s in scenario()) {
        let run = run(&s, false)?;
        let mut folded: BTreeMap<String, i128> = run
            .opening
            .positions()
            .map(|(id, p)| (id.as_str().to_owned(), qty_units(p.qty())))
            .collect();
        for step in &run.steps {
            match (&step.input, &step.result) {
                (Input::Fill(e), Ok(Record::Fill { received, .. })) => {
                    *folded.entry(e.instrument.as_str().to_owned()).or_default() += qty_units(*received);
                }
                (Input::CorporateAction(CorporateAction::Split(split)), Ok(Record::Split { before, after, .. })) => {
                    let (new, old) = (i128::from(split.ratio.new_shares()), i128::from(split.ratio.old_shares()));
                    let increment = match split.increment { ShareIncrement::Whole => QTY, ShareIncrement::Fractional => 1 };
                    let (q, q_after) = (qty_units(*before), qty_units(*after));
                    let held = folded.entry(split.instrument.as_str().to_owned()).or_default();
                    prop_assert_eq!(q, *held);
                    prop_assert_eq!(q_after % increment, 0);
                    let residual = q * new - q_after * old;
                    prop_assert!(residual == 0 || (residual > 0) == (q > 0), "truncated away from zero");
                    prop_assert!(residual.abs() < old * increment, "not truncated to the increment");
                    *held = q_after;
                }
                _ => {}
            }
            for (name, q) in &folded {
                prop_assert_eq!(qty_units(step.after.position(&id(name)).qty()), *q, "{}", name);
            }
        }
    }

    /// §8.5: a dividend's entitlement is the position after every fill traded before the ex-date
    /// and none traded on or after it; the amount is round(Q × d, 2, half_even).
    #[test]
    #[ignore = "pending E3-2"]
    fn a_dividend_is_entitled_on_the_position_after_fills_traded_before_the_ex_date(s in scenario()) {
        let run = run(&s, false)?;
        for (n, step) in run.steps.iter().enumerate() {
            let (Input::CorporateAction(CorporateAction::CashDividend(c)), Ok(Record::CashDividend { entitlement, amount })) = (&step.input, &step.result) else { continue };
            prop_assert_eq!(*entitlement, step.before.position(c.instrument()).qty());
            let q = qty_units(*entitlement);
            prop_assert_eq!(money(*amount), floor_round(q * units(&c.per_share().to_string(), 9), 10_000_000_000_000_000) * CENT);
            for (m, other) in run.steps.iter().enumerate() {
                let (Input::Fill(e), Ok(Record::Fill { trade_date: Some(traded), .. })) = (&other.input, &other.result) else { continue };
                if &e.instrument == c.instrument() {
                    prop_assert_eq!(m < n, *traded < c.ex_date(), "fill {} against the dividend", e.fill_id);
                }
            }
        }
    }

    /// Every receivable and payable a corporate action records becomes settled cash exactly once:
    /// net receivables move by what is recorded less what is paid, a payment moves exactly its
    /// amount into settled cash and leaves equity unchanged, and once the clock has passed every
    /// pay date and every cash in lieu is posted, nothing is outstanding.
    #[test]
    #[ignore = "pending E3-2"]
    fn receivables_become_settled_cash_exactly_once(s in scenario()) {
        let run = run(&s, false)?;
        let (mut recorded, mut paid) = (0i128, 0i128);
        for step in &run.steps {
            let (b, a) = (&step.before, &step.after);
            let delta_net = money(a.net_receivables().unwrap()) - money(b.net_receivables().unwrap());
            let delta_settled = money(a.settled()) - money(b.settled());
            match &step.result {
                Ok(Record::CashDividend { amount, .. }) => {
                    recorded += money(*amount);
                    prop_assert_eq!(delta_net, money(*amount));
                }
                Ok(Record::Split { cash_in_lieu, .. }) => {
                    recorded += money(*cash_in_lieu);
                    prop_assert_eq!(delta_net, money(*cash_in_lieu));
                }
                Ok(Record::DividendPaid { amount } | Record::CashInLieuPosted { amount }) => {
                    paid += money(*amount);
                    prop_assert_eq!(delta_net, -money(*amount));
                    prop_assert_eq!(delta_settled, money(*amount));
                    prop_assert_eq!(money(a.equity().unwrap()), money(b.equity().unwrap()));
                }
                _ => {
                    prop_assert_eq!(delta_net, 0);
                }
            }
        }
        let last = run.steps.last().map_or(&run.opening, |s| &s.after);
        prop_assert!(last.receivables().is_empty(), "{:?}", last.receivables());
        prop_assert_eq!(recorded, paid);
        prop_assert!(run.oracle.dividends.is_empty() && run.oracle.cash_in_lieu.is_empty());
    }

    /// I6: folding the round-tripped inputs from the same opening gives the same account and the
    /// same result after every event.
    #[test]
    #[ignore = "pending E3-2"]
    fn i6_folding_round_tripped_inputs_is_identical(s in scenario()) {
        let run = run(&s, false)?;
        let config: Config = no_fees();
        let mut account = run.opening.clone();
        for step in &run.steps {
            let result = account.apply(&round_trip(&step.input), &config);
            match result {
                Ok(applied) => {
                    prop_assert_eq!(Ok(&applied.record), step.result.as_ref());
                    account = applied.account;
                }
                Err(e) => prop_assert_eq!(Err(&e), step.result.as_ref()),
            }
            prop_assert_eq!(&account, &step.after);
        }
    }
}
