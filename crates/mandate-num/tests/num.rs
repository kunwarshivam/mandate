//! Exact-or-error arithmetic and the spec's roundings (trading-domain spec §2.1, ADR-0001 ES-04).
//! The oracle holds values as `i128` integers at a fixed scale and rounds through floor division,
//! independent of the library's 256-bit sign-and-magnitude arithmetic.

use mandate_num::{
    Adverse, Bps, CostBasis, FeeCap, FeePerShare, FeeRate, Fraction, MarkPrice, NumError, Price,
    Qty, Rounding, ShareIncrement, SignedQty, SplitRatio, Usd,
};
use proptest::prelude::*;

/// Canonical text for `mantissa × 10^−scale`, written digit by digit.
fn text(mantissa: i128, scale: u32) -> String {
    let digits = mantissa.unsigned_abs().to_string();
    let width = scale as usize + 1;
    let padded = format!("{digits:0>width$}");
    let (int, frac) = padded.split_at(padded.len() - scale as usize);
    let frac = frac.trim_end_matches('0');
    let sign = if mantissa < 0 { "-" } else { "" };
    if frac.is_empty() {
        format!("{sign}{int}")
    } else {
        format!("{sign}{int}.{frac}")
    }
}

/// Reads canonical text as an integer at `scale` fractional digits; `None` if it has more.
fn at_scale(text: &str, scale: u32) -> Option<i128> {
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (int, frac) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if frac.len() > scale as usize {
        return None;
    }
    let digits = format!("{int}{frac:0<width$}", width = scale as usize);
    let value: i128 = digits.parse().ok()?;
    Some(if negative { -value } else { value })
}

fn pow10(exp: u32) -> i128 {
    10i128.pow(exp)
}

/// `n ÷ d` (d > 0) rounded to an integer, starting from the floor.
fn div_round(n: i128, d: i128, mode: Rounding) -> i128 {
    let q = n.div_euclid(d);
    let r = n.rem_euclid(d);
    let up = match mode {
        Rounding::HalfEven => 2 * r > d || (2 * r == d && q % 2 != 0),
        Rounding::HalfUp => 2 * r > d || (2 * r == d && n >= 0),
        Rounding::Ceiling => r > 0,
    };
    if up { q + 1 } else { q }
}

fn modes() -> impl Strategy<Value = Rounding> {
    prop_oneof![
        Just(Rounding::HalfEven),
        Just(Rounding::HalfUp),
        Just(Rounding::Ceiling)
    ]
}

/// A signed decimal with at most `max_scale` fractional digits: (mantissa, scale).
fn decimal(bound: i64, max_scale: u32) -> impl Strategy<Value = (i128, u32)> {
    (-bound..=bound, 0..=max_scale).prop_map(|(m, s)| (i128::from(m), s))
}

fn unsigned(bound: i64, max_scale: u32) -> impl Strategy<Value = (i128, u32)> {
    (0..=bound, 0..=max_scale).prop_map(|(m, s)| (i128::from(m), s))
}

fn usd(v: (i128, u32)) -> Usd {
    Usd::parse(&text(v.0, v.1)).unwrap()
}

fn qty(v: (i128, u32)) -> Qty {
    Qty::parse(&text(v.0, v.1)).unwrap()
}

fn signed(v: (i128, u32)) -> SignedQty {
    SignedQty::parse(&text(v.0, v.1)).unwrap()
}

/// The oracle value of `v` at `scale` (≥ v's scale).
fn scaled(v: (i128, u32), scale: u32) -> i128 {
    v.0 * pow10(scale - v.1)
}

const WIDE: u32 = 18;

/// Whether `value × 10^−scale` has a 96-bit significand once trailing zeros are dropped.
fn fits(value: i128, scale: u32) -> bool {
    let mut m = value.unsigned_abs();
    let mut s = scale;
    while s > 0 && m.is_multiple_of(10) {
        m /= 10;
        s -= 1;
    }
    m < 1u128 << 96
}

/// The library result must be the exact oracle value, or an overflow exactly when it cannot fit.
fn exact_or_overflow(
    result: Result<String, NumError>,
    expected: i128,
    scale: u32,
) -> Result<(), TestCaseError> {
    match result {
        Ok(actual) => prop_assert_eq!(at_scale(&actual, scale), Some(expected)),
        Err(e) => {
            prop_assert_eq!(e, NumError::Overflow);
            prop_assert!(!fits(expected, scale));
        }
    }
    Ok(())
}

