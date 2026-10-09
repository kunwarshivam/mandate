# Backtest fill model

- **Spec:** `docs/specs/trading-domain.md` §6.4 (the fill model, rules 1 to 9), §2.1 (backtest fill
  prices are not tick-rounded), §4.1 to §4.4 (bars, data profiles, sessions, halts), §8.2 (a
  backtest mark is the bar close); DEC-106 records the choices §6.4 leaves open.
- **Code:** `mandate-sim`: `crates/mandate-sim/src/lib.rs` (bars with their session labels, orders,
  configuration, fills, end states, errors), `crates/mandate-sim/src/fill.rs` (the pure walk over
  bars: timing, sessions, the shared volume cap, market, limit, stop, stop-limit, and OCO rules,
  and `check_sessions_of_asset_class`, which refuses a bar on a session its instrument's asset class
  never trades, spec §4.3, E4-3, DEC-377);
  the arithmetic is `mandate-num`'s (`Fraction`, `Qty::portion`, `Price::slipped`,
  `Bps::sqrt_impact`).
- **Tests:** `crates/mandate-sim/tests/hand.rs` (every order of RC-10, RC-12, and RC-19 recomputed
  by hand, plus the rules those cases do not reach: latency, day-order cancellation, the shared cap,
  the 20-session median, `sqrt` impact, adverse rounding, crypto, and E4-3's session-of-asset-class
  refusal by bar index, code, and message),
  `crates/mandate-sim/tests/properties.rs` (one property per rule and per never-or-always clause,
  against an order-major `i128` simulator as the oracle, and E4-3's refusal against spec §4.3's
  session table), `crates/mandate-num/tests/num.rs` (the
  fill model's arithmetic against an i128 oracle and hand-computed roots),
  `crates/mandate-refcases/tests/harness.rs` (the backtest harness reads every key the cases state).
- **Reference cases:** `trading_domain::RC-10`, `RC-12`, and `RC-19` in
  `fixtures/refcases/trading-domain.json`.
- **Run:** `cargo nextest run -p mandate-sim`, and for the three cases
  `cargo test -p mandate-refcases --test refcases -- --include-ignored --exact trading_domain::RC-10
  trading_domain::RC-12 trading_domain::RC-19`.
