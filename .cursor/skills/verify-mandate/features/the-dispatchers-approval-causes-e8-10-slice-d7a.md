# The dispatcher's approval causes: `ApprovalRequested` as a cause (E8-10, slice D7a)

- **Spec:** [notifications spec](../../../../docs/specs/notifications.md) §3.2 (`approval_requested`:
  its trigger the agent stream's `ApprovalRequested`, class `action`, text key `approval_needed`),
  §3.3 (the interim recipients: every row resolves to the founder before roles), §3.4 (the causes
  and their keys), §5.1 (the journal as outbox; `NoticeIssued` before any send), §5.3 (`action`
  retries until not pending: the stop is slice D7b), NT-1, NT-8; [journal
  spec](../../../../docs/specs/journal.md) §9.15 rules 124 and 125; DEC-707 item 1; DEC-710 item 5;
  DEC-2501.
- **Code:** `crates/mandate-dispatcher/src/step.rs` (`step` and its cause read). The tests PR adds
  no production code: `step` already runs, so the D7a tests fail on its answer (no notice for an
  approval) and are named in `xtask/behaviour-only/` until the code PR.
- **Tests:** `crates/mandate-dispatcher/src/step/tests/approval.rs` commits real `ApprovalRequested`
  records (versions 1 and 2, through `MemoryJournal`, so rule 125 judges every notice the step
  appends) and pins: one `approval_requested` notice per request, class `action`, `cause` and
  `causation_id` the request, `cause_stream` its agent stream, recipients the configured audience;
  the notice journaled before its first send, one attempt and one idempotency key per channel; the
  NT-1 canary (a request whose instrument, rule, prices, plan, deadline, and hash are unique
  strings: none reaches a capture or a notice record, and the message is the fixed text and the
  link); retries with no window for an approval while an alert beside it is abandoned at 24 hours;
  and an interleaving property over alerts, requests, steps, and restarts whose oracle derives one
  key per cause from the committed subject rows and §3.2's table typed in the test, shown to catch
  a missing, a doubled, and a mis-kinded approval notice. Live tests pin rule 125 (an approval
  notice only from an agent stream of its workspace) and that an `ApprovalRequested` shown on an
  account stream is no cause and holds back no alert. `crash.rs` runs NT-8's crash oracle, which
  now counts approval requests on agent streams, with two requests among the causes.
- **Run:** `cargo nextest run -p mandate-dispatcher` (the D7a tests are pending until the code PR;
  `cargo nextest run -p mandate-dispatcher --run-ignored ignored-only` shows them failing).
