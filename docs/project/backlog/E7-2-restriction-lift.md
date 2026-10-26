- **E7-2y (Proposed, after the demo; L1)** As an owner, I want an account restriction lifted when
  each of its causes clears, as trading spec §7.3's "Lifts when" says, so that a restricted account
  trades again once the broker and I have both cleared it. Today nothing lifts a restriction: the
  executor's account state never returns to active, and no journaled acknowledgment can name a
  restriction (§9.28's `OwnerAcknowledged` names only a `RiskLimitTriggered`; the executor does not
  copy it, E7-4). Journal spec rules 310 to 312 are held for it ([DEC-2816](../decisions/DEC-2816.md)).
  *Accepted when:* an owner acknowledgment can name a restriction (`OwnerAcknowledged` version 2),
  a lift record names the cause it lifts and is admitted only for a cause in force, a broker cause
  only after that acknowledgment and a later account report showing no restriction, and
  `connection_unavailable` only after the connection's `condition_cleared` and then the
  acknowledgment; the executor recomputes the account state from the causes left, and moves the
  agents back only when none remains. The executor's fold of `ConnectionStateChanged` (DEC-2826)
  keeps only the causes journaled in the current state: a degrading cause held while `suspended`
  is never journaled, and those before `degraded` to `suspended` are dropped, so the
  acknowledgment guard needs DEC-687's outstanding set tracked across that change.
<!-- section: E7 Alpaca connector and recovery -->
