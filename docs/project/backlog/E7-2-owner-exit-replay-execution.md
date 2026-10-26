- **E7-2r (Proposed, M7; EX)** As the owner, I want an owner's exit handed again after a retry or
  a restart in one process (the paper runner, `mandate-shell`) to reach the executor, so that a
  crash between the journal and the handoff never strands it there (`AGENTS.md` rule 13). The
  runtime re-hands every committed `IntentProposed` with `execution: None`, and the shell's
  `ExecutorSink` refuses an order without it (`Cause::Absent`), for every order, not only an
  owner's exit (DEC-2387 review, finding 4). The cell is not affected: its executor reads the
  journal, never the handoff. *Accepted when:* a retry and a restart of an owner's exit, an
  opening and a risk exit each reach the shell's executor with the time in force and asset class
  their record and the mandate give, and a test drives each through `ExecutorSink`.
