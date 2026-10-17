//! Per-fill, per-fee rounding of US equities regulatory fees (trading-domain spec §6.2,
//! `fee_rounding: per_fee_per_fill`, DEC-960). Each expected value is worked out by hand in its doc
//! comment from the spec's formulas and `test_default`'s rates (SEC 0.00003 of proceeds, TAF
//! 0.0002 a share, CAT 0.00001 a share), never by running the code.

mod common;

use common::{Fill, charge, fee_cap, id, no_fees, step, test_default, usd};
use mandate_accounting::{
    Account, AccountType, AssetClass, Config, EquityFees, FeeFamily, FeeKind, FeeRounding, Input,
    Position, Record, Side, TafCapBasis,
};
use mandate_num::{CostBasis, SignedQty, Usd};

fn per_fee(config: Config) -> Config {
    Config {
        equities: EquityFees {
            fee_rounding: FeeRounding::PerFeePerFill,
            ..config.equities
        },
        ..config
    }
}

fn xyz(fill_id: &str, order: &str, side: Side, qty: &str, price: &str, at: &str) -> Input {
    Fill {
        fill_id,
        order: Some(order),
        instrument: "XYZ",
        asset_class: AssetClass::UsEquity,
        side,
        qty,
        price,
        liquidity: None,
        at,
    }
    .input()
}

/// The fill's fee lines as `(kind, usd)`, and the account after it.
fn booked(account: &Account, input: &Input, config: &Config) -> (Vec<(FeeKind, Usd)>, Account) {
    let applied = account
        .apply(input, config)
        .unwrap_or_else(|e| panic!("{input:?}: {e}"));
    let Record::Fill { fees, .. } = applied.record else {
        panic!("a fill books a fill record");
    };
    let lines = fees.into_iter().map(|fee| (fee.kind, fee.usd)).collect();
    (lines, applied.account)
}

fn lines(fees: &[(FeeKind, &str)]) -> Vec<(FeeKind, Usd)> {
    fees.iter()
        .map(|(kind, amount)| (*kind, usd(amount)))
        .collect()
}

