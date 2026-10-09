# Account-stream risk-state records (journal spec §9.3, under E7-10)

- **Spec:** `docs/specs/journal.md` §9.3 (`MandateVersionApplied` and `UniverseChanged`, the `asset_id`
  type, rules 29 to 33, and the mapping table); DEC-403, DEC-404. Both are registered, and the mapping re-derives
  §9.2's classification from the two stored documents and refuses a mismatch (DEC-404 items 5 and 8).
- **Code:** `crates/mandate-journal/src/control.rs` (`governs`, `MANDATE_VERSION_APPLIED`,
  `UNIVERSE_CHANGED`, and rules 29 to 33 in `payload`), `crates/mandate-journal/src/catalogue.rs`
  (`UniverseChanged`'s entry), and `crates/mandate-spec/src/context.rs` (`JournaledFact::from_record`
  with `change::classify`).
- **Tests:** `crates/mandate-refcases/tests/risk_state.rs` (the vectors through `append`, the
  mapping, and the classification re-derivation), `crates/mandate-refcases/tests/asset_id.rs` (the
  journal's `asset_id` check and `AssetId::parse` agree over a generated corpus), `control::tests` in `control.rs`
  (`every_risk_state_draft_is_judged_as_its_vectors_say`, `a_required_risk_state_member_is_never_null`,
  `a_risk_state_record_at_another_schema_version_is_an_unknown_schema`,
  `rule_33_refuses_nothing_that_shows_no_raise`),
  `crates/mandate-journal/tests/catalogue.rs` (`risk_state_records_are_routed_to_section_9_3`,
  `risk_state_records_refuse_an_unlisted_member`), and `record_tests` in `context.rs`
  (`a_version_maps_only_under_the_classification_its_documents_give`, over 14 §9.2 rows, and
  `a_version_document_that_is_absent_or_an_impostor_is_refused`).
- **Run:** `cargo nextest run -p mandate-journal -p mandate-spec -p mandate-refcases
  -E 'binary(risk_state) | test(/risk_state/) | test(/universe_change/) |
  test(/version_maps_only/) | test(/impostor/) | test(/schema_version/) | test(/rule_33/)'`.
