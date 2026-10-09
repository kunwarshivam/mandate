# The dispatcher's step over the journal (E8-10, slice D2)

- **Spec:** [notifications spec](../../../../docs/specs/notifications.md) §3.3 (the interim
  recipients), §3.4 (one notice per cause), §5.1 (the journal is the outbox), §5.5 (records),
  NT-3, NT-8; journal spec §5.1 (fencing) and §9.15; DEC-701 item 3; DEC-704.
- **Code:** `mandate-dispatcher` (layer 4, pure, safety-critical, now over `mandate-journal`):
  `crates/mandate-dispatcher/src/step.rs` (`NoticeWriter`, which holds only a notice stream;
  the `Journal` trait, whose only append takes a `NoticeWriter`; `Config`; `step`). Tests PR: the
  writer's three methods and `step` are stubs.
- **Tests:** `crates/mandate-dispatcher/src/step/tests.rs`, over `MemoryJournal` and the fixture
  provider: a writer refuses every stream but `ntf:`; only committed alerts are causes, no other
  stream is written, and each send finds its `NoticeIssued` committed and its attempt not yet
  recorded, with exactly one `delivered` record after it; one user kill switch is one notice and a
  second step issues and sends nothing; a dispatcher fenced by a newer epoch sends nothing and
  the live one sends.
- **Run:** `cargo nextest run -p mandate-dispatcher`; `cargo xtask ci pending`.
