# Action-bound step-up (E9-4)

- **Spec:** identity spec §7.1 to §7.3 and §10.1's cool-off; mandate spec §6.1 (valid step-up);
  workspace API spec §3.6 (the action kinds); DEC-662 (refusal codes and their order); §7.2 step 1
  and DEC-665 (who may obtain a challenge); ID-8 and DEC-649 (a presentation's workspace and
  principal come from the request's context); DEC-2103 (the API's challenge route).
- **Code:** `crates/mandate-passkey/src/stepup.rs` (`ChallengeRecord` and its canonical form and
  WebAuthn challenge, `consume`, `reverify`, `StepUpRefusal`, and `issue_challenge`, which takes
  the `TenantContext` `authorize` yields and refuses `forbidden` unless its row is the one the
  action's kind needs, and `Presentation::new`, which takes the workspace and principal from the
  request's `TenantContext`), over `mandate-identity`'s
  `PrincipalId`, `WorkspaceId`, `AssertionId`, `StepUpActionKind`, `StepUpMethod`, and
  `StepUpEvidence`; `permission_for` names the row a caller authorizes first. The API's route:
  `crates/mandate-api-server/src/challenges.rs` (`POST /v1/workspaces/{ws}/step-up/challenges`
  and the in-memory `Challenges` store, DEC-2103), over `mandate-api`'s `StepUpChallengeRequest`.
  A connect start's passkey wire (DEC-2126 item 1): `mandate-api`'s `StepUpAssertion` and
  `OauthStartRequest::step_up_assertion`, admitted only beside `passkey` evidence. The port's
  binding (DEC-2126 item 2): `crates/mandate-api-server/src/methods.rs` (`Methods`, each method
  to its own verifier, and the `Passkey` verifier, a stub until E9-4 S4.2), and `StepUp::asserted`
  in `src/stepup.rs`, which the connect start calls.
- **Tests:** `crates/mandate-passkey/tests/stepup_api.rs` (the refusal codes),
  `tests/consume.rs` (the record, its canonical form, each refusal, the order with two failures,
  re-verification), `tests/consume_properties.rs` (the refusal is the first injected failure
  in DEC-662's order), with the fixture in `tests/stepup/mod.rs`; `tests/issue.rs` (issuance
  against `authorize` and an oracle parsed from §4.2, and no context for a service account or the
  host CLI); `tests/presentation.rs` (a challenge counts only under the context it was issued
  to); and the `compile_fail` doctests on
  `ChallengeRecord`, `Used`, `Consumed` (no caller builds one), and `issue_challenge` and
  `Presentation::new` (no call without a context).
  The route: `crates/mandate-api-server/tests/stepup_http.rs` (who obtains a challenge, the
  record it binds recomputed from §7.2, retries, expiry, live, the bounds, and a seeded walk
  against an independent model of the store).
  One's own passkeys: `crates/mandate-api-server/tests/passkeys_http.rs` (the registration
  challenge and the list, workspace API §3.6, DEC-2124: who asks, a fresh sign-in for a first
  passkey, retries and tombstones, the shared bounds, a broken fold, and a seeded walk against an
  independent fold), against `src/passkeys.rs` and the store's registration challenges and
  tombstones in `src/challenges.rs`.
  The connect start's assertion wire: `crates/mandate-api/tests/step_up_assertion.rs` (optional,
  read as `null`, one violation beside `cli_confirm` or `null` evidence, and each member's pattern
  at its bounds). The passkey verifier: `crates/mandate-api-server/tests/passkey_stepup.rs`
  (pending E9-4: each challenge refusal, its evidence and its order; the credential's
  refusals, cool-off, removal, version 1, its backup-eligible flag, the counter, a holder broken by
  rule 251 or 254, and a stream the fold cannot read; and the record a counted step-up returns, the
  challenge it takes, and the stored assertion's re-verification), with the software
  authenticator in `tests/support_passkey/mod.rs`. The connect start under passkey step-up over
  HTTP: `tests/connect_passkey_http.rs` (pending E9-4: the batch of `ConnectionRequested` then
  `StepUpAsserted` and the stored raw assertion, nothing appended on a refusal, on a stream that
  breaks rule 254, an unreadable one, or a store that refuses the assertion, a replay answered alike, and a `cli_confirm` start in the interim; and the two `invalid` wire cases).
- **Run:** `cargo nextest run -p mandate-passkey`, `cargo test -p mandate-passkey --doc`,
  `cargo nextest run -p mandate-api-server --test stepup_http --test passkeys_http`, and
  `cargo nextest run -p mandate-api --test step_up_assertion`; the pending tests with
  `--run-ignored all --test passkey_stepup --test connect_passkey_http`.
