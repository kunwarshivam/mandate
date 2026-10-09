# Baseline backtest and metrics report

- **Spec:** `docs/specs/trading-domain.md` §2.1 (decimals, one rounding per formula, limit prices on
  the tick), §2.2 (trade dates), §5.1 and §5.3 (the order policy the baseline obeys), §6.2 and §6.3
  (the instants fees are charged at), §8.2 to §8.4 (the backtest mark is the bar close, cash,
  settlement); PRD FR-4.1, FR-4.2, FR-4.5; `docs/specs/journal.md` §12 (`BacktestRunRecorded`,
  appended by a later story). The metric definitions have no spec section yet: they live in
  `docs/project/tasks/E4-2-backtest-baseline.md` and DEC-127.
- **Code:** `mandate-backtest`: `crates/mandate-backtest/src/lib.rs` (the loop's inputs, its per-bar
  order, the run's fills, orders, and observations), `crates/mandate-backtest/src/strategy.rs` (the
  strategy dispatch and the buy-and-hold benchmark),
  `crates/mandate-backtest/src/strategy/ma_crossover.rs` (the division-free moving-average
  crossover and the `Signal` it answers with, in one file so DEC-504's content hash can list it;
  E15-13 M0),
  `crates/mandate-backtest/src/metrics.rs` (every figure of a block),
  `crates/mandate-backtest/src/report.rs` (both blocks, the input digests, the canonical bytes and
  their SHA-256). It drives `mandate-sim`'s fill model and `mandate-accounting`'s fold unchanged; the
  exact arithmetic is `mandate-num`'s (`Ratio`, `Usd::ratio_to`, `Usd::shares_at`,
  `Ratio::sample_variance`, the 12-place roots, and `Price::on_tick` with the Reg NMS table).
- **Tests:** `crates/mandate-backtest/tests/hand.rs` (the brief's fixture and its three degenerate
  series recomputed, the loop's per-bar order, the fee instants, the periods, the signal, the
  benchmark, the identifiers, and the canonical bytes),
  `crates/mandate-backtest/tests/properties.rs` (an `i128` statistics oracle and a sequencer that
  rebuilds the expected input series through `Account`, one property per never-or-always clause),
  `crates/mandate-num/tests/num.rs` (the new arithmetic against the i128 oracle and the brief's
  hand-computed figures). Planted bugs per test: the task brief.
- **Reference cases:** none. The backtest cases `trading_domain::RC-10`, `RC-12`, and `RC-19` belong
  to E4-1, and `fixtures/refcases/trading-domain.json` holds no metrics case.
- **Run:** `cargo nextest run -p mandate-backtest -p mandate-num`; the pending tests with
  `cargo nextest run -p mandate-backtest --run-ignored all`.
