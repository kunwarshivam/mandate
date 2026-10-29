- **E7-4-unknown-order-hold-bound (Proposed; story; needs a decision)** As an owner, I want an
  `Unknown` order's hold on an instrument to be bounded, so that an order the executor cannot
  resolve never holds a risk exit or a protective order indefinitely.
  - **Decided for the kill switch.** [DEC-2857](../decisions/DEC-2857.md) settles the kill-switch
    reading, by the founder's ruling: an agent-scoped kill switch's flatten never waits on an
    `Unknown` order. Its certain part sells at once, and its remainder is held and alerted
    (`held_long`).
  - **What remains.** Every other exit and protective order is still held by `Unknown` orders, as
    rule 13 allows (`unknown_order_in_flight`), with one `held_long` alert and no bound. Bounding
    that hold changes a safety rule's meaning, so it goes to the founder (DEC-79).
  - *Accepted when:* a decision states the bound for those exits, and tests pin the hold ending at
    the bound with the owner alerted.
<!-- section: E7 Brokers and venues -->
