//! Journal decimal grammar (spec §4.6). The property oracle starts from a canonical decimal, writes
//! equivalent spellings of it, and checks that each normalizes back.

use mandate_canon::{DecError, DecStr};
use proptest::prelude::*;

fn norm(input: &str) -> Result<String, DecError> {
    DecStr::parse(input).map(|d| d.as_str().to_owned())
}

/// `^-?(0|[1-9][0-9]*)(\.[0-9]*[1-9])?$`, not `-0`, written without the library.
fn in_grammar(s: &str) -> bool {
    let body = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = match body.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };
    let int_ok = int == "0"
        || (!int.is_empty() && !int.starts_with('0') && int.bytes().all(|b| b.is_ascii_digit()));
    let frac_ok = frac.is_none_or(|f| {
        !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()) && !f.ends_with('0')
    });
    int_ok && frac_ok && s != "-0"
}

#[test]
#[ignore = "pending E5-1"]
fn vector_cases() {
    let accept = [
        ("150.00", "150"),
        ("0.020140", "0.02014"),
        ("0.00", "0"),
        ("-0", "0"),
        ("-0.00", "0"),
        ("-0.50", "-0.5"),
        ("1e3", "1000"),
        ("1E+3", "1000"),
        (".5", "0.5"),
        ("007.50", "7.5"),
        ("1.0E-8", "0.00000001"),
    ];
    for (input, expected) in accept {
        assert_eq!(norm(input).as_deref(), Ok(expected), "{input:?}");
    }
    for input in ["NaN", "Infinity", "+1", "1_000", " 1", "1e-30", "8e28"] {
        assert!(norm(input).is_err(), "{input:?}");
    }
}

#[test]
#[ignore = "pending E5-1"]
fn more_accepts() {
    let cases = [
        ("0", "0"),
        ("1.", "1"),
        ("-.5", "-0.5"),
        ("00", "0"),
        ("0e-999999", "0"),
        ("0.0e5", "0"),
        ("1e-28", "0.0000000000000000000000000001"),
        ("1e28", "10000000000000000000000000000"),
        ("1E03", "1000"),
        ("12.5e-1", "1.25"),
        ("12.5e1", "125"),
        ("0.000125e3", "0.125"),
        ("-12345.678900", "-12345.6789"),
        (
            "78999999999999999999999999999.9999999999999999999999999999",
            "78999999999999999999999999999.9999999999999999999999999999",
        ),
        ("7.8999e28", "78999000000000000000000000000"),
        (
            "-78999999999999999999999999999",
            "-78999999999999999999999999999",
        ),
        ("0.1000000000000000000000000000000000", "0.1"),
    ];
    for (input, expected) in cases {
        assert_eq!(norm(input).as_deref(), Ok(expected), "{input:?}");
    }
}

#[test]
#[ignore = "pending E5-1"]
fn more_rejects() {
    let syntax = [
        "", "-", ".", "-.", "e3", "1e", "1e+", "1e-", "1.2.3", "1,000", "0x10", "\u{661}", "1 ",
        "--1", "1e1.5", "inf", "nan", "+.5", "1e+-3", "1d3", "\u{ff11}", "1.5e3x",
    ];
    for input in syntax {
        assert_eq!(norm(input), Err(DecError::Syntax), "{input:?}");
    }
    let range = [
        "1e-29",
        "0.00000000000000000000000000001",
        "79000000000000000000000000000",
        "-79000000000000000000000000000",
        "79000000000000000000000000000.1",
        "7.9e28",
        "1e29",
        "100000000000000000000000000000",
        "1e99999999999999999999",
        "1e-99999999999999999999",
    ];
    for input in range {
        assert_eq!(norm(input), Err(DecError::OutOfRange), "{input:?}");
    }
    assert_eq!(
        (DecError::Syntax.code(), DecError::OutOfRange.code()),
        ("syntax", "out_of_range")
    );
    let d = DecStr::parse("-1.50").unwrap();
    assert_eq!(d.to_string(), "-1.5");
}

