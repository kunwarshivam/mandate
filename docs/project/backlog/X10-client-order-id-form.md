# X10 follow-up: the journal binds an abandonment's client order id to its intent

- **X10a (Proposed; trading core)** As a reviewer, I want journal spec §9.5 to bind an
  `OrderAbandoned`'s `client_order_id` to its `intent_id` (`md-{intent}`, the rule DEC-2803 item 5
  left out), now that trading spec §2.3 states the form the executor writes (v0.21, DEC-2807), so
  that a record naming another intent's order is refused. *Accepted when:* journal spec §9.5 adds
  the rule with its vectors, and its sentence saying §2.3 does not yet state the form is updated.
<!-- section: E7 Alpaca connector and recovery -->
