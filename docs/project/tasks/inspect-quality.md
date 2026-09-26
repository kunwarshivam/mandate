# Task: E2-2 and E2-4 follow-up: `inspect` data-quality reporting (M1 rehearsal)

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). Claim
[#116](https://github.com/kunwarshivam/mandate/issues/116), coordinator `cursor`.

## Story

- **Story:** follow-up to E2-2 ([E2-2-inspect.md](E2-2-inspect.md)) and E2-4
  ([E2-4-sessions-and-corporate-actions.md](E2-4-sessions-and-corporate-actions.md), PR 4 in
  [#96](https://github.com/kunwarshivam/mandate/pull/96)). The Cursor coordinating session ran
  `download` and `inspect` over real Alpaca history on #96's head (a rehearsal of the M1 exit
  run). An independent re-classifier agreed with `inspect` on every one of about 5 million slots,
  and the split checks were exact. The findings below are what the rehearsal showed `inspect`
  getting wrong or leaving out.
- **Acceptance criteria (the findings):**
  - **A.** On an IEX early-close day, the slots from 17:00 to 20:00 ET are session closures, not
    unclassified. Repro: IEX SPY 1-minute bars, 2024-11-29 22:00Z to 00:59Z, 180 slots reported
    unclassified.
  - **B.** Records stamped while the venue is closed are reported. Examples: SPY SIP 1-minute
    bars at 20:00:00 ET (00:00Z on 2024-03-27, 2024-05-01, 2024-06-15, and 2024-06-21); JPM SIP
    trades at 17:00:00.002 ET on the 2024-11-29 early close.
  - **C.** Crypto bar quality is surfaced. In the rehearsal, 88% of BTC/USD 1-minute bars had no
    trades and zero volume, and 39,481 single-trade bars had unequal open, high, low, and close.
  - **D.** Trade size totals that count official-close restatements twice are documented.
    Example: JPM on 2024-11-29 had 11.04M shares in trades against 5.27M in bars, because NYSE's
    closing cross (condition 6) is reported again as the official close (condition M).
  - **E.** `tests/live.rs` no longer reports a pass when it did not run.
  - **F.** A daily dataset's `empty:` line no longer lists every weekend.
- **PRD / HLD / spec anchors:** trading domain spec §1 principle 2 (venue facts are data) and
  principle 3, §4.1 (a bar has OHLC, volume, VWAP, and a trade count), §4.2 ("a bar exists only
  when trades occur"; `inspect` distinguishes no-trade minutes, session closures, and true gaps),
  §4.3; PRD FR-4.5; HLD §9 "Market data service".
- **Decisions that apply:** DEC-89 (raw records as the vendor sent them, never de-duplicated or
  altered), DEC-80, DEC-116. No new decision: each choice below follows from the spec and is
  recorded here.

## Scope

- **Reference cases that must move from pending to passing:** none (no market-data reference
  cases exist).
- **Invariants touched:** see the test table below.
- **Crates in scope:** `mandate-marketdata` (`venue.rs`, `inspect.rs`, `data/*.venue`,
  `tests/live.rs` and its feature) and `mandate-cli` (`inspect.rs`).
- **Crates out of scope:** `mandate-time` (the calendar is right: after-hours ends at 17:00 on an
  early close), everything else.
- **New dependencies allowed:** none.
- **Safety-critical:** no, so code and tests ship in one PR, as in E2-2 and E2-4 PR 4.
- **Size budget:** 800 non-generated lines (ES-13).

## Interpretations and choices

1. **A: open only while both the calendar and the venue say so.** E2-4 interpretation 6 states
   that rule, but PR 4 left IEX unclassified up to 20:00 on an early close, though the calendar's
   after-hours session ends at 17:00 and IEX's own full-day hours end at 17:00. The unclassified
   window now runs from the regular close to the earliest of the recorded
   `unpublished_after_early_close` time (20:00), the venue's close (17:00), and the calendar's
   after-hours end (17:00). Every later minute is closed. `iex.venue` keeps its 20:00 record,
   since it says what IEX leaves unpublished; the code applies the rule.
2. **B: records while the venue is closed are warnings.** A record's time is checked against its
   feed's venue: a bar by its start, a daily bar by its whole day (a daily bar starts at midnight
   ET, which is closed even on a trading day), and a trade or quote by its time. A bar starting at
   the close (20:00:00 for SIP) starts while the venue is closed. The vendor sent the record and
   DEC-89 keeps it, and nothing in the spec says such a record is corrupt. So the report gives the
   count and the first five distinct times, and the exit status does not change. Unclassified
   instants are not counted.
3. **C: zero volume and single-trade spreads are warnings; a bar outside its own range is a
   problem.** §4.1 defines a bar by its open, high, low, and close. A bar whose open or close is
   below its low or above its high is not a bar of any set of trades. The E4-1 fill model rejects
   such a bar outright (`SimError::InconsistentBar` in `crates/mandate-sim/src/fill.rs`), so the
   partition cannot be trusted. It becomes a `Problem`
   (`InconsistentBars`): left out of the statistics, its open hours true gaps, and counted in the
   exit status, like every other problem. The rehearsal found none. A zero-volume bar contradicts
   §4.2's "a bar exists only when trades occur", but it is well formed and the vendor sends it
   (Alpaca's crypto bars carry prices with no trades), so it is a warning. So is a bar whose
   trade count is one but whose prices differ. The recorded BTC/USD bars show why: their prices
   move within a minute that reports one trade, so the vendor's trade count and prices do not
   come from the same trades. For a bar inside its own range, "all four prices equal" is the same
   as "low equals high", and that is what is checked.
4. **D: documented, not excluded.** The trading domain spec defines no trade-condition codes, so
   `inspect` has no rule for which trades restate another. The size total stays over every
   stored trade. Its doc comment and the stock trades report say that a restated official open or
   close counts each time. Stored data is unchanged (DEC-89). Excluding restatements needs a
   condition table as venue data (principle 2); see "Not done here".
5. **E: the live test is built only with the `live-alpaca` feature.** The markers check allows no
   `#[ignore]` except `pending <story>`, and libtest has no runtime skip. So the test was a
   `return` that passed. Now `[[test]] live` has `required-features = ["live-alpaca"]`: without
   the feature, a workspace run lists no live test, and naming it fails with cargo's
   "requires the features" error. With the feature, missing credentials fail the test. CI never
   enables the feature (ES-19), so absent keys cannot fail CI. The `MANDATE_LIVE_ALPACA_DATA`
   switch is gone. One cost: `cargo clippy --all-targets` no longer compiles `tests/live.rs`.
   Lint it with `cargo clippy -p mandate-marketdata --features live-alpaca --all-targets`.
6. **F: listed days without records on closed days are counted.** A listed day with no records
   on which the venue is closed all day (weekends and holidays; never for crypto) leaves the
   `empty:` runs and is reported as `empty while the venue is closed: N days`. A trading day
   without records, and a day the venue's hours do not cover (unclassified), stay in `empty:`.
   Days never fetched stay in `missing:`, whatever the calendar says.

## Tests and planted bugs

| Finding | Test |
|---|---|
| A | `iex_is_closed_from_17_00_to_20_00_after_the_2024_11_29_early_close`, `an_unpublished_early_close_ends_at_the_earliest_of_its_record_the_venue_close_and_after_hours`, `on_an_early_close_sip_closes_at_17_00_and_iex_is_unclassified_from_13_00_to_17_00`, `a_venue_is_open_only_while_both_the_calendar_and_its_own_hours_are`, and the 2026 schedule property in `crates/mandate-marketdata/tests/venue.rs`; `the_iex_evening_after_the_2024_11_29_early_close_is_a_session_closure_from_17_00` (the repro: 227 unclassified slots, then 3,780 closures) in `tests/inspect_quality.rs` |
| B | `spy_sip_bars_starting_at_the_20_00_close_are_records_while_the_venue_is_closed`, `jpm_trades_after_the_17_00_early_close_are_closed_period_records_and_restated_closes_count_again`, `closed_period_records_keep_their_earliest_distinct_times_in_any_vendor_order`, `a_daily_bar_is_a_closed_period_record_only_when_the_market_is_closed_all_day`, `records_of_a_partition_with_a_problem_are_not_warnings` |
| C | `zero_volume_bars_and_single_trade_bars_whose_prices_differ_are_warnings_not_problems`, `zero_volume_is_judged_by_volume_and_a_single_trade_bar_by_its_low_and_high`, `bars_whose_open_or_close_lies_outside_their_range_make_their_partition_a_problem` |
| D | `jpm_trades_after_the_17_00_early_close_are_closed_period_records_and_restated_closes_count_again` (size 4,054,255: the 1,351,385-share cross counted three times) |
| F | `listed_days_without_records_while_the_venue_is_closed_are_counted_not_listed` |
| Report | `warnings_give_their_count_and_first_times_and_bars_outside_their_range_are_a_problem`, `a_crypto_trades_report_has_no_note_on_restated_closes`, and the exact reports in `crates/mandate-cli/tests/inspect.rs` |

The SPY, JPM, and BTC/USD records in `tests/inspect_quality.rs` were recorded from Alpaca's
market-data host on 2026-09-26 and are copied field for field. They are a few records, not whole
days. The other test data is hand-built.

**Oracles.** Each test's expectation comes from the published hours and the recorded values: the
slot counts from the minutes between two instants, the closed instants from the SIP and IEX hours
and the NYSE calendar, and the totals from the recorded sizes. None comes from running the code.

**Planted bugs**, each caught (failing tests in brackets):
- the early-close window ending at the recorded 20:00, the code before this change [6];
- the window ending at the venue's close, ignoring the record [1];
- a daily bar judged at its start instant instead of its day [1];
- unclassified instants counted as closed [1];
- examples kept in arrival order, not the earliest [1];
- a repeated time kept as two examples [1];
- a problem partition's records counted as warnings [1];
- zero volume judged by a zero trade count [1];
- a single-trade bar judged by its open and close instead of its low and high [1];
- the range check exclusive at its ends [21];
- only the open checked against the range [1];
- an inconsistent partition kept in the statistics [2];
- closed empty days still listed as empty [3];
- unclassified empty days counted as closed [1];
- the report's "and N more" counting every occurrence [1];
- the restatement note on crypto trades [1].

`cargo mutants --in-diff` over this change's diff against its merge base, for `mandate-marketdata`
and `mandate-cli`, first missed one mutant: `index < EXAMPLES` as `index <= EXAMPLES` in
`Occurrences::add`, which is equivalent because the truncation that follows drops a sixth example.
The guard is gone. The final run: 39 mutants, 36 caught, 3 unviable, 0 missed.

## Not done here (with the story that owns each)

| Story | What |
|---|---|
| Unowned (needs a backlog item and a spec entry) | A trade-condition table as venue data (principle 2), so `inspect` can total trade size without official open and close restatements and compare it with bar volume |
| The M1 exit run (`claude-code`) | Rerunning `inspect` over the basket and BTC/USD with these warnings |

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-marketdata -p mandate-cli
cargo nextest run -p mandate-marketdata --features live-alpaca --test live
cargo mutants --in-diff <diff against the merge base> -p mandate-marketdata -p mandate-cli
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

## Definition of done

- [x] The cited reference cases pass, and none that passed before now fails (none are cited).
- [x] Each finding has a named test whose oracle is independent and was shown to fail on a
      planted bug; the first planted bug is the code before this change.
- [x] New state changes emit journal events (none: market data is not journaled state).
- [x] Docs updated where behavior, interfaces, or decisions changed.
- [ ] `cargo xtask check` is green (paste the summary in the PR).
- [ ] The PR description is complete (see the PR template).
