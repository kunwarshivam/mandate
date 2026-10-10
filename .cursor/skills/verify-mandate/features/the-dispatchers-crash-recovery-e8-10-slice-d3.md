# The dispatcher's crash recovery and NT-8's journal-only oracle (E8-10, slice D3)

- **Spec:** [notifications spec](../../../../docs/specs/notifications.md) §5.1 (at least once,
  never lost; the idempotency key; restart replays the notice stream), NT-8 (crash injection at
  every step, an oracle that reads only the journal); DEC-704 item 3; DEC-710 item 5.
- **Code:** no new production code. `crates/mandate-dispatcher/src/step.rs` (`step`, from slice
  D2) already re-reads the notice stream at every step and keys each send by
  `IdempotencyKey::of(notice, address)`, so a restart re-sends an unrecorded send under its first
  key.
- **Tests:** `crates/mandate-dispatcher/src/step/tests/crash.rs`, over `MemoryJournal` and the
  fixture provider. A fuse crashes a step right after its n-th effect (the issue batch commits but
  the step sees `Ambiguous`; a send is captured but the step sees an error) and counts any effect
  after it, which must be none. Every single crash point and every pair of crash points, each
  followed by a restarted dispatcher at a new writer epoch, ends with NT-8's oracle passing: from
  the committed journal (and the configured audience for channels) one `NoticeIssued` per notice
  key, exactly one `NoticeAttempted` per due `(notice, recipient, channel)`, and every captured
  send a due send under that send's own idempotency key, with each due send captured; a send is
  captured twice only when a crash fell between it and its record. A send whose `NoticeAttempted`
  append returns `Unavailable` or `Ambiguous` (unwritten) is made again by the next step under the
  same key and recorded once. The oracle is shown to fail on a seeded bug (a restart that forgets
  the address in flight at the crash) at exactly the four send-boundary crashes.
- **Run:** `cargo nextest run -p mandate-dispatcher`.
