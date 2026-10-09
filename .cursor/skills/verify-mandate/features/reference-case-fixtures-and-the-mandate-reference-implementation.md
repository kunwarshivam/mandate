# Reference-case fixtures and the mandate reference implementation

- **Spec:** `docs/specs/mandate.md`; ADR-0001 ES-10, ES-11.
- **Code:** `python/mandate_tools/src/mandate_tools/export_refcases.py` (YAML to JSON, strict
  loader); `reference/mandate/` (founder-owned reference implementation);
  `python/mandate_tools/src/mandate_tools/mutation_anchors.py` (the anchor-only check: every `old`
  text of every mutant table in `reference/mandate/mutants.py` still occurs in `ref.py`, per PR and
  in under a second, where the nightly's sweep would fail before its first run; DEC-493).
- **Tests:** `python/mandate_tools/tests/test_export_refcases.py`,
  `python/mandate_tools/tests/test_mutation_anchors.py` (passes on the committed reference, names a
  stale anchor, refuses a `mutants.py` with no table); `reference/mandate/fuzz.py`,
  `reference/mandate/check_cases.py`, `reference/mandate/mutants.py`.
- **Run:** `cargo xtask refcases`; `cargo xtask ci reference` (the mandate generator, case check,
  fuzz seeds 1 to 3 and `reference/journal/generate.py --check` in the environment
  `reference/mandate/requirements.txt` pins, then the anchor check from the `python/` workspace);
  `cargo xtask ci nightly` for seeds 1 to 10 and the `mutants.py` sweep.
