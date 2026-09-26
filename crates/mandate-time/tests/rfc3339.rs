//! `UtcNanos::parse_rfc3339`: RFC 3339 with an upper-case `T`, `Z` or a `±HH:MM` offset, and zero to
//! nine fractional digits, which Alpaca sends (claim #91, needed by E2-1). The oracles never call
//! the parser under test: expected instants come from `UtcNanos::from_parts` and whole-second text
//! from `Display` (checked against a naive day count in `time.rs`), and a fraction's nanoseconds are
//! its digits' place values summed. The canonical parser, `UtcNanos::parse`, keeps exactly nine
//! digits (journal spec §4.7).

use mandate_time::{TimeError, UtcNanos};
use proptest::prelude::*;

const MAX_SECS: i64 = 253_402_300_799;
/// 2026-09-21T13:00:00Z.
const SECS: i64 = 1_789_995_600;

fn at(secs: i64, nanos: u32) -> UtcNanos {
    UtcNanos::from_parts(secs, nanos).unwrap()
}

fn rfc(text: &str) -> Result<UtcNanos, TimeError> {
    UtcNanos::parse_rfc3339(text)
}

/// `YYYY-MM-DDTHH:MM:SS` of the whole second `secs`.
fn whole_second_text(secs: i64) -> String {
    at(secs, 0).to_string()[..19].to_owned()
}

/// The nanoseconds named by up to nine fraction digits: each digit times its place value.
fn nanos_of(digits: &str) -> u32 {
    const PLACES: [u32; 9] = [
        100_000_000,
        10_000_000,
        1_000_000,
        100_000,
        10_000,
        1_000,
        100,
        10,
        1,
    ];
    assert!(digits.len() <= PLACES.len(), "{digits}");
    digits
        .bytes()
        .zip(PLACES)
        .map(|(digit, place)| u32::from(digit - b'0') * place)
        .sum()
}

/// `.digits`, or nothing for no digits.
fn dot(digits: &str) -> String {
    if digits.is_empty() {
        String::new()
    } else {
        format!(".{digits}")
    }
}

/// `mandate_marketdata::timestamp::parse_rfc3339_utc` as of `c8efb09`, ported verbatim apart from
/// its error type. `mandate-time` (layer 0) may not depend on `mandate-marketdata` (layer 6), so the
/// differential test runs against this copy.
fn marketdata_parse_rfc3339_utc(raw: &str) -> Option<UtcNanos> {
    const NANO_DIGITS: usize = 9;
    const SECONDS_LEN: usize = "YYYY-MM-DDTHH:MM:SS".len();
    let body = raw.strip_suffix('Z')?;
    let (seconds, fraction) = match body.split_once('.') {
        Some((seconds, fraction)) => (seconds, fraction),
        None => (body, ""),
    };
    let fraction_ok = !body.contains('.')
        || (!fraction.is_empty()
            && fraction.len() <= NANO_DIGITS
            && fraction.bytes().all(|b| b.is_ascii_digit()));
    if seconds.len() != SECONDS_LEN || !fraction_ok {
        return None;
    }
    let canonical = format!("{seconds}.{fraction:0<NANO_DIGITS$}Z");
    UtcNanos::parse(&canonical).ok()
}

/// Whole seconds, with `Z` or an offset, parse exactly as before this change.
#[test]
fn whole_second_forms_parse_as_before() {
    let cases = [
        ("2026-09-21T13:00:00Z", SECS),
        ("2026-09-21T09:00:00-04:00", SECS),
        ("2026-09-21T18:30:00+05:30", SECS),
        ("2026-09-21T13:00:00+00:00", SECS),
        ("2026-09-21T13:00:00-00:00", SECS),
        ("2026-09-21T13:00:00+23:59", SECS - 86_340),
        ("2026-09-21T13:00:00-23:59", SECS + 86_340),
        ("2026-09-22T00:00:00-04:00", SECS + 54_000),
        ("1970-01-01T00:00:00Z", 0),
        ("9999-12-31T23:59:59Z", MAX_SECS),
    ];
    for (text, secs) in cases {
        assert_eq!(rfc(text), Ok(at(secs, 0)), "{text}");
    }
    for text in [
        "2026-09-21T13:00:00+24:00",
        "2026-09-21T13:00:00-24:00",
        "2026-09-21T13:00:00+00:60",
        "2026-09-21T13:00:00-00:60",
        "2026-02-29T13:00:00Z",
        "2026-09-21T13:00:60Z",
    ] {
        assert_eq!(rfc(text), Err(TimeError::InvalidDate), "{text}");
    }
    for text in ["1969-12-31T23:59:59Z", "1970-01-01T00:00:00+00:01"] {
        assert_eq!(rfc(text), Err(TimeError::OutOfRange), "{text}");
    }
}

