# The client ceiling (E6-12)

Planned by [the E6-12 task brief](../../../../docs/project/tasks/E6-12-client-ceiling.md) and DEC-262.
The DEC-77 tests PR ([#369](https://github.com/kunwarshivam/mandate/pull/369)) stubbed
`client_ceiling` with 12 tests pending on it; the implementation PR makes them live, and no E6-12
test is pending.

- **Spec:** `docs/specs/mandate.md` §6.2 step 5a and MI-30 (DEC-185), §6.4's `decided_by`.
- **Code:** `crates/mandate-builder/src/autonomy.rs` (`RequestedBy`, `ActionContext::requested_by`,
  `DecidedBy::ClientCeiling`, and the ceiling as the last step of `classify`), and the one line of
  `crates/mandate-builder/src/builder.rs` that stamps `propose`'s buys as the agent's own.
- **Tests:** `crates/mandate-builder/tests/hand.rs` (twelve tests, from
  `a_proposed_buy_is_the_order_builders_own_request` on),
  `crates/mandate-builder/tests/properties.rs` (two generated properties against the naive rule walk
  extended by step 5a, and an exhaustive sweep of 4,212 decisions with its own deny-or-ask oracle).
  Planted bugs: the task brief.
- **Reference cases:** none; no mandate case states `requested_by` (DEC-262 item 7).
- **Run:** `cargo nextest run -p mandate-builder --run-ignored all`.
