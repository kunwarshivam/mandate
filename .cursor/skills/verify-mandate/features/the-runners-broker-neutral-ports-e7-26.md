# The runner's broker-neutral ports, its Robinhood login, `--confirm-live` and core dumps off (E7-26)

- **Spec:** `docs/project/tasks/first-live-trade.md` rows G1a ("the connector chosen from the
  connection record"; "the `live` feature"; "LT-1's build test"), LT-1 and LT-9 (with the risk
  table's "the runner disables core dumps at start"); `docs/project/decisions/DEC-922.md`
  items 1 to 3; `docs/project/decisions/earlier-900-1999.md#dec-924` item 3; `docs/project/decisions/earlier-900-1999.md#dec-928`
  item 2; `docs/project/decisions/DEC-937.md` (this row's choices).
- **Code:** `mandate-paper`, `crates/mandate-paper/` (layer 9, safety-critical):
  `src/lib.rs` — `Ports` (`Session`, `connect` given the door's `Connection`, and `harden`),
  `Session` (the preflight and the production cycle over one broker; every Alpaca transport
  is one), `acknowledge` (the run's environment against `--confirm-paper` or `--confirm-live`,
  then `Ports::harden` for a live run, called right after the door and before every credential
  read), `parse`'s acknowledgement, and `Production`'s `Connected`; `src/robinhood.rs` —
  `endpoint` (no host in the default build, `MCP_HOST` alone behind `live`, the loopback
  `MANDATE_RH_SIM_ENDPOINT` alone behind `rh-sim`), `connect` and `login` (O1b's
  `AuthServer` and `CallbackListener`), and `Linked`, which stops at the preflight until O2;
  `src/harden.rs` — `disable_core_dumps`.
- **Tests:** `crates/mandate-paper/tests/live_entry.rs` (the acknowledgement's parse and its
  four cases with the hardening counted, a failed hardening, the kernel read back after
  `disable_core_dumps`, every build's endpoint, the rehearsal's loopback-only reading, the
  refusals' messages and codes, and LT-1's build test over the binary's bytes);
  `crates/mandate-paper/tests/run.rs`'s
  `a_paper_run_connects_the_chosen_paper_connection_and_its_own_acknowledgement`.
- **Run:** `cargo nextest run -p mandate-paper`; `cargo clippy -p mandate-paper --all-targets
  --features live -- -D warnings` and the same with `live,rh-sim` (local only: CI compiles
  `live` once and never runs it).
