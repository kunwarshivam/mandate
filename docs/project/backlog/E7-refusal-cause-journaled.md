- **E7-refusal-cause-journaled (Proposed; trading core; lane X)** As an owner, I want the account
  stream to record why a batch was refused, so that the reconciliation hold DEC-2805 applies names
  its cause in the record and not only in an operator log line. DEC-2846 item 4 alerts each new
  cause, as the record type, the journal's reason code and the path, to the operator alone.
  `AgentModeApplied` is closed (journal spec §9.5) and has no member for it. *Accepted when:*
  journal spec names where the cause is journaled (a spec-only change, ES-22), and the executor
  journals it beside the hold.
