# Action-bound step-up (E9-4)

- **Spec:** identity spec §7.1 to §7.3 and §10.1's cool-off; mandate spec §6.1 (valid step-up);
  workspace API spec §3.6 (the action kinds); DEC-662 (refusal codes and their order); §7.2 step 1
  and DEC-665 (who may obtain a challenge).
- **Code:** `crates/mandate-passkey/src/stepup.rs` (`ChallengeRecord` and its canonical form and
  WebAuthn challenge, `consume`, `reverify`, `StepUpRefusal`, and `issue_challenge`, which takes
  the `TenantContext` `authorize` yields), over `mandate-identity`'s
  `PrincipalId`, `WorkspaceId`, `AssertionId`, `StepUpActionKind`, `StepUpMethod`, and
  `StepUpEvidence`.
- **Tests:** `crates/mandate-passkey/tests/stepup_api.rs` (the refusal codes),
  `tests/consume.rs` (the record, its canonical form, each refusal, the order with two failures,
  re-verification), `tests/consume_properties.rs` (the refusal is the first injected failure
  in DEC-662's order), with the fixture in `tests/stepup/mod.rs`; `tests/issue.rs` (pending E9-4:
  issuance against `authorize` and an oracle parsed from §4.2); and the `compile_fail` doctests on
  `ChallengeRecord`, `Used`, `Consumed` (no caller builds one), and `issue_challenge` (no call
  without a context).
- **Run:** `cargo nextest run -p mandate-passkey` and `cargo test -p mandate-passkey --doc`.
