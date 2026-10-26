- **E7-31 (Must, M6, the local cell's trade loop; SC)** As an owner of a founded workspace, I want
  my approval to pass the runtime's approver check, so that an `ask` order I approve in the web app
  or the CLI is sent. Today the runtime resolves no approver once the control stream holds a
  §9.12 record, and every founded workspace holds its founding grant
  ([DEC-2129](decisions/DEC-2129.md) item 8). *Accepted when:* the run folds §9.12 to a roster of
  active members and resolves each reference only to active approvers (`role:owner` to the founder),
  and every stream it cannot fold resolves no one. Design: [DEC-2384](decisions/DEC-2384.md).
  Follow-up: re-read the roster when the stream's memberships change during a run (item 6).
<!-- section: E7 Alpaca connector and recovery -->
