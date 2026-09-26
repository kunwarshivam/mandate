//! Accounting invariants as properties (trading-domain spec §8.6), over random sequences of fills
//! (partial, averaging, reducing, closing, and crossing zero, on equities and crypto), marks, fee
//! charges, settlement, and duplicate deliveries.
//!
//! The oracle is a separate ledger on `i128` integers: money in units of 10⁻¹², quantities in
//! 10⁻⁹, prices in cents. It applies the spec's formulas with its own floor-based rounding and its
//! own trade-date and settlement arithmetic (a fixed −4 h offset, valid for the generated window,
//! and weekday counting), and every reported value is compared with it after every event.

mod common;

use std::collections::BTreeMap;

use common::{d, equity, fee_cap, id, no_fees, round_trip, usd};
use mandate_accounting::{
    Account, AccountingError, AssetClass, Config, CryptoFees, EquityFees, Execution, FeeFamily,
    FeeKind, Input, Liquidity, Position, Record, Side, TafCapBasis,
};
use mandate_num::{Bps, CostBasis, FeePerShare, FeeRate, Price, Qty, SignedQty};
use mandate_time::{Date, UtcNanos};
use proptest::collection::vec;
use proptest::prelude::*;

const MONEY: i128 = 1_000_000_000_000;
const QTY: i128 = 1_000_000_000;
/// 2026-09-21T00:00:00-04:00, a Monday; the generated window stays in daylight-saving time.
const LOCAL_BASE: i64 = 1_789_963_200;
/// 2026-09-21T00:00:00Z.
const UTC_BASE: i64 = 1_789_948_800;
/// 2026-10-12 (a settlement holiday) as a day index from 2026-09-21.
const BANK_HOLIDAY: i64 = 21;
const SEC_RATE: i128 = 278;
const SEC_RATE_DENOM: i128 = 10_000_000;
const TAF_PER_SHARE: i128 = 166;
const CAT_PER_SHARE: i128 = 35;
const PER_SHARE_DENOM: i128 = 1_000_000;
const MAKER_BPS: i128 = 15;
const TAKER_BPS: i128 = 25;
const INSTRUMENTS: [&str; 3] = ["AAA", "BBB", "BTC"];

fn config(taf_cap_cents: i128, per_order: bool) -> Config {
    Config {
        equities: EquityFees {
            sec_rate: FeeRate::parse("0.0000278").unwrap(),
            taf_per_share: FeePerShare::parse("0.000166").unwrap(),
            taf_cap: fee_cap(&cents_text(taf_cap_cents)),
            taf_cap_basis: if per_order {
                TafCapBasis::PerOrder
            } else {
                TafCapBasis::PerExecution
            },
            cat_per_share: FeePerShare::parse("0.000035").unwrap(),
        },
        crypto: CryptoFees {
            maker: Bps::parse("15").unwrap(),
            taker: Bps::parse("25").unwrap(),
        },
        calendar: common::us_2026(),
    }
}

fn cents_text(cents: i128) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
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

fn floor_round(n: i128, d: i128, half_even: bool) -> i128 {
    let q = n.div_euclid(d);
    let r = n.rem_euclid(d);
    let up = 2 * r > d || (2 * r == d && (!half_even || q % 2 != 0));
    if up { q + 1 } else { q }
}

fn half_up_magnitude(n: i128, d: i128) -> i128 {
    assert!(n >= 0);
    (2 * n + d) / (2 * d)
}

fn exact_div(n: i128, d: i128) -> i128 {
    assert_eq!(n % d, 0, "oracle scale too small for {n} / {d}");
    n / d
}

fn day_date(day: i64) -> Date {
    let mut date = d("2026-09-21");
    for _ in 0..day {
        date = date.next().unwrap();
    }
    date
}

fn is_weekday(day: i64) -> bool {
    day.rem_euclid(7) < 5
}

#[derive(Debug, Clone)]
enum Event {
    Fill {
        instrument: usize,
        buy: bool,
        qty: u32,
        cents: u32,
        maker: bool,
        order: u8,
    },
    Mark {
        instrument: usize,
        cents: u32,
    },
    Charge {
        crypto: bool,
        pick: usize,
    },
    Advance,
    /// Posts one unsettled date directly, skipping any earlier ones.
    SettleThrough {
        pick: usize,
    },
    /// Trades exactly half the position (to the quantity grid), producing basis-reduction ties.
    Halve {
        instrument: usize,
    },
    Duplicate,
}

fn event() -> impl Strategy<Value = Event> {
    prop_oneof![
        8 => (0usize..3, any::<bool>(), 1u32..=300_000, 100u32..=20_000, any::<bool>(), 0u8..3)
            .prop_map(|(instrument, buy, qty, cents, maker, order)| Event::Fill { instrument, buy, qty, cents, maker, order }),
        2 => (0usize..3, 100u32..=20_000).prop_map(|(instrument, cents)| Event::Mark { instrument, cents }),
        2 => (any::<bool>(), any::<usize>()).prop_map(|(crypto, pick)| Event::Charge { crypto, pick }),
        2 => Just(Event::Advance),
        1 => any::<usize>().prop_map(|pick| Event::SettleThrough { pick }),
        2 => (0usize..3).prop_map(|instrument| Event::Halve { instrument }),
        1 => Just(Event::Duplicate),
    ]
}

