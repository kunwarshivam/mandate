- **E7-4b executor request subject vectors (Proposed, M7; SC)** As an owner, I want journal spec
  §9.28's reference cases to carry rule 240's subject comparison, so that the reference model and
  every writer judge it as the executor does. The executor's check 4 compares the request's and the
  copy's subjects as [DEC-2838](../decisions/DEC-2838.md) reads them (a version-1 record names
  `limit`, a `subject` that is not text matches nothing), refusing `request_invalid`, with Rust tests
  only; the reference vectors could not ship with it (ES-22). *Accepted when:* §9.28's
  `reconciliation_acknowledgments` (or `acknowledgments`) section holds a vector of a version-1
  request answered by a version-2 `reconciliation` copy, refused `request_invalid`, and one of a
  `reconciliation` request answered by the same subject, which lifts, and
  `reference/journal/reconciliation_acks.py`'s oracle and mutants cover them.
<!-- section: E7 Alpaca connector and recovery -->
