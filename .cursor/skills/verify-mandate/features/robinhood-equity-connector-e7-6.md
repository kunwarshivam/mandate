# Robinhood equity connector (E7-6)

- **Spec:** connections spec §6.2; trading spec §5.2 and §5.7; the first live trade brief's C1
  to C4; DEC-470 item 1.
- **Code:** `mandate-robinhood`, `crates/mandate-robinhood/` (layer 7, safety-critical, pure;
  C1 implemented: the profile, `ref_id`, the states, `Submit` and `Cancel`; DEC-860; C3
  implemented: `RobinhoodConnector::restore`, DEC-870, DEC-872, DEC-874, DEC-876; C2
  implemented: `agentic_account` and `read`, the reads scoped to the agentic account, CN-8,
  DEC-875, and their filter-key allowlist, DEC-879; C4 implemented: `account_snapshot` and
  `preflight_facts`, DEC-470 item 1's facts from the scoped reads, quotes and tradability,
  DEC-902).
- **Tests:** `crates/mandate-robinhood/tests/shape.rs` (profile, `ref_id`, states),
  `crates/mandate-robinhood/tests/restore.rs` (C3, DEC-870, DEC-872, DEC-874 and DEC-876: the
  `order_id` map rebuilt from the account stream, the distinct-id property, the odd-id and
  record-order tests, the interleaving property, and DEC-876's two-`replaces` tests and its
  interleaving property),
  `crates/mandate-robinhood/tests/scope.rs` (C2, CN-8 and DEC-875: the byte-equal account
  check, run afresh on every read, the filtered reads, the shorter-prefix and empty near misses,
  DEC-879's filter-key allowlist, and rule 13's exits beside them; and #1264's minor 5: the
  one-member envelope check is the preflight's `read_sole_member`, not `read`'s, until C3
  pages),
  `crates/mandate-robinhood/tests/preflight.rs` (C4, DEC-470 item 1 and DEC-902, was pending
  E7-6, its ten tests now live: cash, buying power, every position and the working orders
  mapped, each working order with the side its record names, sell as `Sell` and buy as `Buy`;
  any unknown or missing field, other type, JSON number, non-canonical trailing-zero decimal on
  every money and quantity field, or next page refusing, and an order record with an empty id
  refusing; the one symbol's quote and tradability; every account read through the scoped
  `read`; no number invented, with every money and quantity field's exact decimal text kept;
  and the snapshot property against an independent oracle, its generated orders of both sides),
  `crates/mandate-rh-sim/tests/robinhood.rs` (`Submit` against `SimServer` over loopback: review
  then place, a deduplicated re-send, LT-6's lost and garbled answers, LT-5's alerts, `Cancel`
  by `order_id`, what the profile does not offer, and a cancel after a restart by
  the id the journal holds), and
  `crates/mandate-rh-sim/tests/preflight.rs` (C4, DEC-875 item 6 and DEC-902: the whole preflight
  against `SimServer` over loopback, a halted symbol `NotTradable` before the quote, the served
  reads refusing what the core does not hold, and the core's own portfolio, positions, average
  and tradability figures), kept in the simulator's crate so no product crate depends on it.
- **Run:** `cargo nextest run -p mandate-robinhood -p mandate-rh-sim`.
