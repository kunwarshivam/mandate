# The dispatcher's pure policy (E8-10, slice D1)

- **Spec:** [notifications spec](../../../../docs/specs/notifications.md) §3.4 (one notice per
  cause), §5.3 (retries), §5.4 (coalescing, never dropping), §5.8 (quiet hours by class), NT-6,
  NT-7; DEC-438 items 7 to 9, 28; DEC-700 item 5; DEC-701 item 1; DEC-725.
- **Code:** `mandate-notify` (layer 1, pure, safety-critical): `crates/mandate-notify/src/quiet.rs`
  (`quiet_hours` with `QuietHours`, `Offset`, `QuietVerdict`; slice D1a) and
  `crates/mandate-notify/src/delivery.rs` (`coalesce` with `Read`, `Pass`, `Message`;
  `next_attempt` with `Retry`; `notice_keys` with `Alert`; slice D1b). The zone's UTC offsets are
  the caller's (DEC-725 item 2). Tests PRs: the four functions are stubs.
- **Tests:** `crates/mandate-notify/tests/quiet_hours.rs`: quiet hours by class in both DST states
  and across both 2026 changes, the skipped and repeated hour, `safety` reading no offset, the tie
  between two offsets from one instant, and a property against a minute walk over random offset
  tables. `crates/mandate-notify/tests/delivery.rs`: coalescing by two hand tables (stale causes
  and windows nothing joins included), and NT-6's property, which tallies every cause from the
  reads (exactly one message each, none before its pass, `safety` within its commit plus 60 s or
  at its pass); the retry schedule, its windows, and its stops by the provider's verdict (a
  `permanent` result before an ended window), and a monotonicity property; one notice per user
  kill switch.
- **Run:** `cargo nextest run -p mandate-notify`; `cargo xtask ci pending`.
