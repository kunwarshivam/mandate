# Simulated Robinhood broker (E7-25)

- **Spec:** the Robinhood tool contract (E7-15: "The equity order tools", "Order states") and
  slice S1 of the first live trade brief, both from PR #706; connections spec §6; DEC-124;
  DEC-441 items 11 and 16.
- **Code:** `mandate-rh-sim`, `crates/mandate-rh-sim/` (layer 10, above every product crate, so
  a dev-dependency only; safety-critical, pure):
  `src/lib.rs` (`Sim`, its scripted `Event`s and `Fault`s, the order and request types,
  `SimError`).
- **Tests:** `crates/mandate-rh-sim/tests/rules.rs` (only an agentic account reviews or places;
  account numbers are unique), with fixtures in `tests/common/mod.rs`. Pending E7-25.
- **Run:** `cargo nextest run -p mandate-rh-sim`.
