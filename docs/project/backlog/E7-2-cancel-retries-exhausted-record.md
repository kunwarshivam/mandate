- **E7-2x (Proposed, M7; EX)** As the owner, I want a kill switch's cancel that is still
  unconfirmed when its retries are spent to be a journaled cause, so that the dispatcher tells me
  (notifications spec §3.2 `exit_stalled`). Today the executor raises the alert on a tick about the
  cancel's earlier record, which is in no batch of its own, so it records no `OwnerAlertSent`
  (DEC-2514 item 3). *Accepted when:* the executor journals a record of the spent retries (journal
  spec §9.5, with vectors) in the tick's batch, the alert names it, and the journal accepts the
  batch.