/// One to nine digits give the instant their place values name, after `Z` or an offset.
#[test]
#[ignore = "pending E2-1"]
fn fractions_of_one_to_nine_digits_parse_exactly() {
    let expected: [u32; 9] = [
        100_000_000,
        120_000_000,
        123_000_000,
        123_400_000,
        123_450_000,
        123_456_000,
        123_456_700,
        123_456_780,
        123_456_789,
    ];
    for (k, nanos) in (1..=9).zip(expected) {
        let digits = &"123456789"[..k];
        for text in [
            format!("2026-09-21T13:00:00.{digits}Z"),
            format!("2026-09-21T09:00:00.{digits}-04:00"),
            format!("2026-09-21T18:30:00.{digits}+05:30"),
        ] {
            assert_eq!(rfc(&text), Ok(at(SECS, nanos)), "{text}");
        }
    }
    for (text, nanos) in [
        ("2026-09-21T13:00:00.000000001Z", 1),
        ("2026-09-21T13:00:00.999999999Z", 999_999_999),
        ("2026-09-21T13:00:00.05Z", 50_000_000),
        ("2026-09-21T13:00:00.9Z", 900_000_000),
        ("2026-09-21T13:00:00.00012Z", 120_000),
    ] {
        assert_eq!(rfc(text), Ok(at(SECS, nanos)), "{text}");
    }
}

/// Trailing zeros change nothing: every spelling of an instant parses to one value, whose text is
/// the canonical nine-digit form (journal spec §4.7).
#[test]
#[ignore = "pending E2-1"]
fn trailing_zeros_name_the_same_instant() {
    let half = [
        "2026-09-21T13:00:00.5Z",
        "2026-09-21T13:00:00.50Z",
        "2026-09-21T13:00:00.500Z",
        "2026-09-21T13:00:00.500000000Z",
        "2026-09-21T09:00:00.5000-04:00",
    ];
    for text in half {
        let t = rfc(text);
        assert_eq!(t, Ok(at(SECS, 500_000_000)), "{text}");
        assert_eq!(
            t.map(|t| t.to_string()).as_deref(),
            Ok("2026-09-21T13:00:00.500000000Z"),
            "{text}"
        );
    }
    let whole = [
        "2026-09-21T13:00:00Z",
        "2026-09-21T13:00:00.0Z",
        "2026-09-21T13:00:00.000Z",
        "2026-09-21T13:00:00.000000000Z",
        "2026-09-21T13:00:00.0+00:00",
    ];
    for text in whole {
        assert_eq!(rfc(text), Ok(at(SECS, 0)), "{text}");
    }
}

/// A shorter fraction is not a smaller one: `.1` is later than `.09`, and `.9` than `.123456789`.
#[test]
#[ignore = "pending E2-1"]
fn shorter_fractions_are_not_smaller() {
    let ordered = [
        "2026-09-21T13:00:00Z",
        "2026-09-21T13:00:00.000000001Z",
        "2026-09-21T13:00:00.09Z",
        "2026-09-21T13:00:00.1Z",
        "2026-09-21T13:00:00.123456789Z",
        "2026-09-21T13:00:00.9Z",
        "2026-09-21T13:00:00.999999999Z",
        "2026-09-21T13:00:01Z",
    ];
    let parsed: Vec<UtcNanos> = ordered.iter().map(|text| rfc(text).unwrap()).collect();
    for pair in parsed.windows(2) {
        assert!(pair[0] < pair[1], "{} < {}", pair[0], pair[1]);
    }
}

