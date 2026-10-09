# Robinhood equity connector (E7-6)

- **Spec:** connections spec §6.2; trading spec §5.2 and §5.7; the first live trade brief's C1.
- **Code:** `mandate-robinhood`, `crates/mandate-robinhood/` (layer 7, safety-critical, pure;
  stubs until C1's implementation; DEC-860).
- **Tests:** `crates/mandate-robinhood/tests/shape.rs` (profile, `ref_id`, states; pending).
- **Run:** `cargo nextest run -p mandate-robinhood`.
