//! The journal keeps the canonical timestamp form (journal spec §4.7) whatever else
//! `UtcNanos::parse_rfc3339` accepts (claim #91): an RFC 3339 spelling that is not exactly
//! `YYYY-MM-DDTHH:MM:SS.nnnnnnnnnZ` is `non_canonical`, and `risk_clock` must also fall on a whole
//! second (mandate spec §5.2, DEC-81).

mod common;

use common::{edit, mark_draft, mark_draft_at};
use mandate_journal::{Draft, InvalidReason};

/// RFC 3339 spellings of 2026-09-21T14:00:01Z, or of a fraction past it, none canonical.
const NOT_CANONICAL: [&str; 10] = [
    "2026-09-21T14:00:01Z",
    "2026-09-21T14:00:01.0Z",
    "2026-09-21T14:00:01.000Z",
    "2026-09-21T14:00:01.00000000Z",
    "2026-09-21T14:00:01.000000000+00:00",
    "2026-09-21T10:00:01.000000000-04:00",
    "2026-09-21T10:00:01-04:00",
    "2026-09-21T14:00:01.5Z",
    "2026-09-21T14:00:01.000000001-00:00",
    "2026-09-21T14:00:01.0000000000Z",
];

fn reason(draft: &[u8]) -> (InvalidReason, String) {
    let e = Draft::parse(draft).unwrap_err();
    (e.reason, e.path)
}

#[test]
fn risk_clock_rejects_every_non_canonical_rfc3339_spelling() {
    for text in NOT_CANONICAL {
        assert_eq!(
            reason(&mark_draft_at(1, "1", text)),
            (InvalidReason::NonCanonical, "payload.risk_clock".to_owned()),
            "{text}"
        );
    }
}

/// A canonical timestamp with any non-zero fraction, one significant digit at each of the nine
/// places, is not a whole second, so it is not a `risk_clock`.
#[test]
fn risk_clock_rejects_a_fraction_at_every_place() {
    for place in 0..9 {
        let digits = format!("{}1{}", "0".repeat(place), "0".repeat(8 - place));
        let text = format!("2026-09-21T14:00:01.{digits}Z");
        assert_eq!(
            reason(&mark_draft_at(1, "1", &text)),
            (InvalidReason::NonCanonical, "payload.risk_clock".to_owned()),
            "{text}"
        );
    }
    let whole = Draft::parse(&mark_draft_at(1, "1", "2026-09-21T14:00:01.000000000Z")).unwrap();
    assert_eq!(
        whole.risk_clock().map(|t| (t.secs(), t.nanos())),
        Some((1_789_999_201, 0))
    );
}

#[test]
fn event_time_rejects_every_non_canonical_rfc3339_spelling() {
    for text in NOT_CANONICAL {
        let draft = edit(
            &mark_draft(1, "1"),
            "event_time",
            Some(&format!("\"{text}\"")),
        );
        assert_eq!(
            reason(&draft),
            (InvalidReason::NonCanonical, "event_time".to_owned()),
            "{text}"
        );
    }
    let fractional = edit(
        &mark_draft(1, "1"),
        "event_time",
        Some("\"2026-09-21T14:00:01.500000000Z\""),
    );
    assert!(
        Draft::parse(&fractional).is_ok(),
        "event_time keeps nanoseconds"
    );
}
