# Task: E2-1 Download historical bars and trades

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story.

## Story

- **Story:** E2-1 ([backlog](../06-backlog-v1.md#e2-market-data))
- **Acceptance criteria (verbatim):** "`download` fetches Alpaca historical data into Parquet;
  re-running is idempotent."
- **Story text:** "As a researcher, I want to download historical bars and trades for US stocks,
  ETFs, and crypto (starting with a stock/ETF basket and BTC/USD) for a date range so that I can
  backtest."
- **PRD / HLD / spec anchors:** PRD 6.4 (FR-4.1 backtests on historical data, FR-4.5 reproducible
  from a data snapshot); HLD §9 "Market data service" (point-in-time historical store) and §12
  risk 3 (data comes through the user's own Alpaca account, never redistributed); trading domain
  spec §2.1 (prices and quantities at most 9 decimal places), §2.2 (UTC, nanoseconds), §2.3
  (crypto symbols), §4.1 (Bar and Trade fields), §4.2 (data profiles `sip`, `iex`, `crypto`);
  milestone M1 ([WBS](../02-milestones-and-wbs.md)).
- **Decisions that apply:** DEC-23 (Alpaca first), DEC-35 (paper uses IEX; SIP for backtests),
  DEC-72 (ADR-0001: ES-02, ES-06, ES-09, ES-14, ES-19, ES-23), DEC-79 (agents merge after review),
  DEC-80 (no plain comments); taken here: DEC-88 (dependencies), DEC-89 (dataset format and
  idempotency), DEC-90 (research basket).

## Scope

- **Reference cases that must move from pending to passing:** none (no market-data reference
  cases exist).
- **Invariants touched** (each a named test; "never" and "always" clauses from the story, ES-23,
  and AGENTS.md):

  | Clause | Test |
  |---|---|
  | ES-23 vendor numbers never pass through `f64`; converted exactly | `number::vendor_numbers_convert_exactly_where_f64_would_not`, `number::scaled_units_round_trip_to_the_same_decimal` (oracle builds the units from digit strings) |
  | Exact or rejected, never rounded | `number::more_fraction_digits_than_the_scale_are_rejected_not_rounded`, `number::magnitudes_beyond_38_digits_are_rejected`, `number::json_values_that_are_not_numbers_are_rejected` |
  | ES-23 Parquet `Decimal128(38, s)`, scale recorded per dataset | `dataset::columns_are_decimal128_with_the_scales_the_manifest_records`, `dataset::a_value_that_does_not_fit_its_column_fails_the_partition`, `dataset::crypto_bar_aggregates_with_ten_fraction_digits_are_stored_exactly` |
  | Round trip is exact | `dataset::parquet_round_trip_preserves_every_recorded_value_exactly` |
  | Re-running is idempotent (story) | `download::second_run_changes_no_file` (SHA-256 and modification time of every file) |
  | Stored data is never silently replaced | `download::revised_vendor_data_is_a_conflict_and_leaves_the_file_untouched` |
  | Pagination follows every token, once | `client::pagination_follows_tokens_until_null_in_order`, `client::a_repeated_page_token_is_an_error` |
  | Rate limits and server errors back off; client errors do not retry | `client::rate_limits_and_server_errors_retry_with_exponential_backoff`, `client::retries_stop_after_the_attempt_budget`, `client::client_errors_are_not_retried` |
  | AGENTS.md rule 7: credentials never reach logs, fixtures, or errors | `http::credentials_are_redacted_in_debug_output`, `http::missing_credentials_name_the_variable_not_a_value`, `fixtures::recorded_fixtures_contain_no_credentials` |
  | Secret-scan exceptions apply only in their own files: page tokens in the recorded fixture pages, the sentinel key ID in `tests/http.rs` (DEC-89) | `cargo xtask ci supply-chain` (`gitleaks-exceptions`: planted cases in a temporary directory) |
  | ES-23 only the paper and data hosts are compiled in | `http::the_data_host_is_the_only_alpaca_host_in_the_source`, `http::only_market_data_paths_are_requested` |
  | ES-19 CI never calls Alpaca | `live::paper_data_smoke` runs only with `MANDATE_LIVE_ALPACA_DATA=1` |

- **Crates in scope:** `mandate-marketdata` (layer 6, adapter) and `mandate-cli` (layer 7, binary
  `mandate`). Neither is safety-critical under ES-02, so there is no tests-first PR pair and no
  mutation gate.
- **Crates used, not changed:** `mandate-canon` (`DecStr`), `mandate-time` (`Date`, `UtcNanos`,
  `Date::next`). `mandate-num`, `mandate-accounting`, and `mandate-domain` (not created) are out of
  scope; interpretation 8 says which E3-1 types this story uses and why.
- **New dependencies allowed:** those registered by DEC-88: `reqwest`, `rustls`, `parquet`,
  `arrow-array`, `arrow-schema`, `clap` (new), and `tokio` (ES-06), `secrecy` (ES-09), `serde` and
  `serde_json` with `raw_value` (ES-23), `anyhow` (ES-09), `toml` (already registered).
- **Safety-critical:** no.
- **Size budget:** 800 non-generated lines per PR (ES-13, non-safety-critical); recorded fixtures
  are generated. The story is about 4,600 lines with its tests, so it ships as eight PRs against
  `main`, one after another, each with its own tests: (1) this brief, the decisions, exact numbers
  and timestamps; (2) the dataset model; (3) Alpaca page parsing and the recorded fixtures;
  (4) pagination and retries; (5) the HTTPS transport and credentials; (6) Parquet partitions;
  (7) the manifest, idempotent writes, and `download`; (8) the `mandate download` command.

## Interpretations and decisions

1. **Date range.** `--start` and `--end` are UTC dates, both inclusive; each day is the half-open
   interval `[D 00:00Z, D+1 00:00Z)`. A record stamped exactly at the next midnight belongs to the
   next day and is dropped from this one; a record outside the day is a vendor error. Days must be
   complete: `--end` must be before today's UTC date, so a partition never changes because it was
   fetched while still open (DEC-89).
2. **Partitions.** One Parquet file per symbol per UTC day, fetched by its own paginated query.
   Session interpretation (ET days, early closes) is E2-4's, on E3-1's `TradingCalendar`.
3. **Feeds.** Stocks and ETFs require `--feed sip` or `--feed iex`, with no default: SIP is the
   backtest profile and IEX the paper profile (DEC-35), and the agent paper account can read
   historical SIP older than 15 minutes (checked 2026-09-26). Crypto uses Alpaca's `us` location,
   recorded as feed `crypto-us`.
4. **Raw data.** Bars use `adjustment=raw` (spec §4.5: raw prices plus explicit corporate
   actions). Records are stored as the vendor sent them, in vendor order, with no de-duplication,
   which is E2-2's job to report.
5. **Scales.** Trade prices and sizes use scale 9 (spec §2.1). Every bar column (open, high, low,
   close, volume, VWAP) uses scale 18: bars are vendor aggregates that the spec does not bound,
   and the first live run found BTC/USD 1-minute bars with ten fractional digits in prices and
   VWAP and nine in volume. A value needing more digits than its column's scale is rejected,
   never rounded, and the download fails (DEC-89).
6. **Idempotency.** Every run re-fetches and compares bytes before writing. Identical bytes leave
   the file untouched; a new partition is written atomically (temporary file, fsync, rename);
   different bytes for a stored partition are a conflict that fails the run and leaves the file
   as it was. There is no overwrite flag: stored research data is never replaced in place.
7. **Types.** `Bar` and `Trade` live in `mandate-marketdata` with `DecStr` numbers until
   `mandate-domain` exists; then they move there (DEC-89).
8. **E3-1 types (merged before this story).** Used: `mandate_time::Date::next` for the next UTC
   day, replacing a local `next_day` that did the same arithmetic. Not used, each recorded in
   DEC-89:
   - `mandate_num::Price` and `Qty`: they hold at most 9 fractional digits and bars need 18
     (interpretation 5). Trades would fit, but `mandate-num` has no integer-unit accessor, so
     filling a `Decimal128` column would go back through text, and `number::to_units` already
     rejects a tenth fractional digit.
   - `mandate_time::TradingCalendar`: it needs a holiday table, and `main` has none outside tests.
     Crypto trades every UTC day, and a stock's closed day is stored as an empty day (0 rows, no
     file). Skipping closed days is E2-4's, with the session rules.
   - `UtcNanos::parse_rfc3339`: it takes no fractional seconds, and Alpaca sends up to nine.
   - `mandate_accounting::AssetClass`: the same two variants as the dataset model's `AssetClass`,
     but market data should not depend on accounting, and the dataset model also needs ordering
     and text forms. DEC-82 moves `AssetClass` to `mandate-domain` once a second crate needs it,
     and this is that second crate. Moving it takes a type out of a safety-critical crate, so it is
     left to the story that creates `mandate-domain`, which should merge both definitions.

## Not done here (with the story that owns each)

- `inspect` (coverage, gaps, duplicates, statistics): E2-2.
- Corporate actions, market sessions, split-adjusted prices: E2-4.
- Quotes and top of book: E2-3.
- Instrument IDs (the broker `asset_id` from the paper trading host's asset API) and symbol
  history: with the instrument loader; files are keyed by symbol for now.
- Reading `~/.config/mandate/paper.env` or the OS keychain (ES-19): environment variables only.
- `tracing` JSON logs: the CLI prints a per-partition summary to stdout and logs nothing.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-marketdata -p mandate-cli
MANDATE_LIVE_ALPACA_DATA=1 cargo nextest run -p mandate-marketdata --test live
cargo run -p mandate-cli -- download --basket config/research-basket.toml --asset-class us-equity \
  --kind bars --timeframe 1Min --feed sip --start 2026-09-21 --end 2026-09-25 --out data
```

## Stop conditions

Handled per DEC-79 ("decide, record, continue"): new dependencies are DEC-88; the dataset
interpretations above are DEC-89; the basket is DEC-90. No test was weakened and no accepted
decision is deviated from.

## Definition of done

- [x] The cited reference cases pass, and none that passed before now fails (none cited).
- [x] Tests came first; each clause above has a named test.
- [x] New state changes emit journal events (none: downloads are research data, not journaled
      state).
- [x] Docs updated: this brief, the decision log, `docs/dependencies.md`, the feature map.
- [x] `cargo xtask check` is green (summary in the PR).
- [x] The PR description is complete.
