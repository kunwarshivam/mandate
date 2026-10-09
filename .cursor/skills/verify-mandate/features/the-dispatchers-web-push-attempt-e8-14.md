# The dispatcher's web-push attempt (E8-14, deployment side of the relay)

- **Spec:** [notifications spec](../../../../docs/specs/notifications.md) §4.6 (the VAPID header
  built per attempt and never kept, the relayed subject), §5.2 (reasons), §5.3 (retries), NT-1,
  NT-2 and NT-3; DEC-701 (the dispatcher crate and its `forbidden_internal` list), DEC-726 item 3,
  DEC-727, DEC-728 (every relay refusal, and a relayed subject that is not a role mailbox, is a
  permanent `provider_error`), DEC-790 item 4 (`exp` 12 hours after the attempt).
- **Code:** `mandate-dispatcher` (layer 3, safety-critical, over `mandate-notify`,
  `mandate-webpush` and `mandate-push-relay`): `crates/mandate-dispatcher/src/lib.rs` (`prepare`,
  `Attempt`, `Route`, `Prepared`, `Relayed`, `relay_refusal`, the closed `DispatchError`).
  Pending E8-14: every body is a stub.
- **Tests:** `crates/mandate-dispatcher/src/tests.rs`: two attempts sign two headers whose `exp`
  differs by the attempts' gap; no header lapses at its own attempt over the whole 24-hour `safety`
  schedule; a relayed send with a person's subject builds no header and is refused
  `provider_error`; every relay refusal is a permanent `provider_error` that marks nothing and is
  not retried; and a canary subject reaches no outcome or error.
- **Run:** `cargo nextest run -p mandate-dispatcher --run-ignored all`; `cargo xtask ci pending`.