proptest! {
    #[test]
    fn canonical_text_round_trips_and_nothing_else_parses(v in decimal(i64::MAX, 9)) {
        let canonical = text(v.0, v.1);
        prop_assert_eq!(SignedQty::parse(&canonical).unwrap().to_string(), canonical.as_str());
        prop_assert_eq!(Usd::parse(&canonical).unwrap().to_string(), canonical.as_str());
        let (int, frac) = canonical.split_once('.').unwrap_or((&canonical, ""));
        let trailing_zero = if frac.is_empty() { format!("{int}.0") } else { format!("{canonical}0") };
        let leading_zero = canonical.replacen(int.trim_start_matches('-'), &format!("0{}", int.trim_start_matches('-')), 1);
        for bad in [trailing_zero, leading_zero, format!("+{canonical}"), format!("{canonical}e0"), format!(" {canonical}")] {
            prop_assert_eq!(SignedQty::parse(&bad), Err(NumError::NotCanonical), "{}", bad);
            prop_assert_eq!(Usd::parse(&bad), Err(NumError::NotCanonical), "{}", bad);
        }
    }

    #[test]
    fn quantities_and_prices_never_round_on_the_way_in(m in 1..=i64::MAX, extra in 1u32..=10) {
        let canonical = text(i128::from(m) * 10 + 1, 9 + extra);
        prop_assert_eq!(Qty::parse(&canonical), Err(NumError::TooPrecise));
        prop_assert_eq!(SignedQty::parse(&canonical), Err(NumError::TooPrecise));
        prop_assert_eq!(Price::parse(&canonical), Err(NumError::TooPrecise));
    }

    #[test]
    fn signs_are_checked_on_the_way_in(v in decimal(i64::MAX, 9)) {
        let canonical = text(v.0, v.1);
        match v.0.signum() {
            -1 => {
                prop_assert_eq!(Qty::parse(&canonical), Err(NumError::Negative));
                prop_assert_eq!(Price::parse(&canonical), Err(NumError::NotPositive));
                prop_assert_eq!(FeeRate::parse(&canonical), Err(NumError::Negative));
                prop_assert_eq!(FeePerShare::parse(&canonical), Err(NumError::Negative));
                prop_assert_eq!(Bps::parse(&canonical), Err(NumError::Negative));
            }
            0 => {
                prop_assert!(Qty::parse(&canonical).unwrap().is_zero());
                prop_assert_eq!(Price::parse(&canonical), Err(NumError::NotPositive));
            }
            _ => {
                prop_assert_eq!(Qty::parse(&canonical).unwrap().to_string(), canonical.as_str());
                prop_assert_eq!(Price::parse(&canonical).unwrap().to_string(), canonical.as_str());
            }
        }
    }

    #[test]
    fn money_addition_and_subtraction_are_exact(a in decimal(i64::MAX, 18), b in decimal(i64::MAX, 18)) {
        exact_or_overflow(usd(a).checked_add(usd(b)).map(|v| v.to_string()), scaled(a, WIDE) + scaled(b, WIDE), WIDE)?;
        exact_or_overflow(usd(a).checked_sub(usd(b)).map(|v| v.to_string()), scaled(a, WIDE) - scaled(b, WIDE), WIDE)?;
        prop_assert_eq!(usd(a).negated().to_string(), text(-a.0, a.1));
        prop_assert_eq!(usd(a).is_negative(), a.0 < 0);
        prop_assert_eq!(usd(a).cmp(&usd(b)), scaled(a, WIDE).cmp(&scaled(b, WIDE)));
    }

    #[test]
    fn signed_quantities_add_exactly(a in decimal(i64::MAX / 2, 9), b in decimal(i64::MAX / 2, 9)) {
        let sum = signed(a).checked_add(signed(b)).unwrap();
        prop_assert_eq!(at_scale(&sum.to_string(), 9), Some(scaled(a, 9) + scaled(b, 9)));
        prop_assert_eq!(signed(a).negated().to_string(), text(-a.0, a.1));
        prop_assert_eq!(signed(a).abs().to_string(), text(a.0.abs(), a.1));
        prop_assert_eq!(signed(a).is_negative(), a.0 < 0);
    }

    #[test]
    fn quantity_subtraction_is_exact_or_negative(a in unsigned(i64::MAX, 9), b in unsigned(i64::MAX, 9)) {
        let expected = scaled(a, 9) - scaled(b, 9);
        match qty(a).checked_sub(qty(b)) {
            Ok(rest) => prop_assert_eq!(at_scale(&rest.to_string(), 9), Some(expected)),
            Err(e) => {
                prop_assert_eq!(e, NumError::Negative);
                prop_assert!(expected < 0);
            }
        }
        prop_assert_eq!(SignedQty::from(qty(a)).to_string(), text(a.0, a.1));
    }

    #[test]
    fn products_are_exact(q in decimal(100_000_000_000_000, 9), p in unsigned(100_000_000_000_000, 9)) {
        prop_assume!(p.0 > 0);
        let price = Price::parse(&text(p.0, p.1)).unwrap();
        let expected = text(q.0 * p.0, q.1 + p.1);
        prop_assert_eq!(signed(q).value_at(price).unwrap().to_string(), expected.as_str());
        let magnitude = text((q.0 * p.0).abs(), q.1 + p.1);
        prop_assert_eq!(signed(q).abs().notional(price).unwrap().to_string(), magnitude.as_str());
        let per_share = FeePerShare::parse(&text(p.0, p.1)).unwrap();
        prop_assert_eq!(signed(q).abs().times_per_share(per_share).unwrap().to_string(), magnitude.as_str());
    }

    #[test]
    fn rate_times_proceeds_is_exact(q in unsigned(1_000_000_000, 9), p in unsigned(1_000_000_000, 9), r in unsigned(1_000_000, 9)) {
        prop_assume!(p.0 > 0);
        let proceeds = qty(q).notional(Price::parse(&text(p.0, p.1)).unwrap()).unwrap();
        let sec = proceeds.times_rate(FeeRate::parse(&text(r.0, r.1)).unwrap()).unwrap();
        prop_assert_eq!(sec.to_string(), text(q.0 * p.0 * r.0, q.1 + p.1 + r.1));
    }

    #[test]
    fn rounding_matches_the_floor_based_oracle(v in decimal(i64::MAX, 18), scale in 0u32..=12, mode in modes()) {
        let rounded = usd(v).round(scale, mode).unwrap();
        let expected = if v.1 <= scale { scaled(v, scale) } else { div_round(v.0, pow10(v.1 - scale), mode) };
        prop_assert_eq!(rounded.to_string(), text(expected, scale));
    }

    #[test]
    fn crypto_fee_quantity_is_rounded_once(g in unsigned(i64::MAX / 10_000, 9), bps in 0i64..=10_000, mode in modes()) {
        let fee = qty(g).times_bps(Bps::parse(&bps.to_string()).unwrap(), mode).unwrap();
        let expected = div_round(scaled(g, 9) * i128::from(bps), 10_000, mode);
        prop_assert_eq!(fee.to_string(), text(expected, 9));
    }

    #[test]
    fn usd_fee_in_basis_points_is_rounded_once(x in unsigned(1_000_000_000_000, 18), bps in unsigned(10_000, 2), mode in modes()) {
        let fee = usd(x).times_bps(Bps::parse(&text(bps.0, bps.1)).unwrap(), 2, mode).unwrap();
        let expected = div_round(scaled(x, 18) * scaled(bps, 2), 10_000 * pow10(18), mode);
        prop_assert_eq!(fee.to_string(), text(expected, 2));
    }

    #[test]
    fn basis_reduction_is_one_rounding_of_the_exact_proportion(
        b in decimal(1_000_000_000_000, 12),
        whole_units in 1i64..=10_000_000_000_000,
        part_ratio in 0u32..=1000,
        mode in modes(),
    ) {
        let whole_units = i128::from(whole_units);
        let part_units = whole_units * i128::from(part_ratio) / 1000;
        let basis = CostBasis::parse(&text(b.0, b.1)).unwrap();
        let removed = basis.portion(qty((part_units, 9)), qty((whole_units, 9)), 12, mode).unwrap();
        let expected = div_round(scaled(b, 12) * part_units, whole_units, mode);
        prop_assert_eq!(removed.to_string(), text(expected, 12));
    }

    #[test]
    fn cost_basis_moves_exactly(b in decimal(i64::MAX, 18), a in decimal(i64::MAX, 18)) {
        let basis = CostBasis::parse(&text(b.0, b.1)).unwrap();
        exact_or_overflow(basis.checked_add(usd(a)).map(|v| v.to_string()), scaled(b, WIDE) + scaled(a, WIDE), WIDE)?;
        let removed = basis.checked_sub(CostBasis::parse(&text(a.0, a.1)).unwrap());
        exact_or_overflow(removed.map(|v| v.to_string()), scaled(b, WIDE) - scaled(a, WIDE), WIDE)?;
        prop_assert_eq!(basis.to_usd().to_string(), text(b.0, b.1));
    }
}

