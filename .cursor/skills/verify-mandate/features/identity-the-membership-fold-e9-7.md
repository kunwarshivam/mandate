# Identity: the membership fold (E9-7)

- **Spec:** journal spec §9.12 (the membership records and the fold, DEC-648), identity spec §5.1,
  §5.3 and §8.3; ID-7; the design in DEC-657.
- **Code:** `mandate-identity`, `crates/mandate-identity/src/membership.rs` (`MembershipRecord`,
  `MembershipEvent`, `MembershipFold` with its readings at an instant, `InvitationId`,
  `InvitationState`, and the writer's `check_independence` with `RecordRefusal`). Stubs until
  E9-7's implementation.
- **Tests:** all pending E9-7. `crates/mandate-identity/tests/membership_fold.rs`: every history
  of the `membership_fold` section of `fixtures/refcases/journal.json`, its probes, unreadable
  latch, and refused record (the reference fold is `reference/journal/membership_fold.py`).
  `crates/mandate-identity/tests/membership_gaps.rs`: the backlog's three access-reducing gaps, the
  kept roles, the order check (DEC-657 item 4), and the independence check on every record type
  that carries the flag. Shared builders: `crates/mandate-identity/tests/membership/mod.rs`.
- **Run:** `cargo test -p mandate-identity -- --include-ignored`.
