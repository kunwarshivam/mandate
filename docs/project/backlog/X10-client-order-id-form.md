# X10 follow-up: trading spec §2.3 states the client order id the executor writes

- **X10a (Proposed; trading core)** As a reviewer, I want trading spec §2.3 to state the client order
  id as the executor derives it, `md-{intent}` (DEC-449, DEC-860; `w-`/`k-` intents per DEC-485),
  rather than `"{intent_ulid}-{attempt}"`, so that the journal can bind an `OrderAbandoned`'s
  `client_order_id` to its `intent_id` (the rule DEC-2803 item 5 left out). *Accepted when:* §2.3
  states the form the code writes, and journal spec §9.5 adds the rule with its vectors.
<!-- section: E7 Alpaca connector and recovery -->
