# Control-stream payload schemas and the `JournaledFact` mapping (E7-10)

- **Spec:** `docs/specs/journal.md` §9.2 (the control stream's closed schemas, `OwnerCommandRefused`
  on the agent and account streams, the `pointer` and `date` types, consistency rules 17 to 24,
  subject rules 25, 26 and 28, copy rule 27, and the mapping table); DEC-168, DEC-261, DEC-302, DEC-303,
  DEC-304, DEC-402. `AccountSnapshotRecorded` and rule 24 are registered with the executor's fee-step
  writer (DEC-261 item 7, DEC-402).
- **Code:** `crates/mandate-journal/src/control.rs` (`Draft::parse` routes each §9.2 type on its
  stream; `payload`, then `subject_and_copy`), `crates/mandate-journal/src/schema.rs` (`Ty::Pointer`),
  `crates/mandate-spec/src/context.rs` (`JournaledFact::from_record`, the mapping, with its in-module
  `record_tests`; DEC-303 items 10 and 15).
- **Tests:** `crates/mandate-refcases/tests/control_stream.rs` (one test per §9.2 family, the chain,
  and the mapping over the vectors' `journaled_facts`), `crates/mandate-spec/tests/journal_record.rs`
  (the mapping from hand-written records), `crates/mandate-journal/tests/catalogue.rs` (unlisted
  members at every depth; `account_snapshot_recorded_is_routed_to_section_9_2`),
  `control::tests` in `control.rs` (the vectors inside the crate), and
  `mandate-executor`'s `reconcile::tests::the_fee_steps_snapshot_is_never_refused_for_its_members`.
- **Run:** `cargo nextest run -p mandate-journal -p mandate-spec -p mandate-refcases -p
  mandate-executor -E 'binary(control_stream) | binary(journal_record) | binary(catalogue) |
  test(/control::tests/) | test(the_fee_steps_snapshot_is_never_refused_for_its_members)'`.
