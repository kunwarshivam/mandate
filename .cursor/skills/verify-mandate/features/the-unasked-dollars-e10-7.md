# The unasked dollars (E10-7)

Specified by mandate spec §4.2 and DEC-695 (Accepted), the formula for DEC-189's figure. E10-7 slice
S1a's DEC-77 tests PR stubs `unasked_usd` in `mandate-spec`, so every test below is pending on it
until the implementation PR replaces the stub.

- **Spec:** `docs/specs/mandate.md` §4.2 (the unasked dollars), §6.2 steps 4a, 5b, and 5c, §6.5;
  `docs/specs/workspace-api.md` §5.3 (`unasked_usd_after`); DEC-189, DEC-695.
- **Code:** `crates/mandate-spec/src/unasked.rs` (`unasked_usd`, `UnaskedInputs`,
  `DelegationUsage`, `EffectivePolicy`).
- **Tests:** `crates/mandate-spec/tests/unasked.rs`: a table whose figures `reference/mandate/ref.py`'s
  `unasked_usd` produced (unknown inputs, the known zeros, each condition's order bound, delegation
  slices, the count, and rounding up to the cent); a property that no simulated risk day runs more
  unasked than the figure, with its own decision walk and accumulators and with marks, cancels, and
  exits freeing headroom within the day; and a property that a greedy day reaches the figure in
  fuzz.py's family of `lte` bounds.
- **Reference model:** `reference/mandate/ref.py` (`unasked_usd`, `order_usd_bound`),
  `reference/mandate/fuzz.py` (`fuzz_unasked`), `reference/mandate/mutants.py` (`UNASKED_MUTANTS`).
- **Reference cases:** none; DEC-695 item 9 adds no reference case or fixture.
- **Run:** `cargo nextest run -p mandate-spec --run-ignored all -E 'binary(unasked)'`.