/// An opening long in AAA: whole shares, a mark in cents, and a basis with an arbitrary tail at
/// 12 places so that halving it can land exactly on a rounding tie.
#[derive(Debug, Clone, Copy)]
struct Opening {
    shares: u32,
    cents: u32,
    tail: i64,
}

#[derive(Debug, Clone)]
struct Scenario {
    opening: Opening,
    events: Vec<(Event, u32)>,
    taf_cap_cents: i128,
    per_order: bool,
}

fn scenario() -> impl Strategy<Value = Scenario> {
    (
        (1u32..=200, 100u32..=20_000, 0i64..1_000_000).prop_map(|(shares, cents, tail)| Opening {
            shares,
            cents,
            tail,
        }),
        vec((event(), 0u32..=72_000), 1..40),
        prop_oneof![Just(1i128), Just(5), Just(979)],
        any::<bool>(),
    )
        .prop_map(|(opening, events, taf_cap_cents, per_order)| Scenario {
            opening,
            events,
            taf_cap_cents,
            per_order,
        })
}

fn opening_units(o: Opening) -> (i128, i128) {
    let shares = i128::from(o.shares);
    (
        shares * QTY,
        shares * i128::from(o.cents) * 10_000_000_000 + i128::from(o.tail),
    )
}

fn opening_account(o: Opening, config: &Config) -> Account {
    let (q, b) = opening_units(o);
    let position = Position::new(
        SignedQty::parse(&qty_text(q)).unwrap(),
        CostBasis::parse(&money_text(b)).unwrap(),
    )
    .unwrap();
    Account::opening(usd("100000"), [(id("AAA"), position)])
        .apply(
            &Input::Mark {
                instrument: id("AAA"),
                price: price(o.cents),
            },
            config,
        )
        .unwrap()
        .account
}

fn money_text(units: i128) -> String {
    let int = units / MONEY;
    let frac = format!("{:012}", units % MONEY);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        int.to_string()
    } else {
        format!("{int}.{frac}")
    }
}

/// The oracle ledger.
#[derive(Debug, Clone, Default)]
struct Oracle {
    positions: BTreeMap<usize, (i128, i128)>,
    marks: BTreeMap<usize, i128>,
    last_fill: BTreeMap<usize, i128>,
    settled: i128,
    unsettled: BTreeMap<i64, i128>,
    accrued: BTreeMap<(bool, i64), i128>,
    charged: i128,
    asset_fees: i128,
    realized: i128,
    taf_by_order: BTreeMap<u8, i128>,
}

impl Oracle {
    fn mark_or(&self, i: usize, default: i128) -> i128 {
        self.marks
            .get(&i)
            .or_else(|| self.last_fill.get(&i))
            .copied()
            .unwrap_or(default)
    }

    fn mark(&self, i: usize) -> i128 {
        *self
            .marks
            .get(&i)
            .or_else(|| self.last_fill.get(&i))
            .unwrap()
    }

    fn market_value(&self) -> i128 {
        self.positions
            .iter()
            .map(|(i, (q, _))| q * self.mark(*i) * 10)
            .sum()
    }

    fn unrealized(&self) -> i128 {
        self.positions
            .iter()
            .map(|(i, (q, b))| q * self.mark(*i) * 10 - b)
            .sum()
    }

    fn accrued_total(&self) -> i128 {
        self.accrued.values().sum()
    }

    fn equity(&self) -> i128 {
        self.settled + self.unsettled.values().sum::<i128>() - self.accrued_total()
            + self.market_value()
    }

    /// Applies signed quantity `q` (10⁻⁹) at `cents` to instrument `i`: §8.1 with crossings split.
    fn trade(&mut self, i: usize, q: i128, cents: i128) {
        let (held, basis) = self.positions.get(&i).copied().unwrap_or((0, 0));
        let (new_q, new_b, realized) = if q == 0 {
            (held, basis, 0)
        } else if held == 0 || (held > 0) == (q > 0) {
            (held + q, basis + q * cents * 10, 0)
        } else if q.abs() < held.abs() {
            let removed = floor_round(basis * q.abs(), held.abs(), true);
            (held + q, basis - removed, -q * cents * 10 - removed)
        } else {
            let closing = -held;
            let realized = -closing * cents * 10 - basis;
            let rest = q + held;
            (rest, rest * cents * 10, realized)
        };
        self.realized += realized;
        if new_q == 0 {
            assert_eq!(new_b, 0);
            self.positions.remove(&i);
        } else {
            self.positions.insert(i, (new_q, new_b));
        }
    }
}

struct Step {
    input: Input,
    /// A second delivery of the previous fill.
    duplicate: bool,
    before: Account,
    after: Account,
    result: Result<Record, AccountingError>,
}

