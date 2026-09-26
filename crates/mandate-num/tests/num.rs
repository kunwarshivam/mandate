//! Exact-or-error arithmetic and the spec's roundings (trading-domain spec §2.1, ADR-0001 ES-04).
//! The oracle holds values as `i128` integers at a fixed scale and rounds through floor division,
//! independent of the library's 256-bit sign-and-magnitude arithmetic.

use mandate_num::{
    Bps, CostBasis, FeeCap, FeePerShare, FeeRate, NumError, Price, Qty, Rounding, SignedQty, Usd,
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
#[ignore = "pending E3-1"]
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
    ];
    for (error, code) in all {
        assert_eq!(error.code(), code);
    }
}
