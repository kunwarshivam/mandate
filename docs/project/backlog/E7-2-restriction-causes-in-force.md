- **E7-2v (Proposed, M7; X)** As an owner, I want every cause of an account restriction that is in
  force journaled and lifted on its own, so that a broker's acknowledgment never lifts a restriction
  the connection still imposes, and the owner is told which cause it is (trading spec §7.3, DEC-441
  item 23, [DEC-2137](../decisions/DEC-2137.md)). Today the executor's `restrict()` journals an
  `AccountRestrictionChanged` only when the account state it sets is stricter than the one in force,
  and the fold keeps the state alone, so a `connection_unavailable` restriction raised while a
  broker's is in force is never recorded. The notifications spec's one `account_restriction` type
  also has no cause-keyed alert for `connection_unavailable`, which §7.3 and connections spec CN-6
  promise. *Accepted when:* each new cause in force is journaled under journal spec rule 309
  ([DEC-2816](../decisions/DEC-2816.md)), the fold holds the causes in force, the account state is
  the strictest of them (never the last record's, so the fold change lands with the first record
  that can follow a stricter one), and a record that leaves it unchanged moves no agent. The lift is E7-2y;
  the connection's restriction and its distinct alert are E7-2z.
<!-- section: E7 Alpaca connector and recovery -->