fn qty_text(units: i128) -> String {
    let int = units / QTY;
    let frac = format!("{:09}", units % QTY);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        int.to_string()
    } else {
        format!("{int}.{frac}")
    }
}

fn price(cents: u32) -> Price {
    Price::parse(
        cents_text(i128::from(cents))
            .trim_end_matches('0')
            .trim_end_matches('.'),
    )
    .unwrap()
}

/// The fill that trades half of the position held at this point in the run (to the quantity grid,
/// at least one unit) at its current mark, reducing it; on a flat instrument, a one-unit buy.
fn halved(oracle: &Oracle, instrument: usize) -> Event {
    let held = oracle.positions.get(&instrument).map_or(0, |(q, _)| *q);
    let grid = if instrument == 2 { 1_000 } else { 1_000_000 };
    let half = held.abs() / 2 / grid;
    let cents = oracle.mark_or(instrument, 10_000);
    Event::Fill {
        instrument,
        buy: held < 0,
        qty: u32::try_from(half.max(1)).unwrap_or(u32::MAX),
        cents: u32::try_from(cents).unwrap(),
        maker: false,
        order: 0,
    }
}

/// Drives the library through a scenario; with `check_oracle`, compares every reported value with
/// the oracle after every event. Invariant properties run without it so each stands on its own.
fn run(scenario: &Scenario, check_oracle: bool) -> Result<Vec<Step>, TestCaseError> {
    let Scenario {
        opening,
        events,
        taf_cap_cents,
        per_order,
    } = scenario;
    let (taf_cap_cents, per_order) = (*taf_cap_cents, *per_order);
    let config = config(taf_cap_cents, per_order);
    let mut account = opening_account(*opening, &config);
    let mut oracle = Oracle {
        settled: 100_000 * MONEY,
        ..Oracle::default()
    };
    oracle.positions.insert(0, opening_units(*opening));
    oracle.marks.insert(0, i128::from(opening.cents));
    if check_oracle {
        compare(&account, &oracle)?;
    }
    let mut steps = Vec::new();
    let mut secs = LOCAL_BASE + 13 * 3_600;
    let mut last_fill: Option<Input> = None;
    for (n, (event, gap)) in events.iter().enumerate() {
        secs += i64::from(*gap);
        let at = UtcNanos::from_parts(secs, 0).unwrap();
        let local_secs = secs - LOCAL_BASE;
        let local_day = local_secs.div_euclid(86_400);
        let utc_day = (secs - UTC_BASE).div_euclid(86_400);
        let mut inputs = Vec::new();
        let event = match event {
            Event::Halve { instrument } => halved(&oracle, *instrument),
            other => other.clone(),
        };
        match &event {
            Event::Fill {
                instrument,
                buy,
                qty,
                cents,
                maker,
                order,
            } => {
                let crypto = *instrument == 2;
                let q_units = if crypto {
                    i128::from(*qty) * 1_000
                } else {
                    i128::from(*qty) * 1_000_000
                };
                let p = i128::from(*cents);
                let notional = q_units * p * 10;
                let side = if *buy { Side::Buy } else { Side::Sell };
                let input = Input::Fill(Execution {
                    fill_id: format!("f{n}"),
                    client_order_id: (*order > 0).then(|| format!("o{order}")),
                    instrument: id(INSTRUMENTS[*instrument]),
                    asset_class: if crypto {
                        AssetClass::Crypto
                    } else {
                        AssetClass::UsEquity
                    },
                    side,
                    qty_gross: Qty::parse(&qty_text(q_units)).unwrap(),
                    price: price(*cents),
                    liquidity: if crypto {
                        Some(if *maker {
                            Liquidity::Maker
                        } else {
                            Liquidity::Taker
                        })
                    } else {
                        None
                    },
                    executed_at: at,
                });
                if crypto {
                    let bps = if *maker { MAKER_BPS } else { TAKER_BPS };
                    if *buy {
                        let fee_qty = half_up_magnitude(q_units * bps, 10_000);
                        oracle.asset_fees += fee_qty * p * 10;
                        oracle.settled -= notional;
                        oracle.trade(*instrument, q_units - fee_qty, p);
                    } else {
                        let fee = half_up_magnitude(notional * bps, 10_000 * 10_000_000_000)
                            * 10_000_000_000;
                        oracle.settled += notional;
                        *oracle.accrued.entry((true, utc_day)).or_default() += fee;
                        oracle.trade(*instrument, -q_units, p);
                    }
                } else {
                    let mut trade_day =
                        local_day + i64::from(local_secs.rem_euclid(86_400) >= 20 * 3_600);
                    while !is_weekday(trade_day) {
                        trade_day += 1;
                    }
                    let cat = exact_div(q_units * CAT_PER_SHARE * 1_000, PER_SHARE_DENOM);
                    let fees = if *buy {
                        oracle.settled -= notional;
                        oracle.trade(*instrument, q_units, p);
                        cat
                    } else {
                        let mut settle_day = trade_day + 1;
                        while !is_weekday(settle_day) || settle_day == BANK_HOLIDAY {
                            settle_day += 1;
                        }
                        *oracle.unsettled.entry(settle_day).or_default() += notional;
                        oracle.trade(*instrument, -q_units, p);
                        let sec = exact_div(notional * SEC_RATE, SEC_RATE_DENOM);
                        let uncapped = exact_div(q_units * TAF_PER_SHARE * 1_000, PER_SHARE_DENOM);
                        let cap = taf_cap_cents * 10_000_000_000;
                        let taf = if per_order && *order > 0 {
                            let so_far = oracle.taf_by_order.entry(*order).or_default();
                            let taf = uncapped.min((cap - *so_far).max(0));
                            *so_far += taf;
                            taf
                        } else {
                            uncapped.min(cap)
                        };
                        sec + taf + cat
                    };
                    *oracle.accrued.entry((false, trade_day)).or_default() += fees;
                }
                oracle.last_fill.insert(*instrument, p);
                last_fill = Some(input.clone());
                inputs.push(input);
            }
            Event::Mark { instrument, cents } => {
                oracle.marks.insert(*instrument, i128::from(*cents));
                inputs.push(Input::Mark {
                    instrument: id(INSTRUMENTS[*instrument]),
                    price: price(*cents),
                });
            }
            Event::Charge { crypto, pick } => {
                let days: Vec<i64> = oracle
                    .accrued
                    .keys()
                    .filter(|(c, _)| c == crypto)
                    .map(|(_, day)| *day)
                    .collect();
                let day = if days.is_empty() {
                    if *crypto { utc_day } else { local_day }
                } else {
                    days[pick % days.len()]
                };
                let accrued = oracle.accrued.remove(&(*crypto, day)).unwrap_or(0);
                let charge = if *crypto {
                    accrued
                } else {
                    accrued.div_euclid(10_000_000_000) * 10_000_000_000
                        + if accrued % 10_000_000_000 > 0 {
                            10_000_000_000
                        } else {
                            0
                        }
                };
                oracle.settled -= charge;
                oracle.charged += charge;
                let family = if *crypto {
                    FeeFamily::Crypto
                } else {
                    FeeFamily::Equities
                };
                inputs.push(Input::FeesCharged {
                    family,
                    day: day_date(day),
                });
            }
            Event::Advance => {
                let due: Vec<i64> = oracle
                    .unsettled
                    .keys()
                    .copied()
                    .filter(|day| LOCAL_BASE + day * 86_400 <= secs)
                    .collect();
                let reported = account.settlements_due(at).unwrap();
                if check_oracle {
                    prop_assert_eq!(
                        &reported,
                        &due.iter().map(|day| day_date(*day)).collect::<Vec<_>>()
                    );
                }
                for day in due {
                    oracle.settled += oracle.unsettled.remove(&day).unwrap();
                }
                inputs.extend(
                    reported
                        .into_iter()
                        .map(|date| Input::SettlementPosted { date }),
                );
            }
            Event::SettleThrough { pick } => {
                let days: Vec<i64> = oracle.unsettled.keys().copied().collect();
                if !days.is_empty() {
                    let through = days[pick % days.len()];
                    for day in days.into_iter().filter(|day| *day <= through) {
                        oracle.settled += oracle.unsettled.remove(&day).unwrap();
                    }
                    inputs.push(Input::SettlementPosted {
                        date: day_date(through),
                    });
                }
            }
            Event::Halve { .. } => unreachable!("resolved against the oracle before the match"),
            Event::Duplicate => {
                if let Some(input) = &last_fill {
                    inputs.push(input.clone());
                }
            }
        }
        for input in inputs {
            let result = account.apply(&input, &config);
            let before = account.clone();
            let record = match result {
                Ok(applied) => {
                    account = applied.account;
                    Ok(applied.record)
                }
                Err(e) => Err(e),
            };
            let duplicate = matches!(event, Event::Duplicate);
            if !duplicate {
                prop_assert!(record.is_ok(), "{:?}: {:?}", input, record);
            }
            steps.push(Step {
                input,
                duplicate,
                before,
                after: account.clone(),
                result: record,
            });
        }
        if check_oracle {
            compare(&account, &oracle)?;
        }
    }
    Ok(steps)
}

