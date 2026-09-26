//! Hand-calculated settlement and buying-power cases (trading-domain spec §7.2, §8.3, §8.4;
//! DEC-104). Each expected value is worked out by hand in its doc comment from the spec's formulas,
//! not produced by running the code.

mod common;

use common::{
    Fill, at, charge, d, equity, holding_in, mark, opening, reserved, step, test_default, text, usd,
};
use mandate_accounting::{
    Account, AccountType, AccountingError, AssetClass, FeeFamily, Input, Liquidity, Reservations,
    Side,
};
use mandate_num::NumError;

fn buying_power(account: &Account) -> String {
    text(account.buying_power(Reservations::NONE).unwrap())
}

fn unsettled(account: &Account) -> Vec<(String, String)> {
    account
        .unsettled()
        .map(|(date, amount)| (text(date), text(amount)))
        .collect()
}

/// The QQQ order of RC-08 and RC-18: 6 shares at a 100.00 limit, 600, plus the fee reservation
/// round(6 × 0.00001, 2, ceiling) = 0.01 (spec §9.5).
const QQQ_ORDER_REQUIREMENT: &str = "600.01";

/// RC-08, a cash account with 1000 settled. Buy 5 XYZ at 100: settled 500; CAT 5 × 0.00001 =
/// 0.00005 accrued, so buying power = 500 − round(0.00005, 2, ceiling) = 500 − 0.01 = 499.99.
/// Sell 5 at 110: proceeds 550 unsettled to 2026-09-22; SEC 550 × 0.00003 = 0.0165, TAF
/// 5 × 0.0002 = 0.001, CAT 0.00005; accrued 0.0176; buying power = 500 − 0.02 = 499.98: the
/// unsettled 550 does not count. The QQQ order needs 600.01 > 499.98, which the gate denies
/// (`insufficient_settled_buying_power`, E6-3). Charge: ceil(0.0176, 2) = 0.02, settled 499.98,
/// buying power 499.98. Settlement at 00:00 ET on 2026-09-22 moves 550: settled 1049.98, buying
/// power 1049.98.
#[test]
#[ignore = "pending E3-3"]
fn rc_08_cash_account_buying_power_is_settled_cash_less_pending_charges() {
    let config = test_default();
    let a = opening(AccountType::Cash, "1000");
    assert_eq!(a.account_type(), AccountType::Cash);
    assert_eq!(buying_power(&a), "1000");

    let a = step(
        &a,
        &equity(
            "f1",
            "XYZ",
            Side::Buy,
            "5",
            "100",
            "2026-09-21T14:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(text(a.settled()), "500");
    assert_eq!(text(a.fees_accrued().unwrap()), "0.00005");
    assert_eq!(buying_power(&a), "499.99");

    let a = step(
        &a,
        &equity(
            "f2",
            "XYZ",
            Side::Sell,
            "5",
            "110",
            "2026-09-21T15:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(text(a.settled()), "500");
    assert_eq!(unsettled(&a), [("2026-09-22".into(), "550".into())]);
    assert_eq!(text(a.fees_accrued().unwrap()), "0.0176");
    assert_eq!(buying_power(&a), "499.98");
    assert!(
        usd(QQQ_ORDER_REQUIREMENT) > a.buying_power(Reservations::NONE).unwrap(),
        "the QQQ order exceeds settled buying power"
    );

    let a = step(&a, &charge(FeeFamily::Equities, "2026-09-21"), &config);
    assert_eq!(text(a.fees_charged()), "0.02");
    assert_eq!(text(a.settled()), "499.98");
    assert_eq!(buying_power(&a), "499.98");

    let due = a.due(at("2026-09-22T00:00:00-04:00")).unwrap();
    assert_eq!(
        due,
        [Input::SettlementPosted {
            date: d("2026-09-22")
        }]
    );
    let a = step(&a, &due[0], &config);
    assert_eq!(text(a.settled()), "1049.98");
    assert!(unsettled(&a).is_empty());
    assert_eq!(buying_power(&a), "1049.98");
    assert_eq!(a.account_type(), AccountType::Cash);
}

/// RC-18, a margin account with 1000 settled, and its `generic_cash_account` variant. After the
/// same buy and sell as RC-08, buying power = 500 + 550 − 0.02 = 1049.98: the unsettled proceeds
/// count. The QQQ order needs 600.01 ≤ 1049.98, so the gate allows it and reserves 600.01, leaving
/// 1049.98 − 600.01 = 449.97. Its fill of 6 at 100 releases the reservation: settled 500 − 600 =
/// −100, total 450, accrued 0.0176 + 0.00006 = 0.01766, buying power 450 − 0.02 = 449.98, and the
/// no-debit rule holds: 450 − 0.01766 ≥ 0. Charge ceil(0.01766, 2) = 0.02: settled −100.02, total
/// 449.98. Settlement: settled 449.98. In the cash variant, buying power after the sell is 499.98,
/// so the same order is denied.
#[test]
#[ignore = "pending E3-3"]
fn rc_18_margin_account_reuses_unsettled_proceeds_and_its_cash_variant_does_not() {
    let config = test_default();
    let buy = equity(
        "f1",
        "XYZ",
        Side::Buy,
        "5",
        "100",
        "2026-09-21T14:00:00-04:00",
    );
    let sell = equity(
        "f2",
        "XYZ",
        Side::Sell,
        "5",
        "110",
        "2026-09-21T15:00:00-04:00",
    );
    let margin = opening(AccountType::Margin, "1000");
    let margin = step(&step(&margin, &buy, &config), &sell, &config);
    assert_eq!(text(margin.fees_accrued().unwrap()), "0.0176");
    assert_eq!(buying_power(&margin), "1049.98");
    assert!(usd(QQQ_ORDER_REQUIREMENT) <= margin.buying_power(Reservations::NONE).unwrap());
    assert_eq!(
        text(
            margin
                .buying_power(reserved(QQQ_ORDER_REQUIREMENT))
                .unwrap()
        ),
        "449.97"
    );

    let margin = step(
        &margin,
        &Fill {
            fill_id: "f3",
            order: Some("buy_q"),
            instrument: "QQQ",
            asset_class: AssetClass::UsEquity,
            side: Side::Buy,
            qty: "6",
            price: "100",
            liquidity: None,
            at: "2026-09-21T15:30:05-04:00",
        }
        .input(),
        &config,
    );
    assert_eq!(text(margin.settled()), "-100");
    assert_eq!(unsettled(&margin), [("2026-09-22".into(), "550".into())]);
    assert_eq!(text(margin.cash_total().unwrap()), "450");
    assert_eq!(text(margin.fees_accrued().unwrap()), "0.01766");
    assert_eq!(buying_power(&margin), "449.98");
    let no_debit = margin
        .cash_total()
        .unwrap()
        .checked_sub(margin.fees_accrued().unwrap())
        .unwrap();
    assert!(!no_debit.is_negative(), "{no_debit}");

    let margin = step(&margin, &charge(FeeFamily::Equities, "2026-09-21"), &config);
    assert_eq!(text(margin.fees_charged()), "0.02");
    assert_eq!(text(margin.settled()), "-100.02");
    assert_eq!(text(margin.cash_total().unwrap()), "449.98");
    assert_eq!(buying_power(&margin), "449.98");

    let margin = step(
        &margin,
        &Input::SettlementPosted {
            date: d("2026-09-22"),
        },
        &config,
    );
    assert_eq!(text(margin.settled()), "449.98");
    assert!(unsettled(&margin).is_empty());
    assert_eq!(buying_power(&margin), "449.98");

    let cash = opening(AccountType::Cash, "1000");
    let cash = step(&step(&cash, &buy, &config), &sell, &config);
    assert_eq!(buying_power(&cash), "499.98");
    assert!(usd(QQQ_ORDER_REQUIREMENT) > cash.buying_power(Reservations::NONE).unwrap());
}

/// The accrued-fee term is the charge each open bucket will post, summed (DEC-104). Two equity
/// buys of 5 at 1.00 on different trade dates each accrue CAT 0.00005; a crypto sell of 0.001 BTC
/// at 50000 (notional 50, taker 25 bps: 0.125, half-up 0.13) accrues 0.13 on its UTC day. Cash:
/// 100 − 5 − 5 + 50 = 140. Charges due: 0.01 + 0.01 + 0.13 = 0.15, so buying power is 139.85,
/// not 140 − round(0.13010, 2, ceiling) = 139.86. Charging all three leaves settled 139.85: the
/// buying power reported before them.
#[test]
#[ignore = "pending E3-3"]
fn pending_charges_are_rounded_up_per_family_and_day() {
    let config = test_default();
    let a = holding_in(AccountType::Cash, "100", "BTC", "0.001", "40");
    let a = step(
        &a,
        &equity(
            "f1",
            "XYZ",
            Side::Buy,
            "5",
            "1",
            "2026-09-21T10:00:00-04:00",
        ),
        &config,
    );
    let a = step(
        &a,
        &equity(
            "f2",
            "XYZ",
            Side::Buy,
            "5",
            "1",
            "2026-09-22T10:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(text(a.fees_accrued().unwrap()), "0.0001");
    assert_eq!(buying_power(&a), "89.98");
    let a = step(
        &a,
        &Fill {
            fill_id: "f3",
            order: None,
            instrument: "BTC",
            asset_class: AssetClass::Crypto,
            side: Side::Sell,
            qty: "0.001",
            price: "50000",
            liquidity: Some(Liquidity::Taker),
            at: "2026-09-21T10:00:00Z",
        }
        .input(),
        &config,
    );
    assert_eq!(text(a.settled()), "140");
    assert_eq!(text(a.fees_accrued().unwrap()), "0.1301");
    assert_eq!(buying_power(&a), "139.85");

    let a = step(&a, &charge(FeeFamily::Equities, "2026-09-21"), &config);
    let a = step(&a, &charge(FeeFamily::Equities, "2026-09-22"), &config);
    let a = step(&a, &charge(FeeFamily::Crypto, "2026-09-21"), &config);
    assert_eq!(text(a.fees_accrued().unwrap()), "0");
    assert_eq!(text(a.fees_charged()), "0.15");
    assert_eq!(text(a.settled()), "139.85");
    assert_eq!(buying_power(&a), "139.85");
}

/// Crypto settles at fill (spec §8.4). In a cash account holding 1 BTC, a maker sell at 40000
/// puts 40000 in settled cash at once with no unsettled bucket; the USD fee 40000 × 15 ÷ 10000 =
/// 60 accrues, so buying power is 39940 immediately. A buy of 0.5 at 40000 (fee in the asset,
/// nothing accrued) takes 20000: buying power 19940.
#[test]
#[ignore = "pending E3-3"]
fn crypto_proceeds_are_settled_at_fill_in_a_cash_account() {
    let config = test_default();
    let a = holding_in(AccountType::Cash, "0", "BTC", "1", "30000");
    let crypto = |fill_id, side, qty, liquidity| {
        Fill {
            fill_id,
            order: None,
            instrument: "BTC",
            asset_class: AssetClass::Crypto,
            side,
            qty,
            price: "40000",
            liquidity: Some(liquidity),
            at: "2026-09-21T14:00:00Z",
        }
        .input()
    };
    let a = step(
        &a,
        &crypto("f1", Side::Sell, "1", Liquidity::Maker),
        &config,
    );
    assert_eq!(text(a.settled()), "40000");
    assert!(unsettled(&a).is_empty());
    assert_eq!(text(a.fees_accrued().unwrap()), "60");
    assert_eq!(buying_power(&a), "39940");

    let a = step(
        &a,
        &crypto("f2", Side::Buy, "0.5", Liquidity::Taker),
        &config,
    );
    assert_eq!(text(a.settled()), "20000");
    assert_eq!(text(a.fees_accrued().unwrap()), "60");
    assert_eq!(buying_power(&a), "19940");
}

/// The fold records every fill the broker reports, including one the gate should have denied
/// (DEC-104): a buy of 2 at 100 in a cash account with 100 settled leaves settled −100 and, with
/// CAT 0.00002 rounded up to 0.01, buying power −100.01. The no-debit rule is the gate's promise
/// (I4), not a rejection in the fold.
#[test]
#[ignore = "pending E3-3"]
fn a_fill_that_creates_a_debit_is_recorded_and_buying_power_reports_the_shortfall() {
    let config = test_default();
    let fill = equity(
        "f1",
        "XYZ",
        Side::Buy,
        "2",
        "100",
        "2026-09-21T10:00:00-04:00",
    );
    for account_type in [AccountType::Cash, AccountType::Margin] {
        let a = opening(account_type, "100");
        let applied = a.apply(&fill, &config);
        assert!(applied.is_ok(), "{account_type:?}: {applied:?}");
        let a = applied.unwrap().account;
        assert_eq!(text(a.settled()), "-100");
        assert_eq!(buying_power(&a), "-100.01");
        assert!(
            a.buying_power(Reservations::NONE).unwrap().is_negative(),
            "{account_type:?}"
        );
    }
}

/// Reservations come off buying power one for one (spec §7.2, §9.5) and are never negative: a
/// negative total would add buying power.
#[test]
#[ignore = "pending E3-3"]
fn reservations_reduce_buying_power_and_are_never_negative() {
    let config = test_default();
    let a = step(
        &opening(AccountType::Margin, "1000"),
        &mark("XYZ", "10"),
        &config,
    );
    assert_eq!(text(Reservations::NONE.total()), "0");
    assert_eq!(text(reserved("600.01").total()), "600.01");
    assert_eq!(buying_power(&a), "1000");
    assert_eq!(text(a.buying_power(reserved("600.01")).unwrap()), "399.99");
    assert_eq!(
        text(a.buying_power(reserved("1000.005")).unwrap()),
        "-0.005"
    );
    assert_eq!(
        Reservations::new(usd("-0.01")),
        Err(AccountingError::Num(NumError::Negative))
    );
    assert_eq!(
        Reservations::new(usd("-0.01")).unwrap_err().code(),
        "negative"
    );
}

/// The account type is set at opening, reported, kept through the fold, and part of equality: two
/// otherwise identical accounts of different types are different states (I6 compares accounts).
/// Selling 1 XYZ at 10 accrues SEC 0.0003 + TAF 0.0002 + CAT 0.00001 = 0.00051, rounded up to
/// 0.01: buying power 1000 − 0.01 = 999.99 in the cash account and 1010 − 0.01 = 1009.99 in the
/// margin account.
#[test]
#[ignore = "pending E3-3"]
fn the_account_type_is_set_at_opening_and_kept_through_the_fold() {
    let config = test_default();
    let cash = opening(AccountType::Cash, "1000");
    let margin = opening(AccountType::Margin, "1000");
    assert_eq!(cash.account_type(), AccountType::Cash);
    assert_eq!(margin.account_type(), AccountType::Margin);
    assert_ne!(cash, margin);
    let sell = equity(
        "f1",
        "XYZ",
        Side::Sell,
        "1",
        "10",
        "2026-09-21T10:00:00-04:00",
    );
    let cash = step(&cash, &sell, &config);
    let margin = step(&margin, &sell, &config);
    assert_eq!(cash.account_type(), AccountType::Cash);
    assert_eq!(margin.account_type(), AccountType::Margin);
    assert_eq!(text(cash.settled()), text(margin.settled()));
    assert_eq!(unsettled(&cash), unsettled(&margin));
    assert_eq!(buying_power(&cash), "999.99");
    assert_eq!(buying_power(&margin), "1009.99");
}

/// In a cash account holding 5 XYZ with no settled cash, selling 5 at 110 puts 550 in the
/// 2026-09-22 bucket and accrues SEC 0.0165 + TAF 0.001 + CAT 0.00005 = 0.01755, so buying power
/// is 0 − round(0.01755, 2, ceiling) = −0.02 and every buy is denied. The charge at 20:00 debits
/// ceil(0.01755, 2) = 0.02 from settled cash before the proceeds settle:
/// settled −0.02, a debit bounded by the charge that exists only while a bucket is unsettled
/// (DEC-104). Settlement leaves 549.98.
#[test]
#[ignore = "pending E3-3"]
fn a_cash_account_sale_can_leave_a_fee_debit_until_its_proceeds_settle() {
    let config = test_default();
    let a = holding_in(AccountType::Cash, "0", "XYZ", "5", "500");
    let a = step(
        &a,
        &equity(
            "f1",
            "XYZ",
            Side::Sell,
            "5",
            "110",
            "2026-09-21T15:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(text(a.settled()), "0");
    assert_eq!(unsettled(&a), [("2026-09-22".into(), "550".into())]);
    assert_eq!(text(a.fees_accrued().unwrap()), "0.01755");
    assert_eq!(buying_power(&a), "-0.02");

    let a = step(&a, &charge(FeeFamily::Equities, "2026-09-21"), &config);
    assert_eq!(text(a.settled()), "-0.02");
    assert_eq!(text(a.cash_total().unwrap()), "549.98");
    assert_eq!(buying_power(&a), "-0.02");

    let a = step(
        &a,
        &Input::SettlementPosted {
            date: d("2026-09-22"),
        },
        &config,
    );
    assert_eq!(text(a.settled()), "549.98");
    assert_eq!(buying_power(&a), "549.98");
}
