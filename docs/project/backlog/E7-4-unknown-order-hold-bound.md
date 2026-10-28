- **E7-4-unknown-order-hold-bound (Proposed; story; needs a decision)** As an owner, I want an
  `Unknown` order's hold on an instrument to be bounded, so that an order the executor cannot
  resolve never holds a risk exit, a protective order or a kill switch's flatten indefinitely.
  `AGENTS.md` rule 13 lets an `Unknown` order in the same instrument hold exits and protective
  orders. The gate does that with `reason_code` `unknown_order_in_flight`. But nothing found in
  this review states how long the hold may last, or whether a kill switch's flatten, which rule 13
  says is always available, may be held by one. Found in R3's review of #2764: the test double
  `Paper` leaves a bracket's protective handle `Unknown`, and from then on every sell of the
  instrument holds (E21-30-paper-double-bracket-legs). The production `Unknown`-resolution code
  was not read for this row. Any reading that would let a kill switch wait on an `Unknown` order
  goes to the founder (DEC-79: a safety rule's meaning). *Accepted when:* a decision states the
  bound and the kill-switch reading, and tests pin an `Unknown` order's hold ending at the bound,
  with the owner alerted, and a kill switch's flatten under that decision.
<!-- section: E7 Brokers and venues -->