fn compare(account: &Account, oracle: &Oracle) -> Result<(), TestCaseError> {
    for (i, name) in INSTRUMENTS.iter().enumerate() {
        let p = account.position(&id(name));
        let (q, b) = oracle.positions.get(&i).copied().unwrap_or((0, 0));
        prop_assert_eq!(units(&p.qty().to_string(), 9), q, "{} qty", name);
        prop_assert_eq!(units(&p.basis().to_string(), 12), b, "{} basis", name);
        if q != 0 {
            prop_assert_eq!(
                account.mark(&id(name)).map(|m| units(&m.to_string(), 2)),
                Some(oracle.mark(i))
            );
        }
    }
    prop_assert_eq!(money(account.settled()), oracle.settled, "settled");
    let unsettled: Vec<(Date, i128)> = account.unsettled().map(|(d, a)| (d, money(a))).collect();
    let expected: Vec<(Date, i128)> = oracle
        .unsettled
        .iter()
        .map(|(day, a)| (day_date(*day), *a))
        .collect();
    prop_assert_eq!(unsettled, expected, "unsettled");
    prop_assert_eq!(
        money(account.fees_accrued().unwrap()),
        oracle.accrued_total(),
        "accrued"
    );
    prop_assert_eq!(money(account.fees_charged()), oracle.charged, "charged");
    prop_assert_eq!(money(account.asset_fees()), oracle.asset_fees, "asset fees");
    prop_assert_eq!(money(account.realized_gross()), oracle.realized, "realized");
    prop_assert_eq!(
        money(account.unrealized().unwrap()),
        oracle.unrealized(),
        "unrealized"
    );
    prop_assert_eq!(
        money(account.market_value().unwrap()),
        oracle.market_value(),
        "market value"
    );
    prop_assert_eq!(money(account.equity().unwrap()), oracle.equity(), "equity");
    Ok(())
}

