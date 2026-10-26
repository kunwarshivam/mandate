# E21-62 follow-up: research objects are retained to the cold archive

- **E21-62s (Proposed; infrastructure)** As an owner, I want my agents' research to keep running
  once the research store holds months of runs, so that research never stops for good on a full
  store. DEC-2279 item 4 caps the research store at 512 MiB, and in v1 nothing deletes from it,
  so a full store halts research until an operator acts.
  *Accepted when:* research objects older than a retention the owner can see are copied to the
  cold archive, verified there, and only then removed from the research store; the research
  verifier reads them from the archive; and a test shows a full store returning below its cap
  without any journaled record losing its object.
<!-- section: E21 Operations and infrastructure -->
