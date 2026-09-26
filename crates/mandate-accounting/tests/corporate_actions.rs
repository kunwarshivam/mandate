//! Hand-calculated corporate-action cases (trading-domain spec §8.5, DEC-92 to DEC-95). Each
//! expected value is worked out by hand in its doc comment from the spec's formulas, not produced
//! by running the code.

mod common;

use common::{
    at, cash_in_lieu_posted, d, dividend, dividend_paid, equity, holding, id, mark, no_fees, split,
    step, text, usd,
};
use mandate_accounting::{
    Account, AccountingError, CashDividend, Input, Position, Receivable, ReceivableKind, Record,
    Side,
};
use mandate_num::{CostBasis, NumError, Price, ShareIncrement, SignedQty, Usd};

const FRACTIONAL: ShareIncrement = ShareIncrement::Fractional;
const WHOLE: ShareIncrement = ShareIncrement::Whole;

fn position(account: &Account, instrument: &str) -> (String, String) {
    let p = account.position(&id(instrument));
    (text(p.qty()), text(p.basis()))
}

fn mark_text(account: &Account, instrument: &str) -> Option<String> {
    account.mark(&id(instrument)).map(text)
}

fn record(account: &Account, input: &Input) -> Record {
    account.apply(input, &no_fees()).unwrap().record
}

fn rejected(account: &Account, input: &Input) -> AccountingError {
    account.apply(input, &no_fees()).unwrap_err()
}

fn qty(s: &str) -> SignedQty {
    SignedQty::parse(s).unwrap()
}

fn basis(s: &str) -> CostBasis {
    CostBasis::parse(s).unwrap()
}

fn split_record(before: &str, after: &str, residual: &str, cil: &str, realized: &str) -> Record {
    Record::Split {
        before: qty(before),
        after: qty(after),
        residual_basis: basis(residual),
        cash_in_lieu: usd(cil),
        realized_gross: usd(realized),
    }
}

fn cash_in_lieu(instrument: &str, ex_date: &str, amount: &str) -> Receivable {
    Receivable {
        instrument: id(instrument),
        ex_date: d(ex_date),
        kind: ReceivableKind::CashInLieu,
        amount: usd(amount),
    }
}

fn dividend_due(instrument: &str, ex_date: &str, pay_date: &str, amount: &str) -> Receivable {
    Receivable {
        instrument: id(instrument),
        ex_date: d(ex_date),
        kind: ReceivableKind::Dividend {
            pay_date: d(pay_date),
        },
        amount: usd(amount),
    }
}

/// Δequity = Δrealized + Δunrealized + Δincome − Δfees (I1).
fn conserves(before: &Account, after: &Account) {
    let delta = |f: fn(&Account) -> Usd| f(after).checked_sub(f(before)).unwrap();
    let equity = delta(|a| a.equity().unwrap());
    let parts = delta(Account::realized_gross)
        .checked_add(delta(|a| a.unrealized().unwrap()))
        .unwrap()
        .checked_add(delta(Account::income))
        .unwrap()
        .checked_sub(delta(|a| a.fees_total().unwrap()))
        .unwrap();
    assert_eq!(text(equity), text(parts));
}

/// RC-04 accounting. Q 10, B 4000, mark 400: equity 0 + 4000. A 4:1 split on fractionable ABC:
/// Q' = 10 × 4 ÷ 1 = 40 exactly, f = 0, so R = 0 and B' = 4000 (average 100); mark' =
/// 400 × 1 ÷ 4 = 100; MV 40 × 100 = 4000, equity 4000, realized 0. A later mark of 100 leaves
/// equity 4000 and unrealized 40 × 100 − 4000 = 0.
#[test]
fn rc_04_forward_split_multiplies_quantity_and_divides_the_mark() {
    let config = no_fees();
    let a = holding("0", "ABC", "10", "4000");
    let a = step(&a, &mark("ABC", "400"), &config);
    assert_eq!(text(a.equity().unwrap()), "4000");

    let action = split("ABC", "2026-09-22", (4, 1), FRACTIONAL, None);
    assert_eq!(record(&a, &action), split_record("10", "40", "0", "0", "0"));
    let b = step(&a, &action, &config);
    assert_eq!(position(&b, "ABC"), ("40".into(), "4000".into()));
    assert_eq!(mark_text(&b, "ABC"), Some("100".into()));
    assert_eq!(text(b.equity().unwrap()), "4000");
    assert_eq!(text(b.realized_gross()), "0");
    assert!(b.receivables().is_empty());
    conserves(&a, &b);

    let c = step(&b, &mark("ABC", "100"), &config);
    assert_eq!(text(c.equity().unwrap()), "4000");
    assert_eq!(text(c.unrealized().unwrap()), "0");
}

