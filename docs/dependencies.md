# Dependency registry

Every direct dependency of a workspace crate or Python project needs a row here, added in the same
change that introduces it ([ADR-0001](adr/0001-engineering-setup.md) ES-14). `cargo xtask deps`
fails on any direct dependency without a row, which also blocks misspelled or invented package
names. This file is founder-owned (CODEOWNERS): adding a row is the approval.

Safety-critical crates may also use only the external crates listed in their `allowed_external`
entry in `xtask/layers.toml`.

| Name | Ecosystem | Used by | Purpose | Alternatives considered | License | Approved |
|---|---|---|---|---|---|---|
| `anyhow` | cargo | xtask; mandate-cli | Error context in tooling and binaries only (ES-09) | Plain `Box<dyn Error>` | MIT OR Apache-2.0 | DEC-72 |
| `serde` | cargo | xtask; mandate-marketdata; mandate-cli | Deserializing `cargo metadata`, TOML policy and basket files, and Alpaca response envelopes | Hand parsing | MIT OR Apache-2.0 | DEC-72 |
| `serde_json` | cargo | xtask; mandate-refcases; mandate-marketdata; mandate-canon (dev); mandate-cli (dev) | Reading `cargo metadata` output and the reference-case fixtures, and feeding the differential canonicalizer test; in `mandate-marketdata`, Alpaca responses with every number read as raw text through the `raw_value` feature (ES-23). Never on the hashing path (ES-07); `arbitrary_precision` stays banned | `json` crate | MIT OR Apache-2.0 | DEC-72 |
| `toml` | cargo | xtask; mandate-refcases; mandate-cli | Reading `xtask/layers.toml`, `pyproject.toml`, `crates/mandate-refcases/status.toml`, and `config/research-basket.toml` | `toml_edit` | MIT OR Apache-2.0 | DEC-72 |
| `reqwest` | cargo | mandate-marketdata | HTTPS client for the Alpaca market-data host; `default-features = false`, `rustls-no-provider`, no redirects, HTTPS only. About 109 crates with its TLS stack | `ureq` (blocking), `hyper` directly (more code to own) | MIT OR Apache-2.0 | DEC-88 |
| `rustls` | cargo | mandate-marketdata | TLS with the `ring` provider, installed before the client is built (reqwest's own rustls feature needs aws-lc, whose license set includes OpenSSL); 13 crates | aws-lc-rs (C, OpenSSL license) | Apache-2.0 OR ISC OR MIT | DEC-88 |
| `parquet` | cargo | mandate-marketdata | Writing and reading market-data Parquet with `Decimal128(38, s)` columns (ES-23); only the `arrow` and `snap` features, so no C compression libraries; 48 crates | Arrow IPC (not what research tools read); `polars` (much larger) | Apache-2.0 | DEC-88 |
| `arrow-array` | cargo | mandate-marketdata | Building the typed columns (`Decimal128`, `Timestamp(ns, UTC)`, lists) that `parquet` writes; 32 crates, all shared with `parquet` | The `arrow` umbrella crate (pulls compute kernels) | Apache-2.0 | DEC-88 |
| `arrow-schema` | cargo | mandate-marketdata | Column types and fields for those schemas; 1 crate | The `arrow` umbrella crate | Apache-2.0 | DEC-88 |
| `clap` | cargo | mandate-cli | Argument parsing, validation, and help for the `mandate` binary; derive, without color or suggestions; 19 crates | `lexopt` (no help or validation), `pico-args` | MIT OR Apache-2.0 | DEC-88 |
| `tokio` | cargo | mandate-marketdata; mandate-cli; mandate-journal-pg (dev) | Async runtime in shell crates only, `~1.53` LTS (ES-06): timers for retry backoff and the CLI's current-thread runtime; 13 crates | `async-std` (discontinued); blocking HTTP | MIT | DEC-72 |
| `sqlx` | cargo | mandate-journal-pg | Postgres client, pool, and embedded migrations for the journal hot store (ES-08); `default-features = false` with `postgres`, `runtime-tokio`, `migrate`, and `tls-rustls-ring-native-roots`, no query macros (DEC-109); 48 crates new to the workspace (its SQLite, MySQL, and macro crates are in `Cargo.lock` but never built) | `tokio-postgres` with `refinery` (ES-08 rejected it); sqlx with query macros (needs a database or committed metadata in every build) | MIT OR Apache-2.0 | DEC-109 |
| `secrecy` | cargo | mandate-marketdata | Holding the paper API key ID and secret so `Debug` never prints them (ES-09, AGENTS.md rule 7); 2 crates | A hand-written redacting wrapper | MIT OR Apache-2.0 | DEC-72 |
| `sha2` | cargo | mandate-canon | SHA-256 for event hashes, anchors, and artifact references (ES-07); `default-features = false` | `ring`, `aws-lc-rs` (C and assembly, larger surface) | MIT OR Apache-2.0 | DEC-72 |
| `rust_decimal` | cargo | mandate-num | Storage of typed decimal values behind private-field newtypes (ES-04); `default-features = false`; its arithmetic and `FromStr` are never used for results | `bigdecimal` (heap-allocated, unbounded), `fixed` (binary fractions) | MIT | DEC-72 |
| `ruint` | cargo | mandate-num | 256-bit unsigned intermediates so products and spec formulas are exact and round once (ES-04); `default-features = false` | `primitive-types` (fixed widths only), hand-written 256-bit arithmetic | MIT | DEC-72 |
| `jiff` | cargo | mandate-time | America/New_York rules for trade dates and settlement from the bundled tzdb (ES-05); `default-features = false`, features `std` and `tzdb-bundle-always` so the rules never come from the host | `chrono-tz` (separate tz crate, host-independent but a second time library) | Unlicense OR MIT | DEC-72 |
| `thiserror` | cargo | mandate-canon, mandate-time, mandate-journal, mandate-num, mandate-accounting, mandate-marketdata, mandate-sim, mandate-backtest, mandate-research | Error enums with a stable `code()` per variant (ES-09) | Hand-written `Display` and `Error` impls | MIT OR Apache-2.0 | DEC-72 |
| `libtest-mimic` | cargo | mandate-refcases | One named test per reference case, with pending cases ignored (ES-11) | `datatest-stable` (file-per-case only) | MIT OR Apache-2.0 | DEC-72 |
| `proptest` | cargo | mandate-canon, mandate-time, mandate-journal, mandate-num, mandate-accounting, mandate-marketdata, mandate-sim, mandate-backtest, mandate-research, mandate-artifacts-fs (dev) | Property tests for every invariant, 256 cases per PR (ES-11) | `quickcheck` (weaker shrinking) | MIT OR Apache-2.0 | DEC-72 |
| `serde_json_canonicalizer` | cargo | mandate-canon (dev) | An independent RFC 8785 implementation, used only as the differential oracle for the canonicalizer (ES-07) | `serde_jcs` (unmaintained) | MIT | DEC-72 |
| `pyyaml` | python | mandate-tools | Loading reference-case YAML exactly as the reference implementation does (ES-11) | ruamel.yaml (YAML 1.2 typing differs) | MIT | DEC-72 |
| `pytest` | python | python workspace (dev) | Test runner | unittest | MIT | DEC-72 |
| `ruff` | python | python workspace (dev) | Lint and format | flake8 + black | MIT | DEC-72 |
