# E21-62 follow-up: readers of research references read the research store too

- **E21-62r (Proposed; infrastructure)** As an owner, I want the journal verifier, the cold
  archiver, the backup and restore check, and the API's artifact reads to resolve a research
  record's references, so that my agent's research can be checked and shown like everything else. DEC-2279 item 5 puts research artifacts
  in a store of their own. Item 6 says a reader meeting a research reference reads the shared store
  and then the research store, a copy that does not re-hash stopping the read. Today none of these
  readers is given the research store, so a research reference is missing to each, and a research
  object removed after it was journaled goes undetected in the cell.
  *Accepted when:* each of those readers is given the research store read-only, beside the shared
  one, and a test shows a research record's reference verifies and a planted copy under another
  hash never reads as it.
<!-- section: E21 Operations and infrastructure -->