/// An offset moves the whole seconds and keeps the fraction, across a day boundary too.
#[test]
#[ignore = "pending E2-1"]
fn offsets_keep_the_fraction() {
    for (text, canonical) in [
        (
            "2026-09-21T09:59:59.999999999-04:00",
            "2026-09-21T13:59:59.999999999Z",
        ),
        (
            "2026-09-21T23:30:00.25-04:00",
            "2026-09-22T03:30:00.250000000Z",
        ),
        (
            "2026-09-22T01:15:00.000001+05:30",
            "2026-09-21T19:45:00.000001000Z",
        ),
        (
            "2024-02-29T23:59:59.5-23:59",
            "2024-03-01T23:58:59.500000000Z",
        ),
    ] {
        assert_eq!(
            rfc(text).map(|t| t.to_string()).as_deref(),
            Ok(canonical),
            "{text}"
        );
    }
}

/// The range is the instant's, 1970-01-01T00:00:00Z to 9999-12-31T23:59:59.999999999Z, whatever
/// the fraction or the offset.
#[test]
#[ignore = "pending E2-1"]
fn range_limits_hold_with_fractions() {
    for (text, secs, nanos) in [
        ("9999-12-31T23:59:59.999999999Z", MAX_SECS, 999_999_999),
        ("9999-12-31T23:59:59.9Z", MAX_SECS, 900_000_000),
        ("9999-12-31T18:59:59.999999999-05:00", MAX_SECS, 999_999_999),
        ("1970-01-01T00:00:00.000000001Z", 0, 1),
        ("1970-01-01T05:30:00.000000001+05:30", 0, 1),
    ] {
        assert_eq!(rfc(text), Ok(at(secs, nanos)), "{text}");
    }
    for text in [
        "1969-12-31T23:59:59.999999999Z",
        "0000-01-01T00:00:00.1Z",
        "1970-01-01T00:59:59.999999999+01:00",
        "1970-01-01T00:00:00.5+00:01",
        "9999-12-31T23:59:59.5-00:01",
    ] {
        assert_eq!(rfc(text), Err(TimeError::OutOfRange), "{text}");
    }
}

/// A fraction does not rescue an impossible date, time of day, or offset: no leap seconds, no
/// 24:00, no offset of 24 hours or 60 minutes.
#[test]
#[ignore = "pending E2-1"]
fn invalid_dates_with_fractions_are_invalid_dates() {
    for text in [
        "2026-02-29T00:00:00.5Z",
        "2026-06-30T23:59:60.5Z",
        "2026-06-30T24:00:00.0Z",
        "2026-06-30T23:60:00.000000001Z",
        "2026-06-30T12:00:00.5+24:00",
        "2026-06-30T12:00:00.5-00:60",
    ] {
        assert_eq!(rfc(text), Err(TimeError::InvalidDate), "{text}");
    }
}

/// Anything else near the form is a syntax error: an empty fraction, ten or more digits, a comma
/// (ISO 8601 allows one; RFC 3339 does not), a sign, space, or second point in the fraction,
/// non-ASCII digits, lower-case `t` or `z`, and a missing or doubled zone.
#[test]
fn malformed_fractions_are_syntax_errors() {
    let long = format!("2026-09-21T13:00:00.{}Z", "1".repeat(40));
    let syntax = [
        "",
        "2026-09-21T13:00:00.Z",
        "2026-09-21T13:00:00.+00:00",
        "2026-09-21T13:00:00.1234567890Z",
        "2026-09-21T13:00:00.0000000000Z",
        "2026-09-21T13:00:00.1234567890-04:00",
        long.as_str(),
        "2026-09-21T13:00:00,5Z",
        "2026-09-21T13:00:00,5+00:00",
        "2026-09-21T13:00:00,Z",
        "2026-09-21T13:00:00.-5Z",
        "2026-09-21T13:00:00.+5Z",
        "2026-09-21T13:00:00. 5Z",
        "2026-09-21T13:00:00.5 Z",
        "2026-09-21T13:00:00.5.5Z",
        "2026-09-21T13:00:00..5Z",
        "2026-09-21T13:00:00.1e3Z",
        "2026-09-21T13:00:00.\u{665}Z",
        "2026-09-21T13:00:00.\u{ff15}Z",
        "2026-09-21T13:00:00.5\u{665}Z",
        "2026-09-21T13:00:00.5",
        "2026-09-21T13:00:00.5z",
        "2026-09-21t13:00:00.5Z",
        "2026-09-21 13:00:00.5Z",
        "2026-09-21T13:00:00.5ZZ",
        "2026-09-21T13:00:00.5Z+00:00",
        "2026-09-21T13:00:00.5+00:00Z",
        "2026-09-21T13:00:00.5+0000",
        "2026-09-21T13:00:00.5+00",
        "2026-09-21T13:00:00.5+00:0",
        "2026-09-21T13:00:00.5+00:000",
        "2026-09-21T13:00:00.5\u{2212}04:00",
        "2026-09-21T13:00.5Z",
        "2026-09-21T13:00:0.5Z",
        "2026-09-21T13:00:00.5-4:00",
        ".5Z",
    ];
    for text in syntax {
        assert_eq!(rfc(text), Err(TimeError::Syntax), "{text:?}");
    }
}

