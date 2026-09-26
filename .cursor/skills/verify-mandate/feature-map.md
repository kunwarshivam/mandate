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
  (buying power and the no-debit rule I4 against an i128 oracle whose holidays come from the
  `us_2026` calendar fixture, with a gated generator and a live guard on the branches it reaches).
- **Reference cases:** `trading_domain::*` in `fixtures/refcases/trading-domain.json`.
- **Run:** `cargo nextest run -p mandate-accounting`.

## Backtest fill model

- **Spec:** `docs/specs/trading-domain.md` §6.4 (the fill model, rules 1 to 9), §2.1 (backtest fill
  prices are not tick-rounded), §4.1 to §4.4 (bars, data profiles, sessions, halts), §8.2 (a
  backtest mark is the bar close); DEC-106 records the choices §6.4 leaves open.
- **Code:** `mandate-sim`: `crates/mandate-sim/src/lib.rs` (bars with their session labels, orders,
  configuration, fills, end states, errors), `crates/mandate-sim/src/fill.rs` (the pure walk over
  bars: timing, sessions, the shared volume cap, market, limit, stop, stop-limit, and OCO rules);
  the arithmetic is `mandate-num`'s (`Fraction`, `Qty::portion`, `Price::slipped`,
  `Bps::sqrt_impact`).
