# Feature map

What exists, where it lives, what proves it, and how to run it. `cargo xtask ci lint` checks that
every workspace crate and reference-case suite has an entry and that every path named here exists.

## Canonical JSON and hashing

- **Spec:** `docs/specs/journal.md` §4; ADR-0001 ES-07.
- **Code:** `mandate-canon`: `crates/mandate-canon/src/parse.rs` (strict parser),
  `crates/mandate-canon/src/write.rs` (canonical writer), `crates/mandate-canon/src/lib.rs`
  (value tree, keys, SHA-256 `Digest`).
- **Tests:** `crates/mandate-canon/tests/canon.rs` (vectors, rejections, differential against
  `serde_json_canonicalizer`, scrambled spellings).
- **Reference cases:** `journal::string_escaping` in `fixtures/refcases/journal.json`.
- **Run:** `cargo nextest run -p mandate-canon`.

## Journal decimals

- **Spec:** `docs/specs/journal.md` §4.6; ADR-0001 ES-04.
- **Code:** `crates/mandate-canon/src/dec.rs` (`DecStr`: normalize, reject, never round).
- **Tests:** `crates/mandate-canon/tests/decimal.rs`.
- **Reference cases:** `journal::decimal::*`.
- **Run:** `cargo nextest run -p mandate-canon decimal`.

## Timestamps and dates

- **Spec:** `docs/specs/journal.md` §4.7; ADR-0001 ES-05.
- **Code:** `mandate-time`: `crates/mandate-time/src/lib.rs` (`UtcNanos`, `Date`).
- **Tests:** `crates/mandate-time/tests/time.rs` (naive day-count oracle).
- **Run:** `cargo nextest run -p mandate-time`.

## Typed decimals

- **Spec:** `docs/specs/trading-domain.md` §2.1; ADR-0001 ES-04.
- **Code:** `mandate-num`: `crates/mandate-num/src/lib.rs` (private-field newtypes `Qty`,
  `SignedQty`, `Price`, `Usd`, `CostBasis`, `FeeRate`, `FeePerShare`, `Bps`, `FeeCap`; 12-place
  `MarkPrice`, `SplitRatio`, `ShareIncrement`, and the `SplitQty` result of a split; canonical
  text in, exact-or-error operations), `crates/mandate-num/src/exact.rs` (256-bit intermediates,
  one rounding per formula).
- **Tests:** `crates/mandate-num/tests/num.rs` (i128 oracle for every operation and rounding mode).
- **Run:** `cargo nextest run -p mandate-num`.

## Trading calendars and trade dates

