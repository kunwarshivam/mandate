# Robinhood equity connector (E7-6)

- **Spec:** connections spec §6.2; trading spec §5.2 and §5.7; the first live trade brief's C1.
- **Code:** `mandate-robinhood`, `crates/mandate-robinhood/` (layer 7, safety-critical, pure;
  C1 implemented: the profile, `ref_id`, the states, `Submit` and `Cancel`; DEC-860; C3
  implemented: `RobinhoodConnector::restore`, DEC-870, DEC-872, DEC-874; C2 pending:
  `agentic_account` and `read`, the reads scoped to the agentic account, CN-8, DEC-875).
- **Tests:** `crates/mandate-robinhood/tests/shape.rs` (profile, `ref_id`, states),
  `crates/mandate-robinhood/tests/restore.rs` (C3, DEC-870, DEC-872 and DEC-874: the `order_id`
  map rebuilt from the account stream, the distinct-id property, the odd-id and record-order
  tests, and the interleaving property),
  `crates/mandate-robinhood/tests/scope.rs` (C2, pending: CN-8's byte-equal account check, run
  afresh on every read, the filtered reads, and rule 13's exits beside them), and
  `crates/mandate-rh-sim/tests/robinhood.rs` (`Submit` against `SimServer` over loopback: review
  then place, a deduplicated re-send, LT-6's lost and garbled answers, LT-5's alerts, `Cancel`
  by `order_id`, what the profile does not offer, and a cancel after a restart by
  the id the journal holds), kept in the simulator's crate so no product crate depends on it.
- **Run:** `cargo nextest run -p mandate-robinhood -p mandate-rh-sim`.
