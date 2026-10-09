# Identity: the membership fold (E9-7)

- **Spec:** journal spec §9.12 (the membership records and the fold, DEC-648), identity spec §5.1,
  §5.3 and §8.3; ID-7; the design in DEC-657.
- **Code:** `mandate-identity`, `crates/mandate-identity/src/membership.rs` (`MembershipRecord`,
  `MembershipEvent`, `MembershipFold` with its readings at an instant, `InvitationId`,
  `InvitationState`, and the writer's `check_independence` with `RecordRefusal`). Implemented
  (E9-7 M1): each reading replays the records at or before its instant.
- **Tests:** all live. `crates/mandate-identity/tests/membership_fold.rs`: every history
  of the `membership_fold` section of `fixtures/refcases/journal.json`, its probes, unreadable
  latch, and refused record (the reference fold is `reference/journal/membership_fold.py`).
  `crates/mandate-identity/tests/membership_gaps.rs`: the backlog's three access-reducing gaps, the
  kept roles, the order check (DEC-657 item 4), and the independence check on every record type
  that carries the flag. `crates/mandate-identity/tests/membership_oracle.rs`: ID-7 and §5.1 over
  random histories against an oracle written apart from the crate. Shared builders:
  `crates/mandate-identity/tests/membership/mod.rs`.
- **Run:** `cargo test -p mandate-identity`.
