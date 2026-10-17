//! The live Robinhood fee schedule (the R0 rehearsal's R10, DEC-958): Robinhood's posted US equity
//! fees, transcribed in `config/fees/robinhood-live-2026-10-16.toml`, priced by
//! `robinhood_live_fee_config` for a `live` stream on a `robinhood` connection only. Each expected
//! fee is worked out by hand in its doc comment from §6.2's formulas and the file's rates, or by
//! an integer oracle of this file's own, never by running the code under test. Robinhood rounds
//! each fee of a trade separately, so the schedule rounds each fee of each fill up to the cent
//! (`per_fee_per_fill`, DEC-960, the R0 rehearsal's R10b).

use mandate_accounting::{
    Account, AccountType, AssetClass, Config, Execution, FeeFamily, FeeKind, FeeRounding, Input,
    InstrumentId, Position, Record, Side, TafCapBasis,
};
use mandate_executor::{
    ExecutorError, ROBINHOOD_LIVE_SCHEDULE, paper_only_fee_config, robinhood_live_fee_config,
};
use mandate_num::{Bps, CostBasis, FeeCap, FeePerShare, FeeRate, Price, Qty, SignedQty, Usd};
use mandate_time::{Date, TradingCalendar, UtcNanos};
use proptest::prelude::*;

/// The first trading day on which every transcribed figure is in force (SEC's rate from
/// 2026-04-04, a Saturday), as the registered artifact names it.
const DAY: &str = "2026-04-06";
/// The trade date the fills below execute on.
const TRADE_DAY: &str = "2026-10-16";

fn calendar() -> TradingCalendar {
    TradingCalendar::new(
        Date::parse("2026-01-01").unwrap(),
        Date::parse("2026-12-31").unwrap(),
        [],
        [],
    )
    .unwrap()
}

fn live() -> Config {
    robinhood_live_fee_config("live", "robinhood", calendar(), DAY).unwrap()
}

fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap()
}

/// The dated file's figures as `(key, value)` pairs: the one copy of the transcription.
fn transcribed() -> Vec<(&'static str, &'static str)> {
    include_str!("../../../config/fees/robinhood-live-2026-10-16.toml")
        .lines()
        .filter_map(|line| {
            let (key, rest) = line.split_once(" = \"")?;
            let (value, _) = rest.split_once('"')?;
            Some((key.trim(), value))
        })
        .collect()
}

fn figure(key: &str) -> &'static str {
    transcribed()
        .into_iter()
        .find(|(named, value)| *named == key && !value.is_empty())
        .map(|(_, value)| value)
        .unwrap_or_else(|| panic!("the transcription names no {key}"))
}

/// One fill of `qty` shares at `price` on [`TRADE_DAY`], and the account it leaves after the day's
/// equity fees are charged: the fill's own fee lines and the charged total.
fn booked(side: Side, qty: &str, price: &str) -> (Vec<(FeeKind, Usd)>, Usd) {
    let config = live();
    let held = match side {
        Side::Sell => vec![(
            InstrumentId::new("RHETF").unwrap(),
            Position::new(
                SignedQty::parse(qty).unwrap(),
                CostBasis::parse("1").unwrap(),
            )
            .unwrap(),
        )],
        Side::Buy => Vec::new(),
    };
    let start = Account::opening(AccountType::Cash, usd("10000000"), held);
    let applied = start.apply(&fill("f", side, qty, price), &config).unwrap();
    let Record::Fill { fees, .. } = applied.record else {
        panic!("a fill books a fill record");
    };
    let lines = fees.into_iter().map(|fee| (fee.kind, fee.usd)).collect();
    let charged = applied
        .account
        .apply(
            &Input::FeesCharged {
                family: FeeFamily::Equities,
                day: Date::parse(TRADE_DAY).unwrap(),
            },
            &config,
        )
        .unwrap()
        .account
        .fees_charged();
    (lines, charged)
}

fn fill(fill_id: &str, side: Side, qty: &str, price: &str) -> Input {
    Input::Fill(Execution {
        fill_id: fill_id.to_owned(),
        client_order_id: Some(format!("order-{fill_id}")),
        instrument: InstrumentId::new("RHETF").unwrap(),
        asset_class: AssetClass::UsEquity,
        side,
        qty_gross: Qty::parse(qty).unwrap(),
        price: Price::parse(price).unwrap(),
        liquidity: None,
        executed_at: UtcNanos::parse_rfc3339("2026-10-16T11:00:00-04:00").unwrap(),
    })
}

