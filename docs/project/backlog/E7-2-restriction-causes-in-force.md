- **E7-2v (Proposed, M7; X)** As an owner, I want every cause of an account restriction that is in
  force journaled and lifted on its own, so that a broker's acknowledgment never lifts a restriction
  the connection still imposes, and the owner is told which cause it is (trading spec §7.3, DEC-441
  item 23, [DEC-2134](../decisions/DEC-2134.md)). Today the executor's `restrict()` journals an
  `AccountRestrictionChanged` only when the account state it sets is stricter than the one in force,
  and the fold keeps the state alone, so a `connection_unavailable` restriction raised while a
  broker's is in force is never recorded. The notifications spec's one `account_restriction` type
  also has no cause-keyed alert for `connection_unavailable`, which §7.3 and connections spec CN-6
  promise. *Accepted when:* each new cause in force is journaled, the fold holds the causes in
  force and lifts each by its own condition, and the owner's alert for `connection_unavailable` is
  distinct.
<!-- section: E7 Alpaca connector and recovery -->
