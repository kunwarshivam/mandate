# Backtest and evaluation records (journal spec §9.18, E4-5 and E17-8)

- **Spec:** `docs/specs/journal.md` §9.18 (`BacktestRunRecorded` and `EvaluationRunRecorded` on
  the workspace control stream at schema version 1, rules 143 to 145) and §9's catalogue (`rule`
  and `man` for a backtest, `rule` for an evaluation); DEC-1001 item 4, DEC-1002 item 7, and
  DEC-1141 (the writer is a `user` who asked or a `system` runner, refused `schema` at
  `actor.kind` otherwise).
- **Code:** `crates/mandate-journal/src/backtest_records.rs` (both schemas and `rules`), routed by
  `crates/mandate-journal/src/control.rs` (`governs`, `payload`), and
  `crates/mandate-journal/src/catalogue.rs` (both entries). The reference validator is
  `reference/journal/control.py` (`backtest_violations`); `reference/journal/backtest_records.py`
  builds the `backtest_records` section of `docs/specs/reference-cases/journal.yaml` with its own
  oracle and seeded mutants.
- **Tests:** `crates/mandate-journal/tests/backtest_records.rs` (both records append and the stream
  verifies against the section's stored artifacts, each is refused off the control stream, an
  agent's backtest and a backtest bound to another version commit nothing, and every base, valid,
  and invalid draft of the section is judged as it says) and
  `crates/mandate-journal/tests/catalogue.rs` (the required refs).
- **Reference cases:** the `backtest_records` section of `fixtures/refcases/journal.json`, which
  `cargo xtask refcases --write` exports from the YAML.
- **Run:** `cargo nextest run -p mandate-journal -E 'binary(backtest_records) | binary(catalogue)'`;
  the vectors with `python3 reference/journal/generate.py --check` under Python 3.14.
