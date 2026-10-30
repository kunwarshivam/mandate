- **E21-54 (Should, M9, debt; DR)** The cell demo's envelopes pin `quant.ma_crossover` 1.1.0, whose
  crossover down is an exit ([DEC-2801](../decisions/trading-core.md#dec-2801) item 6), so the always-on agent
  sells on a signal. Deferred past the 2026-10-27 demo: the cell's recording only rises, so 1.1.0
  answers `Long` as 1.0.0 does and no signal exit can fire there, while the pin moves fixtures that
  several crates test against:
  - `deploy/cell/demo/mandate.json`: step 5's backtest runs its version over the runner's committed
    demo outputs (`crates/mandate-backtest-runner/tests/fixtures/demo/outputs.json`, 1.0.0's), which
    must be pinned (`OutputNotPinned`), so those outputs, the scenario's `DEMO_PAYLOAD` and
    `DEMO_REPORT`, and the runner's golden `tests/fixtures/demo-store.json` (`tests/run.rs`) move
    with it; `mandate-daemon`'s `tests/gate.rs` (its `HASH` and `DEPLOYED`), `tests/production.rs`
    and `tests/research_rig` read it too.
  - `web-mandate.json`: `mandate-runtime-models`' test mandate (`tests/common/mod.rs`).
  - `research-mandate.json`: `mandate-cli`'s `tests/research_register.rs` and `mandate-daemon`'s
    `tests/plan.rs`.
  - The scenario's `the_codes_are_the_first_eight_hex_digits_of_the_gestures_hash`, which pins the
    demo's and the research envelope's digests and codes, and `deploy/cell/README.md`'s and
    `docs/project/tasks/e2-paper-runbook.md`'s descriptions of the pin.
  *Accepted when:* the three envelopes pin 1.1.0 (`sha256:ee5806e6…e5a4`), the scenario registers
  1.1.0 where it registers 1.0.0 today, the runner's demo outputs, golden store and the scenario's
  digests are regenerated, the tests named above pass, and a cell run's step 5 still records the
  backtest.
