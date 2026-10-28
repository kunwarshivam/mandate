# E21-62 follow-up: a registry entry's status changes are journaled

- **E21-62v (Proposed; records)** As an owner, I want each change of a model's status (offered,
  deprecated, withdrawn) recorded, so that I can see when the model my agent uses was offered and
  on which gate report. Today the live start reads an entry's offer from the live registry file and
  its gate report (DEC-2216 item 8), and nothing journals the change.
  *Accepted when:* `PlatformOperatorAction` closes (journal spec §9, DEC-261 item 9) or a record of
  its own is added; the service journals each status change with the entry's pin and, for an offer,
  the gate report's reference; and a test replays the journal to the registry's statuses.
<!-- section: E21 Operations and infrastructure -->
