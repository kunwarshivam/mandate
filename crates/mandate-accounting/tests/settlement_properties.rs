//! Buying power and the no-debit rule as properties (trading-domain spec §7.2, §8.3, §8.4, I4;
//! DEC-99), over random sequences of equity and crypto fills, fee charges, and settlements in
//! cash and margin accounts, with and without a gate in front of the buys.
//!
//! The oracle is a separate ledger on `i128` integers: money in units of 10⁻¹², quantities in
//! 10⁻⁹, prices in cents. It has its own fee arithmetic, its own trade-date and settlement-day
//! counting (a fixed −4 h offset and weekdays, valid for the generated window), its own per-bucket
//! ceilings, and its own buying power, with which the gated generator approves buys. Every
//! reported cash value is compared with it after every event, in the account under test and in a
//! twin of the other type folding the same inputs.

mod common;

use std::collections::BTreeMap;

use common::{d, id, test_default, usd};
use mandate_accounting::{
    Account, AccountType, AccountingError, AssetClass, Config, Execution, FeeFamily, Input,
    Liquidity, Position, Reservations, Side,
};
use mandate_num::{CostBasis, NumError, Price, Qty, SignedQty, Usd};
use mandate_time::{Date, UtcNanos};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;

const MONEY: i128 = 1_000_000_000_000;
const CENT: i128 = MONEY / 100;
const NANO: i128 = 1_000_000_000;
/// 2026-09-21T00:00:00-04:00, a Monday; the generated window stays in daylight-saving time.
const LOCAL_BASE: i64 = 1_789_963_200;
/// 2026-09-21T00:00:00Z.
const UTC_BASE: i64 = 1_789_948_800;
/// 2026-10-12 (a settlement holiday) as a day index from 2026-09-21.
const BANK_HOLIDAY: i64 = 21;
/// The `test_default` rates: SEC 0.00003 of the notional, TAF 0.0002 per share capped at 9.79 per
/// execution, CAT 0.00001 per share, crypto 15 bps maker and 25 bps taker.
const SEC_NUMERATOR: i128 = 3;
const SEC_DENOMINATOR: i128 = 100_000;
const TAF_PER_SHARE: i128 = 2 * MONEY / 10_000;
const TAF_CAP: i128 = 979 * CENT;
const CAT_PER_SHARE: i128 = MONEY / 100_000;
const MAKER_BPS: i128 = 15;
const TAKER_BPS: i128 = 25;
const INSTRUMENTS: [&str; 3] = ["AAA", "BBB", "BTC"];
const CRYPTO: usize = 2;

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

fn money(value: Usd) -> i128 {
    units(&value.to_string(), 12)
}

/// Canonical text for an amount in units of 10⁻¹².
fn money_text(units: i128) -> String {
    let sign = if units < 0 { "-" } else { "" };
    let magnitude = units.abs();
    let int = magnitude / MONEY;
    let frac = format!("{:012}", magnitude % MONEY);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        format!("{sign}{int}")
    } else {
        format!("{sign}{int}.{frac}")
    }
}

/// Canonical text for a quantity in units of 10⁻⁹.
fn qty_text(units: i128) -> String {
    let int = units / NANO;
    let frac = format!("{:09}", units % NANO);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        int.to_string()
    } else {
        format!("{int}.{frac}")
    }
}