fn money(v: mandate_num::Usd) -> i128 {
    units(&v.to_string(), 12)
}

proptest! {
    #[test]
    fn every_reported_value_matches_the_oracle_after_every_event(s in scenario()) {
        run(&s, true)?;
    }

    #[test]
    fn i1_conservation_holds_for_every_event(s in scenario()) {
        for step in run(&s, false)? {
            let (b, a) = (&step.before, &step.after);
            let delta_equity = money(a.equity().unwrap()) - money(b.equity().unwrap());
            let delta_realized = money(a.realized_gross()) - money(b.realized_gross());
            let delta_unrealized = money(a.unrealized().unwrap()) - money(b.unrealized().unwrap());
            let delta_fees = money(a.fees_total().unwrap()) - money(b.fees_total().unwrap());
            prop_assert_eq!(delta_equity, delta_realized + delta_unrealized - delta_fees, "{:?}", step.input);
        }
    }

    #[test]
    fn i2_reducing_fills_remove_one_rounding_of_the_proportional_basis(s in scenario()) {
        for step in run(&s, false)? {
            let (Input::Fill(e), Ok(Record::Fill { received, .. })) = (&step.input, &step.result) else { continue };
            let before = step.before.position(&e.instrument);
            let after = step.after.position(&e.instrument);
            let (held, q) = (units(&before.qty().to_string(), 9), units(&received.to_string(), 9));
            if held != 0 && q != 0 && (held > 0) != (q > 0) && q.abs() < held.abs() {
                let basis = units(&before.basis().to_string(), 12);
                let removed = basis - units(&after.basis().to_string(), 12);
                prop_assert_eq!(removed, floor_round(basis * q.abs(), held.abs(), true));
            }
        }
    }

    #[test]
    fn i4_cash_totals_and_settlement_leave_no_due_bucket(s in scenario()) {
        for step in run(&s, false)? {
            let a = &step.after;
            let buckets: i128 = a.unsettled().map(|(_, amount)| money(amount)).sum();
            prop_assert_eq!(money(a.cash_total().unwrap()), money(a.settled()) + buckets);
            prop_assert!(a.unsettled().all(|(_, amount)| money(amount) > 0));
            if let Input::SettlementPosted { date } = step.input {
                prop_assert!(a.unsettled().all(|(d, _)| d > date));
                prop_assert_eq!(money(a.cash_total().unwrap()), money(step.before.cash_total().unwrap()));
            }
        }
    }

    #[test]
    fn i5_quantity_is_the_fold_of_signed_received_quantities(s in scenario()) {
        let mut folded: BTreeMap<String, i128> = BTreeMap::from([("AAA".to_owned(), opening_units(s.opening).0)]);
        for step in run(&s, false)? {
            if let (Input::Fill(e), Ok(Record::Fill { received, fees, .. })) = (&step.input, &step.result) {
                let withheld: i128 = fees.iter().filter_map(|f| f.asset_qty).map(|q| units(&q.to_string(), 9)).sum();
                let gross = units(&e.qty_gross.to_string(), 9);
                let expected = match e.side { Side::Buy => gross - withheld, Side::Sell => -gross };
                prop_assert_eq!(units(&received.to_string(), 9), expected);
                *folded.entry(e.instrument.as_str().to_owned()).or_default() += expected;
            }
            for name in INSTRUMENTS {
                let q = units(&step.after.position(&id(name)).qty().to_string(), 9);
                prop_assert_eq!(q, folded.get(name).copied().unwrap_or(0), "{}", name);
            }
        }
    }

    #[test]
    fn i6_folding_is_deterministic_across_a_text_round_trip(s in scenario()) {
        let steps = run(&s, false)?;
        let config = config(s.taf_cap_cents, s.per_order);
        let mut replayed = opening_account(s.opening, &config);
        for step in &steps {
            let result = replayed.apply(&round_trip(&step.input), &config);
            match (&step.result, result) {
                (Ok(record), Ok(applied)) => {
                    prop_assert_eq!(record, &applied.record);
                    replayed = applied.account;
                }
                (Err(expected), Err(actual)) => prop_assert_eq!(expected, &actual),
                (expected, actual) => prop_assert!(false, "{:?} vs {:?}", expected, actual),
            }
            prop_assert_eq!(&replayed, &step.after);
        }
    }

    #[test]
    fn duplicate_fills_are_never_applied_twice(s in scenario()) {
        for step in run(&s, false)? {
            if step.duplicate {
                prop_assert!(matches!(step.result, Err(AccountingError::DuplicateFill(_))), "{:?}", step.result);
                prop_assert_eq!(&step.before, &step.after);
            }
        }
    }

    #[test]
    fn asset_fees_are_never_accrued_and_charges_round_up_by_less_than_a_cent(s in scenario()) {
        for step in run(&s, false)? {
            let (b, a) = (&step.before, &step.after);
            match (&step.input, &step.result) {
                (Input::Fill(e), Ok(_)) if e.asset_class == AssetClass::Crypto && e.side == Side::Buy => {
                    prop_assert_eq!(a.fees_accrued().unwrap(), b.fees_accrued().unwrap());
                }
                (Input::FeesCharged { family, .. }, Ok(Record::FeesCharged { accrued, charged })) => {
                    let (accrued, charged) = (money(*accrued), money(*charged));
                    prop_assert!(charged >= accrued && charged - accrued < MONEY / 100);
                    prop_assert_eq!(charged % (MONEY / 100), 0);
                    if *family == FeeFamily::Crypto {
                        prop_assert_eq!(charged, accrued);
                    }
                    prop_assert_eq!(money(b.fees_total().unwrap()) + charged - accrued, money(a.fees_total().unwrap()));
                }
                _ => {}
            }
        }
    }
}