/// (negative, integer digits, fraction digits) of a canonical decimal within the bounds.
fn canonical_parts() -> impl Strategy<Value = (bool, String, String)> {
    let int = prop_oneof![
        Just("0".to_owned()),
        "[1-9][0-9]{0,27}",
        "[1-6][0-9]{28}",
        "7[0-8][0-9]{27}",
    ];
    let frac = prop_oneof![Just(String::new()), "[0-9]{0,27}[1-9]"];
    (any::<bool>(), int, frac)
}

fn assemble(negative: bool, int: &str, frac: &str) -> String {
    let zero = int == "0" && frac.is_empty();
    let mut s = String::new();
    if negative && !zero {
        s.push('-');
    }
    s.push_str(int);
    if !frac.is_empty() {
        s.push('.');
        s.push_str(frac);
    }
    s
}

/// An equivalent spelling: the point moved by `shift` places with a compensating exponent, plus
/// padding zeros and a choice of exponent style.
fn respell(
    negative: bool,
    int: &str,
    frac: &str,
    shift: i64,
    lead: usize,
    trail: usize,
    style: u8,
) -> String {
    let digits = format!("{int}{frac}");
    let point = int.len() as i64 - shift;
    let len = digits.len() as i64;
    let mut mantissa = if point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else if point >= len {
        format!("{digits}{}", "0".repeat((point - len) as usize))
    } else {
        format!(
            "{}.{}",
            &digits[..point as usize],
            &digits[point as usize..]
        )
    };
    if trail > 0 {
        if !mantissa.contains('.') {
            mantissa.push('.');
        }
        mantissa.push_str(&"0".repeat(trail));
    }
    let mut s = String::new();
    if negative {
        s.push('-');
    }
    s.push_str(&"0".repeat(lead));
    s.push_str(&mantissa);
    if shift != 0 || !style.is_multiple_of(3) {
        s.push(if style.is_multiple_of(2) { 'e' } else { 'E' });
        if shift < 0 {
            s.push('-');
        } else if style % 4 == 1 {
            s.push('+');
        }
        if style % 5 == 2 {
            s.push('0');
        }
        s.push_str(&shift.unsigned_abs().to_string());
    }
    s
}

proptest! {
    #[test]
    #[ignore = "pending E5-1"]
    fn equivalent_spellings_normalize_to_the_canonical_form(
        (negative, int, frac) in canonical_parts(),
        shift in -40i64..40, lead in 0usize..3, trail in 0usize..3, style in any::<u8>()
    ) {
        let canonical = assemble(negative, &int, &frac);
        prop_assert!(in_grammar(&canonical));
        prop_assert_eq!(norm(&canonical), Ok(canonical.clone()));
        let spelled = respell(negative, &int, &frac, shift, lead, trail, style);
        prop_assert_eq!(norm(&spelled), Ok(canonical), "{}", spelled);
    }

    #[test]
    #[ignore = "pending E5-1"]
    fn more_than_28_fraction_digits_are_rejected_not_rounded(
        negative in any::<bool>(), int in "0|[1-9][0-9]{0,5}", frac in "[0-9]{28,40}[1-9]",
        shift in -3i64..3, style in any::<u8>()
    ) {
        let spelled = respell(negative, &int, &frac, shift, 0, 0, style);
        prop_assert_eq!(norm(&spelled), Err(DecError::OutOfRange), "{}", spelled);
    }

    #[test]
    #[ignore = "pending E5-1"]
    fn magnitudes_from_the_limit_up_are_rejected(
        negative in any::<bool>(),
        int in prop_oneof!["79[0-9]{27}", "[89][0-9]{28}", "[1-9][0-9]{29,40}"],
        frac in prop_oneof![Just(String::new()), "[0-9]{0,5}[1-9]"],
        shift in -3i64..3, style in any::<u8>()
    ) {
        let spelled = respell(negative, &int, &frac, shift, 0, 0, style);
        prop_assert_eq!(norm(&spelled), Err(DecError::OutOfRange), "{}", spelled);
    }

    #[test]
    fn output_is_always_in_the_grammar(s in "[-+.eE0-9]{0,12}") {
        if let Ok(out) = norm(&s) {
            prop_assert!(in_grammar(&out), "{} -> {}", s, out);
            prop_assert_eq!(norm(&out), Ok(out.clone()));
        }
    }
}
