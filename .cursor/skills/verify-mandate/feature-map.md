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
  `SignedQty`, `Price`, `Usd`, `CostBasis`, `FeeRate`, `FeePerShare`, `Bps`, `FeeCap`; canonical
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

- **Spec:** `docs/specs/trading-domain.md` §6, §8 (invariants I1 to I7 in §8.6), §12 (the journal
  events that feed the fold); task brief `docs/project/tasks/E3-1-accounting.md`.
- **Code:** `mandate-accounting`: `crates/mandate-accounting/src/lib.rs` (inputs, fee
  configuration, errors), `crates/mandate-accounting/src/account.rs` (the pure fold: positions,
  cash buckets, fee accrual and charges, realized and unrealized P&L, equity).
- **Tests:** `crates/mandate-accounting/tests/hand.rs` (hand-calculated cases, partial fills, flips),
  `crates/mandate-accounting/tests/properties.rs` (one property per invariant against an i128
  ledger oracle).
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
  prose case), `crates/mandate-refcases/src/trading_domain.rs` (fills, marks, fee charges, and
  settlement; every other step type and expectation key fails as "not interpreted until" its
  owning story), `crates/mandate-refcases/tests/refcases.rs`, `crates/mandate-refcases/status.toml`
  (founder-owned).
- **Suites:** `fixtures/refcases/journal.json` (46 cases, all passing),
  `fixtures/refcases/trading-domain.json` (accounting cases from E3-1; the rest pending their
  stories), `fixtures/refcases/mandate.json` (not yet harnessed; its story adds it).
- **Run:** `cargo nextest run -p mandate-refcases`; pending cases with
  `cargo test -p mandate-refcases -- --include-ignored`.

## Reference-case fixtures and the mandate reference implementation

- **Spec:** `docs/specs/mandate.md`; ADR-0001 ES-10, ES-11.
- **Code:** `python/mandate_tools/src/mandate_tools/export_refcases.py` (YAML to JSON, strict
  loader); `reference/mandate/` (founder-owned reference implementation).
- **Tests:** `python/mandate_tools/tests/test_export_refcases.py`; `reference/mandate/fuzz.py`,
  `reference/mandate/check_cases.py`, `reference/mandate/mutants.py`.
- **Run:** `cargo xtask refcases`, `cargo xtask ci reference`.

## Repository automation

- **Code:** `xtask`: `xtask/src/main.rs` (every CI job), `xtask/layers.toml` (crate layers and
  safety-critical policy), `.cargo/mutants.toml` (approved equivalent mutants).
- **CI:** `.github/workflows/ci.yml` (`fast`, `full`), `.github/workflows/nightly.yml`.
- **Run:** `cargo xtask check`.
