- **E7-2z (Proposed, after the demo; EX)** As an owner, I want my account restricted to closing
  only when the platform cannot reach or use my connection, with a distinct alert that says the
  broker has not restricted it, as trading spec §7.3 row 5 and connections spec CN-6 promise. Today
  the executor's core acts on `Input::Connection`: it journals the restriction, every agent
  `exits_only` and the `connection_unavailable` alert (DEC-2520) first, then the
  `ConnectionStateChanged` in a later batch, and its fold reads the record (DEC-2826, slice C).
  Slice D (DEC-2832) counts the broker's answers in `mandate-executord` and sends it, and the
  Alpaca connector answers a `401` as an unknown outcome (§5.4). What remains:
  - the quote feed's answers, which are not counted (the scheduled health probe, DEC-2837, reads
    the account every minute, so an idle executor notices);
  - Robinhood's connector, which folds every account-snapshot error into one `Unreadable`;
  - contract drift, which only an MCP connection has (§8.1 check 7), so it comes with
    Robinhood's later checks; rate-limit headroom degrades as `rate_headroom` (DEC-2842);
  - `credential_expired` and `refresh_failed`, which stay blocked on U-A3: Alpaca documents no
    token lifetime or refresh. A lost lease restricts as `lease_expired` (DEC-2839), and a failed
    later check as `check_failed` (DEC-2840);
  - Robinhood's later permission checks (DEC-2840 runs Alpaca's only);
  - journal spec §9.5's sentence that leaves this restriction to E7-2z. *Accepted when:* a connection that goes `degraded` or `suspended` journals the restriction
  with that cause before anything else the executor sends (rule 309 admits it beside a broker's), a
  distinct §3.2 kind alerts the owner, and the fold reads it as a cause in force.
<!-- section: E7 Alpaca connector and recovery -->
