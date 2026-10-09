# Historical market data download

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
  compare-before-write), and `crates/mandate-marketdata/src/download.rs` (one dataset over a day
  range).
  `mandate-cli`: `crates/mandate-cli/src/download.rs` (`mandate download`),
  `config/research-basket.toml`.
- **Tests:** `crates/mandate-marketdata/tests/` against recorded responses in
  `crates/mandate-marketdata/tests/fixtures/alpaca/`; `crates/mandate-cli/tests/download.rs`.
- **Run:** `cargo nextest run -p mandate-marketdata -p mandate-cli`; against the data host with the
  paper credentials, `cargo nextest run -p mandate-marketdata --features live-alpaca --test live`
  (without the feature the live test is not built).
