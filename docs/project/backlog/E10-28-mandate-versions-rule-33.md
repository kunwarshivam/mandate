# E10-28: the mandate-versions read re-checks journal rule 33

- **E10-28 (Proposed, Could; lane L1)** As an owner, I want my agent's mandate-versions read to
  refuse a rejected application whose reason only an increase meets (`increase_blocked_while_latched`,
  `waiting_period`, `still_below_new_floor`) unless its recorded classification is
  `risk_increasing`, so that the read never serves a row journal spec §9.3 rule 33 forbids. Today
  `GET /agents/{id}/mandate-versions` ([DEC-2147](../decisions/DEC-2147.md),
  [DEC-2148](../decisions/DEC-2148.md)) checks rule 30 and that an application's classification
  equals the one it computes, but not rule 33; append already refuses such a record, so only a
  journal that is not what its writers wrote could reach it. Also correct `mandate_versions.rs`'s
  module doc, which names rule 33 among the rules it enforces. *Accepted when:* a DEC-77 pair, a
  pending HTTP test then the impl, answers such an application `journal_unavailable`, with a
  planted bug that accepts it; and a docs change then adds the schema's if/then for it.
<!-- section: E10 Workspace services -->
