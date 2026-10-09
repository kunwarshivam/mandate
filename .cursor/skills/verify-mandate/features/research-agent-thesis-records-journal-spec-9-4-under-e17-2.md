# Research-agent thesis records (journal spec §9.4, under E17-2)

- **Spec:** `docs/specs/journal.md` §9.4 (`ThesisProposed` and `ThesisRevised`, one shared schema,
  rules 34 to 38, `instrument_id` an `asset_id`); DEC-413, DEC-414. `append` checks the schema and
  rules; the mandate re-derivation of §8.5 checks 4, 5, 6, 10 and 16's cap (DEC-413 item 5) is
  `check_thesis_record`, which every reader of a thesis record runs before acting on it (DEC-414
  item 3).
- **Code:** `crates/mandate-journal/src/control.rs` (`governs`, `THESIS`, `THESIS_RECORD`,
  `thesis_rules`, `horizon_agrees`), `crates/mandate-journal/src/catalogue.rs` (both entries, `man`
  and `mod`), and `crates/mandate-spec/src/context.rs` (`check_thesis_record`,
  `THESIS_MANDATE_CHECKS`, `MANDATE_ALONE_CHECKS`).
- **Tests:** `control::tests` in `control.rs` (`every_thesis_draft_is_judged_as_its_vectors_say`,
  `a_required_thesis_member_is_never_null`,
  `a_thesis_record_at_another_schema_version_is_an_unknown_schema`),
  `crates/mandate-journal/tests/catalogue.rs` (`thesis_records_are_routed_to_section_9_4` and
  `thesis_records_refuse_an_unlisted_member`), and `thesis_tests` in `context.rs` (the six
  re-derivation tests: pinned universe, admission `deny`, asset class, revision cap, the named
  mandate, an absent or impostor document).
- **Run:** `cargo nextest run -p mandate-journal -p mandate-spec -E 'test(/thesis/)'`; the vectors
  themselves with `uv run --directory python pytest -q
  mandate_tools/tests/test_journal_research_vectors.py`.
