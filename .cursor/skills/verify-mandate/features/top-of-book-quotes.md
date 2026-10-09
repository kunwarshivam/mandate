# Top-of-book quotes

- **Spec:** backlog E2-3; trading domain spec §2.1, §4.1, §4.2, §8.2; DEC-89, DEC-116;
  `docs/project/tasks/E2-3-quotes.md`.
- **Code:** `Quote`, `Kind::Quotes`, and `Records::Quotes` in
  `crates/mandate-marketdata/src/model.rs`; the quotes request and page parsing in
  `crates/mandate-marketdata/src/alpaca.rs`; the quote columns in
  `crates/mandate-marketdata/src/dataset/partition.rs` and their scales in
  `crates/mandate-marketdata/src/dataset.rs`; the statistics (`Values::Quotes`, `Extent`, `Spread`,
  and `QuoteTotals`) in `crates/mandate-marketdata/src/inspect.rs`; `KindArg::Quotes` in
  `crates/mandate-cli/src/download.rs` and the report lines in
  `crates/mandate-cli/src/inspect.rs`. Paging, retries, and storage are the shared paths in
  `crates/mandate-marketdata/src/client.rs` and `Store::put_day`.
- **Tests:** `crates/mandate-marketdata/tests/quotes.rs` against the recorded
  `stock-quotes-*` and `crypto-quotes-*` scenarios in
  `crates/mandate-marketdata/tests/fixtures/alpaca/`, and the quotes tests of
  `crates/mandate-cli/tests/download.rs` and `crates/mandate-cli/tests/inspect.rs`.
- **Run:** `cargo nextest run -p mandate-marketdata --test quotes`,
  `cargo nextest run -p mandate-cli`.