/// The schedule's figures are the dated file's, figure for figure, so the reviewed source and the
/// rates a live run charges cannot drift apart; and the file is the one the schedule id names.
#[test]
fn the_live_schedule_is_the_transcription_figure_for_figure() {
    let config = live();
    assert_eq!(figure("schedule"), ROBINHOOD_LIVE_SCHEDULE);
    assert_eq!(figure("environment"), "live");
    assert_eq!(figure("broker"), "robinhood");
    assert_eq!(
        config.equities.sec_rate,
        FeeRate::parse(figure("sec_rate")).unwrap()
    );
    assert_eq!(
        config.equities.taf_per_share,
        FeePerShare::parse(figure("taf_per_share")).unwrap()
    );
    assert_eq!(
        config.equities.taf_cap,
        FeeCap::parse(figure("taf_cap")).unwrap()
    );
    assert_eq!(figure("taf_cap_basis"), "per_execution");
    assert_eq!(config.equities.taf_cap_basis, TafCapBasis::PerExecution);
    assert_eq!(
        config.equities.cat_per_share,
        FeePerShare::parse(figure("cat_per_share")).unwrap()
    );
    assert_eq!(
        config.crypto.maker,
        Bps::parse(figure("maker_bps")).unwrap()
    );
    assert_eq!(
        config.crypto.taker,
        Bps::parse(figure("taker_bps")).unwrap()
    );
    assert_eq!(config.calendar, calendar());
}

/// The file quotes Robinhood rounding each fee separately (SEC "rounded up to the nearest penny",
/// CAT "rounded to the nearest penny"), so the schedule rounds each fee of each fill up to the cent
/// (DEC-960), and never the day's total alone.
#[test]
fn the_live_schedule_rounds_each_fee_as_robinhood_quotes() {
    let file = include_str!("../../../config/fees/robinhood-live-2026-10-16.toml");
    assert!(file.contains("sale, rounded up to the\n# nearest penny"));
    assert!(file.contains("buys and sells (no side is named), \"rounded to the\n# nearest penny"));
    assert_eq!(live().equities.fee_rounding, FeeRounding::PerFeePerFill);
    assert_eq!(
        paper_only_fee_config("paper", calendar(), DAY)
            .unwrap()
            .equities
            .fee_rounding,
        FeeRounding::DailyTotal
    );
}

/// The published rates, read from their sources (DEC-958): SEC $20.60 per million is 0.0000206,
/// TAF $0.000195 a share capped at $9.79, CAT $0.000003 a share. Each configured figure is exactly
/// that, never rounded down to the types' precision.
#[test]
fn the_live_rates_are_the_published_rates_exactly() {
    let config = live();
    assert_eq!(
        config.equities.sec_rate,
        FeeRate::parse("0.0000206").unwrap()
    );
    assert_eq!(
        config.equities.taf_per_share,
        FeePerShare::parse("0.000195").unwrap()
    );
    assert_eq!(config.equities.taf_cap, FeeCap::parse("9.79").unwrap());
    assert_eq!(
        config.equities.cat_per_share,
        FeePerShare::parse("0.000003").unwrap()
    );
}

/// A live sell books SEC, TAF and CAT, each rounded up to the cent, and nothing for commission.
/// One share at 85: SEC 85 × 0.0000206 = 0.001751 → 0.01, TAF 1 × 0.000195 = 0.000195 → 0.01, CAT
/// 1 × 0.000003 = 0.000003 → 0.01; the day's charge is 0.03. A hundred at 85: SEC
/// 8500 × 0.0000206 = 0.1751 → 0.18, TAF 0.0195 → 0.02, CAT 0.0003 → 0.01; charge 0.21. Sixty
/// thousand at 10: SEC 600000 × 0.0000206 = 12.36, TAF 60000 × 0.000195 = 11.70 capped at 9.79,
/// CAT 0.18, each already whole cents; charge 22.33.
#[test]
fn a_live_sell_books_each_regulatory_fee_rounded_up() {
    let cases = [
        ("1", "85", ["0.01", "0.01", "0.01"], "0.03"),
        ("100", "85", ["0.18", "0.02", "0.01"], "0.21"),
        ("60000", "10", ["12.36", "9.79", "0.18"], "22.33"),
    ];
    for (qty, price, [sec, taf, cat], charge) in cases {
        let (lines, charged) = booked(Side::Sell, qty, price);
        assert_eq!(
            lines,
            [
                (FeeKind::Sec, usd(sec)),
                (FeeKind::Taf, usd(taf)),
                (FeeKind::Cat, usd(cat)),
            ],
            "{qty} at {price}"
        );
        assert_eq!(charged, usd(charge), "{qty} at {price}");
    }
}

/// A live buy books no SEC fee, no TAF and no commission: its one line is CAT, which Robinhood
/// posts on listed equities without naming a side, so the conservative reading books it on buys
/// too. R0's one share at 85 accrues CAT 0.000003 → 0.01 and is charged 0.01, a cent above
/// Robinhood's own figure, which rounds a sub-cent CAT down to 0.
#[test]
fn a_live_buy_books_only_cat() {
    for (qty, price, cat, charge) in [("1", "85", "0.01", "0.01"), ("60000", "10", "0.18", "0.18")]
    {
        let (lines, charged) = booked(Side::Buy, qty, price);
        assert_eq!(lines, [(FeeKind::Cat, usd(cat))], "{qty} at {price}");
        assert_eq!(charged, usd(charge), "{qty} at {price}");
    }
}

