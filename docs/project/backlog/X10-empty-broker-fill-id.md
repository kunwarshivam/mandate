# X10 follow-up: a broker fill with an empty id never refuses the executor's batch

- **X10b (Proposed; trading core)** As an owner, I want the executor to keep running when the broker
  reports a fill with an empty fill id, so that one malformed broker field never stops my agents'
  executor. DEC-2803 writes an empty fill id as `null` on `ExternalActivityIngested`, but the
  `FillApplied` journaled after it keeps `fill_id` as `text`, so the batch is still refused there.
  *Accepted when:* the executor's handling of such a fill is decided (refuse it at the connector, or
  journal it without one) and a test shows the batch is not refused.
<!-- section: E7 Alpaca connector and recovery -->