/// RC-05. Q 25, B 50, mark 2 on non-fractionable LOW: equity 50. A 1:10 reverse split:
/// Q_raw = 2.5, Q' = 2, f = 0.5; R = round(50 × 0.5 ÷ 2.5, 12) = 10, B' = 40 (average 20);
/// cash in lieu round(0.5 × 19, 2) = 9.50 receivable; realized 9.50 − 10 = −0.50; mark' =
/// 2 × 10 ÷ 1 = 20; equity 0 + 9.50 + 2 × 20 = 49.50. The broker's posting of 9.50 settles it:
/// receivables 0, settled 9.50, equity 49.50.
#[test]
fn rc_05_reverse_split_with_cash_in_lieu_then_the_posting() {
    let config = no_fees();
    let a = holding("0", "LOW", "25", "50");
    let a = step(&a, &mark("LOW", "2"), &config);
    assert_eq!(text(a.equity().unwrap()), "50");

    let action = split("LOW", "2026-09-22", (1, 10), WHOLE, Some("19"));
    assert_eq!(
        record(&a, &action),
        split_record("25", "2", "10", "9.5", "-0.5")
    );
    let b = step(&a, &action, &config);
    assert_eq!(position(&b, "LOW"), ("2".into(), "40".into()));
    assert_eq!(mark_text(&b, "LOW"), Some("20".into()));
    assert_eq!(b.receivables(), [cash_in_lieu("LOW", "2026-09-22", "9.5")]);
    assert_eq!(text(b.net_receivables().unwrap()), "9.5");
    assert_eq!(text(b.realized_gross()), "-0.5");
    assert_eq!(text(b.equity().unwrap()), "49.5");
    conserves(&a, &b);

    let posting = cash_in_lieu_posted("LOW", "9.5");
    assert_eq!(
        record(&b, &posting),
        Record::CashInLieuPosted { amount: usd("9.5") }
    );
    let c = step(&b, &posting, &config);
    assert!(c.receivables().is_empty());
    assert_eq!(text(c.net_receivables().unwrap()), "0");
    assert_eq!(text(c.settled()), "9.5");
    assert_eq!(text(c.equity().unwrap()), "49.5");
    conserves(&b, &c);
}

/// RC-06. Q 100, B 5000, settled 5000, mark 50: equity 10000. A 0.25 dividend: 100 × 0.25 = 25.00
/// receivable and income from the ex-date, realized 0; equity 5000 + 25 + 5000 = 10025 until the
/// mark falls. Mark 49.75: MV 4975, equity 5000 + 25 + 4975 = 10000, unrealized 4975 − 5000 = −25,
/// total P&L 0 − 25 + 25 = 0. `DividendPaid` is due at 00:00 ET on 2026-10-01 and not a second
/// before; it moves 25 to settled: 5025, receivables 0, equity 10000.
#[test]
fn rc_06_long_dividend_is_income_on_the_ex_date_and_cash_on_the_pay_date() {
    let config = no_fees();
    let a = holding("5000", "DIV", "100", "5000");
    let a = step(&a, &mark("DIV", "50"), &config);
    assert_eq!(text(a.equity().unwrap()), "10000");

    let action = dividend("DIV", "2026-09-22", "2026-10-01", "0.25");
    assert_eq!(
        record(&a, &action),
        Record::CashDividend {
            entitlement: qty("100"),
            amount: usd("25")
        }
    );
    let b = step(&a, &action, &config);
    assert_eq!(
        b.receivables(),
        [dividend_due("DIV", "2026-09-22", "2026-10-01", "25")]
    );
    assert_eq!(text(b.net_receivables().unwrap()), "25");
    assert_eq!(text(b.income()), "25");
    assert_eq!(text(b.realized_gross()), "0");
    assert_eq!(text(b.equity().unwrap()), "10025");
    conserves(&a, &b);

    let c = step(&b, &mark("DIV", "49.75"), &config);
    assert_eq!(text(c.equity().unwrap()), "10000");
    assert_eq!(text(c.unrealized().unwrap()), "-25");
    assert_eq!(text(c.total_pnl().unwrap()), "0");

    assert!(c.due(at("2026-09-30T23:59:59-04:00")).unwrap().is_empty());
    let due = c.due(at("2026-10-01T00:00:00-04:00")).unwrap();
    assert_eq!(due, [dividend_paid("DIV", "2026-09-22")]);
    assert_eq!(
        record(&c, &due[0]),
        Record::DividendPaid { amount: usd("25") }
    );
    let e = step(&c, &due[0], &config);
    assert!(e.receivables().is_empty());
    assert_eq!(text(e.settled()), "5025");
    assert_eq!(text(e.equity().unwrap()), "10000");
    assert_eq!(text(e.income()), "25");
    conserves(&c, &e);
    assert!(e.due(at("2026-10-01T00:00:00-04:00")).unwrap().is_empty());
}