/// The journal's canonical parser does not follow: it still takes exactly nine digits and `Z`.
#[test]
fn the_canonical_parser_still_takes_exactly_nine_digits() {
    for k in (0..=12).filter(|k| *k != 9) {
        let text = format!("2026-09-21T13:00:00{}Z", dot(&"1".repeat(k)));
        assert_eq!(UtcNanos::parse(&text), Err(TimeError::Syntax), "{text}");
    }
    for text in [
        "2026-09-21T13:00:00.000000000+00:00",
        "2026-09-21T09:00:00.000000000-04:00",
        "2026-09-21T13:00:00+00:00",
    ] {
        assert_eq!(UtcNanos::parse(text), Err(TimeError::Syntax), "{text}");
    }
    assert_eq!(
        UtcNanos::parse("2026-09-21T13:00:00.100000000Z"),
        Ok(at(SECS, 100_000_000))
    );
}

/// `mandate-marketdata`'s own fixtures (`tests/timestamp.rs` as of `c8efb09`) give the same
/// results, except that `+00:00`, which that parser rejects, is an offset here.
#[test]
#[ignore = "pending E2-1"]
fn marketdata_fixtures_parse_the_same() {
    for (raw, canonical) in [
        ("2026-09-24T00:00:00Z", "2026-09-24T00:00:00.000000000Z"),
        ("2026-09-24T13:30:00.1Z", "2026-09-24T13:30:00.100000000Z"),
        (
            "2026-09-24T13:30:00.00012Z",
            "2026-09-24T13:30:00.000120000Z",
        ),
        (
            "2026-09-24T13:30:00.193729757Z",
            "2026-09-24T13:30:00.193729757Z",
        ),
        (
            "2262-04-11T23:47:16.854775808Z",
            "2262-04-11T23:47:16.854775808Z",
        ),
    ] {
        assert_eq!(
            rfc(raw).map(|t| t.to_string()).as_deref(),
            Ok(canonical),
            "{raw}"
        );
        assert_eq!(rfc(raw).ok(), marketdata_parse_rfc3339_utc(raw), "{raw}");
    }
    for raw in [
        "2026-09-24T00:00:00",
        "2026-09-24 00:00:00Z",
        "2026-09-24T00:00:00.Z",
        "2026-09-24T00:00:00.1234567891Z",
        "2026-09-24t00:00:00z",
        "2026-02-30T00:00:00Z",
        "2026-09-24T24:00:00Z",
        "1969-12-31T23:59:59Z",
        "",
    ] {
        assert!(rfc(raw).is_err(), "{raw:?}");
        assert_eq!(marketdata_parse_rfc3339_utc(raw), None, "{raw:?}");
    }
    assert_eq!(
        rfc("2026-09-24T00:00:00+00:00")
            .map(|t| t.to_string())
            .as_deref(),
        Ok("2026-09-24T00:00:00.000000000Z")
    );
    assert_eq!(
        marketdata_parse_rfc3339_utc("2026-09-24T00:00:00+00:00"),
        None
    );
}

