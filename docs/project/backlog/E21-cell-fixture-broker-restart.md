- **E21-cell-fixture-broker-restart (Proposed; debt; lane D1)** As an operator rehearsing in a dev
  or ci cell, I want the executor's fixture broker (`FixturePaper`, `crates/mandate-executord/src/
  paper.rs`, DEC-2007 item 1) to survive an executor restart, or the rehearsal to say it does not,
  so that a restart drill in the cell never reads as a broker that lost a position: today each
  generation starts with an empty in-process broker, so the account's position, cash and resting
  protection reconcile as a mismatch and pause the agent (lane DR's rehearsal, 2026-10-27, g1 to g2).
  *Accepted when:* a restart in the cell either reconciles clean against the broker state the first
  generation left, or the cell's drill output names the fixture reset before any mismatch alert.
