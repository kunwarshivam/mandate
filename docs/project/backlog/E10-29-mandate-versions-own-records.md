# E10-29: the mandate-versions read refuses only the agent's own unreadable records

- **E10-29 (Proposed, Could; lane L1)** As an owner of several agents on one account, I want one
  agent's unreadable `MandateVersionApplied` to fail only that agent's mandate-versions read, so
  that another agent's history stays readable. Today `GET /agents/{id}/mandate-versions`
  checks every application's members on the account stream before it filters by `agent_id`
  ([DEC-2148](../decisions/DEC-2148.md), known consequence 1), so one unreadable record makes every
  agent's read on that account `journal_unavailable`. *Accepted when:* a DEC-77 pair (a pending
  HTTP test, then the impl) reads `agent_id` first and refuses only the agent's own unreadable
  records, keeping a record whose `agent_id` cannot be read `journal_unavailable` for every agent,
  with a planted bug that skips the agent's own unreadable record.
<!-- section: E10 Workspace services -->
