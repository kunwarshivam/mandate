# Scoped live cli_confirm (DEC-923)

- **Spec:** [DEC-923](../../../../docs/project/decisions/earlier-900-1999.md#dec-923) items 1 to 4, read by
  [DEC-927](../../../../docs/project/decisions/earlier-900-1999.md#dec-927); DEC-529 item 3; DEC-155 item 4. Until
  E9-4's step-up lands, a `live` grant with `cli_confirm` counts only on a Robinhood deployment
  whose confirmed mandate caps `max_order_usd` at 100 USD and pins one instrument, for a bound US
  equity order of at most 100 USD on it, from the workspace owner.
- **Code:** `crates/mandate-approval/src/live.rs` (`live_cli_confirm`, the one judge; `LiveScope`,
  `LiveGrant`, `LiveRefusal` and its codes), `crates/mandate-approval/src/admit.rs` (check 6 admits
  the method on `live` only through the judge), `crates/mandate-runtime/src/types.rs`
  (`ApprovalSettings::live_cli_confirm`, `None` outside the scope) and
  `crates/mandate-runtime/src/escalation.rs` (passes it to admission), and
  `crates/mandate-cli/src/approvals.rs` (`approve` on a `live` agent stream reads the scope from the
  store and the control stream's connection record).
- **Tests:** `crates/mandate-approval/tests/live.rs` (the admission, each refusal, check 6 on
  `live` with and without the scope, and a property against an oracle on cents),
  `crates/mandate-runtime/tests/live_cli_confirm.rs` (the admission journaled before the intent it
  releases, every other live deployment refused, an exit never held, and a property), and
  `crates/mandate-cli/tests/live_approve.rs` (the committed grant, each refusal committing nothing,
  and a skip and a paper grant not held).
- **Run:** `cargo nextest run -p mandate-approval -p mandate-runtime -p mandate-cli`.
