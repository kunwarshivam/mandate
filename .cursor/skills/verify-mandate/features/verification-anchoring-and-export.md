# Verification, anchoring, and export

- **Spec:** `docs/specs/journal.md` §6.2, §10, §11.
- **Code:** `crates/mandate-journal/src/verify.rs`, `crates/mandate-journal/src/merkle.rs`,
  `export_line` in `crates/mandate-journal/src/lib.rs`,
  `crates/mandate-journal/src/control_verify.rs` (§11's control-stream range checks
  `anchor_self_mismatch` and `break_glass_cause_mismatch`, run after `verify_events`; E12-3),
  `crates/mandate-journal/src/start.rs` (§9.14's trusted start of a range, resolved from
  genesis, a `SegmentExported`, or a stamped `AnchorComputed` on the workspace's own control
  stream only, and a malformed `stream_id` refused, DEC-784; and `anchor_record`, an
  `AnchorComputed` row read as recorded; E12-3), `walk_range` in `verify.rs` (a range walked
  position by position with its count, AU-8; E12-3),
  `crates/mandate-journal/src/connection_fold.rs` (§11's connection checks; a range's lifecycle
  run from its connection anchor or failing closed without one, DEC-885; one stream's anchor
  folded by `ConnectionAnchor::from_verified` from a `VerifiedPrefix` only, DEC-889, DEC-892; E7-17).
- **Tests:** `crates/mandate-journal/tests/verify.rs`, `crates/mandate-journal/tests/properties.rs`
  (any tampering detected; rewrites caught only by the anchor; independent Merkle construction),
  `crates/mandate-journal/tests/control_verify.rs` (the two control-stream range checks against
  their vectors and independent random walks), `crates/mandate-journal/tests/trusted_start.rs`
  (the trusted-start resolver against its vectors, each §9.14 clause, and a random oracle),
  `crates/mandate-journal/tests/connection_fold.rs` (the connection checks against their vectors,
  and every split of every full chain against the full-chain run and an independent scan;
  `JUDGED_ON_CONTROL` and `JUDGED_ON_ACCOUNT`, the judged-record lists, against §11's sets; and
  the pending E7-17 tests bind each prefix first, and a forged, short, or unbound prefix fails
  closed),
  `crates/mandate-journal/tests/records_access.rs` (`VerificationRun` version 2 against its
  vectors, rules 132 and 133 swept by their own oracle, and version 1 kept with rule 112; E12-3),
  `crates/mandate-journal/tests/range_walk.rs` (seeded faults against an independent comparison)
  and `crates/mandate-journal/tests/anchor_record.rs` (anchors read back as recorded; E12-3).
- **Reference cases:** `journal::tamper::*`, `journal::merkle`, `journal::export_line_seq_1`, and
  the `cold_records.range_checks`, `cold_records.trusted_starts`, and
  `records_access.range_checks`, `verification_runs`, `connections`, `connection_requests`,
  `connection_ranges`, `connection_revocations` (DEC-888; read by E7-17's pending tests until
  its code PR), and `segment_rows` (journal spec v0.38's `segment_rows_mismatch`, DEC-894; no Rust test
  reads it until E12-3's tests PR) vectors of `fixtures/refcases/journal.json`.
- **Run:** `cargo nextest run -p mandate-journal -p mandate-refcases`.
