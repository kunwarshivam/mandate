# Repository automation

- **Code:** `xtask`: `xtask/src/main.rs` (every CI job, including `mutants_outcome`,
  `mutant_verdicts` and `is_stub_function`, which exempt an `Unimplemented` stub body of a crate
  with pending tests, on a missed-mutant exit status, and nothing else, and
  `live_tests_judge_every_mutant` with `listed_mutant_counts`, `live_test_counts` and
  `unjudged_mutants`, which fail a mutated package with no live test to judge its mutants before the
  run, since `cargo mutants` would report every one of them caught, DEC-139; the job takes its
  repository as a parameter, so `Fixture::gated` drives the whole of it and neither it nor the
  pre-flight can be deleted without a test failing; `mutated_crates`, which gates every
  `safety_critical = true` crate whatever its layer, DEC-253; `live_feature_problems`, which
  reads each CI command word by word as the shell does and refuses any word holding the token
  `live` outside the one compile-only form, DEC-529 item 3),
  `xtask/layers.toml` (crate layers and safety-critical policy), `.cargo/mutants.toml` (approved
  equivalent mutants).
- **CI:** `.github/workflows/ci.yml` (`fast`, `full`), `.github/workflows/nightly.yml`.
- **Run:** `cargo xtask check`.
