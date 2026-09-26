//! [`SchemaDec`]: the document's decimals, checked against the whole `$def` the schema declares for
//! the field they came from (DEC-128 item 3).
//!
//! These tests are **live**. They are the answer to the review finding that
//! `mandate_canon::DecStr` is not the schema grammar: the claim that the grammar is canonical, and
//! therefore that a mandate's canonical bytes reproduce the document the owner confirmed, is asserted
//! here rather than promised. Every value below was read off
//! `schemas/mandate.schema.json`'s `$defs`, and the four `DecStr` behaviours the review found are
//! pinned as *rejections* so no later change can quietly let them in.

use mandate_canon::DecStr;
use mandate_spec::{DecGrammar, SchemaDec};
use proptest::prelude::*;

const ALL: [DecGrammar; 5] = [
    DecGrammar::Decimal,
    DecGrammar::PositiveDecimal,
    DecGrammar::Fraction,
    DecGrammar::OpenFraction,
    DecGrammar::UnitPositive,
];

fn accepts(text: &str, grammar: DecGrammar) -> bool {
    SchemaDec::parse(text, grammar).is_ok()
}

/// Each of these is `Ok` from `DecStr::parse`, which normalises instead of rejecting. That is the whole reason the document does not store a bare `DecStr` (MC-S04, MC-S17).
#[test]
fn what_decstr_normalises_is_off_every_grammar() {
    for text in ["0.020", "007.50", "1e3", "1E+3", ".5", "0.50", "-0.0"] {
        assert!(
            DecStr::parse(text).is_ok(),
            "{text} is in the journal grammar, which is the point"
        );
        for grammar in ALL {
            assert!(
                !accepts(text, grammar),
                "{text} must be off `{}`; a parse that leans on DecStr would take it",
                grammar.as_str()
            );
        }
    }
}

/// The `decimal` pattern `^-?(0|[1-9][0-9]{0,27})(\.…)?$` matches `-0`, and `DecStr` turns it into `0`. The `$def` also carries `"not": {"const": "-0"}`, so it is not in the grammar — and a parse that checked the pattern alone would accept a value whose canonical bytes differ from its own text, breaking the round trip (planted bug 21).
#[test]
fn minus_zero_is_excluded_by_the_not_clause_beside_the_pattern() {
    assert_eq!(
        DecStr::parse("-0").map(|d| d.as_str().to_owned()),
        Ok("0".to_owned()),
        "DecStr drops the sign, which is why the `not` clause has to be checked here"
    );
    assert!(!accepts("-0", DecGrammar::Decimal));
    assert!(accepts("0", DecGrammar::Decimal));
    assert!(
        accepts("-0.5", DecGrammar::Decimal),
        "a signed value is fine"
    );
}

#[test]
fn the_integer_part_stops_at_the_schemas_twenty_eight_digits() {
    let twenty_eight = "1".repeat(28);
    let twenty_nine = "1".repeat(29);
    assert!(accepts(&twenty_eight, DecGrammar::PositiveDecimal));
    assert!(
        !accepts(&twenty_nine, DecGrammar::PositiveDecimal),
        "DecStr allows 29 integer digits and the schema allows 28 (planted bug 22)"
    );
    assert!(
        DecStr::parse(&twenty_nine).is_ok(),
        "which is exactly the gap: the journal grammar takes it"
    );
}

#[test]
fn the_fractional_part_stops_at_twenty_eight_digits_and_never_ends_in_zero() {
    let twenty_eight = format!("0.{}1", "0".repeat(27));
    let twenty_nine = format!("0.{}1", "0".repeat(28));
    assert!(accepts(&twenty_eight, DecGrammar::Fraction));
    assert!(!accepts(&twenty_nine, DecGrammar::Fraction), "MC-S17");
    assert!(!accepts("0.10", DecGrammar::Fraction));
}

