# Dependency registry

Every direct dependency of a workspace crate or Python project needs a row here, added in the same
change that introduces it ([ADR-0001](adr/0001-engineering-setup.md) ES-14). `cargo xtask deps`
fails on any direct dependency without a row, which also blocks misspelled or invented package
names. This file is founder-owned (CODEOWNERS): adding a row is the approval.

Safety-critical crates may also use only the external crates listed in their `allowed_external`
entry in `xtask/layers.toml`.

| Name | Ecosystem | Used by | Purpose | Alternatives considered | License | Approved |
|---|---|---|---|---|---|---|
| `anyhow` | cargo | xtask | Error context in tooling (binaries only, ES-09) | Plain `Box<dyn Error>` | MIT OR Apache-2.0 | DEC-72 |
| `serde` | cargo | xtask | Deserializing `cargo metadata` and TOML policy files | Hand parsing | MIT OR Apache-2.0 | DEC-72 |
| `serde_json` | cargo | xtask; mandate-refcases; mandate-canon (dev) | Reading `cargo metadata` output and the reference-case fixtures, and feeding the differential canonicalizer test; never on the hashing path (ES-07) | `json` crate | MIT OR Apache-2.0 | DEC-72 |
| `toml` | cargo | xtask; mandate-refcases | Reading `xtask/layers.toml`, `pyproject.toml`, and `crates/mandate-refcases/status.toml` | `toml_edit` | MIT OR Apache-2.0 | DEC-72 |
| `sha2` | cargo | mandate-canon | SHA-256 for event hashes, anchors, and artifact references (ES-07); `default-features = false` | `ring`, `aws-lc-rs` (C and assembly, larger surface) | MIT OR Apache-2.0 | DEC-72 |
| `rust_decimal` | cargo | mandate-num | Storage of typed decimal values behind private-field newtypes (ES-04); `default-features = false`; its arithmetic and `FromStr` are never used for results | `bigdecimal` (heap-allocated, unbounded), `fixed` (binary fractions) | MIT | DEC-72 |
| `ruint` | cargo | mandate-num | 256-bit unsigned intermediates so products and spec formulas are exact and round once (ES-04); `default-features = false` | `primitive-types` (fixed widths only), hand-written 256-bit arithmetic | MIT | DEC-72 |
| `jiff` | cargo | mandate-time | America/New_York rules for trade dates and settlement from the bundled tzdb (ES-05); `default-features = false`, features `std` and `tzdb-bundle-always` so the rules never come from the host | `chrono-tz` (separate tz crate, host-independent but a second time library) | Unlicense OR MIT | DEC-72 |
| `thiserror` | cargo | mandate-canon, mandate-time, mandate-journal, mandate-num, mandate-accounting, mandate-marketdata | Error enums with a stable `code()` per variant (ES-09) | Hand-written `Display` and `Error` impls | MIT OR Apache-2.0 | DEC-72 |
| `libtest-mimic` | cargo | mandate-refcases | One named test per reference case, with pending cases ignored (ES-11) | `datatest-stable` (file-per-case only) | MIT OR Apache-2.0 | DEC-72 |
| `proptest` | cargo | mandate-canon, mandate-time, mandate-journal, mandate-num, mandate-accounting, mandate-marketdata (dev) | Property tests for every invariant, 256 cases per PR (ES-11) | `quickcheck` (weaker shrinking) | MIT OR Apache-2.0 | DEC-72 |
| `serde_json_canonicalizer` | cargo | mandate-canon (dev) | An independent RFC 8785 implementation, used only as the differential oracle for the canonicalizer (ES-07) | `serde_jcs` (unmaintained) | MIT | DEC-72 |
| `pyyaml` | python | mandate-tools | Loading reference-case YAML exactly as the reference implementation does (ES-11) | ruamel.yaml (YAML 1.2 typing differs) | MIT | DEC-72 |
| `pytest` | python | python workspace (dev) | Test runner | unittest | MIT | DEC-72 |
| `ruff` | python | python workspace (dev) | Lint and format | flake8 + black | MIT | DEC-72 |
