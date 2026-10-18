# The demo, end to end, on the local cell (E21-54)

- **Spec:** the managed-cells plan (`docs/project/tasks/managed-cells.md`), DEC-967, DEC-995 (the
  2026-10-26 demo), DEC-2002, DEC-2004, DEC-2010, DEC-2023; journal spec §11 for the closing
  verification.
- **Code:** `xtask/src/cell/scenario.rs` (`cargo xtask cell demo --scenario demo`, `cargo xtask ci
  cell-demo`), the demo's envelope and instrument under `deploy/cell/demo/`, and the backtest
  runner's demo inputs and golden store under `crates/mandate-backtest-runner/tests/fixtures/`.
- **Tests:** `xtask/src/cell/scenario.rs`'s unit tests: the demo files are canonical JSON and the
  envelope is paper and asks every opening; the plan runs the six steps in order and types only the
  codes it was shown; step 6 kills the demo's agent at agent scope only, before the record is
  exported; every gesture is in the run's workspace and never live; each run takes a fresh ULID
  workspace founded for the demo owner; step 1 checks the API's refusal with no session and the
  owner's sign-in, and names the signed-in read; step 5 registers a rule set, runs the backtest
  runner on its committed demo inputs and records the payload it stores, which is pinned to the
  runner's golden store; every gap cites what of it is on `main` and waits on
  a named lane with its backlog story; each step reads ready, partial or skipped from its parts;
  the closing summary lists every wait once under its lane; and the gestures run in one pod after
  the egress gate.
- **Run:** `cargo xtask cell demo --scenario demo` on a machine with docker, k3d and helm
  (`deploy/cell/README.md`, "The demo's scenario"); CI's `cell-demo` job, which no required check
  depends on.