/// Read off the five `$defs` patterns, one row per value, so a row that moves is a schema change.
///
/// decimal, positive, fraction, open,  unit
#[test]
fn each_grammar_admits_exactly_its_own_values() {
    let rows: [(&str, [bool; 5]); 12] = [
        ("0", [true, false, true, false, false]),
        ("1", [true, true, true, false, true]),
        ("0.5", [true, true, true, true, true]),
        ("0.0001", [true, true, true, true, true]),
        ("2", [true, true, false, false, false]),
        ("1.5", [true, true, false, false, false]),
        ("8", [true, true, false, false, false]),
        ("-5", [true, false, false, false, false]),
        ("-0.5", [true, false, false, false, false]),
        ("10000", [true, true, false, false, false]),
        ("0.08", [true, true, true, true, true]),
        ("58000", [true, true, false, false, false]),
    ];
    for (text, expected) in rows {
        for (grammar, want) in ALL.into_iter().zip(expected) {
            assert_eq!(
                accepts(text, grammar),
                want,
                "`{text}` against `{}`",
                grammar.as_str()
            );
        }
    }
}

/// MC-S03 (a fraction outside the unit interval) and MC-S13 and MC-S14 (a factor of 1, a hysteresis of 0) fail on the grammar their field declares, which is what makes them schema cases rather than V-rules.
#[test]
fn the_cases_own_rejections_are_rejections_here() {
    assert!(
        !accepts("8", DecGrammar::OpenFraction),
        "MC-S03 max_drawdown"
    );
    assert!(!accepts("1", DecGrammar::OpenFraction), "MC-S13 factor");
    assert!(!accepts("0", DecGrammar::OpenFraction), "MC-S14 hysteresis");
    assert!(
        accepts("0.08", DecGrammar::OpenFraction),
        "and the base value"
    );
}

#[test]
fn nothing_but_a_decimal_is_a_decimal() {
    for text in [
        "", "-", ".", "-.", "1.", "+1", " 1", "1 ", "NaN", "Infinity", "1_000", "0x1", "--1", "1-",
        "1.2.3", "０", "1,5",
    ] {
        for grammar in ALL {
            assert!(
                !accepts(text, grammar),
                "`{text}` must be off `{}`",
                grammar.as_str()
            );
        }
    }
}

#[test]
fn a_grammar_is_remembered_and_the_signed_one_is_named() {
    let dec = SchemaDec::parse("-5", DecGrammar::Decimal).expect("in the decimal grammar");
    assert_eq!(dec.grammar(), DecGrammar::Decimal);
    assert!(dec.is_negative());
    assert!(DecGrammar::Decimal.is_signed());
    for grammar in ALL.into_iter().filter(|g| *g != DecGrammar::Decimal) {
        assert!(
            !grammar.is_signed(),
            "`{}` admits no negative, so nothing but a model parameter can be one",
            grammar.as_str()
        );
    }
}

#[test]
fn only_zero_is_zero() {
    let zero = SchemaDec::parse("0", DecGrammar::Fraction).expect("zero is a fraction");
    assert!(zero.is_zero());
    let small = SchemaDec::parse("0.0000000001", DecGrammar::Fraction).expect("a small fraction");
    assert!(
        !small.is_zero(),
        "no grammar admits a trailing zero, so no other spelling of zero exists"
    );
}

fn frac(text: &str) -> SchemaDec {
    SchemaDec::parse(text, DecGrammar::Fraction).expect("a fraction")
}

fn dec(text: &str) -> SchemaDec {
    SchemaDec::parse(text, DecGrammar::Decimal).expect("a decimal")
}

#[test]
fn unsigned_values_order_by_value_not_by_text() {
    let ascending = [
        "0",
        "0.0000000001",
        "0.02",
        "0.1",
        "0.12",
        "0.2",
        "0.9999999999",
        "1",
    ];
    for pair in ascending.windows(2) {
        let (a, b) = (pair.first(), pair.get(1));
        let (a, b) = (a.copied().unwrap_or("0"), b.copied().unwrap_or("0"));
        assert!(
            frac(a) < frac(b),
            "{a} must order below {b}; plain text order would not give this"
        );
    }
    assert_eq!(frac("0.5").cmp(&frac("0.5")), core::cmp::Ordering::Equal);
}

