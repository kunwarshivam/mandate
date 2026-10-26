# The stored bars a decision may rest on: the trust check and the closes object (DEC-2385)

- **Spec:** [data-plane spec](../../../../docs/specs/data-plane.md) §3.6 (signal inputs recorded as
  `ObservationRecorded` with the bar window as an artifact), §3.7; DEC-846 items 2 and 3, DEC-470
  item 4, DEC-138 item 3, DEC-503 items 5 and 6; DEC-2385 (one rule below the shell and the
  runtime process).
- **Code:** `mandate-bars` (`crates/mandate-bars`): `src/trusted.rs` (`trusted_daily_bars`,
  `trusted_daily_closes`: a dataset is admitted only as the pinned symbol's daily bars through the
  last completed equity session, every partition intact, no trading day without its bar, no split
  inside the span), and `src/closes.rs` (`closes_object`, the canonical bytes the observation's
  `data_ref` digests). `mandate-shell`'s `adapters.rs` and `mandate-paper` call them.
- **Tests:** `crates/mandate-bars/src/trusted/tests.rs`: moved unchanged from the shell, every
  refusal and the admitted cases. `tests/closes.rs`: the object's bytes, written out by hand.
- **Run:** `cargo nextest run -p mandate-bars -p mandate-shell -p mandate-paper`.