/// RC-06 short_position_generic_broker. Q −100, B −5000, settled 15000, mark 50: equity
/// 15000 − 5000 = 10000. The dividend is payable: −100 × 0.25 = −25.00, income −25. Mark 49.75:
/// equity 15000 − 25 − 4975 = 10000, unrealized −4975 + 5000 = 25. On the pay date settled
/// 15000 − 25 = 14975, equity 10000.
#[test]
fn rc_06_short_dividend_is_a_payable() {
    let config = no_fees();
    let a = holding("15000", "DIV", "-100", "-5000");
    let a = step(&a, &mark("DIV", "50"), &config);
    assert_eq!(text(a.equity().unwrap()), "10000");

    let b = step(
        &a,
        &dividend("DIV", "2026-09-22", "2026-10-01", "0.25"),
        &config,
    );
    assert_eq!(
        b.receivables(),
        [dividend_due("DIV", "2026-09-22", "2026-10-01", "-25")]
    );
    assert_eq!(text(b.net_receivables().unwrap()), "-25");
    assert_eq!(text(b.income()), "-25");
    conserves(&a, &b);

    let c = step(&b, &mark("DIV", "49.75"), &config);
    assert_eq!(text(c.equity().unwrap()), "10000");
    assert_eq!(text(c.unrealized().unwrap()), "25");

    let e = step(&c, &dividend_paid("DIV", "2026-09-22"), &config);
    assert!(e.receivables().is_empty());
    assert_eq!(text(e.settled()), "14975");
    assert_eq!(text(e.equity().unwrap()), "10000");
}

/// RC-23. Q 10, B 100, mark 30 on fractionable LOWF: equity 300, unrealized 200. A 1:3 reverse
/// split: Q_raw = 10 ÷ 3 = 3.333…, Q' = 3.333333333, f = 10⁻⁹ ÷ 3; R = round(100 × f ÷ Q_raw, 12)
/// = round(100 × 10⁻¹⁰, 12) = 0.00000001; B' = 99.99999999; no cash in lieu price, so realized
/// −0.00000001; mark' = 30 × 3 = 90; MV 3.333333333 × 90 = 299.99999997; unrealized
/// 299.99999997 − 99.99999999 = 199.99999998; Δequity −0.00000003 = −0.00000001 − 0.00000002.
#[test]
fn rc_23_fractionable_residual_removes_its_basis() {
    let config = no_fees();
    let a = holding("0", "LOWF", "10", "100");
    let a = step(&a, &mark("LOWF", "30"), &config);
    assert_eq!(text(a.equity().unwrap()), "300");
    assert_eq!(text(a.unrealized().unwrap()), "200");

    let action = split("LOWF", "2026-09-22", (1, 3), FRACTIONAL, None);
    assert_eq!(
        record(&a, &action),
        split_record("10", "3.333333333", "0.00000001", "0", "-0.00000001")
    );
    let b = step(&a, &action, &config);
    assert_eq!(
        position(&b, "LOWF"),
        ("3.333333333".into(), "99.99999999".into())
    );
    assert_eq!(mark_text(&b, "LOWF"), Some("90".into()));
    assert_eq!(text(b.realized_gross()), "-0.00000001");
    assert_eq!(text(b.unrealized().unwrap()), "199.99999998");
    assert_eq!(text(b.equity().unwrap()), "299.99999997");
    assert!(b.receivables().is_empty());
    conserves(&a, &b);
}

