# The live run's artifacts: only the live workspace's own registrations (E7-26, DEC-957)

- **Spec:** `docs/project/decisions/earlier-900-1999.md#dec-957` (the R0 rehearsal's R9); DEC-952 item 1 (paper
  and live never share a stream); DEC-924 items 1 and 2 (a live version only on all four
  conditions); DEC-526 item 4 (the paper-only fee schedule refuses a live one).
- **Code:** `crates/mandate-gate-inputs/src/control.rs` — `Origin` (`Origin::of`), carried by
  `ControlRecord`, `Registered` and `ConfirmedVersion::deployed_on`;
  `crates/mandate-shell/src/paper/artifacts.rs` — `Artifacts::from_registered`'s provenance and
  fee-environment checks; `crates/mandate-paper/src/lib.rs` (`control_record`) and
  `crates/mandate-paper/src/wiring.rs` (`record_of`, `journaled`) read each row's origin.
- **Tests:** `crates/mandate-shell/tests/live_artifacts.rs` (a live version on its own live
  stream passes every provenance check and stops only at the paper-only fee schedule; each
  binding from a paper stream or another workspace refuses, in both environments; a deployment
  in the other environment refuses; a `paper`-named fee schedule refuses a live run; a property
  over every environment and workspace combination against an origin-equality oracle); the
  `control_record` test in `crates/mandate-paper/src/tests.rs` and the grant-fold origin test
  in `crates/mandate-paper/src/wiring.rs`.
- **Run:** `cargo nextest run -p mandate-gate-inputs -p mandate-shell -p mandate-paper`.
