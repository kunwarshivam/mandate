# Identity: roles, the permission matrix, and the authorization step (E9-2)

- **Spec:** identity spec §3 to §5, §4.2's matrix and its grammar (DEC-641), §4.5's authorization
  step, the context contract (DEC-642), the org-scope and own-data contexts (DEC-832), and the
  refusal codes (DEC-643); ID-2, ID-8, ID-13, ID-16; DEC-655's demanded permission (E9-8).
- **Code:** `mandate-identity`, `crates/mandate-identity/` (layer 1, safety-critical, pure):
  `src/lib.rs` (`authorize`, `change_roles`, `Authorized`, `TenantContext`, `OrgContext` and its
  two consumptions, `PrincipalContext`, the sealed `Tenant`
  and `MembershipLookup`, `MembershipQuery`, `Session`, `Membership`, `Refusal`, `ClientScope`,
  `StepUpActionKind`, `StepUpEvidence`) and `src/permission.rs` (one `Permission` per §4.2 row).
  `src/demand.rs` holds what a sensitive data API demands (DEC-655, E9-8): the sealed
  `RequiredPermission`, its one marker `ReadRecords`, and the `Permitted<'a, P>` witness, which
  only `TenantContext::require` yields and which is a `Tenant`.
  `change_roles` (which yields the change's resolved step-up, DEC-654),
  `PrincipalContext::into_tenant`, `TenantContext::require`, and the witness's `Tenant` methods
  are stubs.
- **Tests:** in the crate, because its session, membership, and lookup types are sealed to it:
  `crates/mandate-identity/src/tests/matrix.rs` (ID-2 and the failed membership read, pending
  E9-2: every role set, membership state, cool-off, session kind, principal kind, permission, and
  scope, workspace pairs the store does not hold included, and each grant's context, against §4.2 parsed from `docs/specs/identity.md` at test time by the grammar its doc
  states; the grammar check is live), with the hand-written row map in
  `crates/mandate-identity/src/tests/rows.rs` and the test doubles in
  `crates/mandate-identity/src/tests/mod.rs`; `crates/mandate-identity/src/tests/wire.rs` (each
  refusal's code, and the step-up kinds and client scopes read from workspace API §3.6 and §3.8,
  live); `org_fanout_workspaces_come_from_the_store` in `matrix.rs`;
  `crates/mandate-identity/src/tests/roles.rs` (pending E9-2: ID-13's own roles, the leave row,
  the last active owner and admin of §5.2, and §4.2's role-change rows, read as DEC-654 says, with
  a property for each of ID-13 and §5.2; and `into_tenant`'s workspace, DEC-832 item 7); and the
  `compile_fail` doctests in `src/lib.rs` (a `TenantContext` cannot be built, defaulted, or cloned,
  nor `Tenant` implemented, an `OrgContext` or `PrincipalContext` cannot be built, and an
  `OrgContext` is not a `Tenant`, outside the crate), with their in-crate controls;
  `crates/mandate-identity/src/tests/demand.rs` (pending E9-8: a context for each other workspace row
  is refused `forbidden` by `require::<ReadRecords>()`, and the witness reports the context it
  was called on) and the `compile_fail` doctests in `src/demand.rs` (the witness cannot be built,
  defaulted, or cloned, nor `RequiredPermission` implemented, outside the crate, and a data API
  demanding it rejects a bare `TenantContext`), with their controls.
- **Run:** `cargo nextest run -p mandate-identity --run-ignored all` and
  `cargo test -p mandate-identity --doc`.
- **Seal and test support:** `mandate-identity-seal` (layer 0, safety-critical; the `Seal` token
  and `LookupSeal`, closed by its `allowed_dependents` list in `xtask/layers.toml`, which
  `cargo xtask layers` checks, DEC-642 item 7) and `mandate-identity-testkit` (layer 11, so only
  dev-dependencies reach it, and `dev_only` in `xtask/layers.toml`; `StaticLookup`, `FailingLookup`, `session`, `membership`; DEC-645).
  The ULID text codecs are pending in `crates/mandate-identity/src/tests/ulid.rs`.
