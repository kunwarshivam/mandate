- **E10-30 (lane X, from the production plan's request pair): an owner's request under an
  `accumulate` goal or an unpinned universe** (DEC-2858). The deployment's plan proposes a
  request only for a pinned instrument and a `continuous` or `profit_stop` goal, the coordinator's
  slice B scope (the deployment's own asset). Under an `accumulate` goal, `mandate-builder`'s
  `requested_buy` proposes nothing, because the goal's bounds (`target_qty`, `max_spend_usd`,
  `max_avg_price`) have no rule for a request yet. An unpinned mandate's request journals nothing,
  because its one instrument is its thesis's. Both add no risk (`AGENTS.md` rule 3), but the owner
  sees no decision. *Accepted when:* a decision says how a request is clipped by an accumulate
  goal's bounds and which instrument an unpinned mandate's request may name, and tests show each
  journaled as DEC-2858 says.
