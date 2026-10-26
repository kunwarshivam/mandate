- **E7-2z (Proposed, after the demo; EX)** As an owner, I want my account restricted to closing
  only when the platform cannot reach or use my connection, with a distinct alert that says the
  broker has not restricted it, as trading spec §7.3 row 5 and connections spec CN-6 promise. Today
  the executor's core acts on `Input::Connection`: it journals the restriction, every agent
  `exits_only` and the `connection_unavailable` alert (DEC-2520) first, then the
  `ConnectionStateChanged` in a later batch, and its fold reads the record (DEC-2826, slice C).
  What remains is slice D: `mandate-executord` sends no `Input::Connection` yet, since it maps no
  connector error to it (an ambiguous error must read as `degraded`, never as healthy). *Accepted when:* a connection that goes `degraded` or `suspended` journals the restriction
  with that cause before anything else the executor sends (rule 309 admits it beside a broker's), a
  distinct §3.2 kind alerts the owner, and the fold reads it as a cause in force.
<!-- section: E7 Alpaca connector and recovery -->