/// Inputs near the UTC form: valid whole seconds or random digits and separators, then an optional
/// fraction of up to twelve digits or junk, then `Z` or a malformed zone. Never an offset, which
/// `mandate-marketdata`'s parser rejects by design.
fn near_utc_form() -> impl Strategy<Value = String> {
    let seconds = prop_oneof![
        4 => (0i64..=MAX_SECS).prop_map(whole_second_text),
        1 => "[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}",
        1 => "[0-9T:. -]{17,21}",
    ];
    let fraction = prop_oneof![
        1 => Just(String::new()),
        4 => "\\.[0-9]{0,12}",
        1 => "[.,][0-9az+ .,-]{0,4}",
    ];
    let zone = prop_oneof![
        8 => Just("Z"),
        1 => Just(""),
        1 => Just("z"),
        1 => Just("ZZ"),
        1 => Just(" Z"),
    ];
    (seconds, fraction, zone).prop_map(|(s, f, z)| format!("{s}{f}{z}"))
}

proptest! {
    /// Generated UTC timestamps with zero to nine digits parse to the oracle's instant.
    #[test]
    #[ignore = "pending E2-1"]
    fn generated_utc_fractions_parse_to_the_oracle_instant(
        secs in 0i64..=MAX_SECS, digits in "[0-9]{0,9}"
    ) {
        let text = format!("{}{}Z", whole_second_text(secs), dot(&digits));
        prop_assert_eq!(rfc(&text), Ok(at(secs, nanos_of(&digits))), "{}", text);
    }

    /// With any offset from -23:59 to +23:59, the instant is the local time minus the offset and
    /// the nanoseconds are the fraction's.
    #[test]
    #[ignore = "pending E2-1"]
    fn generated_offsets_keep_the_fraction(
        secs in 0i64..=MAX_SECS, digits in "[0-9]{1,9}", minutes in -1_439i64..=1_439
    ) {
        let local = secs + minutes * 60;
        prop_assume!((0..=MAX_SECS).contains(&local));
        let sign = if minutes < 0 { '-' } else { '+' };
        let (h, m) = (minutes.abs() / 60, minutes.abs() % 60);
        let text = format!("{}.{digits}{sign}{h:02}:{m:02}", whole_second_text(local));
        prop_assert_eq!(rfc(&text), Ok(at(secs, nanos_of(&digits))), "{}", text);
    }

    /// The canonical text of every instant is also RFC 3339, and reads back to the same instant.
    #[test]
    #[ignore = "pending E2-1"]
    fn canonical_text_round_trips(secs in 0i64..=MAX_SECS, nanos in 0u32..1_000_000_000) {
        let t = at(secs, nanos);
        prop_assert_eq!(rfc(&t.to_string()), Ok(t));
    }

    /// Parsed instants order as their (seconds, place-value nanoseconds) do; neighbouring seconds
    /// make ties on the second common.
    #[test]
    #[ignore = "pending E2-1"]
    fn order_follows_the_oracle(
        base in 0i64..=MAX_SECS - 2,
        da in 0i64..=2, fa in "[0-9]{0,9}",
        db in 0i64..=2, fb in "[0-9]{0,9}",
    ) {
        let text = |secs: i64, digits: &str| format!("{}{}Z", whole_second_text(secs), dot(digits));
        let a = rfc(&text(base + da, &fa)).unwrap();
        let b = rfc(&text(base + db, &fb)).unwrap();
        prop_assert_eq!(
            a.cmp(&b),
            (base + da, nanos_of(&fa)).cmp(&(base + db, nanos_of(&fb)))
        );
        prop_assert_eq!(a.cmp(&b), a.to_string().cmp(&b.to_string()));
    }

    /// Differential: on inputs without an offset, this parser and `mandate-marketdata`'s accept the
    /// same strings and give the same instants.
    #[test]
    #[ignore = "pending E2-1"]
    fn agrees_with_the_marketdata_parser_without_offsets(raw in near_utc_form()) {
        prop_assert_eq!(rfc(&raw).ok(), marketdata_parse_rfc3339_utc(&raw), "{:?}", raw);
    }
}
