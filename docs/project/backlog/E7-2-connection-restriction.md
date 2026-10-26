- **E7-2z (Proposed, after the demo; EX)** As an owner, I want my account restricted to closing
  only when the platform cannot reach or use my connection, with a distinct alert that says the
  broker has not restricted it, as trading spec §7.3 row 5 and connections spec CN-6 promise. Today
  the executor receives no connection state (no `Input` for it; `ConnectionStateChanged` is not
  interpreted), so nothing journals an `AccountRestrictionChanged` with cause
  `connection_unavailable`, and notifications spec §3.2 has one `account_restriction` kind for every
  cause. *Accepted when:* a connection that goes `degraded` or `suspended` journals the restriction
  with that cause before anything else the executor sends (rule 309 admits it beside a broker's), a
  distinct §3.2 kind alerts the owner, and the fold reads it as a cause in force.
<!-- section: E7 Alpaca connector and recovery -->
