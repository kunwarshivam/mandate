- **E6-15n (Proposed, M7; RK)** As the owner of an agent whose executor crashed between adopting an
  order's state and journaling its compensation, I want the compensation the reconciler then owes to
  leave the account risk view readable, so that one crash does not hold the agent's openings until an
  operator recovers it. Today, only a compensation right after the `OrderStateChanged` it corrects,
  in the same append, steps nothing ([DEC-2829](../decisions/DEC-2829.md) item 1). The one `owed`
  journals at the start of a later batch, with no state change beside it, fails closed (item 2), so
  openings wait while exits, protection and the kill switch go on.
  - **To assess:** pair such a compensation with the earlier adopted `OrderStateChanged` that its
    `causation_id` names, which must be an adoption no compensation has corrected yet, as the
    executor's own fold tracks `uncompensated`. It needs its own decision and a property test showing
    that it moves no figure.
<!-- section: E6 Agent runtime and risk -->