#[test]
fn results_that_do_not_fit_are_errors() {
    let max = Usd::parse("79228162514264337593543950335").unwrap();
    assert_eq!(
        max.checked_add(Usd::parse("1").unwrap()),
        Err(NumError::Overflow)
    );
    assert_eq!(
        max.negated().checked_sub(Usd::parse("1").unwrap()),
        Err(NumError::Overflow)
    );
    assert_eq!(
        Usd::parse("79228162514264337593543950336"),
        Err(NumError::Overflow)
    );
    let big = Qty::parse("7922816251426").unwrap();
    let price = Price::parse("79228162514264337.59354395").unwrap();
    assert_eq!(big.notional(price), Err(NumError::Overflow));
    let tiny = Usd::parse("0.0000000000000000000000000001").unwrap();
    let rate = FeeRate::parse("0.1").unwrap();
    assert_eq!(tiny.times_rate(rate), Err(NumError::TooPrecise));
    let whole = Qty::ZERO;
    assert_eq!(
        CostBasis::ZERO.portion(Qty::ZERO, whole, 12, Rounding::HalfEven),
        Err(NumError::DivisionByZero)
    );
    let widest = Qty::parse("79228162514264337593.543950335").unwrap();
    assert_eq!(
        widest.times_bps(Bps::parse("30000").unwrap(), Rounding::HalfEven),
        Err(NumError::Overflow),
        "237684487542793012780.631851005 has the full 9 places, so it is too wide, not too precise"
    );
}

/// A fee cap is a non-negative amount of money: a negative cap would turn a capped fee into a
/// credit (DEC-87).
#[test]
fn fee_caps_are_non_negative_amounts_of_money() {
    assert_eq!(FeeCap::parse("-0.01"), Err(NumError::Negative));
    assert_eq!(FeeCap::parse("9.790"), Err(NumError::NotCanonical));
    assert_eq!(FeeCap::parse("0").unwrap().to_usd(), Usd::ZERO);
    assert_eq!(
        FeeCap::parse("9.79").unwrap().to_usd(),
        Usd::parse("9.79").unwrap()
    );
    assert_eq!(
        FeeCap::parse("0.0000000000000000000000000001")
            .unwrap()
            .to_usd(),
        Usd::parse("0.0000000000000000000000000001").unwrap()
    );
}

#[test]
fn zero_is_accepted_where_allowed_and_is_never_negative() {
    assert_eq!(Qty::parse("0"), Ok(Qty::ZERO));
    assert_eq!(Usd::parse("0"), Ok(Usd::ZERO));
    assert_eq!(SignedQty::parse("0"), Ok(SignedQty::ZERO));
    assert!(FeeRate::parse("0").is_ok());
    assert!(FeePerShare::parse("0").is_ok());
    assert!(Bps::parse("0").is_ok());
    assert_eq!(Price::parse("0"), Err(NumError::NotPositive));
    assert!(!SignedQty::ZERO.is_negative());
    assert!(!Usd::ZERO.is_negative());
    assert!(SignedQty::parse("-0.000000001").unwrap().is_negative());
    assert!(Usd::parse("-0.01").unwrap().is_negative());
}

