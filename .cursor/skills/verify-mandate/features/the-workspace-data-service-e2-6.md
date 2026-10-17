# The workspace data service: normalized quotes, receive times, and the bounded hot cache (E2-6)

- **Spec:** [data-plane spec](../../../../docs/specs/data-plane.md) v0.2 §3.1 (normalized types,
  `vendor_time` and `received_at`), §3.3 (order and duplicates), §3.4 (quote age, future stamps,
  the host clock), §5.1 (the hot cache); [trading spec](../../../../docs/specs/trading-domain.md)
  §8.2 (the sane quote); DEC-433 items 5 to 8; DEC-994.
- **Code:** `mandate-dataservice` (`crates/mandate-dataservice`). `src/quote.rs` (`Quote`, `Feed`,
  and `Freshness`: the age basis, the future-stamp, sanity and freshness bounds) and
  `src/cache.rs` (`HotCache`: the subscription set and its capacity, `intake` and its `Intake`
  outcomes, `connection_lost`, the clock tolerance, `opening_quote` as the gate's `SaneQuote`, and
  `last_marked` for exits).
- **Tests:** `tests/oracle.rs`: a property over random arrival orders, duplicates, replays, vendor
  clocks ahead of and behind the host, connection losses and clock-tolerance changes, comparing
  after every event the opening quote and the last marked quote of two instruments with an oracle
  that finds the mark from the whole history at once in whole milliseconds and cents; and a
  property that every opening quote is sane, fresh and not future-stamped. `tests/rules.rs`: each
  rule as a case, with the threshold and the skew to the nanosecond.
- **Run:** `cargo nextest run -p mandate-dataservice`.
