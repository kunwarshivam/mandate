- **E6-15m (Proposed, M7; RK)** As an owner of a thesis-driven (unpinned) agent, I want its openings
  judged by the account risk state the same way a pinned agent's are, so that the research loop can
  trade once its account has traded. Today the account risk fold binds one instrument for life, at
  the agent's first deployment ([DEC-2804](../decisions/DEC-2804.md) item 2, DEC-2308 item 4). An
  unpinned agent whose deployment resolves to no surviving thesis opens no fold. One whose thesis
  moves from one instrument to another makes its view unreadable at the new instrument's mark.
  Either way every opening is held, and exits, protection and the kill switch go on (fail closed;
  the coordinator's ruling of 2026-10-26 on R4b, option 1).
  - **The full fix** is DEC-2308 item 5's multi-instrument risk state: E summed across the
    agent's positions. It changes how limits are computed, so it is the founder's.
  - **A narrower reading to assess first**, its own DEC under DEC-176 only if it adds no risk:
    bind, or rebind, the fold only while the agent is flat (no position, no working or Unknown
    order in any instrument), at its first opening intent rather than its deployment. Its
    invariant, as a property test: a rebind while flat leaves every limit's state unchanged.
<!-- section: E6 Agent runtime and risk -->