fn cents_text(cents: i128) -> String {
    let text = format!("{}.{:02}", cents / 100, cents % 100);
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn price(cents: u32) -> Price {
    Price::parse(&cents_text(i128::from(cents))).unwrap()
}

fn exact_div(n: i128, d: i128) -> i128 {
    assert_eq!(n % d, 0, "oracle scale too small for {n} / {d}");
    n / d
}

fn half_up_magnitude(n: i128, d: i128) -> i128 {
    assert!(n >= 0);
    (2 * n + d) / (2 * d)
}

/// `round(units, 2, ceiling)` for a fee accrual, which is never negative.
fn ceil_cents(units: i128) -> i128 {
    assert!(units >= 0, "a fee accrual is never negative: {units}");
    (units + CENT - 1) / CENT * CENT
}

fn is_weekday(day: i64) -> bool {
    day.rem_euclid(7) < 5
}

/// The equity trade day of a fill at `secs`: the New York date, the next day from 20:00, then
/// the first weekday on or after it (spec §2.2).
fn trade_day(secs: i64) -> i64 {
    let local = secs - LOCAL_BASE;
    let mut day = local.div_euclid(86_400) + i64::from(local.rem_euclid(86_400) >= 20 * 3_600);
    while !is_weekday(day) {
        day += 1;
    }
    day
}

/// T+1 on the settlement calendar (spec §8.4).
fn settle_day(trade: i64) -> i64 {
    let mut day = trade + 1;
    while !is_weekday(day) || day == BANK_HOLIDAY {
        day += 1;
    }
    day
}

fn utc_day(secs: i64) -> i64 {
    (secs - UTC_BASE).div_euclid(86_400)
}

fn day_date(day: i64) -> Date {
    let mut date = d("2026-09-21");
    for _ in 0..day {
        date = date.next().unwrap();
    }
    date
}

#[derive(Debug, Clone)]
enum Event {
    Equity {
        instrument: usize,
        buy: bool,
        shares: u32,
        cents: u32,
    },
    Crypto {
        buy: bool,
        micros: u32,
        cents: u32,
        maker: bool,
    },
    Charge {
        crypto: bool,
        pick: usize,
    },
    Advance,
}

fn event() -> impl Strategy<Value = Event> {
    prop_oneof![
        6 => (0usize..2, any::<bool>(), prop_oneof![9 => 1u32..=500, 1 => 1u32..=100_000], 100u32..=50_000)
            .prop_map(|(instrument, buy, shares, cents)| Event::Equity { instrument, buy, shares, cents }),
        3 => (any::<bool>(), 1u32..=2_000_000, 100_000u32..=9_000_000, any::<bool>())
            .prop_map(|(buy, micros, cents, maker)| Event::Crypto { buy, micros, cents, maker }),
        3 => (any::<bool>(), any::<usize>()).prop_map(|(crypto, pick)| Event::Charge { crypto, pick }),
        2 => Just(Event::Advance),
    ]
}

#[derive(Debug, Clone)]
struct Scenario {
    cash: bool,
    /// Buys go through the oracle's buying-power check first, and sells never exceed the position:
    /// the fills of orders the gate approved (I4). Sells are never held back by buying power
    /// (AGENTS.md rule 13).
    gated: bool,
    /// Whole shares of AAA held at the opening, so that a gated scenario can sell with no cash.
    opening_shares: u32,
    settled_cents: u32,
    reserved_cents: u32,
    events: Vec<(Event, u32)>,
}

fn scenario() -> impl Strategy<Value = Scenario> {
    (
        any::<bool>(),
        any::<bool>(),
        prop_oneof![9 => 0u32..=500, 1 => 0u32..=100_000],
        prop_oneof![1 => Just(0u32), 1 => 0u32..=10_000, 2 => 0u32..=20_000_000],
        0u32..=100_000,
        vec((event(), 0u32..=72_000), 1..40),
    )
        .prop_map(
            |(cash, gated, opening_shares, settled_cents, reserved_cents, events)| Scenario {
                cash,
                gated,
                opening_shares,
                settled_cents,
                reserved_cents,
                events,
            },
        )
}

/// The oracle ledger: cash only, since buying power needs no positions or marks.
#[derive(Debug, Clone, Default)]
struct Oracle {
    settled: i128,
    unsettled: BTreeMap<i64, i128>,
    /// Open fee buckets by (crypto, day).
    accrued: BTreeMap<(bool, i64), i128>,
    /// Signed quantity held, in 10⁻⁹, for the gated generator's sells.
    held: BTreeMap<usize, i128>,
}

impl Oracle {
    fn unsettled_total(&self) -> i128 {
        self.unsettled.values().sum()
    }

    fn accrued_total(&self) -> i128 {
        self.accrued.values().sum()
    }

    /// Σ over open buckets of the charge each will post: the ceiling to a cent for equities and
    /// the accrual itself for crypto, which is already whole cents.
    fn charges_due(&self) -> i128 {
        self.accrued
            .iter()
            .map(|((crypto, _), bucket)| {
                if *crypto {
                    *bucket
                } else {
                    ceil_cents(*bucket)
                }
            })
            .sum()
    }

    fn buying_power(&self, cash: bool, reserved: i128) -> i128 {
        let available = if cash {
            self.settled
        } else {
            self.settled + self.unsettled_total()
        };
        available - reserved - self.charges_due()
    }
}

struct Step {
    input: Input,
    after: Account,
    /// The account of the other type after the same inputs.
    twin: Account,
    oracle: Oracle,
}

/// A scenario's steps and its final state, which is the opening state when no input was applied.
struct Run {
    steps: Vec<Step>,
    account: Account,
    oracle: Oracle,
}

fn account_type(cash: bool) -> AccountType {
    if cash {
        AccountType::Cash
    } else {
        AccountType::Margin
    }
}

fn reservations(cents: u32) -> Reservations {
    Reservations::new(usd(&cents_text(i128::from(cents)))).unwrap()
}

/// Folds an input into the account under test and its twin, failing on any error.
fn fold(
    account: &mut Account,
    twin: &mut Account,
    input: &Input,
    config: &Config,
) -> Result<(), TestCaseError> {
    for target in [account, twin] {
        let applied = target.apply(input, config);
        prop_assert!(applied.is_ok(), "{:?}: {:?}", input, applied);
        *target = applied.unwrap().account;
    }
    Ok(())
}

/// Drives the library and the oracle through a scenario. Every fill's cash effect, fee accrual,
/// trade day, and settlement day are computed by the oracle; the generator asks the oracle, never
/// the library, whether a gated buy fits.
fn run(s: &Scenario) -> Result<Run, TestCaseError> {
    let config = test_default();
    let reserved = i128::from(s.reserved_cents) * CENT;
    let opening = i128::from(s.settled_cents) * CENT;
    let held = i128::from(s.opening_shares) * NANO;
    let position = Position::new(
        SignedQty::parse(&qty_text(held)).unwrap(),
        CostBasis::parse("0").unwrap(),
    )
    .unwrap();
    let positions = [(id(INSTRUMENTS[0]), position)];
    let mut account = Account::opening(
        account_type(s.cash),
        usd(&money_text(opening)),
        positions.clone(),
    );
    let mut twin = Account::opening(account_type(!s.cash), usd(&money_text(opening)), positions);
    let mut oracle = Oracle {
        settled: opening,
        ..Oracle::default()
    };
    oracle.held.insert(0, held);
    let mut steps = Vec::new();
    let mut secs = LOCAL_BASE + 10 * 3_600;
    for (n, (event, gap)) in s.events.iter().enumerate() {
        secs += i64::from(*gap);
        let at = UtcNanos::from_parts(secs, 0).unwrap();
        let mut inputs: Vec<(Input, Oracle)> = Vec::new();
        match event {
            Event::Equity {
                instrument,
                buy,
                shares,
                cents,
            } => {
                let held = oracle.held.get(instrument).copied().unwrap_or(0);
                let requested = i128::from(*shares) * NANO;
                let traded = if !s.gated || *buy {
                    requested
                } else {
                    requested.min(held)
                };
                let p = i128::from(*cents);
                let notional = traded * p * 10;
                let cat = exact_div(traded * CAT_PER_SHARE, NANO);
                let fits = !s.gated
                    || !*buy
                    || oracle.buying_power(s.cash, reserved) >= notional + ceil_cents(cat);
                if traded <= 0 || !fits {
                    continue;
                }
                let day = trade_day(secs);
                let fees = if *buy {
                    oracle.settled -= notional;
                    cat
                } else {
                    *oracle.unsettled.entry(settle_day(day)).or_default() += notional;
                    let sec = exact_div(notional * SEC_NUMERATOR, SEC_DENOMINATOR);
                    let taf = exact_div(traded * TAF_PER_SHARE, NANO).min(TAF_CAP);
                    sec + taf + cat
                };
                *oracle.accrued.entry((false, day)).or_default() += fees;
                *oracle.held.entry(*instrument).or_default() += if *buy { traded } else { -traded };
                let fill = Input::Fill(Execution {
                    fill_id: format!("f{n}"),
                    client_order_id: None,
                    instrument: id(INSTRUMENTS[*instrument]),
                    asset_class: AssetClass::UsEquity,
                    side: if *buy { Side::Buy } else { Side::Sell },
                    qty_gross: Qty::parse(&qty_text(traded)).unwrap(),
                    price: price(*cents),
                    liquidity: None,
                    executed_at: at,
                });
                inputs.push((fill, oracle.clone()));
            }
            Event::Crypto {
                buy,
                micros,
                cents,
                maker,
            } => {
                let held = oracle.held.get(&CRYPTO).copied().unwrap_or(0);
                let requested = i128::from(*micros) * 1_000;
                let traded = if !s.gated || *buy {
                    requested
                } else {
                    requested.min(held)
                };
                let p = i128::from(*cents);
                let notional = traded * p * 10;
                let fits = !s.gated || !*buy || oracle.buying_power(s.cash, reserved) >= notional;
                if traded <= 0 || !fits {
                    continue;
                }
                let bps = if *maker { MAKER_BPS } else { TAKER_BPS };
                if *buy {
                    let fee_qty = half_up_magnitude(traded * bps, 10_000);
                    oracle.settled -= notional;
                    *oracle.held.entry(CRYPTO).or_default() += traded - fee_qty;
                } else {
                    let fee = half_up_magnitude(notional * bps, 10_000 * CENT) * CENT;
                    oracle.settled += notional;
                    *oracle.accrued.entry((true, utc_day(secs))).or_default() += fee;
                    *oracle.held.entry(CRYPTO).or_default() -= traded;
                }
                let fill = Input::Fill(Execution {
                    fill_id: format!("f{n}"),
                    client_order_id: None,
                    instrument: id(INSTRUMENTS[CRYPTO]),
                    asset_class: AssetClass::Crypto,
                    side: if *buy { Side::Buy } else { Side::Sell },
                    qty_gross: Qty::parse(&qty_text(traded)).unwrap(),
                    price: price(*cents),
                    liquidity: Some(if *maker {
                        Liquidity::Maker
                    } else {
                        Liquidity::Taker
                    }),
                    executed_at: at,
                });
                inputs.push((fill, oracle.clone()));
            }
            Event::Charge { crypto, pick } => {
                let days: Vec<i64> = oracle
                    .accrued
                    .keys()
                    .filter(|(c, _)| c == crypto)
                    .map(|(_, day)| *day)
                    .collect();
                let day = if days.is_empty() {
                    if *crypto {
                        utc_day(secs)
                    } else {
                        trade_day(secs)
                    }
                } else {
                    days[pick % days.len()]
                };
                let bucket = oracle.accrued.remove(&(*crypto, day)).unwrap_or(0);
                oracle.settled -= if *crypto { bucket } else { ceil_cents(bucket) };
                let charge = Input::FeesCharged {
                    family: if *crypto {
                        FeeFamily::Crypto
                    } else {
                        FeeFamily::Equities
                    },
                    day: day_date(day),
                };
                inputs.push((charge, oracle.clone()));
            }
            Event::Advance => {
                let due: Vec<i64> = oracle
                    .unsettled
                    .keys()
                    .copied()
                    .filter(|day| LOCAL_BASE + day * 86_400 <= secs)
                    .collect();
                for day in due {
                    oracle.settled += oracle.unsettled.remove(&day).unwrap();
                    inputs.push((
                        Input::SettlementPosted {
                            date: day_date(day),
                        },
                        oracle.clone(),
                    ));
                }
            }
        }
        for (input, oracle) in inputs {
            fold(&mut account, &mut twin, &input, &config)?;
            steps.push(Step {
                input,
                after: account.clone(),
                twin: twin.clone(),
                oracle,
            });
        }
    }
    Ok(Run {
        steps,
        account,
        oracle,
    })
}

fn compare(
    account: &Account,
    oracle: &Oracle,
    cash: bool,
    reserved: u32,
) -> Result<(), TestCaseError> {
    prop_assert_eq!(account.account_type(), account_type(cash));
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
    prop_assert_eq!(
        money(account.buying_power(reservations(reserved)).unwrap()),
        oracle.buying_power(cash, i128::from(reserved) * CENT),
        "buying power"
    );
    Ok(())
}

proptest! {
    /// Settled cash, every unsettled bucket, accrued fees, and buying power with the scenario's
    /// reservations match the oracle after every event, in the account and in its twin of the
    /// other type.
    #[test]
    #[ignore = "pending E3-3"]
    fn buying_power_matches_the_oracle_after_every_event(s in scenario()) {
        for step in run(&s)?.steps {
            compare(&step.after, &step.oracle, s.cash, s.reserved_cents)?;
            compare(&step.twin, &step.oracle, !s.cash, s.reserved_cents)?;
        }
    }

    /// I4 and spec §8.3 for the fills of orders the gate approved (DEC-99, item 5 proposed to the
    /// founder; this asserts the strictest reading the fee model's rounded charges allow). After
    /// every such buy, settled cash is ≥ 0 in a cash account and buying power is ≥ 0 in both: a
    /// buy never spends unsettled proceeds or the cash the pending charges need. After every
    /// event, settled + Σ unsettled − accrued fees ≥ 0 in both account types. In a cash account,
    /// settled cash is ≥ 0 whenever no bucket is unsettled, and while one is, settled cash is
    /// never below minus the charges posted since the last moment nothing was unsettled: the fee
    /// charge on a sale is debited before the proceeds settle, and nothing else can create the
    /// debit. The account type, the charges, and whether anything is unsettled are the scenario's
    /// and the oracle's, never the fold's report of them.
    #[test]
    #[ignore = "pending E3-3"]
    fn i4_the_no_debit_rule_holds_after_every_gate_approved_fill(s in scenario()) {
        let s = Scenario { gated: true, ..s };
        let cash = s.cash;
        let mut previous_oracle_settled = i128::from(s.settled_cents) * CENT;
        let mut charges_while_unsettled = 0;
        for step in run(&s)?.steps {
            let a = &step.after;
            let settled = money(a.settled());
            let total = money(a.cash_total().unwrap());
            let accrued = money(a.fees_accrued().unwrap());
            prop_assert!(total - accrued >= 0, "debit {} after {:?}", total - accrued, step.input);
            if matches!(step.input, Input::FeesCharged { .. }) {
                charges_while_unsettled += previous_oracle_settled - step.oracle.settled;
            }
            if cash {
                if step.oracle.unsettled.is_empty() {
                    prop_assert!(settled >= 0, "settled debit {} with nothing unsettled after {:?}", settled, step.input);
                    charges_while_unsettled = 0;
                } else {
                    prop_assert!(settled >= -charges_while_unsettled, "settled {} below the charges {} posted while unsettled after {:?}", settled, charges_while_unsettled, step.input);
                }
            }
            if let Input::Fill(e) = &step.input && e.side == Side::Buy {
                prop_assert!(!cash || settled >= 0, "a buy spent unsettled proceeds: {} after {:?}", settled, e);
                prop_assert!(money(a.buying_power(reservations(s.reserved_cents)).unwrap()) >= 0, "{:?}", e);
            }
            previous_oracle_settled = step.oracle.settled;
        }
    }

    /// Folding the same inputs, a margin account's buying power exceeds a cash account's by exactly
    /// the unsettled proceeds (spec §7.2, DEC-34): nothing else differs between the types.
    #[test]
    #[ignore = "pending E3-3"]
    fn margin_and_cash_buying_power_differ_by_exactly_the_unsettled_proceeds(s in scenario()) {
        for step in run(&s)?.steps {
            let (cash, margin) = if s.cash { (&step.after, &step.twin) } else { (&step.twin, &step.after) };
            let difference = money(margin.buying_power(Reservations::NONE).unwrap()) - money(cash.buying_power(Reservations::NONE).unwrap());
            prop_assert_eq!(difference, step.oracle.unsettled_total(), "{:?}", step.input);
            prop_assert_eq!(money(cash.settled()), money(margin.settled()));
            prop_assert_eq!(money(cash.fees_accrued().unwrap()), money(margin.fees_accrued().unwrap()));
        }
    }

    /// Reservations come off buying power one for one, whatever the state, and a negative total is
    /// rejected (spec §7.2, §9.5).
    #[test]
    #[ignore = "pending E3-3"]
    fn reservations_reduce_buying_power_one_for_one_and_are_never_negative(
        s in scenario(),
        other in 0u32..=100_000,
        negative in 1u32..=100_000,
    ) {
        let last = run(&s)?.account;
        let with_scenario = money(last.buying_power(reservations(s.reserved_cents)).unwrap());
        let with_other = money(last.buying_power(reservations(other)).unwrap());
        prop_assert_eq!(with_scenario - with_other, (i128::from(other) - i128::from(s.reserved_cents)) * CENT);
        let rejected = Reservations::new(usd(&format!("-{}", cents_text(i128::from(negative)))));
        prop_assert_eq!(rejected, Err(AccountingError::Num(NumError::Negative)));
    }

    /// Charging every open fee bucket leaves settled cash (cash account) or total cash (margin)
    /// exactly at the buying power reported before the charges (DEC-99): buying power subtracts
    /// the sum of the per-bucket ceilings, which is what the charges debit, not the ceiling of the
    /// sum.
    #[test]
    #[ignore = "pending E3-3"]
    fn charging_every_open_bucket_leaves_exactly_the_buying_power_in_cash(s in scenario()) {
        let last = run(&s)?;
        let config = test_default();
        let before = money(last.account.buying_power(Reservations::NONE).unwrap());
        let mut account = last.account;
        for (crypto, day) in last.oracle.accrued.keys() {
            let charge = Input::FeesCharged {
                family: if *crypto { FeeFamily::Crypto } else { FeeFamily::Equities },
                day: day_date(*day),
            };
            let applied = account.apply(&charge, &config);
            prop_assert!(applied.is_ok(), "{:?}: {:?}", charge, applied);
            account = applied.unwrap().account;
        }
        prop_assert_eq!(money(account.fees_accrued().unwrap()), 0);
        let remaining = if s.cash {
            money(account.settled())
        } else {
            money(account.cash_total().unwrap())
        };
        prop_assert_eq!(remaining, before);
    }
}

/// The gated generator reaches the state DEC-99 item 5 describes: a cash account whose settled
/// cash a fee charge took below zero while sale proceeds were unsettled, counted with the oracle
/// over a deterministic run of 4000 scenarios, so the bound in `i4_…` is exercised.
#[test]
fn the_gated_generator_produces_fee_debits_while_proceeds_are_unsettled() {
    let mut runner = TestRunner::deterministic();
    let mut debits = 0;
    for _ in 0..4_000 {
        let s = scenario().new_tree(&mut runner).unwrap().current();
        let s = Scenario {
            gated: true,
            cash: true,
            ..s
        };
        let debit = run(&s).unwrap().steps.iter().any(|step| {
            matches!(step.input, Input::FeesCharged { .. })
                && step.oracle.settled < 0
                && !step.oracle.unsettled.is_empty()
        });
        if debit {
            debits += 1;
        }
    }
    assert!(
        debits >= 40,
        "only {debits} of 4000 gated cash scenarios charged a fee into a debit while proceeds were unsettled"
    );
}