/// Reads money text in units of 10⁻¹⁸, the exact scale of a 10⁻⁹ quantity times a 10⁻⁹ price.
fn atto<T: ToString>(value: T) -> i128 {
    units(&value.to_string(), 18)
}

proptest! {
    /// Through partial reductions of any size and price, including bases below the 12-place grid
    /// of the removed basis, a position's basis stays on its side of zero and never moves past
    /// what it held; closing the rest realizes exactly the cash received less the cash paid over
    /// the round trip (DEC-86). Quantities and prices are in 10⁻⁹ and money in 10⁻¹⁸, so the
    /// oracle's sums are exact.
    #[test]
    fn reductions_keep_the_basis_on_the_position_side_and_a_close_realizes_cash_flow(
        short in any::<bool>(),
        opened in 2i128..=1_000,
        opening_price in 1i128..=100_000,
        reductions in vec((1i128..=1_000, 1i128..=100_000), 1..8),
    ) {
        let config = no_fees();
        let (open, close) = if short { (Side::Sell, Side::Buy) } else { (Side::Buy, Side::Sell) };
        let fill = |n: usize, side, qty: i128, nanos: i128| {
            equity(&format!("f{n}"), "AAA", side, &qty_text(qty), &qty_text(nanos), "2026-09-21T10:00:00-04:00")
        };
        let signed = |magnitude: i128| if short { -magnitude } else { magnitude };
        let mut account = common::step(&Account::opening(usd("1"), []), &fill(0, open, opened, opening_price), &config);
        let mut held = opened;
        let mut cash = signed(-opened * opening_price);
        for (n, (qty, nanos)) in reductions.into_iter().enumerate() {
            let traded = qty % held;
            if traded == 0 {
                continue;
            }
            let before = atto(account.position(&id("AAA")).basis());
            account = common::step(&account, &fill(n + 1, close, traded, nanos), &config);
            held -= traded;
            cash += signed(traded * nanos);
            let after = account.position(&id("AAA"));
            prop_assert_eq!(units(&after.qty().to_string(), 9), signed(held));
            let basis = atto(after.basis());
            prop_assert!(signed(basis) >= 0 && signed(basis) <= signed(before), "basis {} after {}", basis, before);
        }
        account = common::step(&account, &fill(99, close, held, opening_price), &config);
        cash += signed(held * opening_price);
        let p = account.position(&id("AAA"));
        prop_assert_eq!((p.qty().to_string(), p.basis().to_string()), ("0".to_owned(), "0".to_owned()));
        prop_assert_eq!(atto(account.realized_gross()), cash);
    }
}

