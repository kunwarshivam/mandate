# Audit verification run (E12-3)

- **Spec:** `docs/specs/workspace-api.md` §4.8.1 "Verification" (J4, FR-7.5), AU-1, AU-6, AU-8;
  journal spec §9.13 rules 107, 110, 111, 132 and 133, §11; DEC-787 (one snapshot, the
  cold-store start, the counted range), DEC-788 items 1, 2 and 4, DEC-893 to DEC-895 (the trusted
  start over stored rows), DEC-897 items 4 and 5(a), DEC-916 (the walk's own-stream rows).
- **Code:** `crates/mandate-audit/src/verification.rs`: `plan`, refusal steps 4 to 7 of a
  `POST /verifications` over one `ControlSnapshot`, scoped to the caller's `ReadRecords` witness,
  asking a `ColdSource` only for a manifest start (pending, E12-3).
  `crates/mandate-audit/src/walk.rs`: `walk`, the coverage walk of a planned range — §11's
  per-event checks position by position from the trusted start, the one `NotFound` for rows of
  another stream (DEC-916), and the `VerificationRun` version 2 record the walk drafts and the
  result the route serves (pending, E12-3).
- **Tests:** `crates/mandate-audit/tests/verification_plan.rs` (pending: the one `NotFound`, the
  range against the head at start, the 1,000,000-event bound on the range only) and
  `crates/mandate-audit/tests/verification_plan_start.rs` (pending: no cold read before step 7,
  the cold answers' refusals, anchor and segment starts that do not fit, and DEC-788's order over
  every set of faults) and
  `crates/mandate-audit/tests/verification_walk.rs` (pending: the walk over the whole range and
  its tail, the first event and its trusted start, AU-8's missing, reordered, duplicated and
  truncated ranges, the own-stream rows of DEC-916, the artifact store's answers through check 6,
  the record's members through §9.13's own validator, and the served result rebuilt from it; plus
  a seeded property over corrupted chains).
- **Run:**
  `cargo nextest run -p mandate-audit -E 'binary(verification_plan) | binary(verification_plan_start) | binary(verification_walk)' --run-ignored all`.
