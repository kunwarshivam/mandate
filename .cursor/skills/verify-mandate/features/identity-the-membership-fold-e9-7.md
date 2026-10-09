# Identity: the membership fold (E9-7)

- **Spec:** journal spec §9.12 (the membership records and the fold, DEC-648), identity spec §5.1,
  §5.3 and §8.3; ID-7; the design in DEC-657.
- **Code:** `mandate-identity`, `crates/mandate-identity/src/membership.rs` (`MembershipRecord`,
  `MembershipEvent`, `MembershipFold` with its readings at an instant, `InvitationId`,
  `InvitationState`). Stubs until E9-7's implementation.
- **Tests:** `crates/mandate-identity/tests/membership_fold.rs`, pending E9-7: every history of the
  `membership_fold` section of `fixtures/refcases/journal.json`, its probes, unreadable latch, and
  refused record. The reference fold is `reference/journal/membership_fold.py`.
- **Run:** `cargo test -p mandate-identity --test membership_fold -- --include-ignored`.