#[test]
fn hand_calculated_values_from_the_reference_cases() {
    let bps = |s: &str| Bps::parse(s).unwrap();
    let q = |s: &str| Qty::parse(s).unwrap();
    let p = |s: &str| Price::parse(s).unwrap();
    let u = |s: &str| Usd::parse(s).unwrap();
    assert_eq!(
        q("0.5")
            .times_bps(bps("25"), Rounding::HalfUp)
            .unwrap()
            .to_string(),
        "0.00125"
    );
    assert_eq!(
        u("30922.5")
            .times_bps(bps("25"), 2, Rounding::HalfUp)
            .unwrap()
            .to_string(),
        "77.31"
    );
    assert_eq!(
        u("0.501").round(2, Rounding::Ceiling).unwrap().to_string(),
        "0.51"
    );
    assert_eq!(
        u("0.02014")
            .round(2, Rounding::Ceiling)
            .unwrap()
            .to_string(),
        "0.03"
    );
    assert_eq!(
        u("0.03").round(2, Rounding::Ceiling).unwrap().to_string(),
        "0.03"
    );
    assert_eq!(
        u("-0.005").round(2, Rounding::Ceiling).unwrap().to_string(),
        "0"
    );
    assert_eq!(
        u("-0.005").round(2, Rounding::HalfUp).unwrap().to_string(),
        "-0.01"
    );
    assert_eq!(
        u("0.125").round(2, Rounding::HalfEven).unwrap().to_string(),
        "0.12"
    );
    assert_eq!(
        u("0.135").round(2, Rounding::HalfEven).unwrap().to_string(),
        "0.14"
    );
    assert_eq!(q("4").notional(p("160")).unwrap().to_string(), "640");
    let sec = u("640")
        .times_rate(FeeRate::parse("0.00003").unwrap())
        .unwrap();
    assert_eq!(sec.to_string(), "0.0192");
    let taf = q("4")
        .times_per_share(FeePerShare::parse("0.0002").unwrap())
        .unwrap();
    assert_eq!(taf.to_string(), "0.0008");
    let basis = CostBasis::parse("1500").unwrap();
    assert_eq!(
        basis
            .portion(q("4"), q("10"), 12, Rounding::HalfEven)
            .unwrap()
            .to_string(),
        "600"
    );
    let third = CostBasis::parse("100")
        .unwrap()
        .portion(q("1"), q("3"), 12, Rounding::HalfEven)
        .unwrap();
    assert_eq!(third.to_string(), "33.333333333333");
    assert_eq!(
        SignedQty::parse("-3")
            .unwrap()
            .value_at(p("55"))
            .unwrap()
            .to_string(),
        "-165"
    );
    assert_eq!(SignedQty::ZERO.negated().to_string(), "0");
    assert_eq!(Usd::ZERO.negated().to_string(), "0");
}

#[test]
fn error_codes_are_stable() {
    let all = [
        (NumError::NotCanonical, "not_canonical"),
        (NumError::TooPrecise, "too_precise"),
        (NumError::Overflow, "overflow"),
        (NumError::Negative, "negative"),
        (NumError::NotPositive, "not_positive"),
        (NumError::DivisionByZero, "division_by_zero"),
        (NumError::AboveOne, "above_one"),
    ];
    for (error, code) in all {
        assert_eq!(error.code(), code);
    }
}

fn ratio(new: u64, old: u64) -> SplitRatio {
    SplitRatio::new(new, old).unwrap()
}

/// `n ÷ d` (d > 0) truncated toward zero, starting from the floor.
fn toward_zero(n: i128, d: i128) -> i128 {
    let q = n.div_euclid(d);
    if n < 0 && n.rem_euclid(d) != 0 {
        q + 1
    } else {
        q
    }
}

fn increments() -> impl Strategy<Value = ShareIncrement> {
    prop_oneof![
        Just(ShareIncrement::Fractional),
        Just(ShareIncrement::Whole)
    ]
}

/// Q' in units of 10⁻⁹: Q × new ÷ old truncated toward zero to the increment (spec §8.5).
fn split_units(q: i128, new: u64, old: u64, increment: ShareIncrement) -> i128 {
    let (new, old) = (i128::from(new), i128::from(old));
    match increment {
        ShareIncrement::Fractional => toward_zero(q * new, old),
        ShareIncrement::Whole => toward_zero(q * new, old * pow10(9)) * pow10(9),
    }
}

/// `n ÷ d` rounded to an integer for a divisor of either sign.
fn signed_div_round(n: i128, d: i128, mode: Rounding) -> i128 {
    if d < 0 {
        div_round(-n, -d, mode)
    } else {
        div_round(n, d, mode)
    }
}

fn positive(bound: i64, max_scale: u32) -> impl Strategy<Value = (i128, u32)> {
    (1..=bound, 0..=max_scale).prop_map(|(m, s)| (i128::from(m), s))
}

