# Authentication: sessions (E9-1)

- **Spec:** [identity spec](../../../../docs/specs/identity.md) §6; the
  [E9-1 brief](../../../../docs/project/tasks/E9-1-authn.md); DEC-652.
- **Code:** `mandate-authn`, sessions (slice A2, DEC-652): `crates/mandate-authn/src/session.rs`
  (`SessionRecord`, `SessionLimits`, `ProviderAnswer`, `EndReason`, `SubjectStanding`, and `admit`,
  which builds `mandate_identity::Session` through the seal), implemented.
- **Tests:** `crates/mandate-authn/tests/session_api.rs` (a refresh secret's `Debug`, the
  `SessionRevoked` reason codes, and the digest-only record);
  `crates/mandate-authn/tests/session.rs` (org-kind limits that may only shorten, the 5-minute
  access token, idle and absolute limits, refresh rotation with reuse revoking the family) and
  `crates/mandate-authn/tests/admit.rs` (the sealed identity session's kind and snapshot, and no
  session on a refusal), and `crates/mandate-authn/tests/routes.rs` (an unreachable provider, a
  failed refresh, a deprovision closing route 2, the reduction-only session, and a property over
  random histories against a folded oracle), all live.
- **Run:** `cargo nextest run -p mandate-authn`.
