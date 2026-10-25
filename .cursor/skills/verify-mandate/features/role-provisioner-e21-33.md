# The role provisioner: each OAuth connect's Kubernetes-auth roles, made, refused, and the exchange's removed (E21-33, DEC-2350)

- **Spec:** [journal spec](../../../../docs/specs/journal.md) §9.33 (`ConnectionRolesMade`,
  `ConnectionRolesRefused`, `ConnectionRolesRemoved`; rules 261 to 263) and §9.21 (the exchange
  waits for its roles); DEC-2350 (items 1, 3 and 5: the provisioner, never removing an executor's
  role, its policy); DEC-2630 item 7 (the dedicated provisioner).
- **Code:** the records and the verifier's fold, `verify_connection_roles`, in
  `crates/mandate-journal/src/connection_roles.rs`; the provisioner's identity, AppRole and policy
  in `crates/mandate-vault/src/kubernetes.rs` (`provisioner_policy`, `role_request`) and
  `src/openbao.rs` (`provisioned_policy`); its vault operations in `src/provisioner.rs`
  (`RoleProvisioner`, `RoleRefusal`). The process's library is `mandate-provisioner`
  (`crates/mandate-provisioner`): `src/plan.rs` (`plan`: the steps the control stream owes),
  `src/record.rs` (the §9.33 drafts with event ids derived from the step) and `src/driver.rs`
  (`Provisioner::tick`: the vault first, then the record; the `connection_roles_refused` alert).
  The process is `mandate-provisionerd` (`crates/mandate-provisionerd`): `src/lib.rs` (`input`, a
  pass's outcome as a lifecycle input; `vault_config` and `vault_login`, the AppRole session from
  the mounted vault directory; `next_login`, the hourly renewal; `FixtureVault`) and
  `src/main.rs` (the loop, probes, drain, and the control stream as `mandate-deployd` writes it).
- **Tests:** `crates/mandate-journal/tests/connection_roles.rs` (the vectors' `connection_roles`
  section through `Draft::parse`, `check_stream` and the fold); `crates/mandate-vault/src/tests/`
  (`kubernetes.rs`: the provisioner's policy pinned; `provisioner.rs`: the refusals' reasons;
  `openbao_server.rs`: the provisioner's own session making and removing roles on a real vault);
  `crates/mandate-provisioner/tests/provisioner.rs` (the plan against every valid vector stream,
  a request's roles once, a refusal and a slow vault, a close and a revocation removing only the
  exchange's role, a step the journal missed, two racing writers);
  `crates/mandate-provisionerd/tests/` (`process.rs`: the lifecycle input, the renewal, the vault
  directory's refusals; `binary.rs`: the refused starts, the probes, and the drain on SIGTERM).
- **Run:** `cargo nextest run -p mandate-journal --test connection_roles`,
  `cargo nextest run -p mandate-vault`, and `cargo nextest run -p mandate-provisioner -p mandate-provisionerd`.
