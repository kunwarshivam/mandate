- **E7-2s (Proposed, M7; EX)** As the platform, I want the `OwnerExitRequested` an
  `Input::Command` kill switch writes to carry `step_up_status` and a `step_up` record or `null`,
  so that the journal accepts it as journal spec §9.1 rule 12 says. Today `step.rs`'s
  `owner_exit` writes `step_up` as the assertion's text when no evidence was judged, which the
  real journal refuses (DEC-2387 review, finding 6). The control-stream path writes the record.
  *Accepted when:* a test journals that path's batch through `mandate-journal`'s validator and it
  is accepted.