/// RC-23 forward_3_for_1_non_terminating_mark. Q 10, B 1000, mark 100: a 3:1 split gives Q' 30,
/// B' 1000, mark' = round(100 ÷ 3, 12, half_even) = 33.333333333333; MV 30 × 33.333333333333 =
/// 999.99999999999, equity 999.99999999999, realized 0. The 10⁻¹¹ lost to the mark is within
/// I3's bound |Q_raw| × 5 × 10⁻¹³ = 1.5 × 10⁻¹¹.
#[test]
fn rc_23_forward_split_with_a_non_terminating_mark() {
    let config = no_fees();
    let a = holding("0", "LOWF", "10", "1000");
    let a = step(&a, &mark("LOWF", "100"), &config);
    let b = step(
        &a,
        &split("LOWF", "2026-09-22", (3, 1), FRACTIONAL, None),
        &config,
    );
    assert_eq!(position(&b, "LOWF"), ("30".into(), "1000".into()));
    assert_eq!(mark_text(&b, "LOWF"), Some("33.333333333333".into()));
    assert_eq!(text(b.equity().unwrap()), "999.99999999999");
    assert_eq!(text(b.realized_gross()), "0");
    conserves(&a, &b);
}

/// DEC-94. Q 1 on fractionable ODD marked 1.000000001, 16:3: Q_raw = 16 ÷ 3, Q' = 5.333333333,
/// r = Q·new − Q'·old = 16 − 15.999999999 = 0.000000001; mark × 3 ÷ 16 = 0.1875000001875, a tie,
/// so mark' = 0.187500000188; MV' = 5.333333333 × 0.187500000188 = 1.000000000940166666604, ΔMV =
/// −0.000000000059833333396. Times old: 3 × ΔMV + r × mark' = −0.000000000179500000188 +
/// 0.000000000187500000188 = 0.000000000008 = Q·new × 5 × 10⁻¹³, the corrected bound exactly,
/// above the spec's |Q'| × old × 5 × 10⁻¹³ = 0.0000000000079999999995.
#[test]
fn i3_is_bounded_by_the_raw_quantity_not_the_split_quantity() {
    let config = no_fees();
    let a = holding("0", "ODD", "1", "1");
    let a = step(&a, &mark("ODD", "1.000000001"), &config);
    let b = step(
        &a,
        &split("ODD", "2026-09-22", (16, 3), FRACTIONAL, None),
        &config,
    );
    assert_eq!(position(&b, "ODD").0, "5.333333333");
    let adjusted = b.mark(&id("ODD")).unwrap();
    assert_eq!(text(adjusted), "0.187500000188");
    assert_eq!(text(b.market_value().unwrap()), "1.000000000940166666604");
    let delta = b
        .market_value()
        .unwrap()
        .checked_sub(a.market_value().unwrap())
        .unwrap();
    assert_eq!(text(delta), "-0.000000000059833333396");
    let residual_value = qty("0.000000001").value_at_mark(adjusted).unwrap();
    let lhs = [delta, delta, delta, residual_value]
        .into_iter()
        .try_fold(Usd::ZERO, Usd::checked_add)
        .unwrap();
    assert_eq!(text(lhs), "0.000000000008");
    assert!(lhs > usd("0.0000000000079999999995"));
}

