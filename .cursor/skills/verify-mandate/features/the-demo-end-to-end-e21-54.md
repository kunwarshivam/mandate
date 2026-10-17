# The demo, end to end, on the local cell (E21-54)

- **Spec:** the managed-cells plan (`docs/project/tasks/managed-cells.md`), DEC-967, DEC-995 (the
  2026-10-26 demo), DEC-2002; journal spec §11 for the closing verification.
- **Code:** `xtask/src/cell/scenario.rs` (`cargo xtask cell demo --scenario demo`, `cargo xtask ci
  cell-demo`), and the demo's envelope and instrument under `deploy/cell/demo/`.
- **Tests:** `xtask/src/cell/scenario.rs`'s unit tests: the demo files are canonical JSON and the
  envelope is paper and asks every opening; the plan runs the six steps in order and types only the
  codes it was shown; every gesture is in the run's workspace and never live; the cell's API is checked only for its refusal with no session; every gap names the
  PR or story that closes it; and the gestures run in one pod after the egress gate.
- **Run:** `cargo xtask cell demo --scenario demo` on a machine with docker, k3d and helm
  (`deploy/cell/README.md`, "The demo's scenario"); CI's `cell-demo` job, which no required check
  depends on.