proptest! {
    #[test]
    fn mark_prices_are_positive_with_at_most_twelve_places(v in decimal(i64::MAX, 12), extra in 1u32..=10) {
        let canonical = text(v.0, v.1);
        match v.0.signum() {
            1 => prop_assert_eq!(MarkPrice::parse(&canonical).unwrap().to_string(), canonical.as_str()),
            _ => prop_assert_eq!(MarkPrice::parse(&canonical), Err(NumError::NotPositive)),
        }
        let too_precise = text(v.0.abs() * 10 + 1, 12 + extra);
        prop_assert_eq!(MarkPrice::parse(&too_precise), Err(NumError::TooPrecise));
        let (int, frac) = canonical.split_once('.').unwrap_or((&canonical, ""));
        let trailing_zero = if frac.is_empty() { format!("{int}.0") } else { format!("{canonical}0") };
        prop_assert_eq!(MarkPrice::parse(&trailing_zero), Err(NumError::NotCanonical));
    }

    #[test]
    fn a_price_is_the_same_mark_and_values_at_a_mark_are_exact(p in positive(i64::MAX, 9), m in positive(1_000_000_000_000, 12), q in decimal(1_000_000_000_000, 9)) {
        let price = Price::parse(&text(p.0, p.1)).unwrap();
        prop_assert_eq!(MarkPrice::from(price).to_string(), text(p.0, p.1));
        prop_assert_eq!(MarkPrice::from(price), MarkPrice::parse(&text(p.0, p.1)).unwrap());
        let mark = MarkPrice::parse(&text(m.0, m.1)).unwrap();
        prop_assert_eq!(signed(q).value_at_mark(mark).unwrap().to_string(), text(q.0 * m.0, q.1 + m.1));
    }

    #[test]
    fn a_split_truncates_q_times_new_over_old_toward_zero_to_the_increment(
        q in decimal(1_000_000_000_000_000, 9),
        new in 1u64..=1_000,
        old in 1u64..=1_000,
        increment in increments(),
    ) {
        let split = ratio(new, old).split(signed(q), increment).unwrap();
        prop_assert_eq!(split.before(), signed(q));
        let expected = split_units(scaled(q, 9), new, old, increment);
        prop_assert_eq!(split.after().to_string(), text(expected, 9));
    }

    /// R = round(B × (Q·new − Q'·old) ÷ (Q·new), scale, mode), with B at up to 18 places (a fill's
    /// q × p) and the oracle's numerator and divisor in integers.
    #[test]
    fn the_residual_basis_is_one_rounding_of_the_exact_fraction(
        b in decimal(100_000_000, 18),
        q in decimal(100_000_000, 9),
        new in 1u64..=100,
        old in 1u64..=100,
        increment in increments(),
        scale in 0u32..=12,
        mode in modes(),
    ) {
        prop_assume!(q.0 != 0);
        let q_units = scaled(q, 9);
        let after = split_units(q_units, new, old, increment);
        let numerator = q_units * i128::from(new) - after * i128::from(old);
        let basis = CostBasis::parse(&text(b.0, b.1)).unwrap();
        let split = ratio(new, old).split(signed(q), increment).unwrap();
        let removed = split.residual_basis(basis, scale, mode).unwrap();
        let expected = signed_div_round(scaled(b, 18) * numerator, q_units * i128::from(new) * pow10(18 - scale), mode);
        prop_assert_eq!(removed.to_string(), text(expected, scale));
    }

    /// Cash in lieu = round(f × price, scale, mode) with f = (Q·new − Q'·old) ÷ old: one rounding
    /// of the exact product, signed like Q.
    #[test]
    fn cash_in_lieu_is_one_rounding_of_the_residual_times_the_price(
        q in decimal(1_000_000_000_000_000, 9),
        p in positive(1_000_000_000_000, 9),
        new in 1u64..=100,
        old in 1u64..=100,
        increment in increments(),
        scale in 0u32..=12,
        mode in modes(),
    ) {
        let q_units = scaled(q, 9);
        let after = split_units(q_units, new, old, increment);
        let numerator = q_units * i128::from(new) - after * i128::from(old);
        let price = Price::parse(&text(p.0, p.1)).unwrap();
        let split = ratio(new, old).split(signed(q), increment).unwrap();
        let cash = split.cash_in_lieu(price, scale, mode).unwrap();
        let expected = div_round(numerator * scaled(p, 9), i128::from(old) * pow10(18 - scale), mode);
        prop_assert_eq!(cash.to_string(), text(expected, scale));
    }

    /// Each adjustment is round(mark × old ÷ new, scale, mode) of the stored mark, so a second split
    /// rounds the already adjusted mark once more, never the original; a result of zero is an error.
    #[test]
    fn adjusted_marks_are_one_rounding_of_mark_times_old_over_new(
        m in positive(1_000_000_000_000, 12),
        splits in proptest::collection::vec((1u64..=1_000, 1u64..=1_000), 1..4),
        scale in 0u32..=12,
        mode in modes(),
    ) {
        let mut mark = MarkPrice::parse(&text(m.0, m.1)).unwrap();
        let mut units = scaled(m, 12);
        for (new, old) in splits {
            let rounded = div_round(units * i128::from(old), i128::from(new) * pow10(12 - scale), mode);
            let adjusted = ratio(new, old).mark(mark, scale, mode);
            if rounded == 0 {
                prop_assert_eq!(adjusted, Err(NumError::NotPositive));
                break;
            }
            if !fits(rounded, scale) {
                prop_assert_eq!(adjusted, Err(NumError::Overflow));
                break;
            }
            mark = adjusted.unwrap();
            units = rounded * pow10(12 - scale);
            prop_assert_eq!(mark.to_string(), text(rounded, scale));
        }
    }
}

