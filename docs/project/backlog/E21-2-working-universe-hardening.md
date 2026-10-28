- **E21-2-working-universe-hardening (Proposed; debt; lane D1)** As an agent, I want the daemon's
  reading of a research agent's working universe (DEC-2841) to match the executor's on two edges
  #2768's review found, each of which fails safe today because the executor refuses at binding
  but costs the owner a wasted ask: (1) replay at the scheduler's tick alone, as the executor's
  `now()` does, not `Journaled::now()`, which an account clock can lead; (2) mark the fold's
  thesis records unread when the
  agent stream cannot be read, so the plan and the gate hold as the view already does; and fall
  back to an earlier admitted record's class when the latest does not read. *Accepted when:* a
  test for each edge asks nothing the executor would hold.
