# Approval core: content, admission, re-validation, and step-up (E8-1 to E8-3)

- **Spec:** the [M7 brief](../../../../docs/project/tasks/M7-escalation-v0.md); mandate spec §6.1
  (owner controls and step-up), §6.4, and MI-21 to MI-25; journal spec v0.5 §9; DEC-155, DEC-156,
  DEC-158 (option (c)), DEC-165, DEC-173. The MC-E cases follow the harness count correction
  (DEC-173 item 1).
- **Reference model:** the escalation section of `reference/mandate/ref.py`, fuzzed by
  `reference/mandate/fuzz.py` (`fuzz_escalation`, `fuzz_policy_quorum`, `fuzz_drift`,
  `fuzz_ask_budget`, `fuzz_quiet_hours`, `fuzz_owner_controls`, `fuzz_content`) and
  mutation-checked by `reference/mandate/mutants.py`. Check 7's quorum is the stricter of the bound
  requirement and the policy overlay (DEC-173 item 13).
- **Code:** `mandate-approval` (layer 1; E8-1, E8-2 and E8-3 implemented, check 7's policy overlay
  included, DEC-173 item 13):
  `crates/mandate-approval/src/content.rs` (`BoundAction`, `content_object`, `content_hash`,
  `confirmation_code`), `crates/mandate-approval/src/admit.rs` (`admit`: checks 1 to 7;
  `PolicyOverlay`, and `quorum`, the stricter of the bound requirement and the overlay),
  `crates/mandate-approval/src/revalidate.rs` (`revalidate`: checks 8 to 12, `GrantedOrder`),
  `crates/mandate-approval/src/drift.rs`, `crates/mandate-approval/src/budget.rs`,
  `crates/mandate-approval/src/notify.rs` (the closed `Notification`),
  `crates/mandate-approval/src/quiet.rs`, and `crates/mandate-approval/src/stepup.rs`
  (`owner_command`, `kill_switch`, whose answer has no refusing variant, and `kill_switch_code`).
- **Tests:** `crates/mandate-approval/tests/content.rs` (E8-1: the content object, its hash and
  code, the notification payload, quiet hours), `crates/mandate-approval/tests/budget.rs` (E8-2:
  the ask budget and suppressions), and `crates/mandate-approval/tests/properties.rs` (the sentinel
  scanner, the budget counter with its own DST table, and content separation), all live, with
  fixtures in `crates/mandate-approval/tests/common/mod.rs`; for E8-3,
  `crates/mandate-approval/tests/admission.rs` (checks 1 to 7, lateness, step-up, owner commands,
  the kill switch), `crates/mandate-approval/tests/revalidation.rs` (checks 8 to 12 and drift),
  and `crates/mandate-approval/tests/grant_properties.rs` (the check-table, clock-accumulator,
  principal, assertion-ledger, scaled-integer drift, field-comparer and kill-switch oracles), all
  live; and `crates/mandate-approval/tests/quorum.rs` (check 7 against the policy overlay, with
  its own scaled-integer oracle), all live. In-module tests in `src/stepup.rs` probe step-up
  evidence at the clock's extremes against an `i128` oracle, and `src/drift.rs` a drift too large
  to compute. The runtime's approval path (M7 tests PR 3 of 4, DEC-257 items 5 to 12):
  `crates/mandate-runtime/tests/approvals.rs` (the grant, skip, timeout, lateness, re-tail,
  principal, step-up, same-step cancellation, re-validation and drift cases, the owner exit whose
  step-up is refused yet still routed, pause, resume and the owner's kill switch) and
  `crates/mandate-runtime/tests/approval_properties.rs` (the transition-table and causation-walker
  oracles over random scripts), live since #395, with fixtures in
  `crates/mandate-runtime/tests/common/escalation.rs`. The `quorum` record (DEC-488): four hand
  cases in `tests/approvals.rs`, of which the first tests PR carries the two on the live
  one-approver binding and the second carries the two stricter ones with the `rebound_shell` and
  `two_approvers` fixtures, and one property in `tests/approval_properties.rs`, whose oracle
  reads the expected `{required, independent}` off the journaled request and decides which
  responses check 7 judged from the generated answer its `causation` names, and one harness test in
  `crates/mandate-refcases/tests/mandate_lifecycle_harness.rs` (the ten quorum cases whole, their
  `quorum` compared), live since E8-3's implementation: `escalation::answered` writes the member
  through `quorum_applied` (DEC-488), and all twenty-six lifecycle cases pass whole
  (`every_lifecycle_case_passes_whole`). The CLI's owner control (M7 tests PR 4 of 4,
  DEC-257 items 13 to 17): `crates/mandate-cli/src/control.rs` (`ControlJournal`, `Owner`, `Ids`,
  `ControlError`), `crates/mandate-cli/src/approvals.rs` (`list`, `show`, `approve`, `skip`,
  `outcome`, `message`) and `crates/mandate-cli/src/agent.rs` (`code`, `kill_code`, `command`,
  `kill`, `status`), stubbed; `crates/mandate-cli/tests/approvals.rs` and
  `crates/mandate-cli/tests/agent.rs`, pending E8-3 but for one live fixture check, with an
  in-memory journal in `crates/mandate-cli/tests/common/mod.rs`.
- **The `approvals` commands (K1a, DEC-533):** `crates/mandate-cli/src/inbox.rs` (`list`, `show`,
  `approve` and `skip` over P0's journal as D1b's paper owner; the renderers `list_lines`,
  `show_lines`, `granted_lines`, `skipped_line`; `assertion_id`) and `main`, with the answer's
  `content_hash` in `artifact_refs` (`control.rs`, DEC-533 item 6);
  `crates/mandate-cli/tests/inbox.rs`, and `crates/mandate-cli/tests/grant.rs`, the binary over
  Postgres (`MANDATE_PG_URL`), playing the runtime that records the grant.
- **Run:** `cargo nextest run -p mandate-approval -p mandate-runtime -p mandate-cli`;
  `cargo xtask ci pending`.