#[test]
fn split_ratios_are_positive_integers() {
    assert_eq!(SplitRatio::new(0, 1), Err(NumError::NotPositive));
    assert_eq!(SplitRatio::new(1, 0), Err(NumError::NotPositive));
    assert_eq!(SplitRatio::new(0, 0), Err(NumError::NotPositive));
    let r = ratio(4, 1);
    assert_eq!((r.new_shares(), r.old_shares()), (4, 1));
    let r = ratio(1, 10);
    assert_eq!((r.new_shares(), r.old_shares()), (1, 10));
}

#[test]
fn split_results_that_do_not_fit_are_errors() {
    let q = |s: &str| SignedQty::parse(s).unwrap();
    let m = |s: &str| MarkPrice::parse(s).unwrap();
    let flat = ratio(1, 10).split(q("0"), ShareIncrement::Whole).unwrap();
    assert_eq!(flat.after(), SignedQty::ZERO);
    assert_eq!(
        flat.residual_basis(CostBasis::ZERO, 12, Rounding::HalfEven),
        Err(NumError::DivisionByZero)
    );
    assert_eq!(
        ratio(u64::MAX, 1).split(
            q("79228162514264337593.543950335"),
            ShareIncrement::Fractional
        ),
        Err(NumError::Overflow)
    );
    assert_eq!(
        ratio(3, 1).mark(m("0.000000000001"), 12, Rounding::HalfEven),
        Err(NumError::NotPositive),
        "0.000000000000333… rounds to zero, which is not a mark"
    );
    assert_eq!(
        ratio(2, 1).mark(m("0.000000000001"), 12, Rounding::HalfEven),
        Err(NumError::NotPositive),
        "0.0000000000005 ties to the even neighbour, zero"
    );
    assert_eq!(
        ratio(1, 3).mark(m("0.000000000001"), 13, Rounding::HalfEven),
        Err(NumError::TooPrecise),
        "a mark holds at most 12 places"
    );
}

/// RC-04: 10 × 4 ÷ 1 = 40, mark 400 × 1 ÷ 4 = 100. RC-05: 25 × 1 ÷ 10 = 2.5, whole shares 2,
/// residual basis 50 × (25 − 20) ÷ 25 = 10, cash in lieu 0.5 × 19 = 9.50, mark 2 × 10 = 20.
/// RC-23: 10 × 1 ÷ 3 = 3.333…, Q' 3.333333333, residual 100 × (10 − 9.999999999) ÷ 10 =
/// 0.00000001, mark 30 × 3 = 90; forward 3:1, mark 100 ÷ 3 = 33.333333333333, and 30 of them are
/// 999.99999999999. One billionth of a share, 1:10 fractional, truncates to 0: the formula's
/// residual is round(B, 12), so 5.27 × 10⁻¹⁶ rounds to 0 and 5.27 × 10⁻¹³ to 10⁻¹².
#[test]
fn hand_calculated_split_values_from_the_reference_cases() {
    let q = |s: &str| SignedQty::parse(s).unwrap();
    let b = |s: &str| CostBasis::parse(s).unwrap();
    let m = |s: &str| MarkPrice::parse(s).unwrap();
    let p = |s: &str| Price::parse(s).unwrap();
    let even = Rounding::HalfEven;

    let rc_04 = ratio(4, 1)
        .split(q("10"), ShareIncrement::Fractional)
        .unwrap();
    assert_eq!(rc_04.after().to_string(), "40");
    assert_eq!(
        rc_04
            .residual_basis(b("4000"), 12, even)
            .unwrap()
            .to_string(),
        "0"
    );
    assert_eq!(
        ratio(4, 1).mark(m("400"), 12, even).unwrap().to_string(),
        "100"
    );

    let rc_05 = ratio(1, 10).split(q("25"), ShareIncrement::Whole).unwrap();
    assert_eq!(rc_05.after().to_string(), "2");
    assert_eq!(
        rc_05.residual_basis(b("50"), 12, even).unwrap().to_string(),
        "10"
    );
    assert_eq!(
        rc_05.cash_in_lieu(p("19"), 2, even).unwrap().to_string(),
        "9.5"
    );
    assert_eq!(
        ratio(1, 10).mark(m("2"), 12, even).unwrap().to_string(),
        "20"
    );
    let short = ratio(1, 10).split(q("-25"), ShareIncrement::Whole).unwrap();
    assert_eq!(short.after().to_string(), "-2");
    assert_eq!(
        short
            .residual_basis(b("-50"), 12, even)
            .unwrap()
            .to_string(),
        "-10"
    );
    assert_eq!(
        short.cash_in_lieu(p("19"), 2, even).unwrap().to_string(),
        "-9.5"
    );

    let rc_23 = ratio(1, 3)
        .split(q("10"), ShareIncrement::Fractional)
        .unwrap();
    assert_eq!(rc_23.after().to_string(), "3.333333333");
    assert_eq!(
        rc_23
            .residual_basis(b("100"), 12, even)
            .unwrap()
            .to_string(),
        "0.00000001"
    );
    assert_eq!(
        rc_23.cash_in_lieu(p("19"), 2, even).unwrap().to_string(),
        "0"
    );
    assert_eq!(
        ratio(1, 3).mark(m("30"), 12, even).unwrap().to_string(),
        "90"
    );
    let forward = ratio(3, 1).mark(m("100"), 12, even).unwrap();
    assert_eq!(forward.to_string(), "33.333333333333");
    assert_eq!(
        q("30").value_at_mark(forward).unwrap().to_string(),
        "999.99999999999"
    );

    let dust = ratio(1, 10)
        .split(q("0.000000001"), ShareIncrement::Fractional)
        .unwrap();
    assert_eq!(dust.after(), SignedQty::ZERO);
    assert_eq!(
        dust.residual_basis(b("0.000000000000000527"), 12, even)
            .unwrap()
            .to_string(),
        "0"
    );
    assert_eq!(
        dust.residual_basis(b("0.000000000000527"), 12, even)
            .unwrap()
            .to_string(),
        "0.000000000001"
    );
}

