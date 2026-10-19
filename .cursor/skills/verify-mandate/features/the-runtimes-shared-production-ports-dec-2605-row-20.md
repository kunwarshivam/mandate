# The runtime's shared production ports (DEC-2605 row 20)

- **Spec:** DEC-968 item 6 and DEC-2605 row 20 (one copy of the glue between the runtime's ports
  and the deterministic crates, shared by every host of the runtime's core); mandate spec §6;
  trading-domain spec §9 (the gate).
- **Code:** `mandate-ports` (`crates/mandate-ports`): `src/context.rs` (the decision-time
  contexts: `BuilderContext`, `AdvisoryOrderFacts`, `AdvisoryGateContext`, `DecisionContext`),
  `src/gate.rs` (`proposed_order`, the runtime proposal mapped to the gate's order; `evaluate`,
  `mandate_risk::evaluate` over the trusted context), `src/view.rs` (`runtime_view` over
  `ViewFacts`, the runtime's view of an admitted mandate), `src/plan.rs` (`size`, the builder's
  `conviction_linear` sizing as the runtime's proposal; `classify`, §6 classification with the
  §6.2 step 5c overlay; `validate_order_execution`), `src/error.rs` (`PortError`). The shell's
  `SpecMandate`, `BuilderPlan`, `RiskGate` and `ExecutorSink`
  (`crates/mandate-shell/src/adapters.rs`) call them, with their reason codes unchanged.
- **Tests:** `crates/mandate-ports/tests/gate.rs`: the proposed order copies only the proposal's
  four fields; the shared gate equals the gate called directly, refusals included, for any
  quantity and over varied passes, modes, account states, eligibility, quotes, participation,
  positions, protective orders, sides, origins, times in force, extended hours, the session and
  fractional quantities (each input shown caught when the port drops it), and a non-vacuity test
  that reaches allow, deny, hold and defer through the port, the fee reservation and the owner's
  confirmed bid included;
  `crates/mandate-ports/tests/plan.rs`: the reference cases' whole-position exit at the threshold,
  and the refusal of an output keyed under another instrument or another model, of an unpinned
  model or model version, of a
  sell that would require protection, and of another asset class's execution policy;
  `src/plan.rs`'s unit test pins the classification's order-amount match. The shell's own
  adapter tests still pin sizing, classification and the view through its stages.
- **Run:** `cargo nextest run -p mandate-ports -p mandate-shell`.