/// Canonical decimal text for a signed amount in units of 10⁻¹⁸.
fn atto_text(value: i128) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let magnitude = value.abs();
    let int = magnitude / 1_000_000_000_000_000_000;
    let frac = format!("{:018}", magnitude % 1_000_000_000_000_000_000);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        format!("{sign}{int}")
    } else {
        format!("{sign}{int}.{frac}")
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    /// DEC-86 stated exactly. Let U = B − round(B × part ÷ whole, 12, half_even), the reduced basis
    /// without the limit. The reduced basis is U whenever U is on the position's side of zero or
    /// zero. Otherwise the rounded removal exceeds the basis held, which needs B to have digits
    /// below the 12th place, and the reduced basis is 0. Realized P&L is the proceeds less the
    /// basis actually removed. The oracle is exact: basis in 10⁻¹⁸ (so removals are multiples of
    /// 10⁶), quantities and prices in 10⁻⁹. Half the bases are below 2 × 10⁻¹², where the limit
    /// can bind, and a sixth are on the 12-place grid, where it never does.
    #[test]
    fn a_reduction_matches_the_rounded_formula_unless_the_removal_exceeds_the_basis_held(
        short in any::<bool>(),
        whole in 2i128..=1_000,
        part_pick in any::<u32>(),
        basis_atto in prop_oneof![
            3 => 0i128..=2_000_000,
            1 => 0i128..=100_000_000,
            1 => 0i128..=10_000_000_000_000_000_000_000,
            1 => (0i128..=10_000_000_000).prop_map(|grid| grid * 1_000_000),
        ],
        nanos in 1i128..=1_000_000,
    ) {
        let part = 1 + i128::from(part_pick) % (whole - 1);
        let signed = |magnitude: i128| if short { -magnitude } else { magnitude };
        let basis = signed(basis_atto);
        let removal = floor_round(basis * part, whole * 1_000_000, true) * 1_000_000;
        let uncapped = basis - removal;
        let expected = if signed(uncapped) >= 0 { uncapped } else { 0 };
        if expected != uncapped {
            prop_assert_ne!(basis_atto % 1_000_000, 0, "the limit bound on a basis on the 12-place grid");
        }
        let qty = |magnitude: i128| if short { format!("-{}", qty_text(magnitude)) } else { qty_text(magnitude) };
        let held = Position::new(
            SignedQty::parse(&qty(whole)).unwrap(),
            CostBasis::parse(&atto_text(basis)).unwrap(),
        )
        .unwrap();
        let side = if short { Side::Buy } else { Side::Sell };
        let fill = equity("f1", "AAA", side, &qty_text(part), &qty_text(nanos), "2026-09-21T10:00:00-04:00");
        let applied = Account::opening(usd("0"), [(id("AAA"), held)]).apply(&fill, &no_fees());
        prop_assert!(applied.is_ok(), "{:?}", applied);
        let account = applied.unwrap().account;
        let after = account.position(&id("AAA"));
        prop_assert_eq!(after.qty().to_string(), qty(whole - part));
        prop_assert_eq!(atto(after.basis()), expected, "B {} part {} whole {}", basis, part, whole);
        prop_assert_eq!(atto(account.realized_gross()), signed(part * nanos) - (basis - expected));
    }
}

/// One input of the fee-configuration property. Each fill carries the fee configuration in force
/// for it (spec §6.1): a TAF cap in units of 10⁻⁴ USD and the cap mode.
#[derive(Debug, Clone)]
enum FeeEvent {
    Equity {
        buy: bool,
        shares: u32,
        cents: u32,
        order: u8,
        day: i64,
        cap: i128,
        per_order: bool,
    },
    Crypto {
        buy: bool,
        micros: u32,
        cents: u32,
        maker: bool,
        day: i64,
    },
    Charge {
        crypto: bool,
        day: i64,
    },
}

fn fee_event() -> impl Strategy<Value = FeeEvent> {
    let cap = prop_oneof![2 => Just(0i128), 6 => 0i128..=300, 1 => Just(97_900i128)];
    prop_oneof![
        6 => (any::<bool>(), 1u32..=300, 1u32..=20_000, 0u8..3, 0i64..2, cap, any::<bool>())
            .prop_map(|(buy, shares, cents, order, day, cap, per_order)| FeeEvent::Equity { buy, shares, cents, order, day, cap, per_order }),
        2 => (any::<bool>(), 1u32..=2_000_000, 100u32..=6_000_000, any::<bool>(), 0i64..2)
            .prop_map(|(buy, micros, cents, maker, day)| FeeEvent::Crypto { buy, micros, cents, maker, day }),
        2 => (any::<bool>(), 0i64..2).prop_map(|(crypto, day)| FeeEvent::Charge { crypto, day }),
    ]
}

