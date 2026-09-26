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
  | A session span holds its start but not its end | `a_session_span_holds_its_start_but_not_its_end` |
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
  DEC-83) and caught (failing tests in brackets; the properties draw fresh cases on each run, so a
  bug on a single date fails them only when a case lands on it): Thanksgiving 2026 removed from the
  data [4 tests]; the 2019-12-24 early close moved to 14:00 [weekday scan]; the after-hours session
  of an early-close day ends at 20:00 [7 tests, both properties among them]; the overnight session
  starts the same evening instead of the previous one [8 tests]; a fixed UTC−5 offset instead of
  America/New_York [9 tests]; `session_at` never looks at the next day's overnight session [4
  tests]; `SessionSpan::contains` holds its end [3 tests]. `cargo mutants` over PR 2's diff: 75
  mutants, 62 caught, 13 unviable, 0 missed.

- **Crates in scope:** `mandate-time` (PRs 1 and 2: `ExchangeCalendar`, `Session`, `SessionSpan`,
  `NewYorkTime`, `new_york_instant`, `CalendarDataError`; three helpers in `calendar.rs` become
  `pub(crate)` in PR 2), `mandate-marketdata` (PRs 3 and 4), `mandate-cli` (PR 4).
- **Crates out of scope:** `mandate-num` (claimed by another session), `mandate-accounting`
  (its `CorporateAction` input is the fold's; market data keeps its own record of the broker's
  announcements), `mandate-journal`.
- **New dependencies allowed:** none. `mandate-time` already has `jiff` with the bundled time-zone
  database (ES-05).
- **Safety-critical:** `mandate-time` yes, delivered per DEC-77: PR 1 is the **tests PR** (22
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
   feed does not cover, per interpretation 6), *no trade* when the venue is open and the day's
   partition was fetched cleanly, a *true gap* when the venue is open and the day was never fetched
   or its fetch was not clean, and *unclassified* when the venue's published hours do not say
   whether it is open at that minute.
5. **Split adjustment (PRs 3 and 4).** Point in time (§4.5): as of date D, a bar is adjusted by
   every split with ex-date ≤ D that took effect after the bar started. A split takes effect at
   20:00 ET on the calendar day before its ex-date: the overnight session into the ex-date trades
   post-split (interpretation 2), and §8.5 applies splits at 20:00 ET on the last trading day
   before the ex-date, which differs only by closed days with no bars. Prices become
   round(p × Π old ÷ Π new, 12, half_even) per §8.5's mark rule; volumes become round(v × Π new ÷
   Π old, 18, half_even) at the bar column scale (DEC-89); trade counts are unchanged. Prices go
   through `mandate_num::SplitRatio::mark` (DEC-91), so market data and the accounting fold round a
   price the same way. Only volumes are exact `i128` scaled units in `mandate-marketdata` (ES-04
   keeps `rust_decimal` in `mandate-num`). Either way an overflow is an error, never a rounding.
   Raw prices are what is stored
   (DEC-89); adjusted prices are derived. Cash dividends are recorded and reported but do not
   adjust prices (the acceptance criterion asks for split-adjusted prices; §8.5 treats dividends
   as cash). Any other action type Alpaca reports (spin-offs, mergers, stock dividends, name
   changes, and the rest) is kept by kind, ID, and process date and reported as not adjusted,
   never silently dropped (§8.5: anything else is out of scope and must be surfaced).
