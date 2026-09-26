//! Vendor numbers convert exactly or are rejected, never rounded (ADR-0001 ES-23).

use mandate_canon::{DecError, DecStr};
use mandate_marketdata::number::{self, NumberError};
use proptest::prelude::*;

#[test]
fn vendor_numbers_convert_exactly_where_f64_would_not() {
    let cases: [(&str, u8, i128); 7] = [
        ("9007199254740993", 9, 9_007_199_254_740_993_000_000_000),
        ("84267.2097946887", 18, 84_267_209_794_688_700_000_000),
        ("0.30000000000000004", 18, 300_000_000_000_000_040),
        ("0.006594329", 9, 6_594_329),
        ("764.050785", 18, 764_050_785_000_000_000_000),
        ("1.5e-3", 9, 1_500_000),
        ("-0", 9, 0),
    ];
    for (raw, scale, units) in cases {
        let value = number::decimal_from_json(raw).unwrap();
        assert_eq!(
            number::to_units(&value, scale),
            Ok(units),
            "{raw} at scale {scale}"
        );
    }
    let through_f64: f64 = "9007199254740993".parse().unwrap();
    assert_ne!(
        through_f64.to_string(),
        "9007199254740993",
        "the first case must be one an f64 cannot hold"
    );
}

fn pow10(n: usize) -> i128 {
    (0..n).fold(1, |p, _| p * 10)
}

proptest! {
    #[test]
    fn scaled_units_round_trip_to_the_same_decimal(
        negative in any::<bool>(),
        int in "[0-9]{1,20}",
        frac in "[0-9]{0,9}",
        wide in any::<bool>(),
    ) {
        let scale: u8 = if wide { 18 } else { 9 };
        let sign = if negative { "-" } else { "" };
        let point = if frac.is_empty() { String::new() } else { format!(".{frac}") };
        let text = format!("{sign}{int}{point}");
        let whole: i128 = int.parse().unwrap();
        let part: i128 = if frac.is_empty() { 0 } else { frac.parse().unwrap() };
        let magnitude = whole * pow10(usize::from(scale)) + part * pow10(usize::from(scale) - frac.len());
        let expected = if negative { -magnitude } else { magnitude };
        let value = DecStr::parse(&text).unwrap();
        prop_assert_eq!(number::to_units(&value, scale), Ok(expected));
        prop_assert_eq!(number::from_units(expected, scale), Ok(value));
    }
}

#[test]
fn more_fraction_digits_than_the_scale_are_rejected_not_rounded() {
    for (raw, scale) in [
        ("84267.2097946887", 9),
        ("0.0000000001", 9),
        ("1.0000000000000000001", 18),
    ] {
        let value = number::decimal_from_json(raw).unwrap();
        assert_eq!(
            number::to_units(&value, scale),
            Err(NumberError::Scale { scale }),
            "{raw}"
        );
    }
    let trailing_zeros = number::decimal_from_json("1.50000000000").unwrap();
    assert_eq!(
        number::to_units(&trailing_zeros, 9),
        Ok(1_500_000_000),
        "trailing zeros carry no digits"
    );
}

#[test]
fn magnitudes_beyond_38_digits_are_rejected() {
    let big = number::decimal_from_json("123456789012345678901").unwrap();
    assert_eq!(
        number::to_units(&big, 18),
        Err(NumberError::Precision { scale: 18 })
    );
    assert_eq!(
        number::to_units(&big, 9),
        Ok(123_456_789_012_345_678_901_000_000_000)
    );
    assert_eq!(
        number::from_units(pow10(38), 9),
        Err(NumberError::Precision { scale: 9 })
    );
    assert_eq!(
        number::from_units(-pow10(38), 9),
        Err(NumberError::Precision { scale: 9 })
    );
    assert_eq!(
        number::decimal_from_json("1e400"),
        Err(NumberError::Decimal(DecError::OutOfRange))
    );
    let one = DecStr::parse("1").unwrap();
    assert_eq!(
        number::to_units(&one, 29),
        Err(NumberError::UnsupportedScale { scale: 29 })
    );
}

#[test]
fn json_values_that_are_not_numbers_are_rejected() {
    for raw in [
        "\"1.5\"", "null", "true", "[]", "{}", "+1", "01", "-01", "1.", ".5", "1e", "1e+", "NaN",
        "Infinity", "-", "", " 1", "1 ", "0x10", "1_000",
    ] {
        assert_eq!(
            number::decimal_from_json(raw),
            Err(NumberError::NotANumber),
            "{raw:?}"
        );
    }
    for raw in [
        "1.0",
        "-1",
        "1e3",
        "01",
        "18446744073709551616",
        "\"7\"",
        "",
    ] {
        assert_eq!(
            number::unsigned_from_json(raw),
            Err(NumberError::NotAnInteger),
            "{raw:?}"
        );
    }
    assert_eq!(
        number::unsigned_from_json("1138822061783138430"),
        Ok(1_138_822_061_783_138_430)
    );
    assert_eq!(number::unsigned_from_json("0"), Ok(0));
}