/// A short reverse split. Q −25, B −50, 1:10 on non-fractionable LOW with 19 per new share:
/// Q_raw −2.5, Q' −2 (toward zero), f −0.5; R = round(−50 × −0.5 ÷ −2.5, 12) = −10, B' = −40;
/// cash in lieu round(−0.5 × 19, 2) = −9.50, a payable; realized −9.50 − (−10) = 0.50. The broker's
/// posting of −9.50 settles it: settled 1000 − 9.50 = 990.50.
#[test]
fn a_short_reverse_split_owes_cash_in_lieu() {
    let config = no_fees();
    let a = holding("1000", "LOW", "-25", "-50");
    let a = step(&a, &mark("LOW", "2"), &config);
    let action = split("LOW", "2026-09-22", (1, 10), WHOLE, Some("19"));
    assert_eq!(
        record(&a, &action),
        split_record("-25", "-2", "-10", "-9.5", "0.5")
    );
    let b = step(&a, &action, &config);
    assert_eq!(position(&b, "LOW"), ("-2".into(), "-40".into()));
    assert_eq!(b.receivables(), [cash_in_lieu("LOW", "2026-09-22", "-9.5")]);
    assert_eq!(text(b.realized_gross()), "0.5");
    conserves(&a, &b);

    assert_eq!(
        rejected(&b, &cash_in_lieu_posted("LOW", "9.5")).code(),
        "cash_in_lieu_mismatch"
    );
    let c = step(&b, &cash_in_lieu_posted("LOW", "-9.5"), &config);
    assert!(c.receivables().is_empty());
    assert_eq!(text(c.settled()), "990.5");
    conserves(&b, &c);
}

/// DEC-92. One share of non-fractionable ODD, B 7, 1:2 with 10 per new share: Q_raw 0.5, Q' 0.
/// No share remains, so the whole basis is removed: R = B = 7; cash in lieu round(0.5 × 10, 2) =
/// 5.00; realized 5 − 7 = −2; the position is flat and gone, and the mark still adjusts: 4 × 2 = 8.
#[test]
fn a_split_leaving_no_share_removes_the_whole_basis() {
    let config = no_fees();
    let a = holding("0", "ODD", "1", "7");
    let a = step(&a, &mark("ODD", "4"), &config);
    let action = split("ODD", "2026-09-22", (1, 2), WHOLE, Some("10"));
    assert_eq!(record(&a, &action), split_record("1", "0", "7", "5", "-2"));
    let b = step(&a, &action, &config);
    assert_eq!(position(&b, "ODD"), ("0".into(), "0".into()));
    assert_eq!(b.positions().count(), 0);
    assert_eq!(mark_text(&b, "ODD"), Some("8".into()));
    assert_eq!(text(b.realized_gross()), "-2");
    assert_eq!(b.receivables(), [cash_in_lieu("ODD", "2026-09-22", "5")]);
    conserves(&a, &b);
}

/// DEC-92, the dust cases the spec's formula gets wrong. Q 0.000000001 on fractionable DUST,
/// 1:2: Q_raw 5 × 10⁻¹⁰, Q' 0. With B = 5.27 × 10⁻¹⁶, round(B, 12) = 0 would leave a flat position
/// holding basis; with B = 5.27 × 10⁻¹³, round(B, 12) = 10⁻¹² > B would leave basis of the wrong
/// sign. The whole basis is removed instead, and realized is −B (no cash in lieu price).
#[test]
fn dust_splits_remove_exactly_the_basis_held() {
    let config = no_fees();
    for b in ["0.000000000000000527", "0.000000000000527"] {
        let a = holding("0", "DUST", "0.000000001", b);
        let a = step(&a, &mark("DUST", "3"), &config);
        let action = split("DUST", "2026-09-22", (1, 2), FRACTIONAL, None);
        let negated = format!("-{b}");
        assert_eq!(
            record(&a, &action),
            split_record("0.000000001", "0", b, "0", &negated)
        );
        let after = step(&a, &action, &config);
        assert_eq!(position(&after, "DUST"), ("0".into(), "0".into()));
        assert_eq!(text(after.realized_gross()), negated);
        assert!(after.receivables().is_empty());
        conserves(&a, &after);
    }
}

