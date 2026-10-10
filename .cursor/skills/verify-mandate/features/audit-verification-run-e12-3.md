# Audit verification run (E12-3)

- **Spec:** `docs/specs/workspace-api.md` §4.8.1 "Verification" (J4, FR-7.5), AU-1, AU-8; DEC-787
  (one snapshot, the cold-store start), DEC-788 item 4 (the order of refusals), DEC-893 to DEC-895
  (the trusted start over stored rows).
- **Code:** `crates/mandate-audit/src/verification.rs`: `plan`, refusal steps 4 to 7 of a
  `POST /verifications` over one `ControlSnapshot`, scoped to the caller's `ReadRecords` witness,
  asking a `ColdSource` only for a manifest start (pending, E12-3).
- **Tests:** `crates/mandate-audit/tests/verification_plan.rs` (pending: the one `NotFound`, the
  range against the head at start, the 1,000,000-event bound on the range only) and
  `crates/mandate-audit/tests/verification_plan_start.rs` (pending: no cold read before step 7,
  the cold answers' refusals, anchor and segment starts that do not fit, and DEC-788's order over
  every set of faults).
- **Run:**
  `cargo nextest run -p mandate-audit -E 'binary(verification_plan) | binary(verification_plan_start)' --run-ignored all`.
