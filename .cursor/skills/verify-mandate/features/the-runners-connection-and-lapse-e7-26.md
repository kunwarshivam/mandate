# The runner's connection and lapse (E7-26)

- **Spec:** `docs/project/tasks/first-live-trade.md` (row G1a, LT-1, LT-14, the founder's-run
  step 4); `docs/project/decisions/DEC-529.md` item 3; `docs/project/decisions/DEC-910.md`.
- **Code:** `mandate-paper`, `crates/mandate-paper/` (layer 9, safety-critical):
  `crates/mandate-paper/src/connection.rs` (`Broker`, `Connection`, `ConnectionRefusal`,
  `Held`, `LapseRefusal`, `connection`, `lapse` — the connector chosen from the connection
  record on the control stream, the environment from the confirmed mandate, and DEC-529
  item 3's lapse: a live run refuses unless the account is flat and the record is unspent,
  each refusal with its own reason code), re-exported by
  `crates/mandate-paper/src/lib.rs` as `PaperError`'s `Connection` and `Lapse` variants. The
  runner's `live` cargo feature (`crates/mandate-paper/Cargo.toml`, the crate
  `xtask/layers.toml` marks `live_feature`) is the only door to the Robinhood connector:
  the default build's connection seam refuses `connector_not_built` (DEC-910 item 1). The
  pins that read the seam (`a_live_record_refuses_the_connector_this_build_does_not_hold`,
  `a_re_establishment_after_a_revocation_restores_the_connection` and
  `a_revocation_of_another_connection_does_not_refuse_this_one`) expect what the build they
  run in serves, `connector_not_built` without `live` and the connection with it, so they hold
  in every build (DEC-963, replacing #1297's reading that they fail under `live` by design).
- **Tests:** `crates/mandate-paper/tests/connection.rs`: the connection refusals (no record, a
  revoked record — a revoked paper record included, a lone revocation with no prior
  establishment, a revocation of another connection refused never, and a re-establishment after
  a revocation restoring the connection; the refusal order DEC-910 item 2 states, the
  revocation before the unreadable record and the unreadable record before the environment
  disagreement, the environment before the broker and the account stream, the broker before the
  account stream; an unreadable record in each shape it refuses — a member missing, a version 1
  record, a corrupt payload — another environment, another broker, another account stream, and
  the connector the default build does not hold, which is LT-1's runtime build test) and the
  lapse pins (a spent record deploys nothing, a fill on a stream the record does not bind
  spending nothing, a fill on an agent stream sharing the bound account segment spending
  nothing too, a non-flat account refusing ahead of the spend, the unspent-and-flat pair
  proceeding to the builder and the gate, and a paper run making no check at all), pinning
  the implemented seam beside a live test that pins every refusal's message and reason code,
  and an agreeing paper record keeping the whole paper connection, the account stream included,
  with a same-`seq` revocation outranking its establishment in whichever order the records
  are read.
- **Run:** `cargo nextest run -p mandate-paper`.
