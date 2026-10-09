# Robinhood equity connector (E7-6)

- **Spec:** connections spec §6.2; trading spec §5.2 and §5.7; the first live trade brief's C1.
- **Code:** `mandate-robinhood`, `crates/mandate-robinhood/` (layer 7, safety-critical, pure;
  stubs until C1's implementation; DEC-860).
- **Tests:** `crates/mandate-robinhood/tests/shape.rs` (profile, `ref_id`, states; pending), and
  `crates/mandate-rh-sim/tests/robinhood.rs` (`Submit` against `SimServer` over loopback: review
  then place, a deduplicated re-send, LT-6's lost and garbled answers, LT-5's alerts, `Cancel`
  by `order_id`, what the profile does not offer; pending), kept in the
  simulator's crate so no product crate depends on it.
- **Run:** `cargo nextest run -p mandate-robinhood -p mandate-rh-sim`.
