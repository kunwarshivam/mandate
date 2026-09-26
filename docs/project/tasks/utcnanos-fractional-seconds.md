# Task: `UtcNanos` fractional seconds in RFC 3339 (shared crate `mandate-time`)

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). A shared-crate change
with its own claim: [#91](https://github.com/kunwarshivam/mandate/issues/91) (coordinator
`cursor`), per the [coordination playbook](../../../.cursor/skills/mandate-mode/playbooks/coordination.md)
§2 and the allocation in #88.

## Story

- **Story:** none of its own. The tracker's known issue, verbatim: "`UtcNanos` parses RFC 3339
  only without fractional seconds; Alpaca sends up to nine". The story that needs it is **E2-1**
  ([backlog](../06-backlog-v1.md)): its Alpaca wire parser works around the gap with
  `mandate_marketdata::timestamp::parse_rfc3339_utc`. Pending tests carry `pending E2-1`, because
  the DEC-77 marker grammar takes a story ID.
- **Acceptance criteria:** `UtcNanos::parse_rfc3339` reads zero to nine fractional digits exactly;
  nothing that validates the journal's canonical form gets looser.
- **Spec anchors:** journal spec §4.7 (the canonical form `YYYY-MM-DDTHH:MM:SS.nnnnnnnnnZ`, exactly
  nine digits); mandate spec §5.2 and DEC-81 (`risk_clock` is a whole second, enforced by the
  journal); RFC 3339 §5.6 (`time-secfrac = "." 1*DIGIT`); ADR-0001 ES-05 (own parser, no clock).
- **Decisions that apply:** DEC-72 (ES-05, ES-11, ES-15), DEC-77 (two-PR mechanics), DEC-80 (no
  plain comments), DEC-110 (pending tests must fail). No new decision: the change implements the
  RFC 3339 grammar the function already claims, within the nanosecond precision the type has.

## Interpretation

1. **Grammar.** `YYYY-MM-DDTHH:MM:SS[.f]Z` or `YYYY-MM-DDTHH:MM:SS[.f]±HH:MM`, where `.f` is one to
   nine ASCII digits. Upper-case `T` and `Z` only, as before (RFC 3339 allows lower case; no vendor
   or case file uses it). `-00:00` is UTC, as before.
2. **Exact or error.** Ten or more digits are a `syntax` error, never rounded or truncated, even
   when the extra digits are zeros: `UtcNanos` cannot hold them and the caller should know. An
   empty fraction (`12:00:00.Z`) and a comma separator (ISO 8601 allows one; RFC 3339 does not)
   are `syntax` errors.
3. **Trailing zeros.** Missing places read as zeros, so `.5`, `.50`, and `.500000000` are one
   instant, and its text is always the canonical nine-digit form (`Display`, journal spec §4.7).
4. **Range** is the instant's (1970-01-01T00:00:00Z to 9999-12-31T23:59:59.999999999Z), after the
   offset, whatever the fraction. Unchanged limitation: the local date must itself be 1970 to 9999,
   so `1969-12-31T23:30:00-01:00` (an in-range instant) is `out_of_range`, as it was before.
5. **Error order**, unchanged: a `syntax` error first, then an impossible offset
   (`invalid_date`), then the local date and time (`invalid_date` or `out_of_range`), then the
   instant's range (`out_of_range`).
6. **The canonical parser does not move.** `UtcNanos::parse` still takes exactly nine digits and
   `Z`. The journal validates `Timestamp` and `RiskClock` fields with it, so every RFC 3339
   spelling other than the canonical one stays `non_canonical`, and `risk_clock` still needs
   `nanos() == 0`.

## Callers audited

Every caller of either parser, and whether anything relied on fractions being rejected:

| Caller | Parser | Effect |
|---|---|---|
| `mandate-journal` `schema.rs` `Ty::Timestamp`, `Ty::RiskClock` | `parse` | None; pinned by `timestamp_forms.rs` (below) |
| `mandate-journal` `draft.rs` `Draft::risk_clock` | `parse` | None (reads fields `Ty::RiskClock` has already checked) |
| `mandate-refcases` `journal.rs` `timestamp` | `parse` | None |
| `mandate-refcases` `trading_domain.rs`: step `at`, bar `start`, `decided_at` | `parse_rfc3339` | Case files may now write fractions; none does, and no check relied on rejecting one (a fractional value used to be a harness error, not a case outcome) |
| `mandate-sim`, `mandate-accounting` test helpers | `parse_rfc3339` | None (whole-second literals) |
| `mandate-marketdata` `timestamp.rs` | `parse` on its own padded text | None; its workaround can now go (follow-up, not in this change: #86 holds that crate) |
| `mandate-artifacts-fs` tests | `parse` | None |
| `mandate-canon` | neither | Parses no timestamps |
| `mandate-time` `tests/calendar.rs` `rfc3339_rejects_other_forms` | `parse_rfc3339` | Pinned the old limitation with `.5Z` and `.000000000Z`; the tests PR replaces those two rows with `.Z` and a ten-digit fraction, which stay rejected, and the pending tests assert the new values of the old rows |

## Scope

- **Reference cases that must move:** none. All trading-domain cases must stay passing.
- **Invariants touched** (oracles: `UtcNanos::from_parts` plus `Display` for whole seconds, which
  `time.rs` checks against a naive day count, and place values summed for the fraction; the
  differential oracle is a verbatim port of `mandate-marketdata`'s parser, because layer 0 may not
  depend on layer 6):

  | Clause | Test (`crates/mandate-time/tests/rfc3339.rs` unless named) |
  |---|---|
  | One to nine digits, exact | `fractions_of_one_to_nine_digits_parse_exactly`, `generated_utc_fractions_parse_to_the_oracle_instant` |
  | Trailing zeros, canonical text | `trailing_zeros_name_the_same_instant`, `canonical_text_round_trips` |
  | Ordering | `shorter_fractions_are_not_smaller`, `order_follows_the_oracle` |
  | Offsets keep the fraction | `offsets_keep_the_fraction`, `generated_offsets_keep_the_fraction` |
  | Range limits | `range_limits_hold_with_fractions` |
  | Impossible dates, times, offsets | `invalid_dates_with_fractions_are_invalid_dates` |
  | Ten or more digits, empty fraction, comma, other near misses | `malformed_fractions_are_syntax_errors`, `calendar::rfc3339_rejects_other_forms` |
  | Same results as the marketdata parser | `marketdata_fixtures_parse_the_same`, `agrees_with_the_marketdata_parser_without_offsets` |
  | Whole seconds unchanged | `whole_second_forms_parse_as_before`, `calendar::rfc3339_offsets_are_subtracted` |
  | Canonical parser unchanged | `the_canonical_parser_still_takes_exactly_nine_digits`, `time::rejects_malformed_timestamps` |
  | Journal keeps the canonical form; `risk_clock` a whole second | `mandate-journal` `timestamp_forms::risk_clock_rejects_every_non_canonical_rfc3339_spelling`, `timestamp_forms::risk_clock_rejects_a_fraction_at_every_place`, `timestamp_forms::event_time_rejects_every_non_canonical_rfc3339_spelling` |

- **Crates in scope:** `mandate-time` (`UtcNanos::parse_rfc3339` only).
- **Crates out of scope:** `mandate-marketdata` (claim #86), `mandate-journal` source (tests only),
  `mandate-refcases`, `mandate-canon`, `xtask`.
- **New dependencies allowed:** none.
- **Safety-critical:** yes. Tests PR: the tests above, 13 of them pending. Implementation PR:
  `parse_rfc3339` and two private helpers; test files only lose the pending markers.
- **Size budget:** 400 non-generated lines per PR (ES-13).

## Stubs in the tests PR

No new API, so no stub: the stub is today's `parse_rfc3339`, which rejects every fraction. The 13
pending tests all fail on it (`cargo xtask ci pending`), and all at that one guard, so the planted
bugs below carry the per-test evidence. The six non-pending tests pass on it and must keep passing.

## Planted bugs

Each bug was planted in the implementation, the `rfc3339`, `calendar`, `time`, `timestamp_forms`,
`append`, and `catalogue` test binaries were run with pending tests included, and the bug was
reverted. Every pending test is caught by at least one bug. This is on top of `cargo mutants` with
zero missed.

| Bug | Change | Caught by |
|---|---|---|
| P1 | ten or more digits accepted, truncated to nine | `malformed_fractions_are_syntax_errors`, `marketdata_fixtures_parse_the_same`, `agrees_with_the_marketdata_parser_without_offsets`, `calendar::rfc3339_rejects_other_forms` |
| P2 | empty fraction `.Z` accepted | `malformed_fractions_are_syntax_errors`, `marketdata_fixtures_parse_the_same`, `agrees_with_the_marketdata_parser_without_offsets`, `calendar::rfc3339_rejects_other_forms` |
| P3 | fraction read as an integer (`.5` is 5 ns) | `fractions_of_one_to_nine_digits_parse_exactly`, `trailing_zeros_name_the_same_instant`, `shorter_fractions_are_not_smaller`, `offsets_keep_the_fraction`, `range_limits_hold_with_fractions`, `marketdata_fixtures_parse_the_same`, and four properties: `generated_utc_fractions_parse_to_the_oracle_instant`, `generated_offsets_keep_the_fraction`, `order_follows_the_oracle`, `agrees_with_the_marketdata_parser_without_offsets` |
| P4 | comma accepted as the separator | `malformed_fractions_are_syntax_errors`, `agrees_with_the_marketdata_parser_without_offsets` |
| P5 | fraction dropped when an offset is given | `fractions_of_one_to_nine_digits_parse_exactly`, `trailing_zeros_name_the_same_instant`, `offsets_keep_the_fraction`, `range_limits_hold_with_fractions`, `generated_offsets_keep_the_fraction` |
| P6 | offset added instead of subtracted | `whole_second_forms_parse_as_before`, `offsets_keep_the_fraction`, `generated_offsets_keep_the_fraction`, and three more |
| P7 | instant built without the range check | `range_limits_hold_with_fractions`, `whole_second_forms_parse_as_before` |
| P8 | offset hour 24 accepted | `invalid_dates_with_fractions_are_invalid_dates`, `whole_second_forms_parse_as_before` |
| P9 | canonical `parse` falls back to RFC 3339 | `the_canonical_parser_still_takes_exactly_nine_digits`, `time::rejects_malformed_timestamps`, `timestamp_forms::risk_clock_rejects_every_non_canonical_rfc3339_spelling`, `timestamp_forms::event_time_rejects_every_non_canonical_rfc3339_spelling`, `append::draft_rejections` |
| P10 | journal `risk_clock` read with `parse_rfc3339` | `timestamp_forms::risk_clock_rejects_every_non_canonical_rfc3339_spelling` |
| P11 | journal `risk_clock` loses the whole-second check | `timestamp_forms::risk_clock_rejects_a_fraction_at_every_place`, `append::risk_clock_is_a_whole_second_and_marks_require_it`, `catalogue::registered_schemas_accept_their_payloads` |
| P12 | journal `Timestamp` read with `parse_rfc3339` | `timestamp_forms::event_time_rejects_every_non_canonical_rfc3339_spelling`, `append::draft_rejections` |
| P13 | only three digits kept (milliseconds) | `fractions_of_one_to_nine_digits_parse_exactly`, `canonical_text_round_trips`, `shorter_fractions_are_not_smaller`, `marketdata_fixtures_parse_the_same`, and five more |
| P14 | nine digits rejected (at most eight) | `canonical_text_round_trips`, `invalid_dates_with_fractions_are_invalid_dates`, `order_follows_the_oracle`, and nine more |
| P15 | offset sign ignored (always east) | `whole_second_forms_parse_as_before`, `offsets_keep_the_fraction`, `generated_offsets_keep_the_fraction`, and three more |

## Commands

```bash
cargo nextest run -p mandate-time -p mandate-journal --run-ignored all
cargo xtask ci pending
cargo xtask check
```

## Not done

- Removing `mandate_marketdata::timestamp::parse_rfc3339_utc` in favour of this parser: a
  follow-up in the tracker, after #86 (another builder holds that crate).
- Lower-case `t` and `z`, and local dates before 1970 with a negative offset (interpretation 1
  and 4): unchanged, and no caller needs them.

## Definition of done

- [x] No reference case changes state; all passing cases stay passing.
- [x] Tests came first; each clause has a named test, and every pending test is caught by a planted
      bug.
- [x] No new state changes, so no journal events.
- [x] Docs: the `parse_rfc3339` doc comment, the feature map, and the tracker.
- [ ] `cargo xtask check` green on each PR (summaries in the PRs).
