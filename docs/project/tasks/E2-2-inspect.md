# Task: E2-2 Inspect a dataset

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story.

## Story

- **Story:** E2-2 ([backlog](../06-backlog-v1.md#e2-market-data))
- **Acceptance criteria (verbatim):** "`inspect` reports gaps with exact timestamps; tests cover gap
  and duplicate detection."
- **Story text:** "As a researcher, I want to inspect a dataset for coverage, gaps, duplicates, and
  summary statistics so that I trust it before using it."
- **PRD / HLD / spec anchors:** PRD 6.4 (FR-4.1 backtests on historical data, FR-4.5 reproducible
  from a data snapshot); HLD §9 "Market data service" (point-in-time historical store); trading
  domain spec §2.1 (exact decimals), §2.2 (UTC, nanoseconds), §4.1 (Bar and Trade), §4.2 ("a
  missing regular-session bar means 'no trade', not a data gap. `inspect` distinguishes no-trade
  minutes, session closures, and true gaps"); milestone M1 ([WBS](../02-milestones-and-wbs.md)).
- **Decisions that apply:** DEC-89 (the dataset format: per-day partitions, the manifest with each
  day's rows, bytes, and SHA-256, raw records with no de-duplication), DEC-88 (`clap`), DEC-79,
  DEC-80. None taken here.

## Scope

- **Reference cases that must move from pending to passing:** none (no market-data reference
  cases exist).
- **Invariants touched** (each a named test in `crates/mandate-marketdata/tests/inspect.rs` unless
  it names the CLI):

  | Clause | Test |
  |---|---|
  | Gaps are reported with exact timestamps (story) | `gaps_inside_a_day_and_across_the_partition_boundary_name_the_bars_on_either_side`, `a_gap_across_the_equity_overnight_session_is_reported_unclassified` |
  | A gap is exactly a run of empty slots on the bar grid, in any vendor order, repeats ignored | `gaps_are_the_runs_of_empty_slots_on_the_bar_grid` (property; the oracle walks every slot) |
  | Daily bars: a gap means a skipped calendar day, not a 23- or 25-hour interval | `daily_bars_have_a_gap_only_when_a_calendar_day_is_skipped` |
  | Duplicate detection (story), independent of vendor order | `bars_sharing_a_start_are_one_duplicate_whatever_the_vendor_order`, `trades_are_duplicates_only_when_time_id_exchange_and_tape_all_match` |
  | Statistics are exact decimals, compared by value (ES-23; no floats) | `statistics_are_exact_decimals_compared_by_value_not_text` |
  | Coverage: listed, empty, and never-fetched days | `a_clean_dataset_has_no_gaps_duplicates_or_problems`, `an_empty_dataset_lists_its_empty_days_and_has_no_statistics`, `days_never_fetched_inside_the_span_are_missing_runs` |
  | A partition that cannot be trusted is reported and left out of everything else | `an_altered_partition_is_a_problem_and_left_out_of_the_statistics`, `a_missing_partition_and_files_the_manifest_does_not_list_are_problems`, `a_partition_the_manifest_vouches_for_but_that_does_not_decode_is_unreadable`, `a_row_count_that_disagrees_with_the_manifest_is_a_problem`, `records_outside_their_partition_day_are_a_problem` |
  | Only a canonical manifest is read | `a_directory_without_a_valid_manifest_is_an_error` |
  | Deterministic text: fixed line order, every problem kind has a line | CLI `the_report_gives_coverage_exact_statistics_gaps_and_duplicates`, `every_problem_and_trade_duplicate_has_a_line`, `a_dataset_without_days_or_rows_says_so` |
  | The problem count drives the exit status | CLI `a_run_reports_every_dataset_in_order_and_counts_the_problems`, `a_directory_without_a_manifest_fails_the_run_and_names_the_path` |

- **Crates in scope:** `mandate-marketdata` (`inspect`, and `dataset::read_manifest`) and
  `mandate-cli` (`mandate inspect`). Neither is safety-critical under ES-02, so this ships as one
  PR with its tests, as E2-1 did.
- **Crates used, not changed:** `mandate-canon` (`DecStr`, `Digest`; a dev-dependency of
  `mandate-cli` for its fixtures), `mandate-time` (`Date`, `UtcNanos`).
- **New dependencies allowed:** none.
- **Safety-critical:** no.
- **Size budget:** 800 non-generated lines (ES-13).

## Interpretations

1. **Input.** `mandate inspect <dir>...` takes one or more dataset directories
   (`<out>/alpaca/<feed>/<kind>/<symbol>`) and reports them in the order given, then a total. The
   dataset's identity comes from its manifest, which must be the canonical manifest `download`
   writes (DEC-89); anything else is an error.
2. **Coverage.** The span runs from the first day the manifest lists to the last. Inside it, days
   listed with no records are *empty* and days not listed are *missing* (never fetched); both are
   printed as runs of consecutive days.
3. **Gaps.** For bars, a gap is two consecutive bar starts, in time order across partitions,
   further apart than one timeframe; for `1Day`, a gap is a skipped UTC calendar day, because
   daily bars start at local midnight and are 23 or 25 hours apart across daylight-saving changes.
   A gap names the starts of the bars on either side, exactly, in UTC; no bar starts strictly
   between them. Time before the first bar and after the last is not a gap. Trades have no fixed
   interval, so they have no gaps.
4. **Gaps are not classified here.** Spec §4.2 separates no-trade intervals, session closures,
   and true gaps; that needs the session model and is E2-4's acceptance criterion. Until then
   every gap is listed, including equity overnight and weekend closures, and the report says they
   are unclassified.
5. **Duplicates.** Records sharing a key: a bar's start; a trade's time, trade ID, exchange, and
   tape (trades at the same time with different IDs are normal). Each duplicate gives the key, the
   count, and whether the records are identical in every field. Records sit in the partition of
   their UTC day (DEC-89), so keys are compared within a partition.
6. **Statistics.** Rows, first and last timestamp, and, exactly as decimals: for bars the lowest
   low, highest high, total volume, and total trade count; for trades the lowest and highest price
   and total size. They are computed over raw rows, duplicates included, as `i128` units at each
   column's scale; a total that does not fit is an error, never rounded.
7. **Problems.** A listed partition that is missing, whose size or SHA-256 differs from the
   manifest, that does not decode, whose row count differs from the manifest, or that holds
   records outside its day is a problem, and its records are left out of the statistics, gaps,
   and duplicates. A `.parquet` file the manifest does not list is a problem too. `mandate
   inspect` prints every report and exits with an error when any problem was found; gaps and
   duplicates alone do not fail it.
8. **Output.** Readable text only. The backlog does not ask for machine use, so there is no
   `--json`; the library returns a typed `Inspection` for the next consumer.

## Not done here (with the story that owns each)

- Classifying gaps as no-trade, session closure, or true gap, and split-adjusted prices: E2-4.
- `UtcNanos::parse_rfc3339` with fractional seconds (a shared `mandate-time` change): not needed
  here, because `inspect` reads stored nanoseconds.
- Quotes and top of book: E2-3.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-marketdata -p mandate-cli inspect
cargo run -p mandate-cli -- inspect data/alpaca/sip/bars-1Min/SPY data/alpaca/crypto-us/bars-1Min/BTC-USD
```

## Stop conditions

None fired: no new dependency, no deviation from an accepted decision, no test weakened. The
backlog's order (E2-4 before E2-2) is kept in substance by leaving gap classification to E2-4
(interpretation 4).

## Definition of done

- [x] The cited reference cases pass, and none that passed before now fails (none cited).
- [x] Tests came first; each clause above has a named test, and each oracle failed on a planted
      bug (gap comparison, daily gap rule, trade key, minimum, altered check, outside-day count,
      missing days, identical flag, gap state across partitions).
- [x] New state changes emit journal events (none: `inspect` only reads research data).
- [x] Docs updated: this brief, the feature map, the work tracker.
- [x] `cargo xtask check` is green (summary in the PR).
- [x] The PR description is complete.
