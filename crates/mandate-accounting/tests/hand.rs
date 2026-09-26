//! Hand-calculated cases (trading-domain spec §6, §8). Each expected value is worked out by hand in
//! its doc comment from the spec's formulas, not produced by running the code.

mod common;

use common::{
    Fill, at, charge, d, equity, fee_cap, id, mark, no_fees, step, test_default, text, usd,
};
use mandate_accounting::{
    Account, AccountType, AccountingError, AssetClass, FeeFamily, FeeKind, Input, Liquidity,
    Position, Record, Side, TafCapBasis,
};
use mandate_num::{CostBasis, Qty, SignedQty, Usd};

fn position(account: &Account, instrument: &str) -> (String, String) {
    let p = account.position(&id(instrument));
    (text(p.qty()), text(p.basis()))
}

fn unsettled(account: &Account) -> Vec<(String, String)> {
    account
        .unsettled()
        .map(|(date, amount)| (text(date), text(amount)))
        .collect()
}

/// RC-01. Buy 10 at 150: settled 10000 − 1500 = 8500; CAT 10 × 0.00001 = 0.0001.
/// Sell 4 at 160: proceeds 640 unsettled to 2026-09-22; R = 1500 × 4 ÷ 10 = 600, realized
/// −(−4 × 160) − 600 = 40; SEC 640 × 0.00003 = 0.0192, TAF 4 × 0.0002 = 0.0008, CAT 0.00004,
/// accrued 0.0001 + 0.02004 = 0.02014. Mark 155: unrealized 6 × 155 − 900 = 30; equity
/// 8500 + 640 − 0.02014 + 930 = 10069.97986. Charge: ceil(0.02014, 2) = 0.03, settled 8499.97,
/// total 9139.97, equity 10069.97. Settlement at 00:00 ET 2026-09-22 moves 640: settled 9139.97;
/// net realized 40 − 0.03 = 39.97; Δequity 69.97 = 40 + 30 − 0.03.
#[test]
fn rc_01_buy_partial_sell_accrual_charge_settlement() {
    let config = test_default();
    let start = Account::opening(AccountType::Margin, usd("10000"), []);
    let a = step(
        &start,
        &equity(
            "f1",
            "AAPL",
            Side::Buy,
            "10",
            "150",
            "2026-09-21T10:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(text(a.settled()), "8500");
    assert_eq!(text(a.cash_total().unwrap()), "8500");
    assert_eq!(text(a.fees_accrued().unwrap()), "0.0001");

    let a = step(
        &a,
        &equity(
            "f2",
            "AAPL",
            Side::Sell,
            "4",
            "160",
            "2026-09-21T11:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(position(&a, "AAPL"), ("6".into(), "900".into()));
    assert_eq!(text(a.realized_gross()), "40");
    assert_eq!(text(a.settled()), "8500");
    assert_eq!(unsettled(&a), [("2026-09-22".into(), "640".into())]);
    assert_eq!(text(a.fees_accrued().unwrap()), "0.02014");

    let a = step(&a, &mark("AAPL", "155"), &config);
    assert_eq!(text(a.unrealized().unwrap()), "30");
    assert_eq!(text(a.equity().unwrap()), "10069.97986");

    let a = step(&a, &charge(FeeFamily::Equities, "2026-09-21"), &config);
    assert_eq!(text(a.fees_accrued().unwrap()), "0");
    assert_eq!(text(a.fees_charged()), "0.03");
    assert_eq!(text(a.fees_total().unwrap()), "0.03");
    assert_eq!(text(a.settled()), "8499.97");
    assert_eq!(text(a.cash_total().unwrap()), "9139.97");
    assert_eq!(text(a.equity().unwrap()), "10069.97");

    let due = a.settlements_due(at("2026-09-22T00:00:00-04:00")).unwrap();
    assert_eq!(due, [d("2026-09-22")]);
    assert!(
        a.settlements_due(at("2026-09-21T23:59:59-04:00"))
            .unwrap()
            .is_empty()
    );
    let a = step(
        &a,
        &Input::SettlementPosted {
            date: d("2026-09-22"),
        },
        &config,
    );
    assert_eq!(text(a.settled()), "9139.97");
    assert!(unsettled(&a).is_empty());
    assert_eq!(text(a.cash_total().unwrap()), "9139.97");
    assert_eq!(text(a.realized_net().unwrap()), "39.97");
    let start_equity = start.equity().unwrap();
    assert_eq!(
        text(a.equity().unwrap().checked_sub(start_equity).unwrap()),
        "69.97"
    );
}

/// RC-02. Buys 10 at 100 and 10 at 110: Q 20, B 2100 (average 105). Sell 5 at 120:
/// R = 2100 × 5 ÷ 20 = 525, B 1575 (average 105 kept), realized 600 − 525 = 75. Fees: CAT 0.0001
/// twice, then SEC 600 × 0.00003 = 0.018, TAF 0.001, CAT 0.00005: accrued 0.01925. Charge
/// ceil(0.01925, 2) = 0.02; cash total 100000 − 2100 + 600 − 0.02 = 98499.98.
#[test]
fn rc_02_averaging_in_and_reducing_keeps_average_cost() {
    let config = test_default();
    let a = Account::opening(AccountType::Margin, usd("100000"), []);
    let a = step(
        &a,
        &equity(
            "f1",
            "MSFT",
            Side::Buy,
            "10",
            "100",
            "2026-09-21T10:00:00-04:00",
        ),
        &config,
    );
    let a = step(
        &a,
        &equity(
            "f2",
            "MSFT",
            Side::Buy,
            "10",
            "110",
            "2026-09-21T10:30:00-04:00",
        ),
        &config,
    );
    assert_eq!(position(&a, "MSFT"), ("20".into(), "2100".into()));
    let a = step(
        &a,
        &equity(
            "f3",
            "MSFT",
            Side::Sell,
            "5",
            "120",
            "2026-09-21T11:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(position(&a, "MSFT"), ("15".into(), "1575".into()));
    assert_eq!(text(a.realized_gross()), "75");
    assert_eq!(text(a.fees_accrued().unwrap()), "0.01925");
    let a = step(&a, &charge(FeeFamily::Equities, "2026-09-21"), &config);
    assert_eq!(text(a.fees_charged()), "0.02");
    assert_eq!(text(a.cash_total().unwrap()), "98499.98");
}

/// RC-03 (accounting). Buy 5 at 50, sell 5 at 55: realized 275 − 250 = 25, flat. Sell 3 at 55
/// opens a short: Q −3, B −165. Buy 3 at 52 closes: realized −(3 × 52) − (−165) = 9, total 34.
#[test]
fn rc_03_flip_through_flat() {
    let config = no_fees();
    let a = Account::opening(AccountType::Margin, usd("100000"), []);
    let a = step(
        &a,
        &equity(
            "f1",
            "XYZ",
            Side::Buy,
            "5",
            "50",
            "2026-09-21T10:00:00-04:00",
        ),
        &config,
    );
    let a = step(
        &a,
        &equity(
            "f2",
            "XYZ",
            Side::Sell,
            "5",
            "55",
            "2026-09-21T11:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(position(&a, "XYZ"), ("0".into(), "0".into()));
    assert_eq!(text(a.realized_gross()), "25");
    let a = step(
        &a,
        &equity(
            "f3",
            "XYZ",
            Side::Sell,
            "3",
            "55",
            "2026-09-21T11:00:01-04:00",
        ),
        &config,
    );
    assert_eq!(position(&a, "XYZ"), ("-3".into(), "-165".into()));
    let a = step(
        &a,
        &equity(
            "f4",
            "XYZ",
            Side::Buy,
            "3",
            "52",
            "2026-09-21T14:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(position(&a, "XYZ"), ("0".into(), "0".into()));
    assert_eq!(text(a.realized_gross()), "34");
}

/// A single fill that crosses zero is split: long 5 at 50 (B 250), sell 8 at 55 closes 5
/// (realized 275 − 250 = 25) and opens −3 at 55 (B −165). Cash: 100000 − 250 settled, and
/// 8 × 55 = 440 unsettled.
#[test]
fn crossing_fill_is_split_into_close_and_open() {
    let config = no_fees();
    let a = Account::opening(AccountType::Margin, usd("100000"), []);
    let a = step(
        &a,
        &equity(
            "f1",
            "XYZ",
            Side::Buy,
            "5",
            "50",
            "2026-09-21T10:00:00-04:00",
        ),
        &config,
    );
    let applied = a
        .apply(
            &equity(
                "f2",
                "XYZ",
                Side::Sell,
                "8",
                "55",
                "2026-09-21T11:00:00-04:00",
            ),
            &config,
        )
        .unwrap();
    let a = applied.account;
    assert_eq!(position(&a, "XYZ"), ("-3".into(), "-165".into()));
    assert_eq!(text(a.realized_gross()), "25");
    assert_eq!(text(a.settled()), "99750");
    assert_eq!(unsettled(&a), [("2026-09-22".into(), "440".into())]);
    match applied.record {
        Record::Fill {
            received,
            realized_gross,
            trade_date,
            settles_on,
            ..
        } => {
            assert_eq!(text(received), "-8");
            assert_eq!(text(realized_gross), "25");
            assert_eq!(trade_date, Some(d("2026-09-21")));
            assert_eq!(settles_on, Some(d("2026-09-22")));
        }
        other => panic!("{other:?}"),
    }
    let a = step(
        &a,
        &equity(
            "f3",
            "XYZ",
            Side::Buy,
            "7",
            "50",
            "2026-09-21T12:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(position(&a, "XYZ"), ("4".into(), "200".into()));
    assert_eq!(text(a.realized_gross()), "40");
}

/// Short reductions round the removed basis once at 12 places, half-even: B −100 over Q −3; buying
/// 1 removes round(−100 × 1 ÷ 3, 12) = −33.333333333333, leaving −66.666666666667; realized
/// −(1 × 30) − (−33.333333333333) = 3.333333333333.
#[test]
fn reducing_a_short_rounds_the_removed_basis_once() {
    let config = no_fees();
    let short = Position::new(
        SignedQty::parse("-3").unwrap(),
        CostBasis::parse("-100").unwrap(),
    )
    .unwrap();
    let a = Account::opening(AccountType::Margin, usd("0"), [(id("XYZ"), short)]);
    let a = step(
        &a,
        &equity(
            "f1",
            "XYZ",
            Side::Buy,
            "1",
            "30",
            "2026-09-21T10:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(
        position(&a, "XYZ"),
        ("-2".into(), "-66.666666666667".into())
    );
    assert_eq!(text(a.realized_gross()), "3.333333333333");
    let a = step(
        &a,
        &equity(
            "f2",
            "XYZ",
            Side::Buy,
            "2",
            "30",
            "2026-09-21T10:01:00-04:00",
        ),
        &config,
    );
    assert_eq!(position(&a, "XYZ"), ("0".into(), "0".into()));
    assert_eq!(text(a.realized_gross()), "10");
}

/// RC-07 (accounting part). Buy 0.5 at 60000, taker 25 bps: fee_qty round(0.5 × 25 ÷ 10000, 9,
/// half_up) = 0.00125, received 0.49875, basis 29925, fee valued 75 and not accrued; settled
/// 100000 − 30000 = 70000. Sell 0.49875 at 62000: proceeds 30922.5 settled at once, realized
/// 30922.5 − 29925 = 997.5, USD fee round(77.30625, 2, half_up) = 77.31 accrued; equity 100922.5 −
/// 77.31 = 100845.19. Charge (crypto, 2026-09-21): settled 100845.19; Δequity 845.19 = 997.5 −
/// 152.31.
#[test]
fn rc_07_crypto_fees_in_the_received_asset() {
    let config = test_default();
    let crypto = |fill_id, side, qty, price, time| {
        Fill {
            fill_id,
            order: None,
            instrument: "BTCUSD",
            asset_class: AssetClass::Crypto,
            side,
            qty,
            price,
            liquidity: Some(Liquidity::Taker),
            at: time,
        }
        .input()
    };
    let start = Account::opening(AccountType::Margin, usd("100000"), []);
    let applied = start
        .apply(
            &crypto("c1", Side::Buy, "0.5", "60000", "2026-09-21T10:00:00-04:00"),
            &config,
        )
        .unwrap();
    let a = applied.account;
    assert_eq!(position(&a, "BTCUSD"), ("0.49875".into(), "29925".into()));
    assert_eq!(text(a.fees_accrued().unwrap()), "0");
    assert_eq!(text(a.fees_total().unwrap()), "75");
    assert_eq!(text(a.settled()), "70000");
    match applied.record {
        Record::Fill {
            fees,
            trade_date,
            settles_on,
            ..
        } => {
            assert_eq!(fees.len(), 1);
            assert_eq!(fees[0].kind, FeeKind::CryptoAsset);
            assert_eq!(fees[0].asset_qty, Some(Qty::parse("0.00125").unwrap()));
            assert_eq!(text(fees[0].usd), "75");
            assert_eq!((trade_date, settles_on), (None, None));
        }
        other => panic!("{other:?}"),
    }
    let a = step(
        &a,
        &crypto(
            "c2",
            Side::Sell,
            "0.49875",
            "62000",
            "2026-09-21T14:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(position(&a, "BTCUSD"), ("0".into(), "0".into()));
    assert_eq!(text(a.realized_gross()), "997.5");
    assert_eq!(text(a.fees_accrued().unwrap()), "77.31");
    assert_eq!(text(a.fees_total().unwrap()), "152.31");
    assert_eq!(text(a.settled()), "100922.5");
    assert_eq!(text(a.equity().unwrap()), "100845.19");
    let a = step(&a, &charge(FeeFamily::Crypto, "2026-09-21"), &config);
    assert_eq!(text(a.fees_accrued().unwrap()), "0");
    assert_eq!(text(a.settled()), "100845.19");
    assert_eq!(text(a.equity().unwrap()), "100845.19");
    assert_eq!(
        text(
            a.equity()
                .unwrap()
                .checked_sub(start.equity().unwrap())
                .unwrap()
        ),
        "845.19"
    );
}

/// RC-13. Buy 300 at 49: CAT 0.003. Three partial sells of 100 at 50 on one order: each SEC
/// 5000 × 0.00003 = 0.15, TAF min(100 × 0.0002, 0.015) = 0.015 (capped per execution), CAT 0.001:
/// 0.166 each; accrued 0.169, 0.335, 0.501. Realized 3 × (5000 − 4900) = 300. Charge ceil(0.501, 2)
/// = 0.51; net 299.49.
///
/// Capped per order instead: sell 40 (SEC 0.06, TAF 0.008, CAT 0.0004) accrues 0.0714; sell 100 on
/// the same order has TAF room 0.015 − 0.008 = 0.007 (SEC 0.15, CAT 0.001) for 0.2294; sell 100 on
/// a new order pays the full capped 0.015 for 0.3954.
#[test]
fn rc_13_partial_fills_with_the_taf_cap_per_execution() {
    let mut config = test_default();
    config.equities.taf_cap = fee_cap("0.015");
    let fill = |fill_id, order, side, qty, price, time| {
        Fill {
            fill_id,
            order: Some(order),
            instrument: "XYZ",
            asset_class: AssetClass::UsEquity,
            side,
            qty,
            price,
            liquidity: None,
            at: time,
        }
        .input()
    };
    let a = Account::opening(AccountType::Margin, usd("20000"), []);
    let a = step(
        &a,
        &fill(
            "f1",
            "01B-1",
            Side::Buy,
            "300",
            "49",
            "2026-09-21T10:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(text(a.fees_accrued().unwrap()), "0.003");
    let a = step(
        &a,
        &fill(
            "f2",
            "01S-1",
            Side::Sell,
            "100",
            "50",
            "2026-09-21T11:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(text(a.fees_accrued().unwrap()), "0.169");
    let a = step(
        &a,
        &fill(
            "f3",
            "01S-1",
            Side::Sell,
            "100",
            "50",
            "2026-09-21T11:00:01-04:00",
        ),
        &config,
    );
    assert_eq!(text(a.fees_accrued().unwrap()), "0.335");
    let a = step(
        &a,
        &fill(
            "f4",
            "01S-1",
            Side::Sell,
            "100",
            "50",
            "2026-09-21T11:00:02-04:00",
        ),
        &config,
    );
    assert_eq!(text(a.fees_accrued().unwrap()), "0.501");
    assert_eq!(text(a.realized_gross()), "300");
    let a = step(&a, &charge(FeeFamily::Equities, "2026-09-21"), &config);
    assert_eq!(text(a.fees_charged()), "0.51");
    assert_eq!(text(a.realized_net().unwrap()), "299.49");

    config.equities.taf_cap_basis = TafCapBasis::PerOrder;
    let b = Account::opening(AccountType::Margin, usd("20000"), []);
    let b = step(
        &b,
        &fill(
            "f1",
            "01B-1",
            Side::Buy,
            "300",
            "49",
            "2026-09-21T10:00:00-04:00",
        ),
        &config,
    );
    let b = step(
        &b,
        &fill(
            "f2",
            "01S-1",
            Side::Sell,
            "40",
            "50",
            "2026-09-21T11:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(text(b.fees_accrued().unwrap()), "0.0714");
    let b = step(
        &b,
        &fill(
            "f3",
            "01S-1",
            Side::Sell,
            "100",
            "50",
            "2026-09-21T11:00:01-04:00",
        ),
        &config,
    );
    assert_eq!(text(b.fees_accrued().unwrap()), "0.2294");
    let b = step(
        &b,
        &fill(
            "f4",
            "01S-2",
            Side::Sell,
            "100",
            "50",
            "2026-09-21T11:00:02-04:00",
        ),
        &config,
    );
    assert_eq!(text(b.fees_accrued().unwrap()), "0.3954");
}

/// RC-11's dates: an evening sale belongs to the next trading day and settles T+1 on the settlement
/// calendar, skipping the 2026-10-12 and 2026-11-11 bank holidays.
#[test]
fn rc_11_trade_and_settlement_dates() {
    let config = no_fees();
    let long = Position::new(
        SignedQty::parse("4").unwrap(),
        CostBasis::parse("40").unwrap(),
    )
    .unwrap();
    let mut a = Account::opening(AccountType::Margin, usd("0"), [(id("XYZ"), long)]);
    let cases = [
        ("2026-09-21T21:00:00-04:00", "2026-09-22", "2026-09-23"),
        ("2026-10-09T10:00:00-04:00", "2026-10-09", "2026-10-13"),
        ("2026-10-12T10:00:00-04:00", "2026-10-12", "2026-10-13"),
        ("2026-11-10T10:00:00-05:00", "2026-11-10", "2026-11-12"),
    ];
    for (n, (time, trade, settles)) in cases.into_iter().enumerate() {
        let applied = a
            .apply(
                &equity(&format!("f{n}"), "XYZ", Side::Sell, "1", "10", time),
                &config,
            )
            .unwrap();
        match applied.record {
            Record::Fill {
                trade_date,
                settles_on,
                ..
            } => {
                assert_eq!(trade_date, Some(d(trade)), "{time}");
                assert_eq!(settles_on, Some(d(settles)), "{time}");
            }
            other => panic!("{other:?}"),
        }
        a = applied.account;
    }
    assert_eq!(
        unsettled(&a),
        [
            ("2026-09-23".into(), "10".into()),
            ("2026-10-13".into(), "20".into()),
            ("2026-11-12".into(), "10".into()),
        ]
    );
    let a = step(
        &a,
        &Input::SettlementPosted {
            date: d("2026-10-13"),
        },
        &config,
    );
    assert_eq!(unsettled(&a), [("2026-11-12".into(), "10".into())]);
    assert_eq!(text(a.settled()), "30");
}

/// Crypto maker buy of 1 at 100: fee_qty round(1 × 15 ÷ 10000, 9, half_up) = 0.0015, received
/// 0.9985, basis 99.85, asset fee 0.15. Sell fees round half up at cents: 3.3333 × 15 bps =
/// 0.00499995 → 0; 3.34 × 15 bps = 0.00501 → 0.01. The fills are at 03:30 UTC on 2026-09-22, so
/// they accrue to that UTC day, not to the New York date.
#[test]
fn crypto_maker_rates_and_cent_rounding() {
    let config = test_default();
    let crypto = |fill_id, side, qty, price| {
        Fill {
            fill_id,
            order: None,
            instrument: "ETHUSD",
            asset_class: AssetClass::Crypto,
            side,
            qty,
            price,
            liquidity: Some(Liquidity::Maker),
            at: "2026-09-21T23:30:00-04:00",
        }
        .input()
    };
    let a = Account::opening(AccountType::Margin, usd("1000"), []);
    let a = step(&a, &crypto("c1", Side::Buy, "1", "100"), &config);
    assert_eq!(position(&a, "ETHUSD"), ("0.9985".into(), "99.85".into()));
    assert_eq!(text(a.asset_fees()), "0.15");
    let a = step(&a, &crypto("c2", Side::Sell, "0.033333", "100"), &config);
    assert_eq!(text(a.fees_accrued().unwrap()), "0");
    let a = step(&a, &crypto("c3", Side::Sell, "0.0334", "100"), &config);
    assert_eq!(text(a.fees_accrued().unwrap()), "0.01");
    let other_day = step(&a, &charge(FeeFamily::Crypto, "2026-09-21"), &config);
    assert_eq!(
        other_day.fees_accrued().unwrap(),
        Usd::parse("0.01").unwrap()
    );
    assert_eq!(other_day.fees_charged(), Usd::ZERO);
    let a = step(&a, &charge(FeeFamily::Crypto, "2026-09-22"), &config);
    assert_eq!(text(a.fees_accrued().unwrap()), "0");
    assert_eq!(text(a.fees_charged()), "0.01");
}

#[test]
fn rejected_inputs_leave_the_account_unchanged() {
    let config = test_default();
    let a = Account::opening(AccountType::Margin, usd("100"), []);
    let buy = equity(
        "f1",
        "XYZ",
        Side::Buy,
        "1",
        "10",
        "2026-09-21T10:00:00-04:00",
    );
    let a = step(&a, &buy, &config);
    assert_eq!(
        a.apply(&buy, &config),
        Err(AccountingError::DuplicateFill("f1".into()))
    );
    let zero = equity(
        "f2",
        "XYZ",
        Side::Buy,
        "0",
        "10",
        "2026-09-21T10:00:00-04:00",
    );
    assert_eq!(a.apply(&zero, &config), Err(AccountingError::ZeroQuantity));
    let no_liquidity = Fill {
        fill_id: "c1",
        order: None,
        instrument: "BTCUSD",
        asset_class: AssetClass::Crypto,
        side: Side::Buy,
        qty: "1",
        price: "10",
        liquidity: None,
        at: "2026-09-21T10:00:00-04:00",
    }
    .input();
    assert_eq!(
        a.apply(&no_liquidity, &config),
        Err(AccountingError::MissingLiquidity)
    );
    let outside = equity(
        "f3",
        "XYZ",
        Side::Buy,
        "1",
        "10",
        "2027-01-04T10:00:00-05:00",
    );
    assert_eq!(
        a.apply(&outside, &config)
            .map(|x| x.account)
            .unwrap_err()
            .code(),
        "outside_calendar"
    );
    assert_eq!(
        Position::new(SignedQty::ZERO, CostBasis::parse("1").unwrap()),
        Err(AccountingError::InvalidPosition)
    );
    let held = Position::new(
        SignedQty::parse("2").unwrap(),
        CostBasis::parse("20").unwrap(),
    )
    .unwrap();
    let unmarked = Account::opening(AccountType::Margin, usd("0"), [(id("OLD"), held)]);
    assert_eq!(unmarked.equity(), Err(AccountingError::NoMark(id("OLD"))));
    assert_eq!(
        unmarked.unrealized(),
        Err(AccountingError::NoMark(id("OLD")))
    );
    let marked = step(&unmarked, &mark("OLD", "12"), &config);
    assert_eq!(text(marked.unrealized().unwrap()), "4");
    assert_eq!(text(marked.equity().unwrap()), "24");
    let codes = [
        (AccountingError::DuplicateFill("x".into()), "duplicate_fill"),
        (AccountingError::ZeroQuantity, "zero_quantity"),
        (AccountingError::MissingLiquidity, "missing_liquidity"),
        (AccountingError::NoMark(id("x")), "no_mark"),
        (AccountingError::InvalidPosition, "invalid_position"),
        (AccountingError::EmptyInstrumentId, "empty_instrument_id"),
    ];
    for (error, code) in codes {
        assert_eq!(error.code(), code);
    }
    assert_eq!(
        mandate_accounting::InstrumentId::new(""),
        Err(AccountingError::EmptyInstrumentId)
    );
}

/// A later mark is not replaced by a fill price; the fill price is the mark only until the first
/// `MarkUpdated` (spec §8.2).
#[test]
fn fill_prices_mark_only_until_a_mark_arrives() {
    let config = no_fees();
    let a = Account::opening(AccountType::Margin, usd("1000"), []);
    let a = step(
        &a,
        &equity(
            "f1",
            "XYZ",
            Side::Buy,
            "2",
            "10",
            "2026-09-21T10:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(a.mark(&id("XYZ")).map(text), Some("10".into()));
    let a = step(
        &a,
        &equity(
            "f2",
            "XYZ",
            Side::Buy,
            "1",
            "13",
            "2026-09-21T10:01:00-04:00",
        ),
        &config,
    );
    assert_eq!(a.mark(&id("XYZ")).map(text), Some("13".into()));
    assert_eq!(text(a.unrealized().unwrap()), "6");
    let a = step(&a, &mark("XYZ", "11"), &config);
    let a = step(
        &a,
        &equity(
            "f3",
            "XYZ",
            Side::Buy,
            "1",
            "12",
            "2026-09-21T10:02:00-04:00",
        ),
        &config,
    );
    assert_eq!(a.mark(&id("XYZ")).map(text), Some("11".into()));
    assert_eq!(text(a.unrealized().unwrap()), "-1");
}

/// A reduction landing exactly on a tie rounds to even: Q 2, B 1.000000000001; selling 1 removes
/// round(0.5000000000005, 12, half_even) = 0.5 (not 0.500000000001), leaving B 0.500000000001;
/// realized 1 × 1 − 0.5 = 0.5.
#[test]
fn reduction_ties_round_to_even() {
    let config = no_fees();
    let long = Position::new(
        SignedQty::parse("2").unwrap(),
        CostBasis::parse("1.000000000001").unwrap(),
    )
    .unwrap();
    let a = Account::opening(AccountType::Margin, usd("0"), [(id("XYZ"), long)]);
    let a = step(
        &a,
        &equity(
            "f1",
            "XYZ",
            Side::Sell,
            "1",
            "1",
            "2026-09-21T10:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(position(&a, "XYZ"), ("1".into(), "0.500000000001".into()));
    assert_eq!(text(a.realized_gross()), "0.5");
}

/// Open positions are listed in instrument order and a closed one disappears: buy 1 ZZZ and 1 BBB,
/// sell 1 BBB, with AAA held from the opening. AAA has no mark and no fill, so unrealized P&L is
/// an error that names it.
#[test]
fn positions_list_open_instruments_and_errors_name_the_unmarked_one() {
    let config = no_fees();
    let held = Position::new(
        SignedQty::parse("3").unwrap(),
        CostBasis::parse("30").unwrap(),
    )
    .unwrap();
    let mut a = Account::opening(AccountType::Margin, usd("1000"), [(id("AAA"), held)]);
    for (fill, instrument, side) in [
        ("f1", "ZZZ", Side::Buy),
        ("f2", "BBB", Side::Buy),
        ("f3", "BBB", Side::Sell),
    ] {
        a = step(
            &a,
            &equity(
                fill,
                instrument,
                side,
                "1",
                "5",
                "2026-09-21T10:00:00-04:00",
            ),
            &config,
        );
    }
    let listed: Vec<(String, String)> = a
        .positions()
        .map(|(instrument, p)| (instrument.to_string(), text(p.qty())))
        .collect();
    assert_eq!(
        listed,
        [("AAA".into(), "3".into()), ("ZZZ".into(), "1".into())]
    );
    let error = a.unrealized().unwrap_err();
    assert_eq!(error, AccountingError::NoMark(id("AAA")));
    assert_eq!(error.to_string(), "no mark or fill price for AAA");
}

fn build(qty: &str, basis: &str) -> Result<Position, AccountingError> {
    Position::new(
        SignedQty::parse(qty).unwrap(),
        CostBasis::parse(basis).unwrap(),
    )
}

/// A position's basis has the sign of its quantity: a long has paid (B ≥ 0), a short has received
/// (B ≤ 0), and a flat position holds none (DEC-86). A long of 1 with basis −100 would realize
/// 100 − (−100) = 200 on a close at 100, so it cannot be built; neither can a short with a positive
/// basis, a flat position with any basis, or a long whose basis is below zero by 10⁻¹⁸. A zero
/// basis on an open position is allowed: a reduction can leave one (see
/// `a_reduction_never_removes_more_basis_than_the_position_holds`).
#[test]
fn positions_whose_basis_opposes_the_quantity_are_rejected() {
    for (qty, basis) in [
        ("1", "-100"),
        ("-1", "100"),
        ("0", "1"),
        ("0", "-1"),
        ("0.000000001", "-0.000000000000000001"),
    ] {
        assert_eq!(
            build(qty, basis),
            Err(AccountingError::InvalidPosition),
            "{qty} {basis}"
        );
    }
    for (qty, basis) in [
        ("1", "100"),
        ("-1", "-100"),
        ("1", "0"),
        ("-1", "0"),
        ("0", "0"),
    ] {
        let p = build(qty, basis).unwrap();
        assert_eq!((text(p.qty()), text(p.basis())), (qty.into(), basis.into()));
    }
}

/// A reduction never removes more basis than the position holds (DEC-86). Buying 0.00000001 at
/// 0.00009 gives B = 0.0000000000009, below the 12-place grid of the removed basis. Selling
/// 0.000000009 (nine tenths) rounds the removal to round(0.00000000000081, 12, half_even) =
/// 0.000000000001, more than B, so the removal is limited to B: Q 0.000000001, B 0, realized
/// 0.00000000000081 − 0.0000000000009 = −0.00000000000009. Closing the rest at 0.00009 realizes
/// 0.00000000000009 − 0, so the round trip at one price realizes 0 in total. The short mirror
/// (sell, then buy back) has every basis and P&L negated.
#[test]
fn a_reduction_never_removes_more_basis_than_the_position_holds() {
    let config = no_fees();
    for (open, close, opened, left, realized) in [
        (
            Side::Buy,
            Side::Sell,
            ("0.00000001", "0.0000000000009"),
            "0.000000001",
            "-0.00000000000009",
        ),
        (
            Side::Sell,
            Side::Buy,
            ("-0.00000001", "-0.0000000000009"),
            "-0.000000001",
            "0.00000000000009",
        ),
    ] {
        let fill = |fill_id, side, qty, time| equity(fill_id, "XYZ", side, qty, "0.00009", time);
        let a = Account::opening(AccountType::Margin, usd("1"), []);
        let a = step(
            &a,
            &fill("f1", open, "0.00000001", "2026-09-21T10:00:00-04:00"),
            &config,
        );
        assert_eq!(position(&a, "XYZ"), (opened.0.into(), opened.1.into()));
        let a = step(
            &a,
            &fill("f2", close, "0.000000009", "2026-09-21T10:01:00-04:00"),
            &config,
        );
        assert_eq!(position(&a, "XYZ"), (left.into(), "0".into()));
        assert_eq!(text(a.realized_gross()), realized);
        let a = step(
            &a,
            &fill("f3", close, "0.000000001", "2026-09-21T10:02:00-04:00"),
            &config,
        );
        assert_eq!(position(&a, "XYZ"), ("0".into(), "0".into()));
        assert_eq!(text(a.realized_gross()), "0");
    }
}

/// The limit on the removed basis binds only when the rounded removal exceeds the basis held, and
/// then the position ends with zero basis (DEC-86). Each case reduces by selling (or, for the
/// short, buying back) at 1 with no fees; realized = proceeds − removed.
///
/// - Q 20, B 0.000000000000527, sell 19: the exact remaining basis is 0.00000000000002635 (on the
///   long side), but round(0.00000000000050065, 12, half_even) = 0.000000000001 exceeds B, so the
///   removal is B: Q 1, B 0, realized 19 − 0.000000000000527 = 18.999999999999473. The short
///   mirror (Q −20, B −0.000000000000527, buy 19) ends at B 0 with realized −18.999999999999473.
/// - Q 20, B 0.000000000527 (on the 12-place grid), sell 19: removal round(0.00000000050065, 12)
///   = 0.000000000501, B 0.000000000026, realized 19 − 0.000000000501 = 18.999999999499.
/// - Q 2, B 1.0000000000005 (digits below the 12th place), sell 1: removal
///   round(0.50000000000025, 12) = 0.5 does not exceed B, so the rounded formula stands:
///   B 0.5000000000005, realized 0.5.
#[test]
fn the_basis_limit_binds_only_when_the_rounded_removal_exceeds_the_basis_held() {
    let config = no_fees();
    for (qty, basis, side, traded, left, realized) in [
        (
            "20",
            "0.000000000000527",
            Side::Sell,
            "19",
            ("1", "0"),
            "18.999999999999473",
        ),
        (
            "-20",
            "-0.000000000000527",
            Side::Buy,
            "19",
            ("-1", "0"),
            "-18.999999999999473",
        ),
        (
            "20",
            "0.000000000527",
            Side::Sell,
            "19",
            ("1", "0.000000000026"),
            "18.999999999499",
        ),
        (
            "2",
            "1.0000000000005",
            Side::Sell,
            "1",
            ("1", "0.5000000000005"),
            "0.5",
        ),
    ] {
        let a = Account::opening(
            AccountType::Margin,
            usd("0"),
            [(id("XYZ"), build(qty, basis).unwrap())],
        );
        let a = step(
            &a,
            &equity("f1", "XYZ", side, traded, "1", "2026-09-21T10:00:00-04:00"),
            &config,
        );
        assert_eq!(
            position(&a, "XYZ"),
            (left.0.into(), left.1.into()),
            "{qty} {basis}"
        );
        assert_eq!(text(a.realized_gross()), realized, "{qty} {basis}");
    }
}

fn fee_of(record: &Record, kind: FeeKind) -> String {
    match record {
        Record::Fill { fees, .. } => {
            let matching: Vec<String> = fees
                .iter()
                .filter(|f| f.kind == kind)
                .map(|f| text(f.usd))
                .collect();
            assert_eq!(matching.len(), 1, "{fees:?}");
            matching.concat()
        }
        other => panic!("{other:?}"),
    }
}

fn sell_on_order(fill_id: &str, order: &str, price: &str, at: &str) -> Input {
    Fill {
        fill_id,
        order: Some(order),
        instrument: "XYZ",
        asset_class: AssetClass::UsEquity,
        side: Side::Sell,
        qty: "100",
        price,
        liquidity: None,
        at,
    }
    .input()
}

fn capped(cap: &str, basis: TafCapBasis) -> mandate_accounting::Config {
    let mut config = test_default();
    config.equities.taf_cap = fee_cap(cap);
    config.equities.taf_cap_basis = basis;
    config
}

/// Each execution is charged under the fee configuration in force for it (spec §6.1), and per
/// order the TAF room is max(0, cap − TAF already charged on the order) (DEC-87). Four sells of 100
/// at 50 on order `o1`, each with SEC 5000 × 0.00003 = 0.15, CAT 0.001, and uncapped TAF 0.02:
/// cap 0.015 charges 0.015 (accrued 0.166); the cap falls to 0, so the room is max(0, −0.015) = 0
/// and TAF is 0, not −0.015 (accrued 0.317); a cap of 0.01, below the 0.015 already charged, also
/// charges 0 (0.468); the cap rises to 0.025, leaving room 0.01 (0.629). The order's TAF totals
/// 0.025, the highest cap in force.
#[test]
fn per_order_taf_is_never_negative_when_the_cap_falls_within_an_order() {
    let held = build("400", "19600").unwrap();
    let mut a = Account::opening(AccountType::Margin, usd("0"), [(id("XYZ"), held)]);
    for (n, (cap, taf, accrued)) in [
        ("0.015", "0.015", "0.166"),
        ("0", "0", "0.317"),
        ("0.01", "0", "0.468"),
        ("0.025", "0.01", "0.629"),
    ]
    .into_iter()
    .enumerate()
    {
        let sell = sell_on_order(
            &format!("f{n}"),
            "o1",
            "50",
            &format!("2026-09-21T11:00:0{n}-04:00"),
        );
        let applied = a.apply(&sell, &capped(cap, TafCapBasis::PerOrder)).unwrap();
        assert_eq!(fee_of(&applied.record, FeeKind::Taf), taf, "cap {cap}");
        a = applied.account;
        assert_eq!(text(a.fees_accrued().unwrap()), accrued, "cap {cap}");
    }
}

/// A day's charge is never a cash credit. Sell 100 at 0.01 on order `o1` with a per-order cap of
/// 0.015: SEC 1 × 0.00003 = 0.00003, TAF 0.015, CAT 0.001, accrued 0.01603 for 2026-09-21. The next
/// trading day the cap is 0 and a second sell on `o1` has TAF max(0, 0 − 0.015) = 0, so 2026-09-22
/// accrues 0.00103 and its charge is ceil(0.00103, 2) = 0.01, a debit. (With a negative TAF of
/// −0.015 the day would accrue −0.01397 and the charge would credit 0.01.)
#[test]
fn a_daily_charge_is_never_a_credit_when_the_cap_falls() {
    let held = build("200", "100").unwrap();
    let a = Account::opening(AccountType::Margin, usd("100"), [(id("XYZ"), held)]);
    let a = step(
        &a,
        &sell_on_order("f1", "o1", "0.01", "2026-09-21T11:00:00-04:00"),
        &capped("0.015", TafCapBasis::PerOrder),
    );
    assert_eq!(text(a.fees_accrued().unwrap()), "0.01603");
    let config = capped("0", TafCapBasis::PerOrder);
    let a = step(
        &a,
        &sell_on_order("f2", "o1", "0.01", "2026-09-22T11:00:00-04:00"),
        &config,
    );
    assert_eq!(text(a.fees_accrued().unwrap()), "0.01706");
    let applied = a
        .apply(&charge(FeeFamily::Equities, "2026-09-22"), &config)
        .unwrap();
    assert_eq!(
        applied.record,
        Record::FeesCharged {
            accrued: usd("0.00103"),
            charged: usd("0.01"),
        }
    );
    assert_eq!(text(applied.account.settled()), "99.99");
}

/// The per-order room counts all TAF already charged on the order, whichever mode charged it
/// (DEC-87). Sell 100 at 50 on `o1` per execution with cap 0.015: TAF 0.015. The configuration
/// switches to per order with cap 0.015: room max(0, 0.015 − 0.015) = 0, TAF 0. Cap 0.02 per
/// order: room 0.005, TAF 0.005. The order's TAF totals 0.02, the highest cap in force.
#[test]
fn per_order_taf_room_counts_executions_charged_per_execution() {
    let held = build("300", "14700").unwrap();
    let mut a = Account::opening(AccountType::Margin, usd("0"), [(id("XYZ"), held)]);
    for (n, (cap, basis, taf)) in [
        ("0.015", TafCapBasis::PerExecution, "0.015"),
        ("0.015", TafCapBasis::PerOrder, "0"),
        ("0.02", TafCapBasis::PerOrder, "0.005"),
    ]
    .into_iter()
    .enumerate()
    {
        let sell = sell_on_order(
            &format!("f{n}"),
            "o1",
            "50",
            &format!("2026-09-21T11:00:0{n}-04:00"),
        );
        let applied = a.apply(&sell, &capped(cap, basis)).unwrap();
        assert_eq!(
            fee_of(&applied.record, FeeKind::Taf),
            taf,
            "{cap} {basis:?}"
        );
        a = applied.account;
    }
}

/// Closing a position at its average cost realizes nothing: a long of 1 with B 100 sold at 100
/// realizes −(−1 × 100) − 100 = 0, and a short of 1 with B −100 bought back at 100 realizes
/// −(1 × 100) − (−100) = 0. Both end flat with zero basis.
#[test]
fn closing_at_the_average_cost_realizes_nothing() {
    let config = no_fees();
    for (qty, basis, side) in [("1", "100", Side::Sell), ("-1", "-100", Side::Buy)] {
        let a = Account::opening(
            AccountType::Margin,
            usd("1000"),
            [(id("XYZ"), build(qty, basis).unwrap())],
        );
        let a = step(
            &a,
            &equity("f1", "XYZ", side, "1", "100", "2026-09-21T10:00:00-04:00"),
            &config,
        );
        assert_eq!(position(&a, "XYZ"), ("0".into(), "0".into()), "{qty}");
        assert_eq!(text(a.realized_gross()), "0", "{qty}");
    }
}

fn taf(record: &Record) -> Vec<String> {
    match record {
        Record::Fill { fees, .. } => fees
            .iter()
            .filter(|f| f.kind == FeeKind::Taf)
            .map(|f| text(f.usd))
            .collect(),
        other => panic!("{other:?}"),
    }
}

/// Both TAF cap modes follow one rule: TAF = min(rate × shares, cap − TAF already charged on the
/// order), where only per-order mode counts earlier executions of the same order (DEC-87). Selling
/// 100 shares at 50 at 0.0002 per share is 0.02 uncapped: a zero cap charges 0, a cap of 0.015
/// charges 0.015, and 9.79 charges 0.02, in both modes, with a client order ID (`o1`) and without
/// one. A second execution of `o1` pays the same again per execution; per order it pays only what
/// is left: 0 − 0 = 0, 0.015 − 0.015 = 0, and min(0.02, 9.79 − 0.02) = 0.02.
#[test]
fn both_taf_cap_modes_charge_one_execution_orders_alike() {
    let sell = |fill_id, order| {
        Fill {
            fill_id,
            order,
            instrument: "XYZ",
            asset_class: AssetClass::UsEquity,
            side: Side::Sell,
            qty: "100",
            price: "50",
            liquidity: None,
            at: "2026-09-21T11:00:00-04:00",
        }
        .input()
    };
    for (cap, capped, per_order_again) in [
        ("0", "0", "0"),
        ("0.015", "0.015", "0"),
        ("9.79", "0.02", "0.02"),
    ] {
        for (basis, again) in [
            (TafCapBasis::PerExecution, capped),
            (TafCapBasis::PerOrder, per_order_again),
        ] {
            let mut config = test_default();
            config.equities.taf_cap = fee_cap(cap);
            config.equities.taf_cap_basis = basis;
            let held = build("300", "14700").unwrap();
            let a = Account::opening(AccountType::Margin, usd("0"), [(id("XYZ"), held)]);
            let first = a.apply(&sell("f1", Some("o1")), &config).unwrap();
            assert_eq!(taf(&first.record), [capped], "{cap} {basis:?} f1");
            let second = first.account.apply(&sell("f2", None), &config).unwrap();
            assert_eq!(taf(&second.record), [capped], "{cap} {basis:?} f2");
            let third = second
                .account
                .apply(&sell("f3", Some("o1")), &config)
                .unwrap();
            assert_eq!(taf(&third.record), [again], "{cap} {basis:?} f3");
        }
    }
}