/// Canonical text for an amount in units of 10⁻⁴.
fn ten_thousandths(units: i128) -> String {
    let text = format!("{}.{:04}", units / 10_000, units % 10_000);
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn fee_config(cap: i128, per_order: bool) -> Config {
    let mut config = config(1, per_order);
    config.equities.taf_cap = fee_cap(&ten_thousandths(cap));
    config
}

proptest! {
    /// Fees are never credits, whatever fee configuration each fill carries (DEC-87). Fills and
    /// charges run with caps that rise, fall, and reach zero within an order, and with the cap mode
    /// switching between fills. After every input: every fee on a fill is ≥ 0; a fill's TAF matches
    /// the oracle, min(rate × shares, room) with room = max(0, cap − TAF already charged on the
    /// order) per order and the cap otherwise; accrued fees never decrease except by a charge, and
    /// a fill raises them by exactly its USD fees other than the crypto asset fee; a charge is at
    /// least the accrual it replaces, never negative, and debits settled cash; total fees never
    /// decrease; and the TAF the fold charged on an order whose sells were all charged per order
    /// never exceeds the highest cap in force at any of them. The oracle is exact: TAF in units of
    /// 10⁻¹².
    #[test]
    fn fees_are_never_credits_under_per_fill_fee_configurations(events in vec(fee_event(), 1..40)) {
        let mut account = Account::opening(usd("100000"), []);
        let mut charged_by_order: BTreeMap<u8, i128> = BTreeMap::new();
        let mut observed: BTreeMap<u8, (i128, i128, bool)> = BTreeMap::new();
        for (n, event) in events.iter().enumerate() {
            let at = |day: i64| format!("2026-09-2{}T11:{:02}:{:02}-04:00", 1 + day, n / 60, n % 60);
            let (input, config, expected_taf) = match event {
                FeeEvent::Equity { buy, shares, cents, order, day, cap, per_order } => {
                    let fill = common::Fill {
                        fill_id: &format!("f{n}"),
                        order: (*order > 0).then(|| format!("o{order}")).as_deref(),
                        instrument: "AAA",
                        asset_class: AssetClass::UsEquity,
                        side: if *buy { Side::Buy } else { Side::Sell },
                        qty: &shares.to_string(),
                        price: &price(*cents).to_string(),
                        liquidity: None,
                        at: &at(*day),
                    }
                    .input();
                    let taf = if *buy {
                        None
                    } else {
                        let uncapped = i128::from(*shares) * TAF_PER_SHARE * 1_000_000;
                        let cap_units = cap * 100_000_000;
                        let before = if *order > 0 { charged_by_order.get(order).copied().unwrap_or(0) } else { 0 };
                        let room = if *per_order && *order > 0 { (cap_units - before).max(0) } else { cap_units };
                        let taf = uncapped.min(room);
                        if *order > 0 {
                            charged_by_order.insert(*order, before + taf);
                        }
                        Some(taf)
                    };
                    (fill, fee_config(*cap, *per_order), taf)
                }
                FeeEvent::Crypto { buy, micros, cents, maker, day } => {
                    let fill = common::Fill {
                        fill_id: &format!("f{n}"),
                        order: None,
                        instrument: "BTC",
                        asset_class: AssetClass::Crypto,
                        side: if *buy { Side::Buy } else { Side::Sell },
                        qty: &qty_text(i128::from(*micros) * 1_000),
                        price: &price(*cents).to_string(),
                        liquidity: Some(if *maker { Liquidity::Maker } else { Liquidity::Taker }),
                        at: &at(*day),
                    }
                    .input();
                    (fill, fee_config(0, false), None)
                }
                FeeEvent::Charge { crypto, day } => (
                    Input::FeesCharged {
                        family: if *crypto { FeeFamily::Crypto } else { FeeFamily::Equities },
                        day: day_date(*day),
                    },
                    fee_config(0, false),
                    None,
                ),
            };
            let applied = account.apply(&input, &config);
            prop_assert!(applied.is_ok(), "{:?}: {:?}", input, applied);
            let applied = applied.unwrap();
            let (before, after) = (&account, &applied.account);
            let accrued_delta = money(after.fees_accrued().unwrap()) - money(before.fees_accrued().unwrap());
            match &applied.record {
                Record::Fill { fees, .. } => {
                    for fee in fees {
                        prop_assert!(money(fee.usd) >= 0, "{:?} in {:?}", fee, input);
                    }
                    let taf: Vec<i128> = fees.iter().filter(|f| f.kind == FeeKind::Taf).map(|f| money(f.usd)).collect();
                    if let FeeEvent::Equity { buy: false, order: order @ 1.., cap, per_order, .. } = event {
                        let seen = observed.entry(*order).or_insert((0, 0, true));
                        seen.0 += taf.iter().sum::<i128>();
                        seen.1 = seen.1.max(cap * 100_000_000);
                        seen.2 &= *per_order;
                        if seen.2 {
                            prop_assert!(seen.0 <= seen.1, "order o{} TAF {} above every cap in force {}", order, seen.0, seen.1);
                        }
                    }
                    prop_assert_eq!(taf, expected_taf.into_iter().collect::<Vec<_>>(), "{:?}", input);
                    let accrued: i128 = fees.iter().filter(|f| f.kind != FeeKind::CryptoAsset).map(|f| money(f.usd)).sum();
                    prop_assert_eq!(accrued_delta, accrued);
                    prop_assert!(accrued_delta >= 0);
                }
                Record::FeesCharged { accrued, charged } => {
                    prop_assert!(money(*accrued) >= 0 && money(*charged) >= money(*accrued), "{:?}", applied.record);
                    prop_assert_eq!(accrued_delta, -money(*accrued));
                    prop_assert_eq!(money(after.settled()), money(before.settled()) - money(*charged));
                }
                other => prop_assert!(false, "{:?}", other),
            }
            prop_assert!(money(after.fees_total().unwrap()) >= money(before.fees_total().unwrap()));
            account = applied.account;
        }
    }
}
