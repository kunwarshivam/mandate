- **E21-dry-run-unreachable-after-hold (Proposed; debt; lane D1)** As an agent, I want the daemon
  dry run's paths that the account's opening hold now always precedes (DEC-2851) either removed or
  reached by a test: the held instruments' marks loop in `production::gate`'s `context` (a held
  position always holds the opening first, so the loop sees none) and the `gate_refused` answer to
  a sell labelled an opening, which only a flat traded account can still reach. *Accepted when:*
  each is deleted, or a test reaches it and a mutant of it is caught.