#[test]
fn a_longer_integer_part_is_a_larger_number() {
    let ascending = ["1", "2", "9", "10", "99", "100", "1000"];
    for pair in ascending.windows(2) {
        let (a, b) = (
            pair.first().copied().unwrap_or("1"),
            pair.get(1).copied().unwrap_or("1"),
        );
        assert!(
            SchemaDec::parse(a, DecGrammar::PositiveDecimal).expect("a")
                < SchemaDec::parse(b, DecGrammar::PositiveDecimal).expect("b"),
            "{a} < {b}"
        );
    }
}

#[test]
fn negatives_order_below_zero_and_among_themselves_by_magnitude_reversed() {
    let ascending = ["-100", "-10", "-2", "-1", "-0.5", "-0.05", "0", "0.05", "1"];
    for pair in ascending.windows(2) {
        let (a, b) = (
            pair.first().copied().unwrap_or("0"),
            pair.get(1).copied().unwrap_or("0"),
        );
        assert!(dec(a) < dec(b), "{a} must order below {b}");
    }
}

/// The property below only asserts on values a grammar accepts, so on its own it could hold vacuously if the generator never produced one. These are read off the base mandates in `fixtures/refcases/mandate.json`, and each must both parse and survive `DecStr` unchanged.
#[test]
fn the_normal_form_identity_holds_for_values_the_cases_actually_carry() {
    let rows = [
        ("10000", DecGrammar::PositiveDecimal),
        ("0.1", DecGrammar::OpenFraction),
        ("0.15", DecGrammar::PositiveDecimal),
        ("58000", DecGrammar::PositiveDecimal),
        ("9000", DecGrammar::PositiveDecimal),
        ("1", DecGrammar::UnitPositive),
        ("0.02", DecGrammar::OpenFraction),
        ("0.08", DecGrammar::OpenFraction),
        ("0.03", DecGrammar::OpenFraction),
        ("0.5", DecGrammar::OpenFraction),
        ("0.005", DecGrammar::OpenFraction),
        ("0.01", DecGrammar::OpenFraction),
        ("0.3", DecGrammar::UnitPositive),
        ("0.05", DecGrammar::Fraction),
        ("0.65", DecGrammar::Decimal),
        ("900", DecGrammar::Decimal),
        ("1.5", DecGrammar::Decimal),
        ("20", DecGrammar::Decimal),
        ("0.0001", DecGrammar::PositiveDecimal),
        ("0.150000000000", DecGrammar::PositiveDecimal),
    ];
    let mut accepted = 0;
    for (text, grammar) in rows {
        match SchemaDec::parse(text, grammar) {
            Ok(value) => {
                accepted += 1;
                let normalised = value
                    .to_dec_str()
                    .expect("a grammar value is a journal decimal");
                assert_eq!(
                    normalised.as_str(),
                    text,
                    "`{text}` in `{}` must survive DecStr unchanged",
                    grammar.as_str()
                );
            }
            Err(_) => assert_eq!(
                text, "0.150000000000",
                "only the trailing-zero spelling is expected off its grammar, not `{text}`"
            ),
        }
    }
    assert_eq!(
        accepted, 19,
        "19 of the 20 rows must be accepted, so the identity property below is not vacuous"
    );
}

