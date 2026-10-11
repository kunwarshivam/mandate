# The runner's connection and lapse (E7-26)

- **Spec:** `docs/project/tasks/first-live-trade.md` (row G1a, LT-1, LT-14, the founder's-run
  step 4); `docs/project/decisions/DEC-529.md` item 3; `docs/project/decisions/DEC-910.md`.
- **Code:** `mandate-paper`, `crates/mandate-paper/` (layer 9, safety-critical):
  `crates/mandate-paper/src/connection.rs` (`Broker`, `Connection`, `ConnectionRefusal`,
  `Held`, `LapseRefusal`, `connection`, `lapse` — the connector chosen from the connection
  record on the control stream, the environment from the confirmed mandate, and DEC-529
  item 3's lapse: a live run refuses unless the account is flat and the record is unspent,
  each refusal with its own reason code), re-exported by
  `crates/mandate-paper/src/lib.rs` as `PaperError`'s `Connection` and `Lapse` variants.
- **Tests:** `crates/mandate-paper/tests/connection.rs`: the connection refusals (no record, a
  revoked record — a revoked paper record included, an unreadable record, another environment,
  another broker, another account stream, and the connector the default build does not hold,
  which is LT-1's runtime build test) and the lapse pins (a spent record deploys nothing, a
  non-flat account refuses ahead of the spend, and the unspent-and-flat pair is the one that
  proceeds to the builder and the gate), each a pending test on its stub until E7-26's
  implementation lands (DEC-77), beside a live test pinning every refusal's message and
  reason code.
- **Run:** `cargo nextest run -p mandate-paper`.
