# Quant model host (E15-13)

- **Spec:** mandate spec §8.1 and §8.2 (the pin, the content hash, `as_of` as the data cut-off);
  `docs/project/tasks/first-paper-trade.md` ("The model host", slices M0 to M2, FT-4, FT-5);
  DEC-503, DEC-504, DEC-517 (the last completed session), DEC-518 (the content object and when
  its hash is pinned), DEC-2390 (`quant.ma_crossover` 1.1.0, whose `Flat` is an exit output).
- **Code:** `mandate-modelhost`, `crates/mandate-modelhost/` (layer 8, safety-critical, pure):
  `src/lib.rs` (`content`,
  `evaluate`, `Evaluation`, `Refusal`), `src/ma_crossover.rs` (1.0.0's own host code and the listed
  sources), `src/ma_crossover_exit.rs` (1.1.0: 1.0.0's object with conviction −1 on `Flat`),
  with the crossover itself in `crates/mandate-backtest/src/strategy/ma_crossover.rs`.
- **Tests:** `crates/mandate-modelhost/tests/host.rs` (the content object against canonical JSON
  written by hand from the files on disk, 1.0.0's content hash pinned as a literal (DEC-518 item 1),
  the output mapping, `as_of` on early closes, weekends and
  a given calendar) and `crates/mandate-modelhost/tests/refusals.rs` (one refusal per failed check
  in the brief's order, each refusal's stable code, FT-4 over all 63 sets of identity changes, the
  check order over every pair of stages, and the signal against an `i128` oracle with ties and a
  clock-independence check), and `crates/mandate-modelhost/tests/exit.rs` (1.1.0's content object
  written by hand, its `Long` and its exit output on a `Flat` or a tie, 1.0.0 unchanged beside it,
  a pin or registry naming the other version's hash refused, and both versions against an
  integer-cent crossover), with fixtures in `tests/common/mod.rs`.
- **Run:** `cargo nextest run -p mandate-modelhost`.