/// DEC-93. Cash in lieu rounds half-even to cents: Q 15 non-fractionable, 1:10 leaves f = 0.5;
/// at 0.25 per new share 0.125 → 0.12; at 0.75, 0.375 → 0.38. At a price where f × price rounds to
/// 0 no receivable is recorded; a split with no residual records none even when a price is given.
#[test]
fn cash_in_lieu_rounds_half_even_to_cents() {
    let config = no_fees();
    let a = holding("0", "LOW", "15", "30");
    let a = step(&a, &mark("LOW", "0.1"), &config);
    for (price, cil) in [("0.25", "0.12"), ("0.75", "0.38")] {
        let b = step(
            &a,
            &split("LOW", "2026-09-22", (1, 10), WHOLE, Some(price)),
            &config,
        );
        assert_eq!(b.receivables(), [cash_in_lieu("LOW", "2026-09-22", cil)]);
    }
    let b = step(
        &a,
        &split("LOW", "2026-09-22", (1, 10), WHOLE, Some("0.009")),
        &config,
    );
    assert!(b.receivables().is_empty());
    let b = step(
        &a,
        &split("LOW", "2026-09-22", (2, 1), WHOLE, Some("5")),
        &config,
    );
    assert!(b.receivables().is_empty());
    assert_eq!(position(&b, "LOW"), ("30".into(), "30".into()));
}

/// DEC-95. Dividend amounts round half-even to cents: 7 × 0.125 = 0.875 → 0.88; 5 × 0.125 =
/// 0.625 → 0.62. A flat instrument's dividend is zero, records no receivable, and still counts as
/// applied, so a second delivery is a duplicate.
#[test]
fn dividends_round_half_even_to_cents_and_flat_holders_get_none() {
    let config = no_fees();
    for (q, amount) in [("7", "0.88"), ("5", "0.62")] {
        let a = holding("0", "DIV", q, "10");
        let b = step(
            &a,
            &dividend("DIV", "2026-09-22", "2026-10-01", "0.125"),
            &config,
        );
        assert_eq!(text(b.income()), amount);
        assert_eq!(
            b.receivables(),
            [dividend_due("DIV", "2026-09-22", "2026-10-01", amount)]
        );
    }
    let flat = Account::opening(usd("100"), []);
    let action = dividend("DIV", "2026-09-22", "2026-10-01", "0.125");
    assert_eq!(
        record(&flat, &action),
        Record::CashDividend {
            entitlement: qty("0"),
            amount: usd("0")
        }
    );
    let b = step(&flat, &action, &config);
    assert!(b.receivables().is_empty());
    assert_eq!(text(b.income()), "0");
    assert_eq!(rejected(&b, &action).code(), "duplicate_corporate_action");
}

/// DEC-95. A split adjusts every stored mark: a last fill price of 30 with no `MarkUpdated`
/// becomes 30 ÷ 3 = 10 after a 3:1 split; a flat instrument with a mark of 12 has its mark halved
/// by a 2:1 split and records a split of nothing.
#[test]
fn splits_adjust_last_fill_prices_and_flat_marks() {
    let config = no_fees();
    let a = Account::opening(usd("1000"), []);
    let a = step(
        &a,
        &equity(
            "f1",
            "XYZ",
            Side::Buy,
            "3",
            "30",
            "2026-09-21T10:00:00-04:00",
        ),
        &config,
    );
    let b = step(
        &a,
        &split("XYZ", "2026-09-22", (3, 1), FRACTIONAL, None),
        &config,
    );
    assert_eq!(position(&b, "XYZ"), ("9".into(), "90".into()));
    assert_eq!(mark_text(&b, "XYZ"), Some("10".into()));
    assert_eq!(text(b.market_value().unwrap()), "90");

    let flat = step(&Account::opening(usd("0"), []), &mark("FLT", "12"), &config);
    let action = split("FLT", "2026-09-22", (2, 1), FRACTIONAL, None);
    assert_eq!(
        record(&flat, &action),
        split_record("0", "0", "0", "0", "0")
    );
    let after = step(&flat, &action, &config);
    assert_eq!(mark_text(&after, "FLT"), Some("6".into()));
}

/// DEC-95. A split whose adjusted mark rounds to zero at 12 places is rejected: 0.000000001 ÷
/// 10000 = 10⁻¹³ → 0.
#[test]
fn a_split_whose_mark_rounds_to_zero_is_rejected() {
    let a = holding("0", "PNY", "1", "1");
    let a = step(&a, &mark("PNY", "0.000000001"), &no_fees());
    assert_eq!(
        rejected(
            &a,
            &split("PNY", "2026-09-22", (10000, 1), FRACTIONAL, None)
        ),
        AccountingError::Num(NumError::NotPositive)
    );
}

