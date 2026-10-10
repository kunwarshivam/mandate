# The dispatcher's step over the journal (E8-10, slice D2)

- **Spec:** [notifications spec](../../../../docs/specs/notifications.md) §3.3 (the interim
  recipients), §3.4 (one notice per cause), §5.1 (the journal is the outbox), §5.5 (records),
  NT-3, NT-8; journal spec §5.1 (fencing) and §9.15; DEC-701 item 3; DEC-704; DEC-705.
- **Code:** `mandate-dispatcher` (layer 4, pure, safety-critical, now over `mandate-journal`):
  `crates/mandate-dispatcher/src/step.rs` (`NoticeWriter`, which holds only a notice stream;
  the `Journal` trait, whose only append takes a `NoticeWriter`; `Config`; `step`), implemented
  per DEC-704 and DEC-705. Every draft is built from `mandate_canon` values and every append goes
  through `Journal::append_notices`; no non-test code calls `MemoryJournal`.
- **Tests:** `crates/mandate-dispatcher/src/step/tests.rs`, over `MemoryJournal` and the fixture
  provider: a writer refuses every stream but `ntf:`; only committed alerts are causes, no other
  stream is written, and each send finds its `NoticeIssued` committed and its attempt not yet
  recorded, with exactly one `delivered` record after it; one user kill switch is one notice and a
  second step issues and sends nothing, and its cause is the API's control-stream alert whose
  subject is the `OwnerCommandIssued`; a dispatcher fenced by a newer epoch sends nothing and
  the live one sends. The tests correction adds DEC-704 items 2 and 3: a step's new
  `NoticeIssued` are one append, opened by `StreamOpened` on a fresh stream, then one append per
  send; recipients are sorted and unique; a `retryable` or `permanent` answer is journaled
  `failed` with its reason before the next send; and an append that does not commit
  (`Unavailable` on the issue batch, `Ambiguous` on an attempt) returns `NotCommitted` with no
  send or append after it. Per DEC-705, the kill switch's cause is the control-stream alert
  even when the subject streams are given as `[A1, A2, CTL]`.
- **Run:** `cargo nextest run -p mandate-dispatcher`.
