# The loss answer and its proposed ladder (E10-7)

Specified by mandate spec §7 and DEC-695 items 6 and 7 (Accepted): how goal-first onboarding's loss
answer (DEC-182) maps to three loss fields, and the ladder drafted beneath them. E10-7 slice S1b
implements `loss_answer_fields` and `proposed_ladder` in `mandate-spec`; every test below runs.

- **Spec:** `docs/specs/mandate.md` §7 (the loss answer), §2.1 (provenance), V-010 to V-014 and V-020
  (§4.1); DEC-182, DEC-695, DEC-901.
- **Code:** `crates/mandate-spec/src/draft.rs` (`loss_answer_fields`, `proposed_ladder`,
  `LossAnswer`, `LossFields`, `ProposedLadder`, `Drafted`, `Draft`, `AskAgain`).
- **Tests:** `crates/mandate-spec/tests/loss_answer.rs`: two tables whose values
  `reference/mandate/ref.py`'s functions of the same names produced (refusals, F rounded down, the
  0.8 and 0.2 ratios, the rungs rounded down, flatten at exactly D, and the collapse refusal), and a
  property over random answers that checks each draft against an exact integer oracle and the
  drafted mandate against `validate`; and two tests the coordinator's condition on #1199 adds, for
  inputs §7 and DEC-695 give no outcome (a zero or negative allocation, a negative answer, a
  drawdown of 0 or less, or of 1 or more; DEC-901): each is a typed error or asked again, never a
  draft or a panic. An allocation of 0 or less and a D outside (0, 1) are `InvalidInput`; a
  negative answer is asked again as `NoLoss`.
- **Reference model:** `reference/mandate/ref.py` (`loss_answer_fields`, `proposed_ladder`),
  `reference/mandate/fuzz.py` (`fuzz_loss_answer`), `reference/mandate/mutants.py`.
- **Reference cases:** none; DEC-695 item 9 adds no reference case or fixture.
- **Run:** `cargo nextest run -p mandate-spec -E 'binary(loss_answer)'`.
