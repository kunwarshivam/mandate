# Task: E2-3 Top-of-book quotes (first slice: model, client, storage)

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story; this story ships in two PRs, and this brief covers the first.

## Story

- **Story:** E2-3, Should ([backlog](../06-backlog-v1.md#e2-market-data))
- **Acceptance criteria (verbatim):** none written. The story text is "As a researcher, I want
  order-book top-of-book data so that slippage models can use spreads."
- **PRD / HLD / spec anchors:** PRD 6.4 (FR-4.1 backtests on historical data, FR-4.5 reproducible
  from a data snapshot); HLD §9 "Market data service"; trading domain spec §2.1 (prices and
  quantities at most 9 decimal places, never floating point), §2.2 (UTC, nanoseconds), §4.1 (the
  `Quote` type: instrument, timestamp, bid, bid size, ask, ask size, feed), §4.2 (the `sip`, `iex`,
  and `crypto` data profiles), §8.2 (a sane quote is 0 < bid ≤ ask; used by marks, not by storage);
  milestone M1 ([WBS](../02-milestones-and-wbs.md)).
- **Decisions that apply:** DEC-89 (the dataset format: per-day partitions, a canonical-JSON
  manifest with each decimal column's scale, `Decimal128(38, s)`, raw vendor records, compare
  before write, the scoped gitleaks allowlist for recorded page tokens), DEC-88 (dependencies),
  DEC-35 (IEX for paper, SIP for backtests and live), DEC-79, DEC-80. None taken here.

## Scope

- **Reference cases that must move from pending to passing:** none (no market-data reference
  cases exist).
- **Invariants touched** (each a named test in `crates/mandate-marketdata/tests/quotes.rs`):

  | Clause | Test |
  |---|---|
  | Bid and ask price and size are exact decimals; exchanges, conditions, and tape are kept (SIP) | `recorded_sip_quotes_parse_with_exact_prices_sizes_exchanges_conditions_and_tape` |
  | The same for IEX | `recorded_iex_quotes_parse_with_exact_values` |
  | Crypto quotes have no exchange, conditions, or tape; sizes keep 8 fractional digits | `recorded_crypto_quotes_parse_without_exchanges_conditions_or_tape` |
  | Timestamps with 0 to 9 fractional digits parse exactly (marketdata's `parse_rfc3339_utc`); 10 are rejected | `fractional_seconds_of_every_width_parse_exactly` |
  | Locked, crossed, and one-sided quotes are kept, in vendor order, never dropped | `locked_crossed_and_one_sided_quotes_are_kept_in_vendor_order`, and the recorded one-sided quote (ask 0, exchange `" "`) |
  | Malformed and negative numbers, missing and repeated fields are rejected | `malformed_quote_numbers_are_rejected` |
  | Empty days, the missing `quotes` member, another symbol, and a repeated symbol are handled as for bars and trades | `quote_pages_are_checked_like_bar_and_trade_pages` |
  | A quote at the next midnight belongs to the next day; an earlier day's quote is an error | `a_quote_at_the_next_midnight_belongs_to_the_next_day_and_earlier_ones_are_errors` |
  | The dataset's feed picks the request (`feed=sip` or `feed=iex`; none for crypto) and the directory; the transport allows the quotes endpoints only | `the_feed_of_the_dataset_picks_the_quote_request` |
  | Paging follows tokens to the end, in time order | `quote_pages_are_followed_to_the_end_in_time_order` |
  | Rate limits and server errors retry with the existing backoff; order across pages is checked | `rate_limited_quote_requests_retry_with_backoff_and_order_is_checked` |
  | Parquet round trip is exact, including locked, crossed, one-sided, and 9-digit values | `quote_partitions_round_trip_every_value_exactly` |
  | Columns are `Decimal128(38, 9)` and the manifest records `kind = "quotes"` and each scale | `quote_columns_are_decimal128_at_scale_9_and_the_manifest_records_them` |
  | Storing a day again changes no byte; different quotes for a stored day conflict and change nothing | `storing_a_quote_day_again_changes_nothing_and_different_quotes_conflict` |
  | A tenth fractional digit is rejected, not rounded, naming the column and row | `a_quote_value_needing_a_tenth_fractional_digit_is_rejected_not_rounded` |
  | A partition of another kind is not read as quotes, and the reverse | `a_partition_of_another_kind_is_not_read_as_quotes` |
  | `inspect` covers a quotes dataset's days but refuses to summarize its records | `inspect_covers_stored_quote_days_but_does_not_summarize_quotes_yet` |

- **Crates in scope:** `mandate-marketdata`. Not safety-critical under ES-02, so one PR with its
  tests, as E2-1 and E2-2 were.
- **Crates touched only where the compiler requires it:** `mandate-cli` (one match arm in
  `src/inspect.rs`).
- **Crates used, not changed:** `mandate-canon` (`DecStr`), `mandate-time` (`Date`, `UtcNanos`).
- **New dependencies allowed:** none.
- **Safety-critical:** no.
- **Size budget:** ES-13 splits changes over 800 non-generated lines outside safety-critical
  crates. This PR is about 200 lines of source, 700 of tests, and 200 of docs; it stays one PR,
  as E2-2 (#64) and E2-4's corporate actions (#74) did, because the source alone is small and the
  CLI work is already split out.

## Interpretations

1. **The record.** `Quote` holds the §4.1 fields (time, bid price and size, ask price and size;
   the instrument and feed are the dataset's) plus what Alpaca sends for stocks: the bid and ask
   exchange codes, the condition list, and the tape. Crypto quotes carry none of the three, so
   they are `None`. Sizes are stored as Alpaca reports them (shares for stocks, units for crypto).
2. **Raw, not judged.** Quotes are stored as sent: locked, crossed, and one-sided quotes (a zero
   price and size on the side with no order, exchange `" "`) are kept, in vendor order, with no
   de-duplication (DEC-89). The §8.2 sanity rule (0 < bid ≤ ask, spread and staleness limits) is
   applied by the readers that mark with a quote, not by storage.
3. **Scale 9.** Quote prices and sizes are prints, which §2.1 bounds at 9 decimal places, so every
   quote decimal column is `Decimal128(38, 9)` (`PRICE_SCALE`, `SIZE_SCALE`), as for trades; a
   tenth digit is rejected. The recorded BTC/USD quotes need at most 8.
4. **Layout.** A quotes dataset is `<out>/alpaca/<feed>/quotes/<symbol>/`, with the partition and
   manifest rules of DEC-89 unchanged. The manifest's `kind` gains the value `quotes`; `format`
   stays `mandate-marketdata/1`, because no existing dataset's layout or columns change.
5. **Feed.** Stock quotes need `sip` or `iex`, exactly as stock bars and trades; the request's
   `feed` parameter is the dataset's feed. Crypto quotes use `crypto-us` and send no feed.
6. **Crypto fixtures.** A BTC/USD quote day is far above 10,000 rows (so are the other pairs
   sampled), so only its first page is recorded, for parsing tests. Paging and storage are proven
   on a whole SIP day of CPHC (147 quotes, three pages of 50), a whole IEX day (37 quotes), and a
   SIP Saturday (no quotes). `record.sh` gained an optional page count for the first-page case.
7. **`inspect` until the CLI PR.** Adding `Kind::Quotes` and `Records::Quotes` makes the matches
   in `inspect` non-exhaustive. Rather than invent quote statistics ahead of the CLI PR, `inspect`
   returns `InspectError::Unsupported(Kind::Quotes)` (code `unsupported`) for a quotes dataset
   with records, and reports coverage for one with only empty days; `mandate inspect` prints
   "gaps: not applicable to quotes" for that case. These are six added lines in
   `crates/mandate-marketdata/src/inspect.rs` (the error variant, its code, and three match arms)
   and one in `crates/mandate-cli/src/inspect.rs`; no existing line changes.
8. **Shared types.** The change to shared types is additive: `Kind::Quotes`, `Records::Quotes`,
   the quotes scale table and manifest kind in `dataset.rs`, and two endpoints in `http.rs`.
   `Store::put_day` and its write helpers (the code PR #90 changes) are not touched.

## Not done here (the CLI integration PR)

- `mandate download --kind quotes` (the `KindArg` value and its validation in
  `crates/mandate-cli/src/download.rs`).
- `inspect` for quotes: statistics (rows, first and last time, lowest bid and highest ask, and a
  spread summary), duplicates by quote key, and counts of locked, crossed, and one-sided quotes;
  then the CLI report lines.
- A live check of quotes in `tests/live.rs`, and a quotes download in the M1 exit run if wanted.

## Evidence

- Tests first: all 17 tests in `tests/quotes.rs` failed on the API stubs (commit "E2-3 tests"),
  then passed on the implementation.
- `cargo mutants --in-diff` on this PR's `src` diff of `mandate-marketdata` and `mandate-cli`: 38
  mutants, 28 caught, 10 unviable, 0 missed.
- Planted bugs, each caught by the named tests and reverted:

  | Planted bug | Caught by |
  |---|---|
  | Bid and ask prices swapped on the wire | the three recorded-parse tests, `locked_crossed_…`, `malformed_…` |
  | Crossed quotes dropped | `locked_crossed_…`, `recorded_sip_…`, paging, scale, and storage tests |
  | The quote request without the feed | `the_feed_of_the_dataset_picks_the_quote_request`, paging, retries |
  | Stock quotes sent to the trades endpoint | `the_feed_…`, paging |
  | Quote pages after the first dropped | paging, retries |
  | Quote prices stored at the bar scale (18) | scale, round trip, re-write, schema, and `inspect` tests |
  | Bid and ask exchanges swapped on read | `quote_partitions_round_trip_every_value_exactly` |
  | Quote manifests written with kind `trades` | scale and `inspect` tests |
  | Quote manifests not read back | scale and `inspect` tests |
  | The quotes endpoint refused by the transport | `the_feed_…`, `http::only_market_data_paths_are_requested` |
  | A quote timestamp parsed without its fraction | fractional-seconds, midnight, and the three recorded-parse tests |
  | `inspect` summarizing quotes as empty | `inspect_covers_stored_quote_days_but_does_not_summarize_quotes_yet` |

- Fixtures: recorded with the paper market-data credentials by `record.sh` (which fails if a body
  holds either value); `tests/fixtures.rs` scans them; gitleaks passes with the DEC-89 allowlist,
  whose path and page-token patterns already cover the new scenario names.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-marketdata --test quotes
MANDATE_ALPACA_PAPER_KEY_ID=... MANDATE_ALPACA_PAPER_SECRET=... bash crates/mandate-marketdata/tests/fixtures/alpaca/record.sh stock-quotes
```

## Stop conditions

None fired: no new dependency, no deviation from an accepted decision (interpretations 3 and 4
apply DEC-89's rules to a new kind), no test weakened. One existing assertion changed meaning on
purpose: `http::only_market_data_paths_are_requested` listed `/v2/stocks/quotes` as refused; it now
lists the two quotes endpoints as allowed and three neighbouring quote and snapshot paths as
refused.

## Definition of done

- [x] The cited reference cases pass, and none that passed before now fails (none cited).
- [x] Tests came first; each clause above has a named test, and each was shown to fail on a
      planted bug.
- [x] New state changes emit journal events (none: market data is research data).
- [x] Docs updated: this brief, the feature map, the work tracker.
- [x] `cargo xtask check` is green (summary in the PR).
- [x] The PR description is complete.
