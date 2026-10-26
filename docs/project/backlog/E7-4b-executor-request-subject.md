- **E7-4b executor request subject (Proposed, M7; SC)** As an owner, I want the executor to refuse
  an acknowledgment whose request names another subject, so that a writer other than the CLI cannot
  pair a request for a limit with an acknowledgment of a restriction or a reconciliation hold. The
  CLI refuses it before append under [DEC-2838](../decisions/DEC-2838.md)'s reading of rule 240 (a
  version-1 record names `limit`), and the workspace API's route does the same; the executor's own
  comparison (`MATCHED` in `mandate-executor`'s `acknowledge.rs`) compares `agent`, `event`,
  `requested_by` and `independent_required`, not `subject`. *Accepted when:* the executor's check 4
  compares the request's and the copy's subjects as DEC-2838 reads them, refusing `request_invalid`,
  with a reference vector for a version-1 request answered by a version-2 `reconciliation` copy.
<!-- section: E7 Alpaca connector and recovery -->
