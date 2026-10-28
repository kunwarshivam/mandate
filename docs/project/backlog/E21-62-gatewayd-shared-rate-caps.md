# E21-62 debt: one source for the recorded gateway's rate caps and settings

- **E21-62d (Proposed, M8; debt; lane GW)** `mandate_gatewayd::recorded::{WORKSPACE_RATE,
  RATE_MARGIN, settings}` copy the cell worker's private `rate` and its settings construction
  (`crates/mandate-daemon/src/worker.rs`), because `mandate-gatewayd` may not depend on the daemon.
  Move both into one helper in `mandate-research-run` that each of them uses, so that a change to
  DEC-2316's caps cannot drift between the worker and the service. Found by the independent review
  of #2173.