6. **Each feed's venue hours are data (PR 4).** Like the US-equities calendar, each feed's venue
   hours are a checked-in data file with its sources: the trading days, closures, and early closes
   are the exchange calendar's, and the file gives the venue's session boundaries in ET. SIP covers
   04:00 to 20:00 ET. IEX runs 08:00 to 17:00 ET (pre-market 08:00 to 09:30, regular 09:30 to
   16:00, post-market 16:00 to 17:00; IEX Rule 11.110(a) and IEX's published trading hours).
   Neither feed carries the overnight session, so its slots are session closures for both. On an
   early-close day IEX publishes the 13:00 regular close but not when its post-market ends, so IEX
   slots after 13:00 on those days, up to the next overnight session at 20:00, are reported as
   unclassified, never as no trade or a session closure. Crypto trades continuously and has no
   closures.

   A venue is open while the calendar has a pre-market, regular, or after-hours session *and* its
   own hours say so, so SIP closes at 17:00 on an early-close day because after-hours does. The
   files are `crates/mandate-marketdata/data/sip.venue` and `iex.venue`, each with a validity range;
   any New York date outside it, or outside the calendar, is unclassified. SIP's range ends at
   2026-12-05. From 2026-12-06 the SIPs run 21:00 Sunday to 20:00 Friday (SEC Releases 34-105779
   and 34-105780; UTP Vendor Alert 2026-20). Those hours are not recorded, so later SIP slots are
   unclassified rather than guessed. IEX's range is the calendar's (2018 to 2028), sourced from
   IEX's trading-hours page and Rule 11.110(a) as quoted in SEC Release 34-105959. A slot's class
   comes from the venue at the slot's start. A `1Day` slot is a UTC day strictly between the two
   bars' days, classed by whether the venue trades that day at all.
7. **Corporate actions are stored with the dataset (PR 4).** After storing its days, `download`
   fetches a stock dataset's actions over its whole stored span (first to last listed day, not
   just this run's days) and writes them as canonical JSON (journal spec §4) to
   `corporate-actions.json` in the dataset directory. It compares bytes first, so a rerun changes
   nothing. `inspect` adjusts prices as of the span's last day only when the recorded range covers
   the span. Otherwise it reports the record as incomplete and applies nothing. Its report lists
   every action with whether it was applied: a split after the span's last day is not yet known,
   dividends are cash, and other kinds are not interpreted. A hand-edited or foreign file is
   refused, as the manifest is.

## PR 4: tests and planted bugs

| Clause | Test |
|---|---|
| §4.2 session closures, no trade, true gaps (interpretation 4) | `iex_gaps_split_into_no_trade_closures_true_gaps_and_the_unclassified_early_close_evening`, `the_open_hours_of_a_day_whose_partition_cannot_be_trusted_are_true_gaps`, `a_gap_across_the_equity_overnight_session_is_a_session_closure`, `stretches_are_the_runs_of_missing_slots_each_classed_by_the_venue_and_the_fetch` (property), in `crates/mandate-marketdata/tests/inspect.rs` |
| Daily bars classed by trading day, calendar edges unclassified | `a_skipped_day_is_a_closure_when_the_market_is_closed_and_a_true_gap_when_it_was_not_fetched` |
| Venue hours (interpretation 6) | `sip_and_iex_agree_with_the_published_2026_schedule_at_every_minute` (property), `on_an_early_close_sip_closes_at_17_00_and_iex_is_unclassified_from_13_00_to_20_00`, `minutes_outside_the_recorded_dates_are_unclassified`, `a_venue_is_open_only_while_both_the_calendar_and_its_own_hours_are`, and the parser tests in `tests/venue.rs` |
| §4.5 raw and split-adjusted prices, point in time | `a_stock_dataset_reports_raw_and_split_adjusted_prices_as_of_its_last_day`, `a_split_after_the_last_day_is_not_yet_known_and_adjusts_nothing`, `trades_before_the_split_takes_effect_are_adjusted_and_the_overnight_session_into_the_ex_date_is_not`, `a_price_adjuster_agrees_with_adjust_bar` (property) |
| Stored actions (interpretation 7) | `each_stock_download_records_the_actions_of_every_day_stored_so_far`, `a_failed_actions_fetch_keeps_the_stored_days_and_a_rerun_records_the_actions`, `actions_that_do_not_cover_the_span_give_no_adjusted_prices`, `tests/actions.rs` |
| The report | `crates/mandate-cli/tests/inspect.rs` (`BARS_REPORT`, `each_gap_lists_its_missing_slots_by_class_with_a_total_per_class`, `every_problem_and_trade_duplicate_has_a_line`), `crates/mandate-cli/tests/download.rs` |

**Oracles.** The venue property keeps its own 2026 closures and early closes and its own US
daylight-saving rule, not the calendar file or the time-zone database. The classification
property classes each slot on its own from `Venue::state_at` and the clean-day set, so it checks
the run grouping and the slot grid, while the venue property checks the states.

**Planted bugs**, each caught (failing tests in brackets):
- every day treated as an early close [3 venue tests];
- a venue's close ignoring the calendar's after-hours end [3];
- a date outside the calendar closed rather than unclassified [1];
- an open slot always no trade [5 inspect tests];
- problem days counted as clean [1];
- daily slots running to the next bar's instant rather than its day [1];
- actions applied when they start after the span's first day [1];
- the overnight session into the ex-date still pre-split [2];
- splits after the as-of date applied [3];
- a non-canonical actions file accepted [1];
- an identical actions file rewritten [3];
- actions fetched for this run's days only [2];
- a later split reported as applied [1 CLI test];
- per-class totals counting stretches instead of slots [3].

`cargo mutants --in-diff` over PR 4's diff against its merge base, for `mandate-marketdata` and
`mandate-cli`, first missed 4 mutants:
- the venue's day-cache guard, where dropping it only recomputes the day;
- `GapClass::as_str`, which only the CLI tests checked.

The cache is now keyed by New York date, so a wrong key answers from another day, and
`gap_classes_have_stable_names` pins the names. The final run: 167 mutants, 121 caught, 46
unviable, 0 missed.

## Not done here (with the story that owns each)

| Story | What |
|---|---|
| E2-2 (#64) | `inspect` itself: coverage, duplicates, statistics; PR 4 adds the classification to it |
| E4-1, E4-2 (backtest) | Feeding corporate actions into the accounting fold at 20:00 ET before the ex-date (§8.5 timing) |
| Live runtime stories | Refreshing the calendar from the broker; halts and LULD (§4.4) |
| Unowned (needs a DEC or a backlog item) | The SIP 21:00-Sunday-to-20:00-Friday schedule from 2026-12-06; until its hours are recorded, SIP slots after 2026-12-05 are unclassified |

Also not here:
- auction windows (§4.3, the gate's);
- crypto sessions (continuous);
- the settlement calendar (DEC-82's `TradingCalendar`, unchanged);
- slots before a dataset's first bar or after its last. They are not gaps under E2-2's
  definition, so the report does not list them;
- the [#85](https://github.com/kunwarshivam/mandate/pull/85) review's documentation minor in `mandate-time`: `session.rs` and
  `data/us-equities.calendar` describe overnight as starting at the previous day's after-hours
  close, but it starts at 20:00 even after an early close. It is deferred because another
  session holds a claim on `mandate-time`.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-time
cargo nextest run -p mandate-time --run-ignored all
MANDATE_BASE_REF=<tests PR head> cargo xtask ci mutants
cargo nextest run -p mandate-marketdata -p mandate-cli
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
- [x] New state changes emit journal events (none: market data is not journaled state).
- [x] Docs updated where behavior, interfaces, or decisions changed.
- [ ] `cargo xtask check` is green (paste the summary in the PR).
- [ ] The PR description is complete (see the PR template).
