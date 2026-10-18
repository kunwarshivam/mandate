# The backtest runner and the record command (E4-5)

- **Spec:** DEC-1144 (the runner stores artifacts only, the CLI re-checks them and journals
  `BacktestRunRecorded` as the `backtest_runner` system; the input format; the store root from `--store` or `MANDATE_ARTIFACTS`, the cell's
  shared store of #1578, or `artifacts_unset`), DEC-1141 (the
  record's writers), DEC-1000 (a workspace service runs the backtest through the journal, never by
  linking it), DEC-986 (the process groups); `docs/specs/journal.md` §9.18 rules 143 and 144,
  §6.3; backlog E4-4, E4-5.
- **Code:** `mandate-backtest-runner`: `crates/mandate-backtest-runner/src/lib.rs` (the mandate's
  validation in the backtest's context, the run, and what it stores, in order),
  `crates/mandate-backtest-runner/src/inputs.rs` (the five input files, strict),
  `crates/mandate-backtest-runner/src/main.rs` (the `run` command). `mandate-version-backtest`'s
  `VersionObjects` (`crates/mandate-version-backtest/src/report.rs`), the objects the input
  digests are of. `mandate-cli`: `crates/mandate-cli/src/backtest.rs` (`backtest record`: every
  re-check, then the append), `crates/mandate-cli/src/control.rs` (the `backtest_runner` writer,
  for `BacktestRunRecorded` alone).
- **Tests:** `crates/mandate-backtest-runner/tests/run.rs` (the cell's demo mandate over
  `crates/mandate-backtest-runner/tests/fixtures/demo/` completes with every reference stored, a
  refused run stores a refused payload, both pass `mandate-journal`'s draft check, determinism and
  the committed store `crates/mandate-backtest-runner/tests/fixtures/demo-store.json`, every
  input and mandate refusal storing nothing, and the binary), `crates/mandate-cli/tests/backtest_record.rs`
  (the runner's committed store journaled once as the system runner and admitted by
  `mandate-journal`, a refused run, and every refusal appending nothing),
  `crates/mandate-version-backtest/tests/version.rs` (`each_digest_is_of_the_object_a_runner_stores`).
- **Run:** `cargo nextest run -p mandate-backtest-runner -p mandate-cli -p mandate-version-backtest`;
  after a change to the run, `MANDATE_BLESS_RUNNER=1 cargo nextest run -p mandate-backtest-runner`
  rewrites the committed store.
