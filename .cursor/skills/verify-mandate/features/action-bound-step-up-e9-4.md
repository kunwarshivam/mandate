# Action-bound step-up (E9-4)

- **Spec:** identity spec §7.1 to §7.3 and §10.1's cool-off; mandate spec §6.1 (valid step-up);
  workspace API spec §3.6 (the action kinds); DEC-662 (refusal codes and their order).
- **Code:** `crates/mandate-passkey/src/stepup.rs` (`ChallengeRecord` and its canonical form and
  WebAuthn challenge, `consume`, `reverify`, `StepUpRefusal`), over `mandate-identity`'s
  `PrincipalId`, `WorkspaceId`, `AssertionId`, `StepUpActionKind`, `StepUpMethod`, and
  `StepUpEvidence`.
- **Tests:** `crates/mandate-passkey/tests/stepup_api.rs` (the refusal codes); the
  `compile_fail` doctests on `ChallengeRecord`, `Used`, and `Consumed` (no caller builds one).
- **Run:** `cargo nextest run -p mandate-passkey` and `cargo test -p mandate-passkey --doc`.
