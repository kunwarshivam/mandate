# Authentication: sessions (E9-1)

- **Spec:** [identity spec](../../../../docs/specs/identity.md) §6; the
  [E9-1 brief](../../../../docs/project/tasks/E9-1-authn.md); DEC-652.
- **Code:** `mandate-authn`, sessions (slice A2, DEC-652): `crates/mandate-authn/src/session.rs`
  (`SessionRecord`, `SessionLimits`, `ProviderAnswer`, `EndReason`, `SubjectStanding`, and `admit`,
  which builds `mandate_identity::Session` through the seal), stubbed.
- **Tests:** `crates/mandate-authn/tests/session_api.rs` (a refresh secret's `Debug` and the
  `SessionRevoked` reason codes, live; the digest-only record, pending);
  `crates/mandate-authn/tests/session.rs` (org-kind limits that may only shorten, the 5-minute
  access token, idle and absolute limits that end the session as `expired`, a clock behind the
  session, refresh rotation with reuse revoking the family) and
  `crates/mandate-authn/tests/routes.rs` (an unreachable provider, a failed refresh, a deprovision
  closing route 2, the reduction-only session, and an idle lapse that a granted refresh does not
  restore) and `crates/mandate-authn/tests/routes_properties.rs` (a property over random histories,
  a backwards clock included, against a folded oracle in which a lapse ends the session), pending.
- **Run:** `cargo nextest run -p mandate-authn`.
