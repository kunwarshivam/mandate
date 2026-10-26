- **E21-54 (Should, M9, debt; DR)** The cell demo's envelopes pin `quant.ma_crossover` 1.1.0, whose
  crossover down is an exit ([DEC-2801](../decisions/DEC-2801.md)), so the always-on agent sells on
  a signal. Deferred past the 2026-10-27 demo: the cell's recording only rises, so 1.1.0 answers
  `Long` as 1.0.0 does and no signal exit can fire there, while the pin moves fixtures several
  crates test against. `mandate.json` is the envelope step 5's backtest runs, whose committed
  outputs (`crates/mandate-backtest-runner/tests/fixtures/demo/outputs.json`) are 1.0.0's and its
  payload and report digests follow them; `web-mandate.json` is `mandate-runtime-models`' test
  mandate; `research-mandate.json` is read by `mandate-cli`'s and `mandate-daemon`'s tests; and the
  scenario's codes test pins the research envelope's digest and codes. *Accepted when:* the three
  envelopes pin 1.1.0 (`sha256:ee5806e6…e5a4`), the scenario registers 1.1.0 where it registers
  1.0.0 today, the runner's demo outputs and digests are regenerated, those crates' tests pass, and
  a cell run's step 5 still records the backtest.