proptest! {
    /// The property the version hash rests on: a value in its field's grammar is already the journal
    /// decimal's normal form, so parsing it as one changes nothing. If this ever fails, a mandate's
    /// canonical bytes could differ from the text the owner confirmed.
    #[test]
    fn a_schema_dec_is_its_own_dec_str_normal_form(text in generated_decimal()) {
        for grammar in ALL {
            if let Ok(value) = SchemaDec::parse(&text, grammar) {
                let normalised = value.to_dec_str().expect("a grammar value is a journal decimal");
                prop_assert_eq!(
                    normalised.as_str(),
                    value.as_str(),
                    "`{}` is in `{}` but DecStr rewrote it",
                    text,
                    grammar.as_str()
                );
            }
        }
    }

    /// Ordering agrees with an independent oracle that does not look at the text at all: it scales both
    /// values to a common scale and compares them as `i128` integers.
    #[test]
    fn ordering_agrees_with_an_integer_oracle(a in small_decimal(), b in small_decimal()) {
        let (x, y) = (dec(&a), dec(&b));
        prop_assert_eq!(
            x.cmp(&y),
            integer_oracle(&a).cmp(&integer_oracle(&b)),
            "{} against {}",
            a,
            b
        );
    }

    /// Order is total: exactly one of the three relations holds, and it is antisymmetric.
    #[test]
    fn ordering_is_total_and_antisymmetric(a in small_decimal(), b in small_decimal()) {
        let (x, y) = (dec(&a), dec(&b));
        prop_assert_eq!(x.cmp(&y), y.cmp(&x).reverse());
        prop_assert_eq!(x == y, a == b);
    }
}

/// Values at most 4 integer and 4 fractional digits, so the oracle's `i128` scaling is exact.
fn small_decimal() -> impl Strategy<Value = String> {
    (
        prop::bool::ANY,
        0i64..10_000,
        prop::option::of(1u32..10_000),
    )
        .prop_map(|(negative, integer, fraction)| {
            let body = match fraction {
                Some(f) => {
                    let digits = format!("{f:04}").trim_end_matches('0').to_owned();
                    if digits.is_empty() {
                        integer.to_string()
                    } else {
                        format!("{integer}.{digits}")
                    }
                }
                None => integer.to_string(),
            };
            if negative && body != "0" {
                format!("-{body}")
            } else {
                body
            }
        })
}

/// The same value as an integer count of 10^-4, computed from the digits rather than from the type.
fn integer_oracle(text: &str) -> i128 {
    let (sign, body) = match text.strip_prefix('-') {
        Some(rest) => (-1i128, rest),
        None => (1i128, text),
    };
    let (integer, fraction) = body.split_once('.').unwrap_or((body, ""));
    let mut padded = fraction.to_owned();
    while padded.len() < 4 {
        padded.push('0');
    }
    let whole: i128 = integer.parse().unwrap_or(0);
    let part: i128 = padded.parse().unwrap_or(0);
    sign * (whole * 10_000 + part)
}

/// Text that reaches every interesting corner of the grammars, valid and not.
fn generated_decimal() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("0".to_owned()),
        Just("1".to_owned()),
        Just("-0".to_owned()),
        Just("0.020".to_owned()),
        Just("007.50".to_owned()),
        Just(".5".to_owned()),
        Just("1e3".to_owned()),
        small_decimal(),
        "[0-9]{1,3}",
        "-?0\\.[0-9]{1,6}",
        "-?[1-9][0-9]{0,5}(\\.[0-9]{1,6})?",
    ]
}

/// The grammar names and a value's `Display` are what a `ParseError` and a policy violation put in
/// front of an author, so they are pinned rather than left to a mutant.
#[test]
fn every_grammar_names_itself_and_a_value_displays_as_its_text() {
    let names = [
        (DecGrammar::Decimal, "decimal"),
        (DecGrammar::PositiveDecimal, "positive_decimal"),
        (DecGrammar::Fraction, "fraction"),
        (DecGrammar::OpenFraction, "open_fraction"),
        (DecGrammar::UnitPositive, "unit_positive"),
    ];
    for (grammar, name) in names {
        assert_eq!(grammar.as_str(), name, "the `$defs` key is the name");
    }
    for text in ["0", "1", "0.5", "10000"] {
        assert_eq!(
            frac_or_positive(text).to_string(),
            text,
            "a decimal displays as the text the document holds, unchanged"
        );
    }
    assert_eq!(dec("-5").to_string(), "-5");
}

fn frac_or_positive(text: &str) -> SchemaDec {
    SchemaDec::parse(text, DecGrammar::Fraction)
        .or_else(|_| SchemaDec::parse(text, DecGrammar::PositiveDecimal))
        .expect("a fraction or a positive decimal")
}
