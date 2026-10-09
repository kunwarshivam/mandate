# The paper adapter (E7-19 slice 5)

- **Spec:** `docs/project/tasks/first-paper-trade.md` (slice E1a, FT-1 to FT-12); DEC-502,
  DEC-503 item 2, DEC-505, DEC-846 (the ports, the observation the adapter stores, the order of a
  run).
- **Code:** `mandate-paper`, `crates/mandate-paper/` (layer 9, safety-critical): `src/lib.rs`
  (`Args`, `parse`, `Ports`, `Outcome`, `PaperError`, `run`), which hands the model host's output
  and its stored closes to `mandate-shell`'s `ProductionCycle::run_observed`; the closes come from
  `mandate_shell::paper::daily_closes`. The binary is not built yet.
- **Tests:** `crates/mandate-paper/tests/run.rs` (one order through the cycle after the closes are
  stored, a dry run that sends nothing and keeps no journal, a `Flat` model, the refusals before
  any credential, SPY and AAPL from journaled inputs alone, a held position, stale bars, the
  closing window, and a stop's message), over `crates/mandate-shell/tests/common/mod.rs`'s
  deployment and recorded broker answers, and `crates/mandate-paper/tests/args.rs` (the
  arguments).
- **Run:** `cargo nextest run -p mandate-paper`.
