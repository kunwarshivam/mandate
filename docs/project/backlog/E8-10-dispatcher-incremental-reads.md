- **E8-10y (Proposed, M7; EX)** As the operator, I want the dispatcher to read each followed
  stream from where it last read rather than from the start, so that a workspace with a long
  history is not re-read in full every second. Today each tick reads the control stream and every
  subject stream whole (DEC-2516 item 4), which is correct and bounded by the cell's history but
  grows with it. *Accepted when:* the process keeps a per-stream read position, a step reads only
  rows after it (a failed read still sends nothing and is no heartbeat), a restart replays from
  the notice stream as before, and a property test over random appends and restarts shows the same
  notice stream as full reads.