- **Tests:** `crates/mandate-sim/tests/hand.rs` (every order of RC-10, RC-12, and RC-19 recomputed
  by hand, plus the rules those cases do not reach: latency, day-order cancellation, the shared cap,
  the 20-session median, `sqrt` impact, adverse rounding, crypto),
  `crates/mandate-sim/tests/properties.rs` (one property per rule and per never-or-always clause,
  against an order-major `i128` simulator as the oracle), `crates/mandate-num/tests/num.rs` (the
  fill model's arithmetic against an i128 oracle and hand-computed roots),
  `crates/mandate-refcases/tests/harness.rs` (the backtest harness reads every key the cases state).
- **Reference cases:** `trading_domain::RC-10`, `RC-12`, and `RC-19` in
  `fixtures/refcases/trading-domain.json`.
- **Run:** `cargo nextest run -p mandate-sim`, and for the three cases
  `cargo test -p mandate-refcases --test refcases -- --include-ignored --exact trading_domain::RC-10
  trading_domain::RC-12 trading_domain::RC-19`.

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

## Postgres journal

- **Spec:** `docs/specs/journal.md` §5.1, §6.1, §11; ADR-0001 ES-08; DEC-109;
  `docs/project/tasks/E5-3-postgres-journal.md`.
- **Code:** `mandate-journal-pg`: `crates/mandate-journal-pg/src/lib.rs` (`PgJournal`: the append
  protocol in one transaction holding the head row, reads that re-verify stored bytes, `migrator`);
  the schema, roles, and triggers in `migrations/`.
- **Tests:** `crates/mandate-journal/tests/conformance/mod.rs` (the append suite every journal
  backend runs: stream rules, fencing, `risk_clock`, retries, rejections, the chain and append
  vectors, and a differential property against `MemoryJournal`), run by
  `crates/mandate-journal/tests/memory_conformance.rs` and
  `crates/mandate-journal-pg/tests/pg_conformance.rs`; `crates/mandate-journal-pg/tests/pg.rs`
  (migrations, privileges and triggers, database-enforced chain rules, concurrent appenders and
  owners, failed and lost commits, re-verification on read, the tamper vectors replayed on stored
  rows). Setup: `crates/mandate-journal-pg/README.md`.
- **Run:** `MANDATE_PG_URL=postgres://… cargo nextest run -p mandate-journal-pg`, or
  `cargo xtask ci postgres`; without `MANDATE_PG_URL` the Postgres tests skip.

## Verification, anchoring, and export

- **Spec:** `docs/specs/journal.md` §6.2, §10, §11.
- **Code:** `crates/mandate-journal/src/verify.rs`, `crates/mandate-journal/src/merkle.rs`,
  `export_line` in `crates/mandate-journal/src/lib.rs`.
- **Tests:** `crates/mandate-journal/tests/verify.rs`, `crates/mandate-journal/tests/properties.rs`
  (any tampering detected; rewrites caught only by the anchor; independent Merkle construction).
- **Reference cases:** `journal::tamper::*`, `journal::merkle`, `journal::export_line_seq_1`.
- **Run:** `cargo nextest run -p mandate-journal -p mandate-refcases`.

## Content-addressed artifacts

- **Spec:** `docs/specs/journal.md` §6.3, §11 check 6; DEC-107;
  `docs/project/tasks/E5-2-artifact-store.md`.
- **Code:** `crates/mandate-journal/src/artifact.rs` (`ArtifactRef`, the `ArtifactSource` and
  `ArtifactStore` traits, `get_artifact`, the map store); `mandate-artifacts-fs`:
  `crates/mandate-artifacts-fs/src/lib.rs` (`FsArtifactStore`: write-once objects under
  `sha256/<2 hex>/<hex>`, flushed and hard-linked into place).
- **Tests:** `crates/mandate-journal/tests/artifacts.rs` (SHA-256 vectors, round trip, one address
  per content, flipped bits, no overwrite, verification through the store),
  `crates/mandate-artifacts-fs/tests/fs.rs` (the same on disk, plus layout, permissions, missing
  versus unavailable, reopening, concurrent puts, readers during a write, and crashed writes).
  Planted bugs per test: the task brief.
- **Run:** `cargo nextest run -p mandate-journal -p mandate-artifacts-fs`.

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

## Concurrent dataset writes

- **Spec:** DEC-89 (compare-before-write, never replace a stored partition);
  `docs/project/tasks/marketdata-write-safety.md`.
- **Code:** `Store::put_day` and its write helpers in `crates/mandate-marketdata/src/dataset.rs`
  (advisory lock on the dataset directory, unique temporary names, hard-link publish).
- **Tests:** `crates/mandate-marketdata/tests/concurrent_writes.rs` (threads and processes, leftover
  temporary files).
- **Run:** `cargo nextest run -p mandate-marketdata --test concurrent_writes`.

## Dataset inspection

- **Spec:** backlog E2-2; trading domain spec §4.2; DEC-89; `docs/project/tasks/E2-2-inspect.md`.
- **Code:** `crates/mandate-marketdata/src/inspect.rs` (coverage, exact statistics, gaps between
  bars, duplicates, untrusted partitions), `read_manifest` in
  `crates/mandate-marketdata/src/dataset.rs`; `crates/mandate-cli/src/inspect.rs`
  (`mandate inspect`, the text report).
- **Tests:** `crates/mandate-marketdata/tests/inspect.rs` (hand-built datasets through the real
  store; a slot-grid oracle for gaps), `crates/mandate-cli/tests/inspect.rs` (the exact report).
- **Run:** `cargo nextest run -p mandate-marketdata -p mandate-cli inspect`.

## Research-agent spike (E17-0)

- **Spec:** ADR-0002; mandate spec §8.1 to §8.3 (the thesis shape and the sizing idea);
  `docs/project/tasks/RS-1-research-spike.md`. A spike, not product code.
- **Code:** `python/research_spike/src/research_spike/config.py` (basket, caps, model, data dir),
  `python/research_spike/src/research_spike/alpaca.py` (data host and paper host only),
  `python/research_spike/src/research_spike/openrouter.py` (completions, tokens, cost),
  `python/research_spike/src/research_spike/propose.py` (prompt and strict thesis validation),
  `python/research_spike/src/research_spike/journal.py` (hash-chained JSON Lines, artifacts),
  `python/research_spike/src/research_spike/execute.py` (deterministic sizing, journal before
  submit), `python/research_spike/src/research_spike/score.py` (returns versus SPY, cost per thesis),
  `python/research_spike/src/research_spike/__main__.py` (`run`, `score`, `report`, `status`).
- **Tests:** `python/research_spike/tests/` (validation, sizing against hand-computed values, the
  hash chain, scoring on a synthetic series, recorded market-data fixtures).
- **Run:** `cd python && uv run pytest research_spike`; live against the paper account,
  `uv run python -m research_spike run --dry-run` (see `python/research_spike/README.md`).

## Pending tests fail on the stubs

- **Spec:** DEC-77 (tests PR, implementation PR, status PR), DEC-110; the story playbook step 8.
- **Code:** `pending_problems` and `pending_tests` in `xtask/src/main.rs` (markers found on tokens
  in every tracked or untracked `.rs` file, run in one nextest `--run-ignored ignored-only`).
- **Tests:** the `xtask` unit tests (markers in comments, doc comments, strings, raw strings, split
  across lines; result matching) and a fixture workspace in a temporary git repository in which
  committed, uncommitted, and untracked pending tests pass on the stubs.
- **Run:** `cargo nextest run -p xtask`; `cargo xtask ci pending`.

## Repository automation

- **Code:** `xtask`: `xtask/src/main.rs` (every CI job), `xtask/layers.toml` (crate layers and
  safety-critical policy), `.cargo/mutants.toml` (approved equivalent mutants).
- **CI:** `.github/workflows/ci.yml` (`fast`, `full`), `.github/workflows/nightly.yml`.
- **Run:** `cargo xtask check`.
