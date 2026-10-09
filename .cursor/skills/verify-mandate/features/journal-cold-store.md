# Journal cold store

- **Spec:** `docs/specs/journal.md` §6.2, §10, §11, §12; DEC-263 to DEC-265;
  `docs/project/tasks/E5-6-journal-cold-store.md`.
- **Code:** `mandate-journal-cold`: `crates/mandate-journal-cold/src/lib.rs` (the segment manifest
  and its canonical bytes — refused above the canonical integer bound, `SeqUnrepresentable` —
  `import_line`, `verify_range`'s per-range checks with the both-direction segment chain,
  `tsa_imprint_matches`'s structural scope and the fail-closed `verify_tsa` entry point
  (`TsaVerificationIncomplete` until DEC-265 item 1's crypto half lands), the inclusion proof
  with its `MalformedAnchor` refusal, and the fallible export bundle's digest
  (`PartUnrepresentable`, never an invented value) — implemented, over `mandate-journal`'s own
  `verify_events`, `export_segment`, `Anchor`, `merkle_root` and `tsa_imprint`).
- **Tests:** `crates/mandate-journal-cold/tests/cold.rs` (the manifest's six fields and their
  canonical bytes with an independent oracle, the importer's split, each cold check at its bar,
  the range walk's reachability and its overlapping-segment refusal, the token's imprint
  containment and the entry point's refusal, the proof's sibling roots, the lying anchor, the
  export digest), live since the implementation PR, and
  `crates/mandate-journal-cold/tests/properties.rs` (E5-6a, DEC-287: the range walk's
  invariants over random segment sequences — entered at the genesis, inside the first supplied
  segment, or exactly at any supplied segment's first seq with the earlier segments omitted, a
  stale copy prepended in front of the trusted start included — against an oracle that computes
  the expected outcome from the scenario it built — the verified state, the failing check and
  its seq by DEC-264's order including the per-event arm and the canonical-form rule, the
  reachability rule, and the token's two answers — plus the manifest round trip, the inclusion
  proof's root rebuilt by the oracle's own §10 fold, and the export digest, 256 cases each).
- **Run:** `cargo nextest run -p mandate-journal-cold`.
