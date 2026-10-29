- **E6-1 (lane X, from #2970's review): a failed step must not leave a later kill switch unhanded** (`AGENTS.md` rule 13). In
  `mandate-daemon`'s `runtime.rs`, `control_pass` hands each control-stream event in order and
  returns at the first whose `handle` fails. `hand` marks the runtime `Failed`, so every later
  event in that read, a `KillSwitchActivated` or a kill-switch `OwnerCommandIssued` included, is
  never handed. Any interpretation bug in one control record therefore starves an owner's kill
  switch behind it. *Accepted when:* a decision says whether `control_pass` should still hand a
  kill switch, or every risk-reducing command, after an earlier event's `handle` failed (and how
  the failed event is then journaled and alerted). A test commits a refused control event and then
  a kill switch in one read, and shows the flatten handed.