/// DEC-95. Ordering: an action on or before the trade date of an applied fill, or before an
/// applied action's ex-date, is out of order; the same action twice is a duplicate, while a split
/// and a dividend may share an ex-date; a fill traded before an applied action's ex-date is
/// rejected. Each rejection leaves the account as it was.
#[test]
fn corporate_actions_and_fills_are_ordered_by_ex_date() {
    let config = no_fees();
    let a = holding("1000", "ABC", "10", "100");
    let filled = step(
        &a,
        &equity(
            "f1",
            "ABC",
            Side::Buy,
            "1",
            "10",
            "2026-09-22T10:00:00-04:00",
        ),
        &config,
    );
    let on_trade_date = split("ABC", "2026-09-22", (2, 1), FRACTIONAL, None);
    let error = rejected(&filled, &on_trade_date);
    assert_eq!(error, AccountingError::CorporateActionOutOfOrder(id("ABC")));
    assert_eq!(error.code(), "corporate_action_out_of_order");
    step(
        &filled,
        &split("ABC", "2026-09-23", (2, 1), FRACTIONAL, None),
        &config,
    );

    let a = step(&a, &on_trade_date, &config);
    assert_eq!(
        rejected(&a, &on_trade_date),
        AccountingError::DuplicateCorporateAction(id("ABC"))
    );
    let earlier = dividend("ABC", "2026-09-21", "2026-09-30", "1");
    assert_eq!(
        rejected(&a, &earlier),
        AccountingError::CorporateActionOutOfOrder(id("ABC"))
    );
    let same_day = step(
        &a,
        &dividend("ABC", "2026-09-22", "2026-09-30", "1"),
        &config,
    );
    assert_eq!(text(same_day.income()), "20");
    step(
        &a,
        &split("OTHER", "2026-09-21", (2, 1), FRACTIONAL, None),
        &config,
    );

    let late = equity(
        "late",
        "ABC",
        Side::Buy,
        "1",
        "10",
        "2026-09-21T15:00:00-04:00",
    );
    let error = rejected(&a, &late);
    assert_eq!(
        error,
        AccountingError::FillBeforeCorporateAction("late".into())
    );
    assert_eq!(error.code(), "fill_before_corporate_action");
    let on_ex_date = equity(
        "f2",
        "ABC",
        Side::Buy,
        "1",
        "5",
        "2026-09-22T09:30:00-04:00",
    );
    assert_eq!(
        position(&step(&a, &on_ex_date, &config), "ABC"),
        ("21".into(), "105".into())
    );
}

/// DEC-93, DEC-95. Settling what is not outstanding is rejected: a `DividendPaid` for no dividend,
/// or a second one; a cash in lieu posting of a different amount, for another instrument, or a
/// second time.
#[test]
fn postings_settle_only_what_is_outstanding() {
    let config = no_fees();
    let a = holding("0", "LOW", "25", "50");
    let error = rejected(&a, &dividend_paid("LOW", "2026-09-22"));
    assert_eq!(error, AccountingError::NoDividendDue(id("LOW")));
    assert_eq!(error.code(), "no_dividend_due");

    let paid = step(
        &a,
        &dividend("LOW", "2026-09-22", "2026-09-25", "0.1"),
        &config,
    );
    let paid = step(&paid, &dividend_paid("LOW", "2026-09-22"), &config);
    assert_eq!(text(paid.settled()), "2.5");
    assert_eq!(
        rejected(&paid, &dividend_paid("LOW", "2026-09-22")),
        AccountingError::NoDividendDue(id("LOW"))
    );

    let b = step(
        &a,
        &split("LOW", "2026-09-22", (1, 10), WHOLE, Some("19")),
        &config,
    );
    for wrong in [
        cash_in_lieu_posted("LOW", "9.49"),
        cash_in_lieu_posted("HIGH", "9.5"),
    ] {
        let error = rejected(&b, &wrong);
        assert_eq!(error.code(), "cash_in_lieu_mismatch");
    }
    let c = step(&b, &cash_in_lieu_posted("LOW", "9.5"), &config);
    assert_eq!(
        rejected(&c, &cash_in_lieu_posted("LOW", "9.5")),
        AccountingError::CashInLieuMismatch(id("LOW"))
    );
}

