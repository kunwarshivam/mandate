# Timestamps and dates

- **Spec:** `docs/specs/journal.md` §4.7; ADR-0001 ES-05; RFC 3339 §5.6 for `parse_rfc3339`.
- **Code:** `mandate-time`: `crates/mandate-time/src/lib.rs` (`UtcNanos`, `Date`;
  `UtcNanos::parse` takes only the canonical form, `UtcNanos::parse_rfc3339` zero to nine
  fractional digits and an offset).
- **Tests:** `crates/mandate-time/tests/time.rs` (naive day-count oracle);
  `crates/mandate-time/tests/rfc3339.rs` (place-value oracle, differential against a port of the
  market-data parser); `crates/mandate-journal/tests/timestamp_forms.rs` (the journal keeps the
  canonical form and a whole-second `risk_clock`).
- **Run:** `cargo nextest run -p mandate-time`.
