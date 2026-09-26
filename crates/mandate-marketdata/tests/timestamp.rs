//! Vendor timestamps parse exactly, through `UtcNanos::parse_rfc3339` and nothing of this crate's
//! own (#98); Parquet's `i64` nanoseconds are range-checked (DEC-89).

mod common;

use std::fs;

use mandate_marketdata::timestamp::{
    TimestampError, day_start, from_unix_nanos, parse_rfc3339_utc, to_unix_nanos,
};
use mandate_time::{Date, UtcNanos};

#[test]
fn vendor_timestamps_with_zero_to_nine_fraction_digits_parse_exactly() {
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
    ] {
        assert_eq!(
            parse_rfc3339_utc(raw).map(|t| t.to_string()),
            Ok(canonical.to_owned()),
            "{raw}"
        );
    }
}

#[test]
fn offsets_and_malformed_timestamps_are_rejected() {
    for raw in [
        "2026-09-24T00:00:00+00:00",
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
        assert!(parse_rfc3339_utc(raw).is_err(), "{raw:?}");
    }
}

#[test]
fn unix_nanos_round_trip_and_overflow_is_an_error() {
    let t = parse_rfc3339_utc("2026-09-24T13:30:00.193729757Z").unwrap();
    assert_eq!(to_unix_nanos(t), Ok(1_790_256_600_193_729_757));
    assert_eq!(from_unix_nanos(1_790_256_600_193_729_757), Ok(t));
    assert_eq!(
        from_unix_nanos(i64::MAX).map(|t| t.to_string()),
        Ok("2262-04-11T23:47:16.854775807Z".to_owned())
    );
    let late = parse_rfc3339_utc("2262-04-11T23:47:16.854775808Z").unwrap();
    assert!(to_unix_nanos(late).is_err());
    assert!(from_unix_nanos(-1).is_err());
}

#[test]
fn days_start_at_midnight_utc() {
    for day in ["2026-09-24", "2028-02-29", "9999-12-31"] {
        assert_eq!(
            day_start(Date::parse(day).unwrap()).map(|t| t.to_string()),
            Ok(format!("{day}T00:00:00.000000000Z"))
        );
    }
}

#[test]
fn the_vendor_parser_agrees_with_utc_nanos_parse_rfc3339_on_every_recorded_timestamp() {
    let mut seen = 0_u64;
    for scenario in fs::read_dir(common::fixtures_dir()).unwrap() {
        let dir = scenario.unwrap().path();
        if !dir.is_dir() {
            continue;
        }
        for page in fs::read_dir(&dir).unwrap() {
            let path = page.unwrap().path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let body = fs::read_to_string(&path).unwrap();
            for raw in rfc3339_strings(&body) {
                assert_eq!(
                    parse_rfc3339_utc(&raw).map_err(|_| ()),
                    UtcNanos::parse_rfc3339(&raw).map_err(|_| ()),
                    "{}: {raw}",
                    path.display()
                );
                seen = seen.saturating_add(1);
            }
        }
    }
    assert!(seen > 500, "the recorded pages hold {seen} timestamps");
}

/// Every JSON string of the recorded bodies that ends in `Z`, which is how Alpaca writes an
/// instant; dates (`YYYY-MM-DD`) and tokens do not.
fn rfc3339_strings(body: &str) -> Vec<String> {
    body.split('"')
        .skip(1)
        .step_by(2)
        .filter(|s| s.ends_with('Z') && s.contains('T'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_vendor_parser_delegates_to_mandate_time_and_keeps_no_parser_of_its_own() {
    let source = include_str!("../src/timestamp.rs");
    assert!(
        source.contains("UtcNanos::parse_rfc3339"),
        "the vendor parse must go through mandate-time (#98)"
    );
    for retired in ["fraction_ok", "NANO_DIGITS", "split_once('.')", "canonical"] {
        assert!(
            !source.contains(retired),
            "`{retired}` is part of the retired RFC 3339 workaround"
        );
    }
}

#[test]
fn an_offset_is_refused_here_although_mandate_time_accepts_it() {
    for raw in [
        "2026-09-24T13:30:00+00:00",
        "2026-09-24T13:30:00.5-04:00",
        "2026-09-24T13:30:00+05:30",
    ] {
        assert!(UtcNanos::parse_rfc3339(raw).is_ok(), "{raw}");
        assert_eq!(parse_rfc3339_utc(raw), Err(TimestampError::Syntax), "{raw}");
    }
}