/// RC-26, with a TAF cap of 0.03. Buy 500 at 49: CAT 0.005 → 0.01. Sell 100 at 50.10 on `01S-1`:
/// SEC 5010 × 0.00003 = 0.1503 → 0.16, TAF 100 × 0.0002 = 0.02, CAT 0.001 → 0.01, so 0.19; twice.
/// Sell 300 at 50.10 on `01S-2`: SEC 15030 × 0.00003 = 0.4509 → 0.46, TAF min(0.06, 0.03) = 0.03,
/// CAT 0.003 → 0.01, so 0.50. Accrued 0.01, 0.20, 0.39, 0.89; the day's charge is
/// ceil(0.89, 2) = 0.89; realized 500 × 1.10 = 550, net 550 − 0.89 = 549.11.
#[test]
fn rc_26_each_fee_of_each_fill_is_rounded_up_before_summing() {
    let mut config = per_fee(test_default());
    config.equities.taf_cap = fee_cap("0.03");
    let a = Account::opening(AccountType::Margin, usd("30000"), []);
    let (fees, a) = booked(
        &a,
        &xyz(
            "f1",
            "01B-1",
            Side::Buy,
            "500",
            "49",
            "2026-09-21T10:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(fees, lines(&[(FeeKind::Cat, "0.01")]));
    assert_eq!(a.fees_accrued().unwrap(), usd("0.01"));
    let sell = lines(&[
        (FeeKind::Sec, "0.16"),
        (FeeKind::Taf, "0.02"),
        (FeeKind::Cat, "0.01"),
    ]);
    let (fees, a) = booked(
        &a,
        &xyz(
            "f2",
            "01S-1",
            Side::Sell,
            "100",
            "50.1",
            "2026-09-21T11:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(fees, sell);
    assert_eq!(a.fees_accrued().unwrap(), usd("0.2"));
    let (fees, a) = booked(
        &a,
        &xyz(
            "f3",
            "01S-1",
            Side::Sell,
            "100",
            "50.1",
            "2026-09-21T11:00:01-04:00",
        ),
        &config,
    );
    assert_eq!(fees, sell);
    assert_eq!(a.fees_accrued().unwrap(), usd("0.39"));
    let (fees, a) = booked(
        &a,
        &xyz(
            "f4",
            "01S-2",
            Side::Sell,
            "300",
            "50.1",
            "2026-09-21T11:00:02-04:00",
        ),
        &config,
    );
    assert_eq!(
        fees,
        lines(&[
            (FeeKind::Sec, "0.46"),
            (FeeKind::Taf, "0.03"),
            (FeeKind::Cat, "0.01"),
        ])
    );
    assert_eq!(a.fees_accrued().unwrap(), usd("0.89"));
    assert_eq!(a.realized_gross(), usd("550"));
    let a = step(&a, &charge(FeeFamily::Equities, "2026-09-21"), &config);
    assert_eq!(a.fees_charged(), usd("0.89"));
    assert_eq!(a.realized_net().unwrap(), usd("549.11"));
}

/// The same fills under the default `daily_total` keep §6.2's old rule: full precision per fill
/// (0.005; + 0.1503 + 0.02 + 0.001 = 0.1763; 0.3476; + 0.4509 + 0.03 + 0.003 = 0.8315), charged
/// ceil(0.8315, 2) = 0.84.
#[test]
fn daily_total_keeps_full_precision_until_the_charge() {
    let mut config = test_default();
    config.equities.taf_cap = fee_cap("0.03");
    assert_eq!(config.equities.fee_rounding, FeeRounding::DailyTotal);
    let mut a = Account::opening(AccountType::Margin, usd("30000"), []);
    for (n, (order, side, qty, accrued)) in [
        ("01B-1", Side::Buy, "500", "0.005"),
        ("01S-1", Side::Sell, "100", "0.1763"),
        ("01S-1", Side::Sell, "100", "0.3476"),
        ("01S-2", Side::Sell, "300", "0.8315"),
    ]
    .into_iter()
    .enumerate()
    {
        let price = if side == Side::Buy { "49" } else { "50.1" };
        a = step(
            &a,
            &xyz(
                &format!("f{n}"),
                order,
                side,
                qty,
                price,
                &format!("2026-09-21T11:00:0{n}-04:00"),
            ),
            &config,
        );
        assert_eq!(a.fees_accrued().unwrap(), usd(accrued), "fill {n}");
    }
    let a = step(&a, &charge(FeeFamily::Equities, "2026-09-21"), &config);
    assert_eq!(a.fees_charged(), usd("0.84"));
}

/// Under a per-order cap the room counts the full-precision TAF already charged on the order, and
/// each fill's TAF is then rounded up (DEC-960 item 4). Cap 0.05; four sells of 75 at 50 on `o1`,
/// each with uncapped TAF 75 × 0.0002 = 0.015: room 0.05, 0.035, 0.02 → TAF 0.015 → 0.02 three
/// times; then room 0.05 − 0.045 = 0.005 → TAF 0.005 → 0.01. The order's TAF lines total 0.07, at
/// least the 0.05 a broker capping the order would charge. Counting the rounded 0.02s instead would
/// leave room 0.01 and then 0, charging 0.01 and 0.
#[test]
fn a_per_order_cap_counts_full_precision_taf_then_each_fill_rounds_up() {
    let mut config = per_fee(test_default());
    config.equities.taf_cap = fee_cap("0.05");
    config.equities.taf_cap_basis = TafCapBasis::PerOrder;
    let held = Position::new(
        SignedQty::parse("300").unwrap(),
        CostBasis::parse("14700").unwrap(),
    )
    .unwrap();
    let mut a = Account::opening(AccountType::Margin, usd("0"), [(id("XYZ"), held)]);
    for (n, taf) in ["0.02", "0.02", "0.02", "0.01"].into_iter().enumerate() {
        let (fees, next) = booked(
            &a,
            &xyz(
                &format!("f{n}"),
                "o1",
                Side::Sell,
                "75",
                "50",
                &format!("2026-09-21T11:00:0{n}-04:00"),
            ),
            &config,
        );
        let taf_line = fees.iter().find(|(kind, _)| *kind == FeeKind::Taf);
        assert_eq!(
            taf_line.map(|(_, amount)| *amount),
            Some(usd(taf)),
            "fill {n}"
        );
        a = next;
    }
}

/// A zero fee stays zero: under `no_fees` a sell books no cent for any fee, and nothing is charged.
#[test]
fn a_zero_fee_is_never_rounded_up_to_a_cent() {
    let config = per_fee(no_fees());
    let held = Position::new(
        SignedQty::parse("10").unwrap(),
        CostBasis::parse("100").unwrap(),
    )
    .unwrap();
    let a = Account::opening(AccountType::Margin, usd("0"), [(id("XYZ"), held)]);
    let (fees, a) = booked(
        &a,
        &xyz(
            "f1",
            "o1",
            Side::Sell,
            "10",
            "12",
            "2026-09-21T11:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(
        fees,
        lines(&[
            (FeeKind::Sec, "0"),
            (FeeKind::Taf, "0"),
            (FeeKind::Cat, "0"),
        ])
    );
    let a = step(&a, &charge(FeeFamily::Equities, "2026-09-21"), &config);
    assert_eq!(a.fees_charged(), Usd::ZERO);
}
