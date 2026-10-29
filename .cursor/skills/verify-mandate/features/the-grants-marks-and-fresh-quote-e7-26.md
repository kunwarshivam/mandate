# The grant's marks and fresh quote: the run journals its IEX marks, and the binding gate judges a fresh quote at the grant (E7-26)

- **Spec:** `docs/project/decisions/earlier-900-1999.md#dec-943` (closing DEC-940 item 6's gaps 1 and 3); mandate
  spec §5.2 (`MarkUpdated` as a risk input) and §6.4 (the request's `reference_mark`, check 12's
  drift band, MI-22); journal spec §9's registered `MarkUpdated` version 1 (`instrument_id`,
  `price`, `source`, `feed`, `risk_clock`); trading-domain spec §8.2 (the bid as a long's risk
  mark); DEC-935 item 2 and DEC-936 (the quote's ten-second bound and the 250 ms skew tolerance
  at the read, unchanged).
- **Code:** `crates/mandate-shell/src/tracer.rs` — `Mark` (an IEX quote's bid at its own stamp),
  `Session::mark` (the account stream's `MarkUpdated`, written as the executor's), the cycle's
  open journaling the preflight's mark before the request, and `AskedCycle::wake(now, mark)`
  journaling each wake's mark before the answers reach the runtime; `run_observed_asked_watched`
  (the free door, beside the method). `crates/mandate-shell/src/paper/context.rs` — `GateQuote`,
  the quote the binding gate judges, handed each fresh read; `paper/facts.rs` — `fresh_quote`
  (the preflight's own IEX read and the judge's checks). `crates/mandate-runtime/src/state.rs` —
  the fold reads the registered `instrument_id`. `crates/mandate-paper/src/wiring.rs` —
  `granted_opening` reads a fresh quote before each wake, a failed read stopping the wait;
  `src/lib.rs` — `Session::fresh_quote`, and `fresh_mark`, the read `run()` hands to the gate and journals as the wake's mark.
- **Tests:** `crates/mandate-paper/tests/grant_marks.rs` (the runner's own wait over the
  production stages, the paper binding gate and the validating journal: a grant 30 s after the
  preflight placed once, a stale fresh read refusing with nothing sent, drift beyond the band
  skipping, and a property over grant delays and quote ages); `tests/run.rs`'s stale fresh read
  at a wake; `tests/live_wiring.rs`'s fresh mark per wake and refused fresh read;
  `crates/mandate-shell/src/stages/grant_install.rs`'s marks and band-edge probes;
  `crates/mandate-shell/src/paper/tests.rs`'s handed-quote pin.
- **Run:** `cargo nextest run -p mandate-shell -p mandate-paper -p mandate-runtime`.
