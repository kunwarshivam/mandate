- **E7-4b runtime acknowledgment subject (Proposed, M7; SC; owner: the runtime's lane, NE)** As an
  owner, I want the runtime to count only an acknowledgment of a limit as one that lifts a kill
  switch, so that acknowledging a reconciliation hold or a restriction never lifts a limit's
  switch. `mandate-runtime`'s `state.rs` folds every `OwnerAcknowledged` copy on the account stream
  into `acknowledged` whatever its version-2 `subject` (journal spec v0.104 §9.28, DEC-2830); the
  executor already copies `subject: reconciliation` acknowledgments there. *Accepted when:* the
  fold counts a version-1 copy, or a version-2 copy whose `subject` is `limit`, and no other, with
  a test that a reconciliation acknowledgment's copy and its lift never lift a kill switch.
<!-- section: E6 Agent runtime and risk -->