/// The backtest fill model's arithmetic (trading-domain spec §6.4, DEC-106; claim #62). A
/// `Fraction` is the volume-cap fraction: non-negative, at most one, at most 9 places.
#[test]
#[ignore = "pending E4-1"]
fn fractions_run_from_zero_to_one_inclusive() {
    for accepted in ["0", "0.1", "0.000000001", "0.5", "1"] {
        assert_eq!(
            Fraction::parse(accepted).unwrap().to_string(),
            accepted,
            "{accepted}"
        );
    }
    assert_eq!(Fraction::parse("1.000000001"), Err(NumError::AboveOne));
    assert_eq!(Fraction::parse("2"), Err(NumError::AboveOne));
    assert_eq!(Fraction::parse("-0.1"), Err(NumError::Negative));
    assert_eq!(Fraction::parse("0.0000000001"), Err(NumError::TooPrecise));
    assert_eq!(Fraction::parse("0.10"), Err(NumError::NotCanonical));
    assert!(Fraction::parse("0").unwrap().is_zero());
    assert_eq!(NumError::AboveOne.code(), "above_one");
}

/// Hand-calculated slippage and volume caps (spec §6.4 rules 3 to 7). s = 3 bps moves a 100.00 open
/// by 100.00 × 0.0003 = 0.03, so a buy pays 100.03 and a sell receives 99.97. A slippage that needs
/// more than the 9 places a price holds is rounded up, once, so both sides move against the order:
/// 100.00 × 0.000123456789 = 0.0123456789 becomes 0.012345679 (DEC-106 item 2). The cap truncates:
/// 10% of 5000 shares is 500 either way, while 10% of 5005 is 500.5, which whole shares truncate to
/// 500 and a fractionable instrument keeps.
#[test]
#[ignore = "pending E4-1"]
fn hand_calculated_slippage_and_volume_caps() {
    let three_bps = Bps::parse("3").unwrap();
    let hundred = Price::parse("100").unwrap();
    assert_eq!(
        hundred.slipped(three_bps, Adverse::Up).unwrap().to_string(),
        "100.03"
    );
    assert_eq!(
        hundred
            .slipped(three_bps, Adverse::Down)
            .unwrap()
            .to_string(),
        "99.97"
    );
    let ten_places = Bps::parse("1.23456789").unwrap();
    assert_eq!(
        hundred
            .slipped(ten_places, Adverse::Up)
            .unwrap()
            .to_string(),
        "100.012345679"
    );
    assert_eq!(
        hundred
            .slipped(ten_places, Adverse::Down)
            .unwrap()
            .to_string(),
        "99.987654321"
    );
    assert_eq!(
        Price::parse("0.000000001")
            .unwrap()
            .slipped(Bps::parse("10000").unwrap(), Adverse::Down),
        Err(NumError::NotPositive)
    );

    let tenth = Fraction::parse("0.1").unwrap();
    assert_eq!(
        Qty::parse("5000")
            .unwrap()
            .portion(tenth, ShareIncrement::Whole)
            .unwrap()
            .to_string(),
        "500"
    );
    assert_eq!(
        Qty::parse("5005")
            .unwrap()
            .portion(tenth, ShareIncrement::Whole)
            .unwrap()
            .to_string(),
        "500"
    );
    assert_eq!(
        Qty::parse("5005")
            .unwrap()
            .portion(tenth, ShareIncrement::Fractional)
            .unwrap()
            .to_string(),
        "500.5"
    );
    assert_eq!(
        Qty::parse("5000")
            .unwrap()
            .portion(Fraction::ZERO, ShareIncrement::Whole)
            .unwrap(),
        Qty::ZERO
    );
}

/// Hand-calculated `sqrt` impacts (spec §6.4, DEC-106 item 3). The root of the filled share of the
/// reference volume is taken at 18 places and rounded up, so the impact is never understated:
/// √0.25 = 0.5 exactly; √0.1 = 0.31622776601683793319…, which rounds up to 0.316227766016837934, so
/// a 10 bps coefficient gives 3.16227766016837934 bps; √0.5 = 0.70710678118654752440… rounds up to
/// 0.707106781186547525, giving 7.07106781186547525 bps; and √2 = 1.41421356237309504880… rounds up
/// to 1.414213562373095049, giving 14.14213562373095049 bps.
#[test]
#[ignore = "pending E4-1"]
fn hand_calculated_sqrt_impacts() {
    let ten = Bps::parse("10").unwrap();
    let cases = [
        ("1", "4", "5"),
        ("1", "10", "3.16227766016837934"),
        ("1", "2", "7.07106781186547525"),
        ("2", "1", "14.14213562373095049"),
        ("1", "1", "10"),
        ("3", "7", "6.54653670707977144"),
    ];
    for (fill, reference, impact) in cases {
        assert_eq!(
            Bps::sqrt_impact(
                ten,
                Qty::parse(fill).unwrap(),
                Qty::parse(reference).unwrap()
            )
            .unwrap()
            .to_string(),
            impact,
            "{fill} of {reference}"
        );
    }
    assert_eq!(
        Bps::sqrt_impact(ten, Qty::parse("1").unwrap(), Qty::ZERO),
        Err(NumError::DivisionByZero)
    );
    assert_eq!(
        Bps::sqrt_impact(
            Bps::ZERO,
            Qty::parse("1").unwrap(),
            Qty::parse("2").unwrap()
        )
        .unwrap(),
        Bps::ZERO
    );
}

