# Liquidity figures

- **Spec:** trading-domain spec §3.2 item 5 and §9.6; DEC-470 item 4; DEC-471 item 3.
- **Code:** `crates/mandate-liquidity/src/lib.rs` (pure typed inputs and the prior close,
  lower-median 20-session dollar volume, truncated 20-session average volume, and trailing
  five-minute volume of complete bars). `crates/mandate-shell/src/paper/facts.rs` validates the
  stored dataset, parses its close and volume fields, maps broker minute bars, and orchestrates.
- **Tests:** the in-module hand cases in `crates/mandate-liquidity/src/lib.rs`, plus the mapping and
  refusal cases in `crates/mandate-shell/src/paper/facts/tests.rs`.
- **Run:** `cargo nextest run -p mandate-liquidity -p mandate-shell`.
