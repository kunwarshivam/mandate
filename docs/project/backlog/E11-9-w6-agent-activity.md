- **E11-9 W-6 (Must, M9, before the founder's demo)** As the owner, I want an agent's screen to show
  its orders, the positions its own fills hold, and the latest records on its stream from the
  deployment's served reads, so that I can see a paper trade from proposal to fill without the CLI
  ([DEC-2377](../decisions/trading-core.md#dec-2377)). *Accepted when:* the agent's screen reads `GET
  /agents/{id}/orders`, `/positions` and the agent stream's events page; a failed read, an answer
  for another agent or stream, or a missing account shows "not reported", never empty; journal lines
  carry the sequence, type and time and no body; and contract tests show only documented operations.
<!-- section: E11 Web app: dashboard and controls -->