/// A paper stream never reads the live schedule, and the live schedule serves no other broker;
/// a live stream never reads the paper-only overestimate either.
#[test]
fn the_live_schedule_refuses_every_other_environment_and_broker() {
    for environment in ["paper", "", "Live", "live "] {
        assert_eq!(
            robinhood_live_fee_config(environment, "robinhood", calendar(), DAY),
            Err(ExecutorError::EnvironmentMismatch {
                opened: environment.to_owned(),
                found: "live".to_owned(),
            }),
            "{environment:?}"
        );
    }
    for broker in ["alpaca", "", "Robinhood"] {
        assert_eq!(
            robinhood_live_fee_config("live", broker, calendar(), DAY),
            Err(ExecutorError::NonCanonicalPayload {
                field: "broker".to_owned()
            }),
            "{broker:?}"
        );
    }
    assert!(matches!(
        paper_only_fee_config("live", calendar(), DAY),
        Err(ExecutorError::EnvironmentMismatch { .. })
    ));
}

/// The first day must be one the calendar covers, as for every transcribed schedule.
#[test]
fn the_live_schedule_takes_effect_only_inside_the_calendar() {
    for day in ["2027-01-04", "2026-13-01", ""] {
        assert_eq!(
            robinhood_live_fee_config("live", "robinhood", calendar(), day),
            Err(ExecutorError::NonCanonicalPayload {
                field: "effective_from".to_owned()
            }),
            "{day:?}"
        );
    }
}

/// Picodollars (10⁻¹² USD), the oracle's unit: every rate × amount below is exact in it.
const PICO_PER_CENT: u128 = 10_000_000_000;

/// The exact regulatory cost of one fill, in picodollars, from the published rates alone:
/// (SEC, TAF capped, CAT). SEC is 206 × 10⁻⁷ of the proceeds, TAF 195 × 10⁻⁶ a share up to 9.79,
/// CAT 3 × 10⁻⁶ a share.
fn exact(side: Side, shares: u128, price_cents: u128) -> (u128, u128, u128) {
    let cat = shares * 3_000_000;
    match side {
        Side::Buy => (0, 0, cat),
        Side::Sell => {
            let sec = shares * price_cents * 206 * 1_000;
            let taf = (shares * 195_000_000).min(979 * PICO_PER_CENT);
            (sec, taf, cat)
        }
    }
}

/// `cents` as dollars.
fn dollars(cents: u128) -> Usd {
    usd(&price_text(cents))
}

/// `cents` as canonical decimal text: no trailing zero after the point, and no point for a whole
/// amount.
fn price_text(cents: u128) -> String {
    match (cents / 100, cents % 100) {
        (whole, 0) => whole.to_string(),
        (whole, part) if part % 10 == 0 => format!("{whole}.{}", part / 10),
        (whole, part) => format!("{whole}.{part:02}"),
    }
}

fn cents_up(pico: u128) -> u128 {
    pico.div_ceil(PICO_PER_CENT)
}

/// Robinhood's own rounding of one fee to cents (its schedule's notes iv, v and vii): SEC up to the
/// next cent; TAF and CAT to the nearest cent, a fee below one cent rounded down to 0.
fn robinhood_cents(side: Side, shares: u128, price_cents: u128) -> u128 {
    let (sec, taf, cat) = exact(side, shares, price_cents);
    let nearest = |pico: u128| {
        if pico < PICO_PER_CENT {
            0
        } else {
            (pico + PICO_PER_CENT / 2) / PICO_PER_CENT
        }
    };
    cents_up(sec) + nearest(taf) + nearest(cat)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// One live fill's day charge is each exact regulatory fee rounded up to the cent, summed
    /// (§6.2 `per_fee_per_fill`, DEC-960): never below the exact cost, and under three cents above
    /// it.
    #[test]
    fn a_live_charge_is_each_exact_fee_rounded_up(
        sell in any::<bool>(),
        shares in prop_oneof![1u128..2_000, 1u128..200_000],
        price_cents in 500u128..100_000,
    ) {
        let side = if sell { Side::Sell } else { Side::Buy };
        let price = price_text(price_cents);
        let (_, charged) = booked(side, &shares.to_string(), &price);
        let (sec, taf, cat) = exact(side, shares, price_cents);
        prop_assert_eq!(charged, dollars(cents_up(sec) + cents_up(taf) + cents_up(cat)));
        prop_assert!(charged >= dollars(cents_up(sec + taf + cat)));
    }

    /// R10b closed: Robinhood rounds each fee of a trade separately (SEC up, TAF and CAT to the
    /// nearest cent), and the platform's charge for a live fill, buy or sell, is never below
    /// Robinhood's own figure.
    #[test]
    fn a_live_charge_is_never_below_robinhoods_own_rounding(
        sell in any::<bool>(),
        shares in 1u128..200_000,
        price_cents in 500u128..100_000,
    ) {
        let side = if sell { Side::Sell } else { Side::Buy };
        let price = price_text(price_cents);
        let (_, charged) = booked(side, &shares.to_string(), &price);
        let robinhood = dollars(robinhood_cents(side, shares, price_cents));
        prop_assert!(charged >= robinhood, "{charged:?} below {robinhood:?}");
    }
}