/// DEC-93. A posting settles the earliest outstanding cash in lieu, by ex-date, whose amount it
/// equals. 25 → 2 at 1:10 with 19 per new share leaves f 0.5: 9.50. Then 2 → 0 at 1:3 with 30
/// leaves f 2 ÷ 3: 20.00, or at 1:4 with 19 leaves f 0.5: 9.50 again.
#[test]
fn a_cash_in_lieu_posting_settles_the_earliest_matching_amount() {
    let config = no_fees();
    let a = holding("0", "LOW", "25", "50");
    let a = step(
        &a,
        &split("LOW", "2026-09-22", (1, 10), WHOLE, Some("19")),
        &config,
    );

    let b = step(
        &a,
        &split("LOW", "2026-09-23", (1, 3), WHOLE, Some("30")),
        &config,
    );
    assert_eq!(
        b.receivables(),
        [
            cash_in_lieu("LOW", "2026-09-22", "9.5"),
            cash_in_lieu("LOW", "2026-09-23", "20"),
        ]
    );
    let c = step(&b, &cash_in_lieu_posted("LOW", "20"), &config);
    assert_eq!(c.receivables(), [cash_in_lieu("LOW", "2026-09-22", "9.5")]);
    assert_eq!(text(c.settled()), "20");

    let b = step(
        &a,
        &split("LOW", "2026-09-23", (1, 4), WHOLE, Some("19")),
        &config,
    );
    let c = step(&b, &cash_in_lieu_posted("LOW", "9.5"), &config);
    assert_eq!(c.receivables(), [cash_in_lieu("LOW", "2026-09-23", "9.5")]);
    let e = step(&c, &cash_in_lieu_posted("LOW", "9.5"), &config);
    assert!(e.receivables().is_empty());
    assert_eq!(text(e.settled()), "19");
}

/// `due` orders by date, then the settlement before dividends, then instrument and ex-date. A sell
/// of AAA traded 2026-09-29 settles 2026-09-30, the pay date of dividends on BBB and AAA.
#[test]
fn due_orders_settlements_before_dividends_on_one_date() {
    let config = no_fees();
    let a = Account::opening(
        usd("0"),
        [
            (id("AAA"), Position::new(qty("10"), basis("10")).unwrap()),
            (id("BBB"), Position::new(qty("10"), basis("10")).unwrap()),
        ],
    );
    let a = step(
        &a,
        &dividend("BBB", "2026-09-22", "2026-09-30", "1"),
        &config,
    );
    let a = step(
        &a,
        &dividend("AAA", "2026-09-22", "2026-10-01", "1"),
        &config,
    );
    let a = step(
        &a,
        &dividend("AAA", "2026-09-23", "2026-09-30", "1"),
        &config,
    );
    let a = step(
        &a,
        &equity(
            "s1",
            "AAA",
            Side::Sell,
            "1",
            "1",
            "2026-09-29T10:00:00-04:00",
        ),
        &config,
    );
    assert_eq!(
        a.due(at("2026-10-01T00:00:00-04:00")).unwrap(),
        [
            Input::SettlementPosted {
                date: d("2026-09-30")
            },
            dividend_paid("AAA", "2026-09-23"),
            dividend_paid("BBB", "2026-09-22"),
            dividend_paid("AAA", "2026-09-22"),
        ]
    );
}

/// A dividend whose pay date is before its ex-date cannot be built; the pay date may equal it.
#[test]
fn a_dividend_is_never_paid_before_its_ex_date() {
    let price = Price::parse("1").unwrap();
    let error = CashDividend::new(id("DIV"), d("2026-09-22"), d("2026-09-21"), price).unwrap_err();
    assert_eq!(error, AccountingError::PayDateBeforeExDate);
    assert_eq!(error.code(), "pay_date_before_ex_date");
    let same = CashDividend::new(id("DIV"), d("2026-09-22"), d("2026-09-22"), price).unwrap();
    assert_eq!(same.pay_date(), d("2026-09-22"));
    assert_eq!(same.per_share(), price);
    assert_eq!(same.instrument(), &id("DIV"));
    assert_eq!(same.ex_date(), d("2026-09-22"));
}
