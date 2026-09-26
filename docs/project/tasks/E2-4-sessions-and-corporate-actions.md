# Task: E2-4 Market sessions and corporate actions in market data

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. With E2-2 it closes milestone M1.

## Story

- **Story:** E2-4 ([backlog](../06-backlog-v1.md#e2-market-data))
- **Acceptance criteria (verbatim):** "As a researcher, I want corporate actions (splits,
  dividends) and market sessions recorded with the data so that stock history and gaps are
  interpreted correctly. *Accepted when:* `inspect` distinguishes session closures from true gaps;
  split-adjusted and raw prices are both available."
- **PRD / HLD / spec anchors:** trading domain spec §1 principle 2 (venue facts come from the
  broker or effective-dated configuration, never code), §2.1 (rounding), §2.2 (trading calendar
  from the broker; sessions evaluated in America/New_York), §4.2 (a missing regular-session bar is
  "no trade"; `inspect` distinguishes no-trade minutes, session closures, and true gaps), §4.3
  (session hours), §4.5 (raw prices plus explicit corporate actions; point-in-time adjustment),
  §8.5 (integer split ratios new:old; marks adjusted by round(mark × old ÷ new, 12, half_even)).
- **Decisions that apply:** DEC-77 (tests PR, then implementation), DEC-79, DEC-80 (no plain
  comments), DEC-82 (`TradingCalendar`: a date outside the range is an error), DEC-83 (mutants skip
  crates with pending markers; tests PRs hold stubs only), DEC-89 (dataset format; bars at scale 18,
  trades at scale 9). No new decision: the interpretations below follow from the spec and are
  recorded here.

## Scope

The story ships in four PRs, each from `main` and never on another PR's branch (ship playbook).
PR 2 opens after PR 1 merges; PR 3 depends on neither and opens alongside them; PR 4 opens after
PRs 1 to 3 and E2-2 (#64) have merged.

| PR | Crates | Safety-critical | Contents |
|---|---|---|---|
| 1. Sessions, tests | `mandate-time` | yes (tests PR) | `data/us-equities.calendar`, the `session` API as stubs, the pending tests, this brief |
| 2. Sessions, implementation | `mandate-time` | yes (implementation PR) | The parser, `sessions`, `session_at`, `new_york_instant`; test files change only by deleting the markers |
| 3. Corporate actions | `mandate-marketdata` | no | Forward and reverse splits and cash dividends as typed records, every other action type kept by kind, ID, and process date; the Alpaca `/v1/corporate-actions` wire format and paged client fetch; point-in-time split adjustment; recorded fixtures; touches none of the files E2-2 (#64) edits |
| 4. `inspect` wiring | `mandate-marketdata`, `mandate-cli` | no | Opened after E2-2 (#64) merges: gap classification in `inspect`, `download` records corporate actions, raw and adjusted prices in the report, the feature map and tracker rows |

- **Reference cases that must move from pending to passing:** none. No reference case covers
  sessions or market data, so there is no status PR.
- **Invariants touched** (each a named test in `crates/mandate-time/tests/session.rs` whose oracle
  is independent and was shown to fail on a planted bug):

  | Clause | Test |
  |---|---|
  | §2.2 NYSE trading days, holidays, and unscheduled closures | `every_weekday_from_2018_to_2028_trades_exactly_when_nyse_published_it_open`, `holidays_and_unscheduled_closures_have_no_session`, `the_checked_in_calendar_covers_2018_to_2028` |
  | §2.2 early closes (regular 13:00, after-hours 17:00 ET) | `early_closes_end_the_regular_session_at_13_00_and_after_hours_at_17_00`, `the_evening_after_an_early_close_is_closed_until_the_next_overnight_session` |
  | §4.3 session hours, overnight Sunday night to Friday morning | `a_full_day_has_four_sessions_at_the_published_hours`, `a_trading_days_sessions_tile_from_the_previous_evening_to_its_after_hours_close` (property) |
  | §2.2 sessions evaluated in America/New_York across daylight-saving changes | `daylight_saving_changes_move_sessions_in_utc_but_not_in_new_york`, `new_york_instants_reject_times_a_daylight_saving_change_skips_or_repeats` |
  | `session_at` names the session holding any instant, `None` while closed | `session_at_names_the_session_holding_an_instant_at_each_boundary`, `session_at_agrees_with_the_published_schedule_at_every_instant` (property) |
  | DEC-82: a date outside the validity range is an error | `every_date_outside_the_validity_range_is_an_error` |
  | The calendar format rejects malformed data with its line (ES-09 codes) | `a_calendar_needs_only_its_header_and_skips_comments_and_blank_lines`, `malformed_records_are_syntax_errors_on_their_line`, `bad_dates_and_times_are_value_errors_with_their_cause`, `valid_comes_first_and_hours_second_each_exactly_once`, `the_range_and_the_session_hours_must_increase`, `listed_dates_are_weekdays_inside_the_range_in_increasing_order`, `an_early_close_ends_inside_the_usual_sessions`, `line_numbers_count_comments_and_blank_lines`, `new_york_times_are_hh_mm_within_a_day` |

  **Oracles.** The test file keeps its own typed lists of every NYSE closure (106) and early close
  (23) from 2018 to 2028, transcribed from the NYSE holiday pages rather than from the data file;
  its own proleptic-Gregorian date arithmetic and weekday; its own US daylight-saving rule (second
  Sunday of March to first Sunday of November, 02:00 local); and its own session boundaries in UTC
  seconds. The two properties draw 2,000 instants and 2,000 trading days across the validity
  range. `session_names_are_stable` and `calendar_data_errors_have_stable_codes_and_name_their_line`
  test the names and codes the stubs already carry, so they are live in PR 1.

  **Planted bugs**, each planted in a local implementation of the stubs (kept out of PR 1 per
  DEC-83) and caught (failing tests in brackets): Thanksgiving 2026 removed from the data [weekday
  scan, both properties]; the 2019-12-24 early close moved to 14:00 [weekday scan, early-close test,
  both properties]; the after-hours session of an early-close day ends at 20:00 [7 tests, both
  properties among them]; the overnight session starts the same evening instead of the previous
  one [6 tests]; a fixed UTC−5 offset instead of America/New_York [9 tests]; `session_at` never
  looks at the next day's overnight session [4 tests]. `cargo mutants` over PR 2's diff: 91
  mutants, 72 caught, 19 unviable, 0 missed.

- **Crates in scope:** `mandate-time` (PRs 1 and 2: `ExchangeCalendar`, `Session`, `SessionSpan`,
  `NewYorkTime`, `new_york_instant`, `CalendarDataError`; three helpers in `calendar.rs` become
  `pub(crate)` in PR 2), `mandate-marketdata` (PRs 3 and 4), `mandate-cli` (PR 4).
- **Crates out of scope:** `mandate-num` (claimed by another session), `mandate-accounting`
  (its `CorporateAction` input is the fold's; market data keeps its own record of the broker's
  announcements), `mandate-journal`.
- **New dependencies allowed:** none. `mandate-time` already has `jiff` with the bundled time-zone
  database (ES-05).
- **Safety-critical:** `mandate-time` yes, delivered per DEC-77: PR 1 is the **tests PR** (21
  tests marked `#[ignore = "pending E2-4"]`), PR 2 the **implementation PR**. `mandate-marketdata`
  and `mandate-cli` are not safety-critical, so PRs 3 and 4 each carry their code and tests
  together.
- **Size budget:** 400 non-generated lines per safety-critical PR (ES-13). PR 1 exceeds it: it
  holds the story's full session test suite with its oracle tables, reviewed as a unit, as for
  E3-1 to E3-3. The calendar data file is data, not code.

## Calendar data and its provenance

`crates/mandate-time/data/us-equities.calendar` is effective-dated configuration (principle 2):
a validity range, the four session boundaries in ET, and each closure and early close with its
name. It was generated from Alpaca's `GET /v2/calendar` (the broker, §2.2) and then checked line by
line against the NYSE holiday page (live for 2026 to 2028, archived copies for 2018 to 2025), the
NYSE Group notices for the two national days of mourning (2018-12-05, 2025-01-09), the Nasdaq
Trader 2026 schedule, and NYSE Arca's published extended-hours session times. The header lists
every source. The range ends at 2028-12-31, the last year NYSE has published; Alpaca's response is
truncated after that. Times are ET wall-clock times; the code converts each to UTC through the
time-zone database for its own date, so daylight saving is never hard-coded.

## Interpretations

1. **The calendar is broker data checked in.** §2.2 says the calendar comes from the broker;
   principle 2 allows effective-dated configuration. Backtests and `inspect` need a deterministic,
   reviewable calendar, so the broker's answer is recorded with its provenance and a validity
   range, and any date outside it is an error (DEC-82). Refreshing it from the broker at runtime
   belongs to the live runtime's stories.
2. **Overnight belongs to the trading day it ends on.** §4.3 runs overnight from 20:00 to 04:00,
   Sunday night to Friday morning, and §2.2 puts fills from 20:00 into the next trading day. So a
   trading day's sessions are overnight (from 20:00 ET on the previous calendar day), pre-market,
   regular, and after-hours, and a closed day has none: there is no overnight session into a
   holiday, and the Sunday session belongs to Monday.
3. **Early closes.** The regular session ends at the listed time (13:00) and after-hours at the
   listed time (17:00, NYSE Arca); the overnight session into the next trading day still starts at
   20:00.
4. **Gap classification (PR 4).** For each expected bar slot of a symbol and day, `inspect` reports
   a *session closure* when the feed's venue is closed at that minute (no session, or a session the
   feed does not cover, as PR 4 records per feed), *no trade* when the venue is open
   and the day's partition was fetched cleanly, and a *true gap* when the venue is open and the day
   was never fetched or its fetch was not clean.
5. **Split adjustment (PRs 3 and 4).** Point in time (§4.5): as of date D, a bar is adjusted by
   every split with ex-date ≤ D that took effect after the bar started. A split takes effect at
   20:00 ET on the calendar day before its ex-date: the overnight session into the ex-date trades
   post-split (interpretation 2), and §8.5 applies splits at 20:00 ET on the last trading day
   before the ex-date, which differs only by closed days with no bars. Prices become
   round(p × Π old ÷ Π new, 12, half_even) per §8.5's mark rule; volumes become round(v × Π new ÷
   Π old, 18, half_even) at the bar column scale (DEC-89); trade counts are unchanged. Arithmetic
   is exact `i128` scaled units in `mandate-marketdata` (ES-04 keeps `rust_decimal` in
   `mandate-num`), and an overflow is an error, never a rounding. Raw prices are what is stored
   (DEC-89); adjusted prices are derived. Cash dividends are recorded and reported but do not
   adjust prices (the acceptance criterion asks for split-adjusted prices; §8.5 treats dividends
   as cash). Any other action type Alpaca reports (spin-offs, mergers, stock dividends, name
   changes, and the rest) is kept by kind, ID, and process date and reported as not adjusted,
   never silently dropped (§8.5: anything else is out of scope and must be surfaced).

## Not done here (with the story that owns each)

| Story | What |
|---|---|
| E2-2 (#64) | `inspect` itself: coverage, duplicates, statistics; PR 4 adds the classification to it |
| E4-1, E4-2 (backtest) | Feeding corporate actions into the accounting fold at 20:00 ET before the ex-date (§8.5 timing) |
| Live runtime stories | Refreshing the calendar from the broker; halts and LULD (§4.4) |

Also not here: auction windows (§4.3, the gate's), crypto sessions (continuous), the settlement
calendar (DEC-82's `TradingCalendar`, unchanged).

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-time
cargo nextest run -p mandate-time --run-ignored all
MANDATE_BASE_REF=<tests PR head> cargo xtask ci mutants
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

## Definition of done

- [x] The cited reference cases pass, and none that passed before now fails (none are cited).
- [x] Tests came first; each touched invariant has a property test whose oracle is independent and
      was shown to fail on a seeded bug.
- [ ] New state changes emit journal events (none: market data is not journaled state).
- [ ] Docs updated where behavior, interfaces, or decisions changed.
- [ ] `cargo xtask check` is green (paste the summary in the PR).
- [ ] The PR description is complete (see the PR template).
