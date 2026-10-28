# E21-62 follow-up: the gateway's call path refuses an entry that is not offered

- **E21-62u (Proposed; safety)** As an owner, I want my agent's research to reach only a model the
  platform offers, so that a model still being evaluated never answers a run. Today an entry that
  is not offered holds no endpoint on the live path (#2832, DEC-2216), so a call to it ends
  `policy_denied`. That holds by how the live registry is built, not by the call path, which
  refuses only a `withdrawn` entry.
  *Accepted when:* a research call naming an `evaluating` entry is refused before any reservation
  with an outcome code the journal spec admits; an `evaluation` call still reaches it; and a test
  registers an evaluating entry with an endpoint and shows the research call refused.
<!-- section: E21 Operations and infrastructure -->
