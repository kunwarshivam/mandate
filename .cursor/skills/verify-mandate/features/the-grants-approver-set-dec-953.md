# The grant's approver set: the run resolves the mandate's approver references to the workspace owner (E7-26, DEC-953)

- **Spec:** `docs/project/decisions/DEC-953.md` (closing DEC-943's "Not covered" approver set);
  mandate spec §6.4 check 3 (a `user` in `autonomy.approval.approvers`, `not_an_approver`) and
  V-024; identity spec §3.2 and §4.3 (the one-person workspace's owner holds every workspace role)
  and §4.2 (only the approver role answers an approval); DEC-923 item 1 (the approver is the workspace
  owner, the `user` of the mandate connection's `ConnectionEstablished`).
- **Code:** `crates/mandate-gate-inputs/src/approvers.rs` — `members` (the owner off the control
  stream; any §9.12 membership record, a revocation, or no readable user names no one) and
  `resolve` (each reference to the owner or to no one); `crates/mandate-gate-inputs/src/control.rs` —
  `ConfirmedVersion::members`; `crates/mandate-shell/src/paper/artifacts.rs` and
  `paper/context.rs` carry them into `RunContext::members`; `crates/mandate-shell/src/adapters.rs`
  — `runtime_view` hands the runtime the resolved set, which the builder's size and classify
  re-derive.
- **Tests:** `crates/mandate-shell/tests/approvers.rs` (the members fold, the resolution table, a
  property over reference sets, members and responders against an independent oracle, and the
  production view with and without the connection's record);
  `crates/mandate-paper/tests/grant_marks.rs` (the production mandate and builder stages: the
  owner's grant placed once, anyone else's refused `not_an_approver` with nothing sent).
- **Run:** `cargo nextest run -p mandate-gate-inputs -p mandate-shell -p mandate-paper`.
