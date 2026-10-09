# Verification, anchoring, and export

- **Spec:** `docs/specs/journal.md` §6.2, §10, §11.
- **Code:** `crates/mandate-journal/src/verify.rs`, `crates/mandate-journal/src/merkle.rs`,
  `export_line` in `crates/mandate-journal/src/lib.rs`,
  `crates/mandate-journal/src/control_verify.rs` (§11's control-stream range checks
  `anchor_self_mismatch` and `break_glass_cause_mismatch`, run after `verify_events`; E12-3).
- **Tests:** `crates/mandate-journal/tests/verify.rs`, `crates/mandate-journal/tests/properties.rs`
  (any tampering detected; rewrites caught only by the anchor; independent Merkle construction),
  `crates/mandate-journal/tests/control_verify.rs` (the two control-stream range checks against
  their vectors and independent random walks).
- **Reference cases:** `journal::tamper::*`, `journal::merkle`, `journal::export_line_seq_1`, and
  the `cold_records.range_checks` and `records_access.range_checks` vectors of
  `fixtures/refcases/journal.json`.
- **Run:** `cargo nextest run -p mandate-journal -p mandate-refcases`.
