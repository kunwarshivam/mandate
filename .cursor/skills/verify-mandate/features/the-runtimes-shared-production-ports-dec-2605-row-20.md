# The runtime's shared production ports (DEC-2605 row 20)

- **Spec:** DEC-968 item 6 and DEC-2605 row 20 (one copy of the glue between the runtime's ports
  and the deterministic crates, shared by every host of the runtime's core); mandate spec §6;
  trading-domain spec §9 (the gate).
- **Code:** `mandate-ports` (`crates/mandate-ports`): `src/context.rs` (the decision-time
  contexts: `BuilderContext`, `AdvisoryOrderFacts`, `AdvisoryGateContext`, `DecisionContext`),
  `src/gate.rs` (`proposed_order`, the runtime proposal mapped to the gate's order; `evaluate`,
  `mandate_risk::evaluate` over the trusted context), `src/error.rs` (`PortError`). The shell's
  `RiskGate` (`crates/mandate-shell/src/adapters.rs`) calls it, with its reason codes unchanged.
- **Tests:** `crates/mandate-ports/tests/gate.rs`: the proposed order copies only the proposal's
  four fields; the shared gate equals the gate called directly, refusals included, for any
  quantity. The shell's own adapter tests still pin its behaviour.
- **Run:** `cargo nextest run -p mandate-ports -p mandate-shell`.
