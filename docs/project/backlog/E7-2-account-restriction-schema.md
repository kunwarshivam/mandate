- **E7-2u (Proposed, M7; X)** As the platform, I want the executor's `AccountRestrictionChanged`
  registered in the journal (journal spec §9.5), so that an account restriction the broker reports
  is recorded rather than refused. Today `Draft::parse` answers `UnknownSchema` for it (found by
  DEC-2514's journal test), so in the cell the executor's batch for a blocked or restricted account
  is refused and the process stops, the reading that adds no risk but leaves the owner unalerted.
  *Accepted when:* the record's schema is in journal spec §9.5 with reference vectors, the journal
  accepts the executor's batch for a blocked account, and a test runs that batch through
  `Draft::parse` and `check_batch`.
