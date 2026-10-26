# E21-62 follow-up: readers of research references read the research store too

- **E21-62r (Proposed; infrastructure)** As an owner, I want the journal verifier, the cold archiver
  and the API's artifact reads to resolve a research record's references, so that my agent's
  research can be checked and shown like everything else. DEC-2279 item 5 puts research artifacts
  in a store of their own. Item 6 says a reader meeting a research reference reads the shared store
  and then the research store, the first copy that re-hashes being the object. No cell reader but
  the research runtime needs this today.
  *Accepted when:* each of those readers is given the research store read-only, beside the shared
  one, and a test shows a research record's reference verifies and a planted copy under another
  hash never reads as it.
<!-- section: E21 Operations and infrastructure -->
