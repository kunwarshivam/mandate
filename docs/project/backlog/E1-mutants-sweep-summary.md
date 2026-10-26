# E1 follow-up: the nightly mutants sweep ends without a summary line

<!-- section: E1 Foundations -->
- **E1-mutants-sweep (Should; tooling, debt from #2648)** As an engineer, I want `cargo xtask ci
  sweep`, the nightly's whole-crate mutants sweep, to clear `target/mutants.out` before it plans and
  to end every shard with the same `mutants summary:` line `cargo xtask ci mutants` prints, so that
  no lane reads an earlier run's outcomes from a sweep shard that held no mutant. Today
  `sweep_scheduled` returns early on an empty shard without clearing, and only `gate_run` clears
  before its own run. Done when the sweep goes through `summarised` and a test plants stale outcomes
  under an empty shard.
