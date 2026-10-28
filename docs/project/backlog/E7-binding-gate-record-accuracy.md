- **E7-binding-gate-record-accuracy (Proposed; debt; lane X)** As an owner reading a denied opening,
  I want the binding gate's `GateDecided` to say which sub-checks it ran: when check 2
  (`UniverseAndLimits`) stops at its first failure, the journal ids it never reached (for example
  `concentration` and `order_size` after an eligibility-floor refusal) are recorded `not_reached`
  with their own evidence, not `fail` with empty `inputs` and `computed`
  (`binding_checks_value` in crates/mandate-executor/src/gate.rs). And I want an intent the core
  keeps because a binding-gate input is missing (`BindingGateInputMissing`) to raise one operator
  alert that names the missing input, so a held intent is never silent. Found in lane GW's research
  replay of 2026-10-27. *Accepted when:* a test shows a floor refusal journaling only the floor's
  id as `fail` and the rest of check 2 as `not_reached`, and a test shows a missing input alerted
  once with its name.
