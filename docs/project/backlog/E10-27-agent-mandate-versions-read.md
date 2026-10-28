# E10-27: an agent's mandate versions over the workspace API

- **E10-27 (Proposed, M8; lane L1)** As an owner, I want to see my agent's mandate versions, which
  one is in effect, how each was classified, and when each was created, confirmed, deployed, and
  applied, so that the agent's page shows its history instead of `ApiNotYetNotice`. Today the
  server serves no read of them. *Accepted when:* `GET /agents/{id}/mandate-versions` serves
  `read-models/agent-mandate-versions` as workspace API §4.7 states it ([DEC-2147](../decisions/DEC-2147.md)),
  read-only, under the "View agents, positions, decisions" row, with the history in order, the
  version in effect only once the executor applied it, each classification computed from the
  stored documents, another workspace's agent `not_found`, and an unreadable stream or record
  `journal_unavailable`, each by an HTTP test.
<!-- section: E10 Workspace services -->