/// A fraction of one at 9 places: the mantissa runs from 0 to 10⁹.
fn fractions() -> impl Strategy<Value = i128> {
    0i128..=pow10(9)
}

proptest! {
    /// Adding quantities and basis points is exact or an error (ES-04).
    #[test]
    #[ignore = "pending E4-1"]
    fn adding_quantities_and_basis_points_is_exact(
        a in unsigned(i64::MAX, 9),
        b in unsigned(i64::MAX, 9),
    ) {
        exact_or_overflow(
            qty(a).checked_add(qty(b)).map(|v| v.to_string()),
            scaled(a, 9) + scaled(b, 9),
            9,
        )?;
        let (x, y) = ((a.0, a.1.min(8)), (b.0, b.1.min(8)));
        exact_or_overflow(
            Bps::parse(&text(x.0, x.1)).unwrap()
                .checked_add(Bps::parse(&text(y.0, y.1)).unwrap())
                .map(|v| v.to_string()),
            scaled(x, 8) + scaled(y, 8),
            8,
        )?;
    }

    /// Spec §6.4 rule 3: a volume cap is the product truncated to the increment, never above the
    /// exact product, and never above the quantity it comes from when the fraction is at most one.
    #[test]
    #[ignore = "pending E4-1"]
    fn a_volume_cap_is_the_truncated_product_and_never_above_it(
        volume in unsigned(1_000_000_000, 9),
        mantissa in fractions(),
        increment in increments(),
    ) {
        let fraction = Fraction::parse(&text(mantissa, 9)).unwrap();
        let exact = scaled(volume, 9) * mantissa / pow10(9);
        let expected = match increment {
            ShareIncrement::Fractional => exact,
            ShareIncrement::Whole => exact / pow10(9) * pow10(9),
        };
        let capped = qty(volume).portion(fraction, increment).unwrap();
        prop_assert_eq!(at_scale(&capped.to_string(), 9), Some(expected));
        prop_assert!(expected <= exact);
        prop_assert!(capped <= qty(volume));
    }

    /// DEC-106 item 2: slippage is `ceil(price × bps ÷ 10⁴)` at 9 places, added for a buy and taken
    /// away for a sell, so the price always moves against the order; a sell price that reaches zero
    /// is `not_positive` rather than a price. Prices stay below 10⁹ and basis points below 10⁶ so
    /// that the `i128` oracle's own product is exact.
    #[test]
    #[ignore = "pending E4-1"]
    fn slippage_moves_a_price_against_the_order_by_a_rounded_up_amount(
        p in positive(1_000_000_000, 9),
        b in unsigned(1_000_000, 8),
    ) {
        let (price, bps) = (
            Price::parse(&text(p.0, p.1)).unwrap(),
            Bps::parse(&text(b.0, b.1)).unwrap(),
        );
        let (price_units, bps_units) = (scaled(p, 9), scaled(b, 8));
        let slip = div_round(price_units * bps_units, pow10(12), Rounding::Ceiling);
        exact_or_overflow(
            price.slipped(bps, Adverse::Up).map(|v| v.to_string()),
            price_units + slip,
            9,
        )?;
        let down = price.slipped(bps, Adverse::Down);
        if price_units - slip <= 0 {
            prop_assert_eq!(down, Err(NumError::NotPositive));
        } else {
            exact_or_overflow(down.map(|v| v.to_string()), price_units - slip, 9)?;
        }
    }

    /// DEC-106 item 3: the `sqrt` impact is the coefficient exactly when the fill is the whole
    /// reference volume, never above it below that, never below it above that, and never falls as
    /// the fill grows.
    #[test]
    #[ignore = "pending E4-1"]
    fn sqrt_impact_is_monotone_and_bounded_by_its_coefficient(
        reference in positive(100_000, 9),
        smaller in positive(100_000, 9),
        larger in positive(100_000, 9),
        c in unsigned(10_000, 8),
    ) {
        let coefficient = Bps::parse(&text(c.0, c.1)).unwrap();
        let reference_qty = qty(reference);
        let (low, high) = if scaled(smaller, 9) <= scaled(larger, 9) {
            (qty(smaller), qty(larger))
        } else {
            (qty(larger), qty(smaller))
        };
        let at_low = Bps::sqrt_impact(coefficient, low, reference_qty).unwrap();
        let at_high = Bps::sqrt_impact(coefficient, high, reference_qty).unwrap();
        prop_assert!(at_low <= at_high, "the impact fell as the fill grew");
        prop_assert_eq!(
            Bps::sqrt_impact(coefficient, reference_qty, reference_qty).unwrap(),
            coefficient
        );
        if low <= reference_qty {
            prop_assert!(at_low <= coefficient);
        }
        if high >= reference_qty {
            prop_assert!(at_high >= coefficient);
        }
    }
}
