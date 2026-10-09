# Simulated Robinhood broker (E7-25)

- **Spec:** the Robinhood tool contract (E7-15: "The equity order tools", "Order states") and
  slice S1 of the first live trade brief, both from PR #706; connections spec §6; DEC-124;
  DEC-441 items 11 and 16.
- **Code:** `mandate-rh-sim`, `crates/mandate-rh-sim/` (layer 10, above every product crate, so
  a dev-dependency only; safety-critical, pure):
  `src/lib.rs` (`Sim`, its scripted `Event`s, among them `RefuseChangedResend` and `EchoRefId`, and `Fault`s,
  the order and request types, `SimError`, and the sell reservation: working sells hold shares,
  a partly filled one only its unfilled remainder).
- **Tests:** `crates/mandate-rh-sim/tests/rules.rs` (quantity forms, sessions and text against
  the contract; only an agentic account reviews or places; each pre-trade alert refuses) and
  `crates/mandate-rh-sim/tests/lifecycle.rs` (fills and positions, `ref_id` after a lost answer
  and its echo and changed-resend switches, `gfd` and `gtc`, sessions, scripted answers, and the
  refusals of cancel, fill and a sell that working sells already hold, a working sell holding
  only its unfilled remainder), and
  `crates/mandate-rh-sim/tests/properties.rs` (over random scripts: a `ref_id` never yields a
  second order; a terminal order never changes and is refused; a fill never exceeds the quantity;
  every state change is a legal transition, and every legal one is accepted; each against the
  test's own oracle), with fixtures in
  `tests/common/mod.rs`.
- **Run:** `cargo nextest run -p mandate-rh-sim`.
