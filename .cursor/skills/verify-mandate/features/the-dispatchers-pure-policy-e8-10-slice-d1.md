# The dispatcher's pure policy (E8-10, slice D1)

- **Spec:** [notifications spec](../../../../docs/specs/notifications.md) §5.8 (quiet hours by
  class), NT-7; DEC-438 item 7; DEC-701 item 1; DEC-725 items 1 to 4.
- **Code:** `mandate-notify` (layer 1, pure, safety-critical): `crates/mandate-notify/src/quiet.rs`
  (`quiet_hours` with `QuietHours`, `Offset`, `QuietVerdict`). The zone's UTC offsets are the
  caller's (DEC-725 item 2). Tests PR: `quiet_hours` is a stub.
- **Tests:** `crates/mandate-notify/tests/quiet_hours.rs`: quiet hours by class in both DST states
  and across both 2026 changes, the skipped and repeated hour, `safety` reading no offset, the tie
  between two offsets from one instant, and a property against a minute walk over random offset
  tables.
- **Run:** `cargo nextest run -p mandate-notify`; `cargo xtask ci pending`.
