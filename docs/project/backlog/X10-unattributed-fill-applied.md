# X10 follow-up: the journal records an unattributed fill

- **X10c (Proposed; trading core; high priority)** As an owner, I want a fill on my account that
  Mandate did not originate to be booked as trading spec §7.1 says (ingested unattributed, every
  agent `exits_only` until I acknowledge), so that the executor's books and protection see it.
  Journal spec §9's `FillApplied` version 1 requires a client order id, so every unattributed
  fill's batch is refused today and DEC-2805 item 1's hold stands in. The activity cursor does not
  pass the refused fill, so every later reconciliation is refused too and the hold stays (DEC-2805
  item 2). *Accepted when:* `FillApplied` (and `LateFillApplied`) gain a version whose
  `client_order_id` is nullable for an unattributed fill, with its vectors; the executor writes
  it; and a test shows an external fill booked and netted, the batch accepted.
<!-- section: E7 Alpaca connector and recovery -->
