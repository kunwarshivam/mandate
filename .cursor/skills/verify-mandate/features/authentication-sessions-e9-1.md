# Authentication: sessions (E9-1)

- **Spec:** [identity spec](../../../../docs/specs/identity.md) §6; the
  [E9-1 brief](../../../../docs/project/tasks/E9-1-authn.md); DEC-652.
- **Code:** `mandate-authn`, sessions (slice A2, DEC-652): `crates/mandate-authn/src/session.rs`
  (`SessionRecord`, `SessionLimits`, `ProviderAnswer`, `EndReason`, `SubjectStanding`, and `admit`,
  which builds `mandate_identity::Session` through the seal), stubbed.
- **Tests:** `crates/mandate-authn/tests/session_api.rs` (a refresh secret's `Debug` and the
  `SessionRevoked` reason codes, live; the digest-only record, pending).
- **Run:** `cargo nextest run -p mandate-authn`.
