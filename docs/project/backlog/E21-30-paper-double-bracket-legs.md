- **E21-30-paper-double-bracket-legs (Proposed; debt; lane LV)** As a builder, I want
  `mandate-executord`'s test broker (`tests/support`'s `Paper`) to list a filled bracket's two legs
  nested under its entry, as Alpaca's open-orders read does, so a test that runs more than one
  reconciliation pass over the recorded opening (`fixtures/opened.jsonl`, a bracket) does not end
  in a hold that only the double causes. Today the double lists no legs. The second
  reconciliation adopts the bracket's protective handle (`md-…A-p…`) as `unknown`, and from then
  on every sell of the instrument, a risk exit and a kill switch's flatten included, is held by
  the gate with `verdict` `hold` and `reason_code` `unknown_order_in_flight`. That is rule 13's
  allowed hold ("an `Unknown` order in the same instrument"), so the executor behaves correctly;
  the gap is the double's. #2764's rule-13 tests (DEC-2842 item 5) work around it by taking the
  exit or the switch in the first passes, as `outage.rs` and `completed.rs` do. Found on
  2026-10-28, probing both holds with healthy rate-limit headroom. A related oddity:
  `Paper::resting` records every resting order as a buy, so `md-protect` in `outage.rs` is
  ingested as an external buy, not as protection. *Accepted when:* the double lists a filled
  bracket's legs nested under its entry and a sell with no legs resting; a test runs five
  reconciliation passes over the recorded opening and then sends a risk exit and a kill switch's
  flatten, with no `unknown` order adopted; and `resting` takes a side.
<!-- section: E21 Operations and infrastructure -->
