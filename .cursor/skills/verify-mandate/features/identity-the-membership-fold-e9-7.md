# Identity: the membership fold (E9-7)

- **Spec:** journal spec §9.12 (the membership records and the fold, DEC-648), identity spec §5.1,
  §5.3 and §8.3; ID-7; the design in DEC-657, and the writer's order guard in DEC-659.
- **Code:** `mandate-identity`, `crates/mandate-identity/src/membership.rs` (`MembershipRecord`,
  `MembershipEvent`, `MembershipFold` with its readings at an instant, `InvitationId`,
  `InvitationState`, and the writer's `check_independence` and `check_order` with
  `RecordRefusal`). Implemented (E9-7 M1): each reading replays the records at or before its
  instant; `check_order` (DEC-659) refuses a record that does not follow the stream's last
  membership record. `crates/mandate-identity/src/writer.rs` (DEC-646): `write_membership`, the
  one writer of `Member*` records, over the `ControlStream` port; a stub (E9-7's writer slice).
- **Tests:** live. `crates/mandate-identity/tests/membership_fold.rs`:
  every history of the `membership_fold` section of `fixtures/refcases/journal.json`, its probes, unreadable
  latch, and refused record (the reference fold is `reference/journal/membership_fold.py`).
  `crates/mandate-identity/tests/membership_gaps.rs`: the backlog's three access-reducing gaps, the
  kept roles, the order check (DEC-657 item 4), and the independence check on every record type
  that carries the flag. `crates/mandate-identity/tests/membership_oracle.rs`: ID-7 and §5.1 over
  random histories against an oracle written apart from the crate.
  `crates/mandate-identity/tests/membership_order.rs`: the writer's order guard
  (DEC-659) on every record type, its agreement with the fold's order rule, and random pairs
  against an oracle written apart from the crate. Shared builders:
  `crates/mandate-identity/tests/membership/mod.rs`. Pending (`pending E9-7`):
  `crates/mandate-identity/tests/membership_writer.rs`: the writer's serialized append (DEC-659
  items 2 and 8), a moved head, `last` by type, no clamping, and random interleavings against an
  accumulator written apart from the writer.
- **Run:** `cargo test -p mandate-identity`.