- **Spec:** `docs/specs/trading-domain.md` §6.2, §6.3; ADR-0001 ES-05.
- **Code:** `crates/mandate-time/src/calendar.rs` (`TradingCalendar`, equity trade date with the
  20:00 New York cutoff, settlement date, New York midnight through jiff's bundled tzdb).
- **Tests:** `crates/mandate-time/tests/calendar.rs` (naive DST-rule and day-count oracle).
- **Run:** `cargo nextest run -p mandate-time calendar`.

## Accounting

- **Spec:** `docs/specs/trading-domain.md` §6, §7.2 (account type and buying power), §8
  (invariants I1 to I7 in §8.6), §12 (the journal events that feed the fold); task briefs
  `docs/project/tasks/E3-1-accounting.md`, `docs/project/tasks/E3-2-corporate-actions.md`, and
  `docs/project/tasks/E3-3-settlement.md`.
- **Code:** `mandate-accounting`: `crates/mandate-accounting/src/lib.rs` (inputs, corporate
  actions, fee configuration, account type, reservations, errors),
  `crates/mandate-accounting/src/account.rs` (the pure fold: positions, cash buckets, fee accrual
  and charges, splits, dividends, cash in lieu, receivables, income, realized and unrealized P&L,
  equity, buying power).
- **Tests:** `crates/mandate-accounting/tests/hand.rs` (hand-calculated cases, partial fills, flips),
  `crates/mandate-accounting/tests/properties.rs` (one property per invariant against an i128
  ledger oracle), `crates/mandate-accounting/tests/corporate_actions.rs` (hand-calculated splits,
  dividends, and cash in lieu), `crates/mandate-accounting/tests/corporate_action_properties.rs`
  (I1, I3, I5, I6 and the split, mark, dividend, and receivable rules against an i128 oracle),
  `crates/mandate-accounting/tests/settlement.rs` (hand-calculated buying power in cash and margin
  accounts, pending charges, reservations), `crates/mandate-accounting/tests/settlement_properties.rs`
  (buying power and the no-debit rule I4 against an i128 oracle, with a gated generator).
- **Reference cases:** `trading_domain::*` in `fixtures/refcases/trading-domain.json`.
- **Run:** `cargo nextest run -p mandate-accounting`.

## Journal drafts and the event catalogue

- **Spec:** `docs/specs/journal.md` §2, §3, §9.
- **Code:** `mandate-journal`: `crates/mandate-journal/src/draft.rs` (envelope validation),
  `crates/mandate-journal/src/catalogue.rs` (stream types, required `config_refs`),
  `crates/mandate-journal/src/schema.rs` (field types and payload schemas; a schema is registered
  by the story that first emits the event).
- **Tests:** `crates/mandate-journal/tests/append.rs`, `crates/mandate-journal/tests/catalogue.rs`
  (the spec table written out again).
- **Run:** `cargo nextest run -p mandate-journal`.

## Append protocol

- **Spec:** `docs/specs/journal.md` §5.1.
- **Code:** `MemoryJournal::append` and `seal` in `crates/mandate-journal/src/lib.rs`.
- **Tests:** `crates/mandate-journal/tests/properties.rs` (append-only, gapless, fenced,
  idempotent against a model), `crates/mandate-journal/tests/append.rs`.
- **Reference cases:** `journal::chain::*`, `journal::append::*`.
- **Run:** `cargo nextest run -p mandate-journal -p mandate-refcases`.

## Verification, anchoring, and export

- **Spec:** `docs/specs/journal.md` §6.2, §10, §11.
- **Code:** `crates/mandate-journal/src/verify.rs`, `crates/mandate-journal/src/merkle.rs`,
  `export_line` in `crates/mandate-journal/src/lib.rs`.
- **Tests:** `crates/mandate-journal/tests/verify.rs`, `crates/mandate-journal/tests/properties.rs`
  (any tampering detected; rewrites caught only by the anchor; independent Merkle construction).
- **Reference cases:** `journal::tamper::*`, `journal::merkle`, `journal::export_line_seq_1`.
- **Run:** `cargo nextest run -p mandate-journal -p mandate-refcases`.

## Reference-case harness

- **Spec:** ADR-0001 ES-11; DEC-77 (pending and passing cases).
- **Code:** `mandate-refcases`: `crates/mandate-refcases/src/journal.rs` (one interpretation per
  prose case), `crates/mandate-refcases/src/trading_domain.rs` (fills, marks, fee charges,
  settlement, corporate actions, dividends, cash-in-lieu postings, the account type, and buying
  power; every other step type and expectation key fails as "not interpreted until" its owning
  story), `crates/mandate-refcases/tests/refcases.rs`, `crates/mandate-refcases/tests/harness.rs`
  (the harness reads the account type and checks `buying_power`: RC-08 and RC-18's cash variant
  without their gate step), `crates/mandate-refcases/status.toml` (founder-owned).
- **Suites:** `fixtures/refcases/journal.json` (46 cases, all passing),
  `fixtures/refcases/trading-domain.json` (accounting cases from E3-1 and E3-2; the rest
  pending their stories), `fixtures/refcases/mandate.json` (not yet harnessed; its story adds it).
- **Run:** `cargo nextest run -p mandate-refcases`; pending cases with
  `cargo test -p mandate-refcases -- --include-ignored`.

## Reference-case fixtures and the mandate reference implementation

- **Spec:** `docs/specs/mandate.md`; ADR-0001 ES-10, ES-11.
- **Code:** `python/mandate_tools/src/mandate_tools/export_refcases.py` (YAML to JSON, strict
  loader); `reference/mandate/` (founder-owned reference implementation).
- **Tests:** `python/mandate_tools/tests/test_export_refcases.py`; `reference/mandate/fuzz.py`,
  `reference/mandate/check_cases.py`, `reference/mandate/mutants.py`.
- **Run:** `cargo xtask refcases`, `cargo xtask ci reference`.

## Historical market data download

- **Spec:** backlog E2-1; ADR-0001 ES-19, ES-23; DEC-88, DEC-89, DEC-90;
  `docs/project/tasks/E2-1-download.md`.
- **Code:** `mandate-marketdata`: `crates/mandate-marketdata/src/number.rs` (raw JSON number text to
  `DecStr` and `Decimal128` units, exact or rejected), `crates/mandate-marketdata/src/timestamp.rs`
  (vendor timestamps, UTC days), `crates/mandate-marketdata/src/model.rs` (datasets, symbols, feeds,
  timeframes, day ranges, bars and trades), `crates/mandate-marketdata/src/alpaca.rs` (request
  paths, page parsing), `crates/mandate-marketdata/src/client.rs` (pagination, retries, backoff),
  `crates/mandate-marketdata/src/http.rs` (the market-data host only, credentials),
  `crates/mandate-marketdata/src/dataset.rs` and
  `crates/mandate-marketdata/src/dataset/partition.rs` (Parquet partitions, manifest,
  compare-before-write), `crates/mandate-marketdata/src/download.rs` (one dataset over a day range).
  `mandate-cli`: `crates/mandate-cli/src/download.rs` (`mandate download`),
  `config/research-basket.toml`.
- **Tests:** `crates/mandate-marketdata/tests/` against recorded responses in
  `crates/mandate-marketdata/tests/fixtures/alpaca/`; `crates/mandate-cli/tests/download.rs`.
- **Run:** `cargo nextest run -p mandate-marketdata -p mandate-cli`; against the data host with the
  paper credentials, `MANDATE_LIVE_ALPACA_DATA=1 cargo nextest run -p mandate-marketdata --test
  live`.

## Repository automation

- **Code:** `xtask`: `xtask/src/main.rs` (every CI job), `xtask/layers.toml` (crate layers and
  safety-critical policy), `.cargo/mutants.toml` (approved equivalent mutants).
- **CI:** `.github/workflows/ci.yml` (`fast`, `full`), `.github/workflows/nightly.yml`.
- **Run:** `cargo xtask check`.
