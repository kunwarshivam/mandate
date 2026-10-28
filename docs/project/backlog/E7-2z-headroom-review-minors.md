- **E7-2z-headroom-review-minors (Proposed; debt; lane LV)** As a builder, I want the two minors
  R3's review of #2764 (DEC-2842) left to close: (1) a test that a lost answer (`Unknown`, no
  response) neither degrades nor clears the headroom class, since the transport forgets the last
  rate limit and `observe_rate_limit(None)` must leave the class as it was; (2) DEC-2842 to say
  that a `429` carrying `X-RateLimit-Remaining: 0` both counts as a lost answer (DEC-2832) and
  degrades by headroom, with a test of that answer. *Accepted when:* both tests pass and the
  decision's item names the `429` case.
<!-- section: E7 Brokers and venues -->
